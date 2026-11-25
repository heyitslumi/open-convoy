//! World-state frames the stock client needs right after its mod list.
//!
//! The client refuses to enter its running state unless the server's
//! `mp_economy` singleton exists, so the initial burst spawns the static server
//! entities (each a `0x02` create), seeds them with `0x06` RPCs, and only then
//! sends the `0x01/0x05` connect ack. Order matters: the ack flips the client to
//! "connected" and it checks `mp_economy` on the next tick.
//!
//! Bytes 8..16 of a `0x02` frame are a base-38 class token, not an entity id;
//! the entity is identified by `mp_id` = (owner, net_id) at bytes 16..22.

/// `prism::token_t` for the server singletons, decoded from the client binary.
pub mod class {
    pub const MP_TRAFFIC: u64 = 0x0007_ee43_2d0d_09d7;
    pub const MP_WEATHER: u64 = 0x0010_aca3_a258_c78f;
    pub const JOURNEY_RE: u64 = 0x0009_409d_6e7f_b7a6;
    pub const MP_ECONOMY: u64 = 0x0014_e8d4_274d_137f;
    pub const PLAYER_GEN: u64 = 0x000e_5380_bb17_4da2;
    pub const MP_CHAT: u64 = 0x0000_0015_3e35_11df;
    pub const PLAYER_INFO: u64 = 0x0237_6334_5f54_45a2;
}

/// `mp_id` of a server-owned singleton: owner `1`, then the network id.
const SERVER_OWNER: u16 = 1;

fn spawn(client_id: u16, class: u64, net_id: u32) -> Vec<u8> {
    let mut out = vec![0x00, 0x00, 0x02];
    out.extend_from_slice(&client_id.to_le_bytes());
    out.extend_from_slice(&[0x01, 0x00, 0x03]);
    out.extend_from_slice(&class.to_le_bytes());
    out.extend_from_slice(&SERVER_OWNER.to_le_bytes());
    out.extend_from_slice(&net_id.to_le_bytes());
    out.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    out.extend_from_slice(&[0x00, 0x00, 0x04, 0xf7, 0xff, 0x01, 0x00, 0x01, 0x00]);
    out.extend_from_slice(&net_id.to_le_bytes());
    out.extend_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x00]);
    out
}

/// `0x06` RPC addressed to one singleton.
fn rpc(client_id: u16, op: u8, net_id: u32, body: &[u8]) -> Vec<u8> {
    let mut out = vec![0x00, 0x00, 0x06];
    out.extend_from_slice(&client_id.to_le_bytes());
    out.extend_from_slice(&[0x01, 0x00, op, 0x00]);
    out.extend_from_slice(&SERVER_OWNER.to_le_bytes());
    out.extend_from_slice(&net_id.to_le_bytes());
    out.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x00]);
    out.extend_from_slice(body);
    out
}

/// 18-byte `00 00 01 00 00 00 00 <cid> 05 00*8`; must be the last frame.
pub fn connect_ack(client_id: u16) -> Vec<u8> {
    let mut out = vec![0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
    out.extend_from_slice(&client_id.to_le_bytes());
    out.push(0x05);
    out.extend_from_slice(&[0u8; 8]);
    out
}

/// Everything the client needs between its mod list and its own spawn.
pub fn initial_frames(client_id: u16, seed: u32, steam_id: u64) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();

    // mp_traffic + set_traffic_settings(100, 50, 50)
    frames.push(spawn(client_id, class::MP_TRAFFIC, 0x0b));
    frames.push(rpc(client_id, 0x0a, 0x0b, &[0x64, 0x00, 0x32, 0x00, 0x32, 0x00]));

    // mp_weather + init_remote_cycles
    frames.push(spawn(client_id, class::MP_WEATHER, 0x0c));
    frames.push(rpc(
        client_id,
        0x02,
        0x0c,
        &[
            0x80, 0x26, 0xb7, 0x6d, 0x16, 0x00, 0x00, 0x00, 0x00, 0x97, 0x5a, 0x83, 0x17, 0x00,
            0x00, 0x00, 0x40, 0x44, 0xca, 0x2f, 0x18, 0x00, 0x00, 0x00, 0x29, 0x5c, 0x8f, 0x3d,
            0x51, 0x32, 0x28, 0xbb, 0x00, 0x00, 0x00, 0x00,
        ],
    ));

    // journey_re
    frames.push(spawn(client_id, class::JOURNEY_RE, 0x0d));

    // mp_economy + init_dynamic_companies_seed
    frames.push(spawn(client_id, class::MP_ECONOMY, 0x0e));
    frames.push(rpc(client_id, 0x03, 0x0e, &seed.to_le_bytes()));

    // player_gen ("mp_server_player")
    frames.push(spawn(client_id, class::PLAYER_GEN, 0x01));

    // mp_chat
    frames.push(spawn(client_id, class::MP_CHAT, 0x0f));

    // Bind the server SteamID to (1, 1).
    let mut bind = vec![0x00, 0x00, 0x01];
    bind.extend_from_slice(&client_id.to_le_bytes());
    bind.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x07]);
    bind.extend_from_slice(&steam_id.to_le_bytes());
    frames.push(bind);

    frames.push(connect_ack(client_id));
    frames
}

