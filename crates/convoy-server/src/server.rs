//! Connection handling and the async server runtime.
//!
//! [`ConvoyServer::handle_packet`] is the pure state machine: it takes the wire
//! bytes a client sent on one connection and returns the packets to send back.
//! [`ConvoyServer::run`] drives it from Steam networking.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use convoy_proto::messages::{AuthTicketMsg, HandshakeAckMsg, ModVerifyMsg, PlayerSpawnMsg};
use convoy_proto::{parse_frame, FrameType, RawFrame};
use convoy_steam::gameserver::SteamServer;
use convoy_steam::sockets::SteamSockets;
use convoy_steam::SteamError;

use crate::admin::BanlistManager;
use crate::client::{ClientSession, ClientState};
use crate::config::ServerConfig;
use crate::router::{broadcast_clock_tick, broadcast_telemetry};
use crate::weather::WeatherManager;

/// First id handed to a client, matching the captures.
const FIRST_CLIENT_ID: u16 = 10;
/// Idle connections are dropped after this long.
const SESSION_TIMEOUT: Duration = Duration::from_secs(30);
/// Callback pump / poll interval.
const TICK: Duration = Duration::from_millis(16);

/// Verifies an auth ticket and returns the SteamID it belongs to.
///
/// Kept behind a trait so the state machine can be tested without the Steam
/// shared library.
pub trait TicketVerifier: Send + Sync {
    fn verify(&self, peer_id: Option<u64>, ticket: &[u8]) -> Result<u64, ServerError>;
}

/// Production verifier backed by the Steam GameServer FFI.
pub struct SteamTicketVerifier {
    server: Arc<SteamServer>,
    allow_unverified: bool,
}

impl SteamTicketVerifier {
    pub fn new(server: Arc<SteamServer>) -> Self {
        Self {
            server,
            allow_unverified: false,
        }
    }

    /// Testing aid: accept a peer whose Steam auth session cannot start, which
    /// happens when the same account is already connected.
    pub fn allowing_unverified(mut self) -> Self {
        self.allow_unverified = true;
        self
    }
}

impl TicketVerifier for SteamTicketVerifier {
    fn verify(&self, peer_id: Option<u64>, ticket: &[u8]) -> Result<u64, ServerError> {
        let steam_id = peer_id.filter(|id| *id != 0).ok_or(ServerError::AuthFailed)?;
        match self.server.begin_auth_session(ticket, steam_id) {
            Ok(()) => {}
            Err(_) if self.allow_unverified => {
                log::warn!("accepting unverified client {steam_id} (testing flag)");
            }
            Err(_) => return Err(ServerError::AuthFailed),
        }
        Ok(steam_id)
    }
}

pub struct ConvoyServer {
    /// Set once Steam logon completes; used for the world frames.
    pub steam_id: u64,
    config: ServerConfig,
    sessions: HashMap<u32, ClientSession>,
    next_client_id: u16,
    verifier: Box<dyn TicketVerifier>,
    banlist: BanlistManager,
    weather: WeatherManager,
    world_clock: (u8, u8),
    /// Frames queued for one specific connection.
    pub(crate) pending: Vec<(u32, Vec<u8>)>,
}

impl ConvoyServer {
    pub fn new(config: ServerConfig, verifier: Box<dyn TicketVerifier>) -> Self {
        let banlist = if let Some(ref path) = config.banlist_path {
            BanlistManager::load_from_file(path).unwrap_or_default()
        } else {
            BanlistManager::new()
        };

        Self {
            steam_id: 0,
            config,
            sessions: HashMap::new(),
            next_client_id: FIRST_CLIENT_ID,
            verifier,
            banlist,
            weather: WeatherManager::new(),
            world_clock: (12, 0),
            pending: Vec::new(),
        }
    }

    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    pub fn banlist(&self) -> &BanlistManager {
        &self.banlist
    }

    pub fn banlist_mut(&mut self) -> &mut BanlistManager {
        &mut self.banlist
    }

    pub fn weather(&self) -> &WeatherManager {
        &self.weather
    }

    pub fn weather_mut(&mut self) -> &mut WeatherManager {
        &mut self.weather
    }

    pub fn world_clock(&self) -> (u8, u8) {
        self.world_clock
    }

