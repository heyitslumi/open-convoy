//! Integration tests for zero-copy telemetry forwarding and clock tick broadcasting.

use convoy_proto::messages::{
    AuthTicketMsg, ClockSyncMsg, HandshakeAckMsg, ModVerifyMsg, PlayerSpawnMsg,
    VehicleTelemetryMsg,
};
use convoy_server::router::{broadcast_clock_tick, broadcast_telemetry};
use convoy_server::{ClientState, ConvoyServer, ServerConfig, ServerError, TicketVerifier};

const STEAM_ID_A: u64 = 76561198000000001;
const STEAM_ID_B: u64 = 76561198000000002;

struct MockVerifier;

impl TicketVerifier for MockVerifier {
    fn verify(&self, _peer: Option<u64>, ticket: &[u8]) -> Result<u64, ServerError> {
        let steam_id = ticket
            .get(..8)
            .map(|b| u64::from_le_bytes(b.try_into().expect("8 bytes")))
            .unwrap_or(STEAM_ID_A);
        Ok(steam_id)
    }
}

fn create_server() -> ConvoyServer {
    ConvoyServer::new(ServerConfig::default(), Box::new(MockVerifier))
}

fn transition_to_active(srv: &mut ConvoyServer, conn: u32, steam_id: u64, name: &str) -> u16 {
    let challenge = srv.handle_connection(conn);
    let cid = AuthTicketMsg::decode(&challenge).unwrap().client_id;

    let ticket = AuthTicketMsg {
        client_id: cid,
        ticket: steam_id.to_le_bytes().to_vec(),
    }
    .encode();
    srv.handle_packet(conn, &ticket).unwrap();

    let mod_verify = ModVerifyMsg {
        client_id: cid,
        cookie: 0x1122_3344_5566_7788,
        payload: vec![0xaa, 0xbb],
    }
    .encode();
    srv.handle_packet(conn, &mod_verify).unwrap();

    let ack = HandshakeAckMsg { client_id: cid }.encode();
    srv.handle_packet(conn, &ack).unwrap();

    let spawn = PlayerSpawnMsg {
        client_id: cid,
        entity_id: u32::MAX,
        player_name: name.into(),
        pos: (10.0, 20.0),
    }
    .encode();
    srv.handle_packet(conn, &spawn).unwrap();

    assert_eq!(srv.session(conn).unwrap().state, ClientState::Active);
    cid
}

#[test]
fn telemetry_relayed_to_other_active_clients_only() {
    let mut srv = create_server();
    let conn_a = 100;
    let conn_b = 200;
    let conn_c = 300;

    let cid_a = transition_to_active(&mut srv, conn_a, STEAM_ID_A, "DriverA");
    let cid_b = transition_to_active(&mut srv, conn_b, STEAM_ID_B, "DriverB");
    assert_eq!(cid_a, 10);
    assert_eq!(cid_b, 11);

    // Client C connects but is not Active (Connecting)
    srv.handle_connection(conn_c);
    assert_eq!(srv.session(conn_c).unwrap().state, ClientState::Connecting);

    // Client A sends 0x04 VehicleTelemetryMsg
    let telem_msg = VehicleTelemetryMsg {
        client_id: cid_a,
        pos: (100.0, 50.0, -20.0),
        quat: [0.0, 0.0, 0.0, 1.0],
        vel: (15.0, 0.0, 0.0),
    };
    let telem_bytes = bytes::Bytes::from(telem_msg.encode());

    let outbound = broadcast_telemetry(srv.sessions(), conn_a, telem_bytes.clone());

    // Client B must receive forwarded frame
    assert_eq!(outbound.len(), 1);
    assert_eq!(outbound[0].conn, conn_b);
    assert_eq!(outbound[0].payload, telem_bytes);
    assert!(!outbound[0].reliable); // Unreliable for 0x04

    // Client A (sender) must NOT receive own frame back
    assert!(!outbound.iter().any(|msg| msg.conn == conn_a));

    // Client C (non-active) must NOT receive broadcast
    assert!(!outbound.iter().any(|msg| msg.conn == conn_c));
}

#[test]
fn vehicle_update_0x02_relayed_reliably() {
    let mut srv = create_server();
    let conn_a = 100;
    let conn_b = 200;

    let _cid_a = transition_to_active(&mut srv, conn_a, STEAM_ID_A, "DriverA");
    let _cid_b = transition_to_active(&mut srv, conn_b, STEAM_ID_B, "DriverB");

    // Client A sends a Type 0x02 vehicle update frame (e.g. trailer/vehicle state update)
    let payload = bytes::Bytes::from_static(&[0x00, 0x00, 0x02, 0x01, 0x02, 0x03]);
    let outbound = broadcast_telemetry(srv.sessions(), conn_a, payload.clone());

    assert_eq!(outbound.len(), 1);
    assert_eq!(outbound[0].conn, conn_b);
    assert_eq!(outbound[0].payload, payload);
    assert!(outbound[0].reliable); // Type 0x02 is reliable
}

#[test]
fn disconnected_client_does_not_receive_broadcast() {
    let mut srv = create_server();
    let conn_a = 100;
    let conn_b = 200;

    let cid_a = transition_to_active(&mut srv, conn_a, STEAM_ID_A, "DriverA");
    let _cid_b = transition_to_active(&mut srv, conn_b, STEAM_ID_B, "DriverB");

    // Client B disconnects
    srv.handle_disconnect(conn_b);

    let telem_msg = VehicleTelemetryMsg {
        client_id: cid_a,
        pos: (0.0, 0.0, 0.0),
        quat: [0.0, 0.0, 0.0, 1.0],
        vel: (0.0, 0.0, 0.0),
    };
    let telem_bytes = bytes::Bytes::from(telem_msg.encode());
    let outbound = broadcast_telemetry(srv.sessions(), conn_a, telem_bytes);

    assert!(outbound.is_empty());
}

#[test]
fn clock_tick_broadcast_reaches_all_active_clients() {
    let mut srv = create_server();
    let conn_a = 100;
    let conn_b = 200;
    let conn_c = 300;

    let cid_a = transition_to_active(&mut srv, conn_a, STEAM_ID_A, "DriverA");
    let cid_b = transition_to_active(&mut srv, conn_b, STEAM_ID_B, "DriverB");
    srv.handle_connection(conn_c); // non-active

    let micros = 123456u64;
    let outbound = broadcast_clock_tick(srv.sessions(), micros);

    // Reaches all active clients (conn_a and conn_b), but not conn_c
    assert_eq!(outbound.len(), 2);
    let conns: Vec<u32> = outbound.iter().map(|o| o.conn).collect();
    assert!(conns.contains(&conn_a));
    assert!(conns.contains(&conn_b));
    assert!(!conns.contains(&conn_c));

    for msg in outbound {
        assert!(msg.reliable);
        let decoded = ClockSyncMsg::decode(&msg.payload).unwrap();
        assert_eq!(decoded.micros, micros);
    }
}
