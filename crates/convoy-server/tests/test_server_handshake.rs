//! End-to-end state machine tests for the 4-step client handshake.
//!
//! Each test feeds the exact wire bytes a client would send into
//! [`ConvoyServer::handle_packet`] and checks the session state.

use convoy_proto::messages::{AuthTicketMsg, HandshakeAckMsg, ModVerifyMsg, PlayerSpawnMsg};
use convoy_server::{ClientState, ConvoyServer, ServerConfig, ServerError, TicketVerifier};

const STEAM_ID: u64 = 76561198000000000;

struct AcceptAll;

impl TicketVerifier for AcceptAll {
    fn verify(&self, _peer: Option<u64>, _ticket: &[u8]) -> Result<u64, ServerError> {
        Ok(STEAM_ID)
    }
}

struct RejectAll;

impl TicketVerifier for RejectAll {
    fn verify(&self, _peer: Option<u64>, _ticket: &[u8]) -> Result<u64, ServerError> {
        Err(ServerError::AuthFailed)
    }
}

fn server() -> ConvoyServer {
    ConvoyServer::new(ServerConfig::default(), Box::new(AcceptAll))
}

fn hex(s: &str) -> Vec<u8> {
    assert_eq!(s.len() % 2, 0, "hex fixture has odd length");
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

#[test]
fn handshake_walks_connecting_authenticating_handshake_active() {
    let mut srv = server();
    let conn = 7;

    // Step 1: new connection gets a challenge carrying its assigned client id.
    let challenge = srv.handle_connection(conn);
    assert_eq!(srv.session(conn).unwrap().state, ClientState::Connecting);
    let challenge = AuthTicketMsg::decode(&challenge).unwrap();
    assert_eq!(challenge.client_id, 10);
    assert!(challenge.ticket.is_empty());

    // Step 2: ticket reply -> Authenticating, Steam id recorded.
    let ticket = AuthTicketMsg {
        client_id: 10,
        ticket: vec![0xde, 0xad, 0xbe, 0xef],
    }
    .encode();
    assert!(srv.handle_packet(conn, &ticket).unwrap().is_empty());
    let session = srv.session(conn).unwrap();
    assert_eq!(session.state, ClientState::Authenticating);
    assert_eq!(session.steam_id, Some(STEAM_ID));

    // Step 3: mod verification -> Handshake plus the server ack packet.
    let mod_verify = ModVerifyMsg {
        client_id: 10,
        cookie: 0x1122_3344_5566_7788,
        payload: vec![0xaa, 0xbb],
    }
    .encode();
    let out = srv.handle_packet(conn, &mod_verify).unwrap();
    assert_eq!(srv.session(conn).unwrap().state, ClientState::Handshake);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0], hex("000001010000000a000004"));

    // Final `0x01`/`0x06` ack is accepted but does not spawn the player.
    let ack = HandshakeAckMsg { client_id: 10 }.encode();
    assert!(srv.handle_packet(conn, &ack).unwrap().is_empty());
    assert_eq!(srv.session(conn).unwrap().state, ClientState::Handshake);

    // Step 4: spawn -> Active, name recorded.
    let spawn = PlayerSpawnMsg {
        client_id: 10,
        entity_id: u32::MAX,
        player_name: "Roadrunner".into(),
        pos: (1.0, 2.0),
    }
    .encode();
    assert!(srv.handle_packet(conn, &spawn).unwrap().is_empty());
    let session = srv.session(conn).unwrap();
    assert_eq!(session.state, ClientState::Active);
    assert_eq!(session.name.as_deref(), Some("Roadrunner"));
    assert_eq!(session.steam_id, Some(STEAM_ID));
}

#[test]
fn client_ids_start_at_ten_and_stay_unique() {
    let mut srv = server();

    assert_eq!(
        AuthTicketMsg::decode(&srv.handle_connection(1))
            .unwrap()
            .client_id,
        10
    );
    assert_eq!(
        AuthTicketMsg::decode(&srv.handle_connection(2))
            .unwrap()
            .client_id,
        11
    );
    assert_eq!(
        AuthTicketMsg::decode(&srv.handle_connection(3))
            .unwrap()
            .client_id,
        12
    );

    // Re-issuing for the same connection must not burn a new id.
    assert_eq!(
        AuthTicketMsg::decode(&srv.handle_connection(2))
            .unwrap()
            .client_id,
        11
    );
}

#[test]
fn packet_from_unknown_connection_is_rejected() {
    let mut srv = server();
    let ticket = AuthTicketMsg {
        client_id: 10,
        ticket: vec![1],
    }
    .encode();

    assert!(matches!(
        srv.handle_packet(99, &ticket),
        Err(ServerError::UnknownConnection(99))
    ));
}

#[test]
fn out_of_order_spawn_is_rejected() {
    let mut srv = server();
    srv.handle_connection(1);

    let spawn = PlayerSpawnMsg {
        client_id: 10,
        entity_id: u32::MAX,
        player_name: "TooSoon".into(),
        pos: (0.0, 0.0),
    }
    .encode();

    assert!(matches!(
        srv.handle_packet(1, &spawn),
        Err(ServerError::UnexpectedFrame { .. })
    ));
    assert_eq!(srv.session(1).unwrap().state, ClientState::Connecting);
}

#[test]
fn failed_ticket_verification_drops_the_session() {
    let mut srv = ConvoyServer::new(ServerConfig::default(), Box::new(RejectAll));
    srv.handle_connection(1);

    let ticket = AuthTicketMsg {
        client_id: 10,
        ticket: vec![0xff],
    }
    .encode();

    assert!(matches!(
        srv.handle_packet(1, &ticket),
        Err(ServerError::AuthFailed)
    ));
    assert!(srv.session(1).is_none());
}

#[test]
fn disconnect_removes_the_session() {
    let mut srv = server();
    srv.handle_connection(1);
    assert_eq!(srv.session_count(), 1);

    srv.handle_disconnect(1);
    assert_eq!(srv.session_count(), 0);
    assert!(srv.session(1).is_none());

    // Disconnecting an unknown connection is a no-op.
    srv.handle_disconnect(1);
}

#[test]
fn stock_unassigned_ticket_header_still_requires_verification() {
    let packet = hex("00000b0000ffffdeadbeef");
    let mut accepted = server();
    accepted.handle_connection(1);
    accepted.handle_packet(1, &packet).unwrap();
    assert_eq!(accepted.session(1).unwrap().state, ClientState::Authenticating);
    let mut rejected = ConvoyServer::new(ServerConfig::default(), Box::new(RejectAll));
    rejected.handle_connection(1);
    assert!(matches!(rejected.handle_packet(1, &packet), Err(ServerError::AuthFailed)));
}

#[test]
fn stock_0x0a_reply_is_answered_with_the_captured_no_mod_ack() {
    let mut srv = server();
    srv.handle_connection(1);
    srv.handle_packet(1, &hex("00000b0000ffffdeadbeef")).unwrap();
    let stock = hex("0000010a0000000a000a00000000b4fd5bc7cd61c4c5");
    let out = srv.handle_packet(1, &stock).unwrap();
    assert_eq!(srv.session(1).unwrap().state, ClientState::Handshake);
    assert_eq!(
        out,
        vec![hex("000001010000000a000400000000000000003323ffff0f7bfeff0518e8f73323ffff0f7bfeff0518e8f7")]
    );
}