    pub fn set_world_clock(&mut self, hours: u8, minutes: u8) {
        self.world_clock = (hours, minutes);
    }

    pub fn session(&self, conn: u32) -> Option<&ClientSession> {
        self.sessions.get(&conn)
    }

    pub fn sessions(&self) -> &HashMap<u32, ClientSession> {
        &self.sessions
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Step 1: register a new connection and return its `0x0b` challenge.
    ///
    /// Idempotent: re-announcing an existing connection resends its challenge.
    pub fn handle_connection(&mut self, conn: u32) -> Vec<u8> {
        if let Some(existing) = self.sessions.get(&conn) {
            return challenge(existing.client_id);
        }
        let client_id = self.alloc_client_id();
        self.sessions
            .insert(conn, ClientSession::new(client_id, conn));
        log::debug!("assigned client {client_id} to connection {conn}");
        challenge(client_id)
    }

    /// Feed one inbound Steam message; returns the packets to send back.
    pub fn handle_packet(&mut self, conn: u32, data: &[u8]) -> Result<Vec<Vec<u8>>, ServerError> {
        if !self.sessions.contains_key(&conn) {
            return Err(ServerError::UnknownConnection(conn));
        }

        let mut out = Vec::new();
        let mut rest = data;
        while !rest.is_empty() {
            match parse_frame(rest)? {
                Some((frame, consumed)) => {
                    out.extend(self.handle_frame(conn, &frame)?);
                    rest = &rest[consumed..];
                }
                // shortcut: no per-connection reassembly buffer; an incomplete
                // trailing frame is dropped. Upgrade for streamed payloads.
                None => break,
            }
        }
        Ok(out)
    }

    /// Remove a connection, logging the departure of a spawned player.
    /// Returns the removed session if it existed.
    pub fn handle_disconnect(&mut self, conn: u32) -> Option<ClientSession> {
        let removed = self.sessions.remove(&conn);
        if let Some(ref session) = removed {
            match session.name {
                Some(ref name) => log::info!("[MP] [Chat] {name} disconnected"),
                None => log::debug!(
                    "connection {conn} (client {}) disconnected",
                    session.client_id
                ),
            }
        }
        removed
    }

    /// Drop sessions idle longer than `timeout`; returns the removed connections and sessions.
    pub fn sweep_stale(&mut self, timeout: Duration) -> Vec<(u32, Option<ClientSession>)> {
        let now = Instant::now();
        let stale: Vec<u32> = self
            .sessions
            .iter()
            .filter(|(_, s)| now.duration_since(s.last_seen) > timeout)
            .map(|(conn, _)| *conn)
            .collect();
        stale
            .into_iter()
            .map(|conn| (conn, self.handle_disconnect(conn)))
            .collect()
    }

    fn handle_frame(&mut self, conn: u32, frame: &RawFrame) -> Result<Vec<Vec<u8>>, ServerError> {
        let (state, client_id) = {
            let session = self
                .sessions
                .get_mut(&conn)
                .ok_or(ServerError::UnknownConnection(conn))?;
            session.last_seen = Instant::now();
            (session.state, session.client_id)
        };
        let bytes = frame_bytes(frame);

        match (state, frame.ftype) {
            // Step 2: auth ticket -> verify -> Authenticating.
            (ClientState::Connecting, FrameType::AuthTicket) => {
                let msg = if bytes.get(3..7) == Some(&[0, 0, 0xff, 0xff]) && bytes.len() > 7 {
                    // Stock client has no assigned ID yet; this connection owns client_id.
                    AuthTicketMsg { client_id, ticket: bytes[7..].to_vec() }
                } else {
                    AuthTicketMsg::decode(&bytes)?
                };
                if msg.client_id != client_id {
                    return Err(ServerError::ClientIdMismatch {
                        expected: client_id,
                        got: msg.client_id,
                    });
                }
                let steam_id = match self.verifier.verify(self.sessions[&conn].peer_id, &msg.ticket) {
                    Ok(id) => id,
                    Err(err) => {
                        self.handle_disconnect(conn);
                        return Err(err);
                    }
                };
                let now_unix = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let ban_reason = self
                    .banlist
                    .is_banned(steam_id, now_unix)
                    .map(|b| b.reason.clone());
                if let Some(reason) = ban_reason {
                    log::warn!(
                        "rejecting connection {conn} for banned SteamID {steam_id}: {reason}"
                    );
                    self.handle_disconnect(conn);
                    return Err(ServerError::Banned(reason));
                }
                let session = self.sessions.get_mut(&conn).expect("session exists");
                session.steam_id = Some(steam_id);
                session.state = ClientState::Authenticating;
                log::info!("client {client_id}: auth session started");
                Ok(Vec::new())
            }

            // Step 3: mod verification -> ack -> Handshake.
            (ClientState::Authenticating, FrameType::Handshake) => match bytes.get(9) {
                Some(0x02) => {
                    ModVerifyMsg::decode(&bytes)?;
                    log::info!("client {client_id}: received mod list");
                    self.sessions.get_mut(&conn).expect("session exists").state =
                        ClientState::Handshake;
                    let seed = 0xf4cb_faa1; // bytes 8..12 of the opening 0x0c frame
                    Ok(crate::world::initial_frames(client_id, seed, self.steam_id))
                }
                // The client sends this when it tears the connection down.
                Some(0x0a) => Ok(Vec::new()),
                other => Err(ServerError::UnexpectedSubtype {
                    got: other.copied(),
                }),
            },

            // Final `0x01`/`0x06` ack: still Handshake until the spawn arrives.
            (ClientState::Handshake, FrameType::Handshake) => match bytes.get(9) {
                Some(0x06) => {
                    let ack = HandshakeAckMsg::decode(&bytes)?;
                    if ack.client_id != client_id {
                        return Err(ServerError::ClientIdMismatch {
                            expected: client_id,
                            got: ack.client_id,
                        });
                    }
                    log::debug!("client {client_id} completed handshake");
                    Ok(Vec::new())
                }
                // Disconnect notice, same as during authentication.
                Some(0x0a) => Ok(Vec::new()),
                other => Err(ServerError::UnexpectedSubtype {
                    got: other.copied(),
                }),
            },

            // Step 4: player spawn -> Active.
            (ClientState::Handshake, FrameType::EntitySetup) => {
                // The stock client announces its entity with a 26-byte `0x02`
                // create (class token + mp_id); older captures also carry a
                // 73-byte form with the player name.
                if bytes.len() < 22 {
                    return Err(ServerError::UnexpectedFrame {
                        state: ClientState::Handshake,
                        ftype: FrameType::EntitySetup,
                    });
                }
                let name = PlayerSpawnMsg::decode(&bytes)
                    .ok()
                    .map(|m| m.player_name);
                let others: Vec<u32> = self
                    .sessions
                    .iter()
                    .filter(|(c, s)| **c != conn && s.state == ClientState::Active)
                    .map(|(c, _)| *c)
                    .collect();
                let session = self.sessions.get_mut(&conn).expect("session exists");
                session.state = ClientState::Active;
                session.name = name.clone();
                session.spawn = Some(bytes.clone());
                log::info!("[MP] [Chat] {} connected", name.as_deref().unwrap_or("player"));

                // Existing players learn about this one from the relayed create
                // the client sent; synthesising one is rejected as "type 1".
                let _ = others;
                // Confirm the client's own entity so it finishes spawning.
                Ok(vec![crate::world::confirm_spawn(&bytes)])
            }

            // Steady state: telemetry is accepted; routing lands in a later task.
            (ClientState::Active, FrameType::Telemetry) => Ok(Vec::new()),
            // Late handshake acks after the world is running.
            (ClientState::Active, FrameType::Handshake) => Ok(Vec::new()),
            // Anything else from an active client is state we do not model yet.
            (ClientState::Active, _) => Ok(Vec::new()),

            (ClientState::Disconnected, _) => Err(ServerError::UnexpectedFrame {
                state,
                ftype: frame.ftype,
            }),

            _ => Err(ServerError::UnexpectedFrame {
                state,
                ftype: frame.ftype,
            }),
        }
    }

    fn alloc_client_id(&mut self) -> u16 {
        loop {
            let id = self.next_client_id;
            self.next_client_id = if id == u16::MAX {
                FIRST_CLIENT_ID
            } else {
                id + 1
            };
            if !self.sessions.values().any(|s| s.client_id == id) {
                return id;
            }
        }
    }

    /// Run the callback pump and inbound-message loop until the process exits.
    pub async fn run(mut self, steam: Arc<SteamServer>) -> Result<(), ServerError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        while steam.steam_id() as u32 == 0 {
            if tokio::time::Instant::now() >= deadline {
                return Err(ServerError::Socket("Steam logon timed out before listener setup".into()));
            }
            steam.run_callbacks();
            tokio::time::sleep(TICK).await;
        }
        let listen_conn = steam
            .sockets()
            .create_listen_socket_p2p(self.config.virtual_port as i32);
        let listen_query = steam
            .sockets()
            .create_listen_socket_p2p((self.config.virtual_port + 1) as i32);
        if listen_conn == 0 {
            return Err(ServerError::Socket("failed to create listen socket".into()));
        }
        if listen_query == 0 {
            return Err(ServerError::Socket("failed to create query listen socket".into()));
        }
        log::info!(
            "{} listening on virtual ports {} and {}",
            self.config.name,
            self.config.virtual_port,
            self.config.virtual_port + 1
        );
        log::info!(
            "Session Search ID: {}/{}",
            steam.steam_id(),
            self.config.virtual_port + 1
        );

        self.steam_id = steam.steam_id();

        let start_time = Instant::now();
        let mut tick = tokio::time::interval(TICK);
        let mut clock_sync_interval = tokio::time::interval(Duration::from_millis(100));
        // The client drops a session after ~15 s without one of these.
        let mut keepalive_interval = tokio::time::interval(Duration::from_millis(2500));
        let mut last_logged_steamid = 0u64;
        let mut queries = std::collections::HashSet::new();

        loop {
            tokio::select! {
                _ = tick.tick() => {
                    steam.run_callbacks();

                    let cur_sid = steam.steam_id();
                    if cur_sid != last_logged_steamid && (cur_sid & 0xffffffff) != 0 {
                        last_logged_steamid = cur_sid;
                        log::info!(
                            "Steam GameServer authenticated! Real Session Search ID: {}/{}",
                            cur_sid,
                            self.config.virtual_port + 1
                        );
                    }

                    for ev in steam.sockets().poll_connection_events() {
                        match ev {
                            convoy_steam::sockets::ConnectionEvent::Connecting(conn, listener) => {
                                log::info!("incoming connection request: {conn}");
                                if let Err(e) = steam.sockets().accept_connection(conn) {
                                    log::warn!("failed to accept connection {conn}: {e}");
                                    continue;
                                }
                                steam.sockets().set_connection_poll_group(conn);
                                if listener == listen_query {
                                    queries.insert(conn);
                                    continue;
                                }
                                let challenge = self.handle_connection(conn);
                                let client_id = self.session(conn).map_or(0, |s| s.client_id);
                                let opening = std::iter::once(challenge)
                                    .chain(setup_frames(client_id))
                                    .chain(std::iter::once(mod_request(client_id, cur_sid)));
                                for packet in opening {
                                    if let Err(e) = send(steam.sockets(), conn, &packet, true) {
                                        log::warn!("failed to send opening packet to {conn}: {e}");
                                    }
                                }
                            }
                            convoy_steam::sockets::ConnectionEvent::Connected(conn) if queries.contains(&conn) => {
                                let info = session_listing(&self.config, cur_sid);
                                if let Err(e) = send(steam.sockets(), conn, &info, true) {
                                    log::warn!("failed to send session listing: {e}");
                                }
                            }
                            convoy_steam::sockets::ConnectionEvent::Closed(conn) => {
                                queries.remove(&conn);
                                log::info!("connection {conn} closed by peer");
                                self.handle_disconnect(conn);
                                steam.sockets().close(conn);
                            }
                            _ => {}
                        }
                    }

                    for msg in steam.sockets().poll_incoming() {
                        if queries.contains(&msg.conn) { continue; }
                        log::info!("inbound {} | {}", msg.conn, hex(&msg.payload));
                        // Relay world traffic from active clients verbatim; the
                        // client expects another player's create exactly as that
                        // player sent it.
                        let relay = self
                            .sessions
                            .get(&msg.conn)
                            .is_some_and(|s| s.state == ClientState::Active)
                            && matches!(msg.payload.get(2), Some(0x02) | Some(0x04) | Some(0x06));
                        if relay {
                            let targets: Vec<u32> = self
                                .sessions
                                .iter()
                                .filter(|(c, s)| **c != msg.conn && s.state == ClientState::Active)
                                .map(|(c, _)| *c)
                                .collect();
                            for target in targets {
                                self.pending.push((target, msg.payload.to_vec()));
                            }
                        }
                        if !self.sessions.contains_key(&msg.conn) {
                            steam.sockets().set_connection_poll_group(msg.conn);
                            let challenge = self.handle_connection(msg.conn);
                            if let Err(e) = send(steam.sockets(), msg.conn, &challenge, true) { log::warn!("send to {} failed: {e}", msg.conn); }
                        }
                        if let Some(session) = self.sessions.get_mut(&msg.conn) {
                            session.peer_id = msg.peer_id;
                        }
                        if msg.payload.is_empty() {
                            continue;
                        }

                        // Broadcast telemetry (0x04) or vehicle updates (0x02) to other active clients
                        if let Some(session) = self.sessions.get(&msg.conn) {
                            if session.state == ClientState::Active {
                                let outbound = broadcast_telemetry(&self.sessions, msg.conn, msg.payload.clone());
                                for out in outbound {
                                    if let Err(e) = send(steam.sockets(), out.conn, &out.payload, out.reliable) { log::warn!("send to {} failed: {e}", out.conn); }
                                }
                            }
                        }

                        match self.handle_packet(msg.conn, &msg.payload) {
                            Ok(packets) => {
                                for packet in packets {
                                    if let Err(e) = send(steam.sockets(), msg.conn, &packet, true) { log::warn!("send to {} failed: {e}", msg.conn); }
                                }
                            }
                            Err(ServerError::UnknownConnection(_)) => {}
                            Err(err) => {
                                log::warn!("dropping connection {}: {err}", msg.conn);
                                steam.sockets().close(msg.conn);
                                if let Some(session) = self.handle_disconnect(msg.conn) {
                                    if let Some(steam_id) = session.steam_id {
                                        steam.end_auth_session(steam_id);
                                    }
                                }
                            }
                        }
                    }

                    for (target, frame) in std::mem::take(&mut self.pending) {
                        if let Err(e) = send(steam.sockets(), target, &frame, true) {
                            log::warn!("send to {target} failed: {e}");
                        }
                    }

                    for (conn, session) in self.sweep_stale(SESSION_TIMEOUT) {
                        steam.sockets().close(conn);
                        if let Some(session) = session {
                            if let Some(steam_id) = session.steam_id {
                                steam.end_auth_session(steam_id);
                            }
                        }
                    }
                }
                _ = keepalive_interval.tick() => {
                    for (conn, session) in self.sessions.iter() {
                        if session.state != ClientState::Active {
                            continue;
                        }
                        if let Err(e) = send(steam.sockets(), *conn, &crate::world::keepalive(), true) {
                            log::warn!("keepalive to {conn} failed: {e}");
                        }
                    }
                }
                _ = clock_sync_interval.tick() => {
                    let micros = start_time.elapsed().as_micros() as u64;
                    let outbound = broadcast_clock_tick(&self.sessions, micros);
                    for out in outbound {
                        if let Err(e) = send(steam.sockets(), out.conn, &out.payload, out.reliable) { log::warn!("send to {} failed: {e}", out.conn); }
                    }
                }
            }
        }
    }
}