/// Keepalive the client expects while idle (subtype `0x14`).
pub fn keepalive() -> Vec<u8> {
    let mut out = vec![0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14];
    out.extend_from_slice(&[0u8; 8]);
    out
}


/// Confirm a client's own entity creation.
///
/// The client sends `0x02` with op `0x00` (bytes 7) carrying a class token at
/// 8..16 and its `mp_id` at 16..22; the server answers with op `0x01` and the
/// same ids. Bytes 5..7 must be `00 00` here or the client flags a type-1 error.
pub fn confirm_spawn(request: &[u8]) -> Vec<u8> {
    let token = request.get(8..16).unwrap_or(&[0u8; 8]);
    let owner = request.get(16..18).unwrap_or(&[0u8; 2]);
    let net = request.get(18..22).unwrap_or(&[0u8; 4]);
    let mut out = vec![0x00, 0x00, 0x02, 0xf7, 0xff, 0x00, 0x00, 0x01];
    out.extend_from_slice(token);
    out.extend_from_slice(owner);
    out.extend_from_slice(net);
    out.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    out
}


/// Server-authored create for another player, sent to one viewer.
///
/// Byte-for-byte layout of a 67-byte capture: dest = viewer, sender = server,
/// op `0x03`, class token `player_info`, mp_id = (player, 1), then an embedded
/// `0x04` block whose sender is the server and whose tail carries the player's
/// initial state.
pub fn remote_player(viewer: u16, player: u16) -> Vec<u8> {
    // Tail from the capture; player-specific state we do not model yet.
    const TAIL: [u8; 22] = [
        0xdd, 0x27, 0x44, 0x47, 0xf3, 0x5f, 0x64, 0xc7, 0x0a, 0x69, 0x75, 0x6c, 0x79, 0x61,
        0x6e, 0x6f, 0x61, 0x62, 0x63, 0x63, 0x00, 0x2a,
    ];
    let mut out = vec![0x00, 0x00, 0x02];
    out.extend_from_slice(&viewer.to_le_bytes());
    out.extend_from_slice(&[0x01, 0x00, 0x03]);
    out.extend_from_slice(&class::PLAYER_INFO.to_le_bytes());
    out.extend_from_slice(&player.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    out.extend_from_slice(&[0x00, 0x00, 0x04, 0xf7, 0xff]);
    out.extend_from_slice(&[0x01, 0x00]);
    out.extend_from_slice(&player.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x00]);
    out.extend_from_slice(&TAIL);
    out
}

/// Keepalive the client expects while idle (subtype `0x14`).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_ack_is_last_and_economy_is_spawned_before_it() {
        let frames = initial_frames(10, 0xf4cb_faa1, 85_568_392_937_044_635);
        assert_eq!(frames.last().unwrap(), &connect_ack(10));
        let economy = frames
            .iter()
            .position(|f| f.len() == 45 && f[8..16] == class::MP_ECONOMY.to_le_bytes())
            .expect("mp_economy spawn");
        let ack = frames.len() - 1;
        assert!(economy < ack);
    }

    #[test]
    fn remote_player_frame_is_long_enough_and_addressed() {
        let f = remote_player(10, 11);
        assert_eq!(f.len(), 67, "capture-exact create length");
        assert_eq!(&f[3..5], &10u16.to_le_bytes());
        assert_eq!(&f[5..7], &[0x01, 0x00]);
        assert_eq!(f[7], 0x03);
        assert_eq!(&f[8..16], &class::PLAYER_INFO.to_le_bytes());
        assert_eq!(&f[16..18], &11u16.to_le_bytes());
    }

    #[test]
    fn spawn_and_rpc_shapes() {
        let s = spawn(10, class::MP_ECONOMY, 0x0e);
        assert_eq!(s.len(), 45);
        assert_eq!(&s[3..5], &[10, 0]);
        assert_eq!(&s[16..22], &[1, 0, 0x0e, 0, 0, 0]);

        let r = rpc(10, 0x03, 0x0e, &0xf4cb_faa1u32.to_le_bytes());
        assert_eq!(r.len(), 24);
        assert_eq!(&r[9..11], &[1, 0]);
        assert_eq!(&r[20..24], &[0xa1, 0xfa, 0xcb, 0xf4]);
    }
}