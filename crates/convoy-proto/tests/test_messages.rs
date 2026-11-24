//! Wire-format tests for convoy message payloads and RPC structures.
//!
//! Byte fixtures come from `scripts/convoy_bot.py` (the hand-built packets that
//! a real ETS2 server accepted) and from `captures/convoy-capture-full.log`.

use convoy_proto::messages::{
    AuthTicketMsg, ClockSyncMsg, HandshakeAckMsg, ModVerifyMsg, PlayerSpawnMsg, VehicleTelemetryMsg,
};
use convoy_proto::rpc::{TrafficRpc, WeatherRpc};

fn hex(s: &str) -> Vec<u8> {
    assert_eq!(s.len() % 2, 0, "hex fixture has odd length");
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

/// `convoy_bot.py`: `b"\x00\x00\x0b" + pack("<H", cid) + b"\x00\x00" + ticket`.
#[test]
fn auth_ticket_matches_bot_wire_format() {
    let msg = AuthTicketMsg {
        client_id: 10,
        ticket: hex("deadbeef"),
    };
    assert_eq!(msg.encode(), hex("00000b0a000000deadbeef"));

    let back = AuthTicketMsg::decode(&msg.encode()).unwrap();
    assert_eq!(back, msg);
}

#[test]
fn auth_ticket_roundtrips_empty_and_long_tickets() {
    for ticket in [vec![], vec![0u8; 1024], (0u8..=255).collect::<Vec<_>>()] {
        let msg = AuthTicketMsg {
            client_id: 0xffff,
            ticket,
        };
        assert_eq!(AuthTicketMsg::decode(&msg.encode()).unwrap(), msg);
    }
}

/// Captured packet `00000100000a000a00060000000000000000` (18 bytes, cid 10).
#[test]
fn handshake_ack_matches_captured_packet() {
    let captured = hex("00000100000a000a00060000000000000000");
    let msg = HandshakeAckMsg { client_id: 10 };
    assert_eq!(msg.encode(), captured);
    assert_eq!(HandshakeAckMsg::decode(&captured).unwrap(), msg);
}

/// `convoy_bot.py` msg01 with cid 10: the 128 bytes a real server accepted.
///
/// The captured packet is 139 bytes; the capture log truncates its hex dump at
/// 128 bytes (`..(11 more)`), which is exactly the prefix asserted here. The
/// trailing 11 bytes are opaque mod-hash data and ride in `payload`.
#[test]
fn mod_verify_roundtrips_captured_packet() {
    let captured = hex(concat!(
        "00000100000a000a000200000000000000003323ffff0f7bfeff0518e8f700000000",
        "00000000000000003645333430423943464642333741393839434135343445364242",
        "37383041324337383930314433464233333733383736383531314133303631374146",
        "4130314400c6c14336b4789dede1770301000030010f37f6f6ea",
    ));
    assert_eq!(captured.len(), 128);

    let msg = ModVerifyMsg::decode(&captured).unwrap();
    assert_eq!(msg.client_id, 10);
    assert_eq!(msg.cookie, 0xfffe_7b0f_ffff_2333);
    assert_eq!(msg.encode(), captured);

    // The opaque tail is length-agnostic: a 139-byte packet roundtrips too.
    let mut longer = msg.clone();
    longer.payload.extend_from_slice(&[0xaa; 11]);
    assert_eq!(ModVerifyMsg::decode(&longer.encode()).unwrap(), longer);
}

/// Captured player spawn (61 bytes) for client 10, name "Lumi".
#[test]
fn player_spawn_roundtrips_captured_packet() {
    let captured = hex(concat!(
        "000002f7ff0a0000a245545f346337020a0001000000ffffffff000004f7ff0a000a0001",
        "000000008000800000cdcd5cc7f778c2c7044c756d69000000",
    ));
    assert_eq!(captured.len(), 61);

    let msg = PlayerSpawnMsg {
        client_id: 10,
        entity_id: u32::MAX,
        player_name: "Lumi".to_string(),
        pos: (-56525.80078125, -99569.9296875),
    };
    assert_eq!(msg.encode(), captured);
    assert_eq!(PlayerSpawnMsg::decode(&captured).unwrap(), msg);
}

#[test]
fn player_spawn_embeds_player_name_and_pads_to_four_bytes() {
    for name in ["A", "Lumi", "GhostTruck", "a-player-with-a-longer-name"] {
        let msg = PlayerSpawnMsg {
            client_id: 7,
            entity_id: 0x57,
            player_name: name.to_string(),
            pos: (1.5, -2.5),
        };
        let bytes = msg.encode();
        let needle = name.as_bytes();
        let at = bytes
            .windows(needle.len())
            .position(|w| w == needle)
            .expect("player name must be embedded verbatim");
        assert_eq!(bytes[at - 1] as usize, needle.len(), "u8 length prefix");

        let block = 1 + needle.len() + (4 - ((1 + needle.len()) % 4));
        assert_eq!((bytes.len() - (at - 1)) % 4, 0, "name block is 4-aligned");
        assert!(block <= bytes.len() - (at - 1));

        assert_eq!(PlayerSpawnMsg::decode(&bytes).unwrap(), msg);
    }
}

/// `convoy_bot.py` `_send_telemetry`: 38-byte header, then pos/vel/quat.
#[test]
fn telemetry_matches_bot_wire_format() {
    let captured = hex(concat!(
        "000004f6ff0a000a002c00000031ff69fec701fa0011005a883d000000cef82cf7f060010001",
        "00005c42cdcc5441000066c33d0c304090c184c299ff35ffa7043840",
    ));

    let msg = VehicleTelemetryMsg::decode(&captured).unwrap();
    assert_eq!(msg.client_id, 10);
    assert_eq!(msg.pos, (55.0, 13.3, -230.0));
    assert_eq!(msg.encode(), captured);
}

#[test]
fn telemetry_roundtrips_position_velocity_and_quaternion() {
    let msg = VehicleTelemetryMsg {
        client_id: 3,
        pos: (-1234.5, 13.25, 98765.0),
        vel: (1.5, -0.25, 12.0),
        quat: [0.0, 0.70710677, 0.0, 0.70710677],
    };
    let bytes = msg.encode();
    assert_eq!(bytes.len(), 66);
    assert_eq!(&bytes[..5], &[0x00, 0x00, 0x04, 0xf6, 0xff]);

    let mut expected_pos = Vec::new();
    for c in [msg.pos.0, msg.pos.1, msg.pos.2] {
        expected_pos.extend_from_slice(&c.to_le_bytes());
    }
    assert_eq!(&bytes[38..50], &expected_pos[..]);

    let back = VehicleTelemetryMsg::decode(&bytes).unwrap();
    assert_eq!(back.client_id, msg.client_id);
    assert_eq!(back.pos, msg.pos);
    assert_eq!(back.vel, msg.vel);
    // Quaternion goes through the smallest-three codec, so compare the
    // decoded rotation rather than the raw floats.
    let q = convoy_proto::codec::CompressedRotation::new(back.quat);
    let expected = convoy_proto::codec::CompressedRotation::new(msg.quat);
    assert_eq!(q.encode(), expected.encode());
}

/// Captured clock tick `000007f7ff01000083507e1200000000` (16 bytes).
#[test]
fn clock_sync_matches_captured_packet() {
    let captured = hex("000007f7ff01000083507e1200000000");
    let msg = ClockSyncMsg {
        client_id: 0,
        vtime: 0x127e_5083,
    };
    assert_eq!(msg.encode(), captured);
    assert_eq!(ClockSyncMsg::decode(&captured).unwrap(), msg);
}

#[test]
fn truncated_or_mistyped_payloads_are_errors() {
    assert!(AuthTicketMsg::decode(&hex("00000b")).is_err());
    assert!(HandshakeAckMsg::decode(&hex("00000100000a00")).is_err());
    assert!(ModVerifyMsg::decode(&hex("00000100000a000a00020000")).is_err());
    assert!(PlayerSpawnMsg::decode(&hex("000002f7ff0a0000a24554")).is_err());
    assert!(VehicleTelemetryMsg::decode(&hex("000004f6ff0a000a00")).is_err());
    assert!(ClockSyncMsg::decode(&hex("000007f7ff01")).is_err());

    // Right length, wrong frame type.
    assert!(AuthTicketMsg::decode(&hex("00000c0a000000")).is_err());
}

#[test]
fn traffic_rpc_carries_all_variants() {
    let all = [
        TrafficRpc::RequestSpawnApproval {
            road_id: 7,
            item_id: 0x1234_5678_9abc_def0,
            flags: 3,
        },
        TrafficRpc::ConfirmSpawn {
            item_id: 42,
            flags: 1,
            variant: 0xdead_beef,
        },
        TrafficRpc::RequestTransferStart {
            from_id: 1,
            to_id: 2,
            road_id: 3,
        },
        TrafficRpc::ConfirmTransferApproval { id: 9, status: true },
        TrafficRpc::ConfirmVehicleTransfer { id: 4, road_id: 5 },
        TrafficRpc::EnableSpawning(false),
        TrafficRpc::RemoveTrafficVehicle(77),
        TrafficRpc::SetTrafficSettings {
            max_total: 100,
            max_player: 50,
            max_spawn: 10,
        },
        TrafficRpc::AddOccupantSingleTrackRoad {
            road_id: 11,
            item_id: 12,
        },
        TrafficRpc::RemoveOccupantSingleTrackRoad {
            road_id: 13,
            item_id: 14,
        },
        TrafficRpc::UpdateSingleTrackRoad {
            road_id: 15,
            state: 0x0102_0304_0506_0708,
        },
        TrafficRpc::RequestTransferStartPlaced {
            from_id: 16,
            to_id: 17,
            pos: (1.0, 2.0, 3.0),
        },
    ];

    for rpc in all {
        let copy = rpc.clone();
        assert_eq!(copy, rpc);
        assert!(!format!("{rpc:?}").is_empty());
    }

    assert!(matches!(
        TrafficRpc::EnableSpawning(true),
        TrafficRpc::EnableSpawning(true)
    ));
    assert_eq!(
        TrafficRpc::SetTrafficSettings {
            max_total: 100,
            max_player: 50,
            max_spawn: 10,
        },
        TrafficRpc::SetTrafficSettings {
            max_total: 100,
            max_player: 50,
            max_spawn: 10,
        }
    );
}

#[test]
fn weather_rpc_carries_all_variants() {
    let all = [
        WeatherRpc::InitRemoteCycles {
            t1: 1,
            t2: 2,
            t3: 3,
            rain_factor: 0.75,
            c1: 4,
            c2: 5,
        },
        WeatherRpc::ReinitRemoteCycles {
            t1: 6,
            t2: 7,
            t3: 8,
            rain_factor: 0.0,
        },
        WeatherRpc::RandomizeWeather {
            seed: 0xcafe_babe,
            force: true,
        },
        WeatherRpc::UpdateNextCycle(0x1234_5678),
    ];

    for rpc in all {
        let copy = rpc.clone();
        assert_eq!(copy, rpc);
        assert!(!format!("{rpc:?}").is_empty());
    }
}