// Stock ETS2 no-mod listing. Opaque fields retain their captured values.
fn session_listing(config: &ServerConfig, steam_id: u64) -> Vec<u8> {
    let mut out = vec![1, 0, 0, 0, 3];
    for field in [&config.name, &config.map_name] {
        out.extend_from_slice(field.as_bytes());
        out.push(0);
    }
    out.extend_from_slice(&config.max_players.to_le_bytes());
    out.extend_from_slice(&0x70a6e0eaf6f6370fu64.to_le_bytes());
    out.extend_from_slice(&steam_id.to_le_bytes());
    out.extend_from_slice(&config.virtual_port.to_le_bytes());
    out.extend_from_slice(&(config.virtual_port + 1).to_le_bytes());
    out.extend_from_slice(&[75, 0, 0]);
    out
}

/// Step 1 packet: `0x0b` with the assigned id and an empty ticket body.
fn challenge(client_id: u16) -> Vec<u8> {
    let mut out = AuthTicketMsg {
        client_id,
        ticket: Vec::new(),
    }
    .encode();
    // The official server writes `01 00` here; the encoder emits `00 00`.
    out[5] = 0x01;
    out
}

/// The three opaque setup frames the official server sends on connect.
#[allow(unused_variables)]
fn setup_frames(client_id: u16) -> Vec<Vec<u8>> {
    // Directed form, confirmed by a client-side capture of a real join.
    let frames = vec![
        [&[0x00, 0x00, 0x0c, 0xf7, 0xff, 0x01, 0x00, 0x02][..], &[0xa1, 0xfa, 0xcb, 0xf4, 0, 0, 0, 0]].concat(),
        [&[0x00, 0x00, 0x07, 0xf7, 0xff, 0x01, 0x00, 0x02][..], &[0x50, 0x6c, 0x2f, 0x00, 0, 0, 0, 0]].concat(),
        [&[0x00, 0x00, 0x07, 0xf7, 0xff, 0x01, 0x00, 0x03][..], &[0x00, 0xd4, 0xa0, 0x64, 0x08, 0, 0, 0]].concat(),
    ];
    frames
}

