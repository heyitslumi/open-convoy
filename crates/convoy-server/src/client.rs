//! Connection state for one client session.

use std::time::Instant;

/// Handshake progress of a single connection.
///
/// `Connecting -> Authenticating -> Handshake -> Active`, with any state able to
/// fall to `Disconnected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// Challenge (`0x0b`) sent, waiting for the auth ticket reply.
    Connecting,
    /// Ticket received, waiting for mod verification.
    Authenticating,
    /// Mod verification accepted, waiting for the final `0x01`/`0x06` ack.
    Handshake,
    /// Spawned; telemetry is streaming.
    Active,
    /// Session ended.
    Disconnected,
}

pub struct ClientSession {
    pub client_id: u16,
    pub conn: u32,
    pub(crate) peer_id: Option<u64>,
    pub steam_id: Option<u64>,
    pub name: Option<String>,
    pub state: ClientState,
    pub last_seen: Instant,
}

impl ClientSession {
    pub fn new(client_id: u16, conn: u32) -> Self {
        Self {
            client_id,
            conn,
            steam_id: None,
            peer_id: None,
            name: None,
            state: ClientState::Connecting,
            last_seen: Instant::now(),
        }
    }
}
