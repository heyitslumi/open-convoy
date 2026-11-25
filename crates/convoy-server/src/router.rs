//! Zero-copy packet routing and periodic clock synchronization.

use std::collections::HashMap;
use bytes::Bytes;
use convoy_proto::messages::ClockSyncMsg;
use crate::client::{ClientSession, ClientState};

/// Outbound message targeted to a specific connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessage {
    pub conn: u32,
    pub payload: Bytes,
    pub reliable: bool,
}

/// Broadcast a telemetry packet (type 0x04) or vehicle update (type 0x02) to all
/// other `Active` sessions, excluding the sender connection.
///
/// Uses unreliable delivery for high-frequency type 0x04 telemetry (`k_nSteamNetworkingSend_Unreliable`),
/// and reliable delivery for type 0x02 vehicle updates.
pub fn broadcast_telemetry(
    sessions: &HashMap<u32, ClientSession>,
    sender_conn: u32,
    payload: Bytes,
) -> Vec<OutboundMessage> {
    if payload.is_empty() {
        return Vec::new();
    }

    // Determine frame type: either from 00 00 <type> or length-prefixed.
    // In convoy wire format, flat frames start with 00 00 <type>.
    // Length-prefixed frames start with <len_le: u16> <type>.
    // In either case, the type tag byte sits at offset 2 when length >= 3.
    let ftype = if payload.len() >= 3 {
        payload[2]
    } else {
        0
    };

    let reliable = ftype != 0x04;

    sessions
        .values()
        .filter(|s| s.state == ClientState::Active && s.conn != sender_conn)
        .map(|s| OutboundMessage {
            conn: s.conn,
            payload: payload.clone(),
            reliable,
        })
        .collect()
}

/// Construct and broadcast Type 0x07 clock sync packets to all active sessions.
/// Each client receives a ClockSyncMsg tailored with its client_id and current `vtime`.
pub fn broadcast_clock_tick(
    sessions: &HashMap<u32, ClientSession>,
    micros: u64,
) -> Vec<OutboundMessage> {
    sessions
        .values()
        .filter(|s| s.state == ClientState::Active)
        .map(|s| {
            let msg = ClockSyncMsg { micros };
            OutboundMessage {
                conn: s.conn,
                payload: Bytes::from(msg.encode()),
                reliable: true,
            }
        })
        .collect()
}