/// Mod request sent with the challenge, matching the official server's order.
///
/// `00 00 01 <cid> 00 00 <cid> 01 4b 00*7 <8-byte session token>`
fn mod_request(client_id: u16, steam_id: u64) -> Vec<u8> {
    let mut out = vec![0x00, 0x00, 0x01];
    out.extend_from_slice(&client_id.to_le_bytes());
    out.extend_from_slice(&[0x00, 0x00]);
    out.extend_from_slice(&client_id.to_le_bytes());
    out.push(0x01);
    // 0x6a in the reference capture; the client answers it with its full
    // 139-byte mod list, where 0x4b only produced an 18-byte stub.
    out.push(0x6a);
    out.extend_from_slice(&[0u8; 7]);
    // The client compares bytes 18..26 with this derived token.
    out.extend_from_slice(&crate::token::server2client(steam_id));
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Rebuild the complete frame (`00 00 <type>` header included) for the payload
/// decoders, which expect the header.
fn frame_bytes(frame: &RawFrame) -> Vec<u8> {
    let mut out = Vec::with_capacity(3 + frame.payload.len());
    out.extend_from_slice(&[0x00, 0x00, frame.ftype.as_u8()]);
    out.extend_from_slice(&frame.payload);
    out
}

fn send(sockets: &SteamSockets, conn: u32, data: &[u8], reliable: bool) -> Result<(), ServerError> {
    sockets.send(conn, data, reliable)?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("unknown client connection {0}")]
    UnknownConnection(u32),
    #[error("client state {state:?} cannot accept {ftype:?}")]
    UnexpectedFrame {
        state: ClientState,
        ftype: FrameType,
    },
    #[error("unexpected handshake subtype {got:?}")]
    UnexpectedSubtype { got: Option<u8> },
    #[error("client id mismatch: expected {expected}, got {got}")]
    ClientIdMismatch { expected: u16, got: u16 },
    #[error("protocol error: {0}")]
    Proto(#[from] convoy_proto::ProtoError),
    #[error("ticket verification failed")]
    AuthFailed,
    #[error("steam error: {0}")]
    Steam(#[from] SteamError),
    #[error("user is banned: {0}")]
    Banned(String),
    #[error("socket error: {0}")]
    Socket(String),
}

#[cfg(test)]
mod opening_tests {
    use super::*;

    #[test]
    fn mod_request_matches_the_official_layout() {
        let frame = mod_request(10, 85_568_392_937_043_937);
        assert_eq!(frame.len(), 26);
        assert_eq!(&frame[..11], &[0, 0, 1, 10, 0, 0, 0, 10, 0, 1, 0x6a]);
        assert_eq!(&frame[11..18], &[0; 7]);
        assert_eq!(&frame[18..], &crate::token::server2client(85_568_392_937_043_937));
    }
}
