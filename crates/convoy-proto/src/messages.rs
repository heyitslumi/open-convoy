//! Typed payloads for the convoy wire protocol.
//!
//! Each `encode` emits a complete frame — the `00 00 <type>` header included —
//! so a caller can hand the bytes straight to `send_message`. Each `decode`
//! takes such a complete frame back to the struct.
//!
//! Byte layouts follow the packets a real ETS2 server accepted, as built by
//! `ets2-handoff/scripts/convoy_bot.py` and seen in
//! `ets2-handoff/captures/convoy-capture-full.log`.

use crate::codec::CompressedRotation;
use crate::frame::ProtoError;

fn malformed() -> ProtoError {
    ProtoError::MalformedMessage("malformed message payload")
}

fn u16_at(b: &[u8], at: usize) -> Result<u16, ProtoError> {
    let s = b.get(at..at + 2).ok_or_else(malformed)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, ProtoError> {
    let s = b.get(at..at + 4).ok_or_else(malformed)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, ProtoError> {
    let s = b.get(at..at + 8).ok_or_else(malformed)?;
    Ok(u64::from_le_bytes(s.try_into().expect("8 bytes")))
}

fn f32_at(b: &[u8], at: usize) -> Result<f32, ProtoError> {
    Ok(f32::from_bits(u32_at(b, at)?))
}

fn expect(b: &[u8], at: usize, magic: &[u8]) -> Result<(), ProtoError> {
    match b.get(at..at + magic.len()) {
        Some(s) if s == magic => Ok(()),
        _ => Err(malformed()),
    }
}

/// Auth ticket reply (type `0x0b`).
///
/// `00 00 0b <client_id u16 LE> 00 00 <ticket...>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthTicketMsg {
    pub client_id: u16,
    pub ticket: Vec<u8>,
}

impl AuthTicketMsg {
    const HEADER_LEN: usize = 7;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.ticket.len());
        out.extend_from_slice(&[0x00, 0x00, 0x0b]);
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&[0x00, 0x00]);
        out.extend_from_slice(&self.ticket);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Self, ProtoError> {
        expect(b, 0, &[0x00, 0x00, 0x0b])?;
        // Bytes 5..7 differ by direction (`01 00` server->client, `00 00` back)
        // and carry no information we use.
        let ticket = b.get(Self::HEADER_LEN..).ok_or_else(malformed)?;
        Ok(Self {
            client_id: u16_at(b, 3)?,
            ticket: ticket.to_vec(),
        })
    }
}

/// Mod verification handshake (type `0x01`, subtype `0x02`).
///
/// `00 00 01 00 00 <client_id x2> 02 00*8 <cookie u64 LE> <opaque mod hash...>`
///
/// `payload` holds everything after the cookie. Its contents are mod-manager
/// specific (hash string, flags, counts); the crate only needs to preserve them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModVerifyMsg {
    pub client_id: u16,
    pub cookie: u64,
    pub payload: Vec<u8>,
}

impl ModVerifyMsg {
    const COOKIE_AT: usize = 18;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::COOKIE_AT + 8 + self.payload.len());
        out.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x00]);
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        out.extend_from_slice(&self.cookie.to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Self, ProtoError> {
        expect(b, 0, &[0x00, 0x00, 0x01, 0x00, 0x00])?;
        expect(b, 9, &[0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])?;
        let payload = b.get(Self::COOKIE_AT + 8..).ok_or_else(malformed)?;
        Ok(Self {
            client_id: u16_at(b, 5)?,
            cookie: u64_at(b, Self::COOKIE_AT)?,
            payload: payload.to_vec(),
        })
    }
}

/// Final connection ACK (type `0x01`, subtype `0x06`), 18 bytes.
///
/// `00 00 01 00 00 <client_id x2> 06 00*8`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandshakeAckMsg {
    pub client_id: u16,
}

impl HandshakeAckMsg {
    pub const LEN: usize = 18;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::LEN);
        out.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x00]);
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&[0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Self, ProtoError> {
        expect(b, 0, &[0x00, 0x00, 0x01, 0x00, 0x00])?;
        expect(b, 9, &[0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00])?;
        if b.len() != Self::LEN {
            return Err(malformed());
        }
        Ok(Self {
            client_id: u16_at(b, 5)?,
        })
    }
}

/// Player declaration / spawn (type `0x02`).
///
/// ```text
/// 00 00 02 f7 ff  <cid>  00 a2
/// "ET_4c7" 02  <cid>  01 00 00 00  <entity_id u32 LE>  00 00
/// 04 f7 ff  <cid> <cid>  01 00 00 00 00 80 00 80 00 00
/// <x f32> <z f32>  <name len u8> <name> <pad to 4-byte block>
/// ```
///
/// `entity_id` occupies the u32 slot that every capture carries as
/// `ff ff ff ff` (no remote entity yet), so `u32::MAX` reproduces the wire form
/// exactly.
///
/// shortcut: the name block is padded with `4 - ((1 + len) % 4)` bytes, which
/// is the bot's rule and always leaves a 4-byte multiple, but emits 4 pad bytes
/// when the name is already aligned. Upgrade if a capture shows 0.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSpawnMsg {
    pub client_id: u16,
    pub entity_id: u32,
    pub player_name: String,
    pub pos: (f32, f32),
}

impl PlayerSpawnMsg {
    /// Bytes before the name length: everything up to and including `pos`.
    const FIXED_LEN: usize = 53;

    const fn pad_for(name_len: usize) -> usize {
        4 - ((1 + name_len) % 4)
    }

    pub fn encode(&self) -> Vec<u8> {
        let name = self.player_name.as_bytes();
        assert!(
            name.len() <= u8::MAX as usize,
            "player name longer than the u8 length prefix: {} bytes",
            name.len()
        );
        let pad = Self::pad_for(name.len());
        let mut out = Vec::with_capacity(Self::FIXED_LEN + 1 + name.len() + pad);

        out.extend_from_slice(&[0x00, 0x00, 0x02, 0xf7, 0xff]);
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&[0x00, 0xa2]);
        out.extend_from_slice(b"ET_4c7");
        out.push(0x02);
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]);
        out.extend_from_slice(&self.entity_id.to_le_bytes());
        out.extend_from_slice(&[0x00, 0x00]);
        out.extend_from_slice(&[0x04, 0xf7, 0xff]);
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x80, 0x00, 0x00]);
        out.extend_from_slice(&self.pos.0.to_le_bytes());
        out.extend_from_slice(&self.pos.1.to_le_bytes());
        out.push(name.len() as u8);
        out.extend_from_slice(name);
        out.resize(out.len() + pad, 0x00);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Self, ProtoError> {
        expect(b, 0, &[0x00, 0x00, 0x02, 0xf7, 0xff])?;
        let client_id = u16_at(b, 5)?;
        expect(b, 7, &[0x00, 0xa2])?;
        expect(b, 9, b"ET_4c7")?;
        expect(b, 15, &[0x02])?;
        if u16_at(b, 16)? != client_id {
            return Err(malformed());
        }
        expect(b, 18, &[0x01, 0x00, 0x00, 0x00])?;
        let entity_id = u32_at(b, 22)?;
        expect(b, 26, &[0x00, 0x00, 0x04, 0xf7, 0xff])?;
        if u16_at(b, 31)? != client_id || u16_at(b, 33)? != client_id {
            return Err(malformed());
        }
        expect(
            b,
            35,
            &[0x01, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x80, 0x00, 0x00],
        )?;
        let pos = (f32_at(b, 45)?, f32_at(b, 49)?);

        let name_len = *b.get(Self::FIXED_LEN).ok_or_else(malformed)? as usize;
        let name_at = Self::FIXED_LEN + 1;
        let name = b.get(name_at..name_at + name_len).ok_or_else(malformed)?;
        let end = name_at + name_len + Self::pad_for(name_len);
        if b.len() != end {
            return Err(malformed());
        }
        let player_name = String::from_utf8(name.to_vec()).map_err(|_| malformed())?;

        Ok(Self {
            client_id,
            entity_id,
            player_name,
            pos,
        })
    }
}

/// 10 Hz vehicle state (type `0x04`).
///
/// `00 00 04 f6 ff <client_id x2> <29-byte constant> <pos 3xf32> <vel 3xf32> <quat u32>`
///
/// 66 bytes total; `pos` starts at offset 38.
///
/// shortcut: real `0x04` traffic is a delta-encoded variable-length stream; this
/// models the fixed layout the bot sends and the server accepts. Upgrade when
/// delta decoding is reverse-engineered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VehicleTelemetryMsg {
    pub client_id: u16,
    pub pos: (f32, f32, f32),
    pub quat: [f32; 4],
    pub vel: (f32, f32, f32),
}

impl VehicleTelemetryMsg {
    pub const LEN: usize = 66;
    const POS_AT: usize = 38;

    const HEADER_SUFFIX: [u8; 29] = [
        0x2c, 0x00, 0x00, 0x00, 0x31, 0xff, 0x69, 0xfe, 0xc7, 0x01, 0xfa, 0x00, 0x11, 0x00, 0x5a,
        0x88, 0x3d, 0x00, 0x00, 0x00, 0xce, 0xf8, 0x2c, 0xf7, 0xf0, 0x60, 0x01, 0x00, 0x01,
    ];

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::LEN);
        out.extend_from_slice(&[0x00, 0x00, 0x04, 0xf6, 0xff]);
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&Self::HEADER_SUFFIX);
        for c in [self.pos.0, self.pos.1, self.pos.2] {
            out.extend_from_slice(&c.to_le_bytes());
        }
        for c in [self.vel.0, self.vel.1, self.vel.2] {
            out.extend_from_slice(&c.to_le_bytes());
        }
        out.extend_from_slice(&CompressedRotation::new(self.quat).encode().to_le_bytes());
        out
    }

    pub fn decode(b: &[u8]) -> Result<Self, ProtoError> {
        if b.len() != Self::LEN {
            return Err(malformed());
        }
        expect(b, 0, &[0x00, 0x00, 0x04, 0xf6, 0xff])?;
        let client_id = u16_at(b, 5)?;
        if u16_at(b, 7)? != client_id {
            return Err(malformed());
        }
        expect(b, 9, &Self::HEADER_SUFFIX)?;

        let p = Self::POS_AT;
        let quat = CompressedRotation::decode(u32_at(b, p + 24)?).0;
        Ok(Self {
            client_id,
            pos: (f32_at(b, p)?, f32_at(b, p + 4)?, f32_at(b, p + 8)?),
            vel: (
                f32_at(b, p + 12)?,
                f32_at(b, p + 16)?,
                f32_at(b, p + 20)?,
            ),
            quat,
        })
    }
}

/// Server clock tick (type `0x07`), 16 bytes.
///
/// `00 00 07 f7 ff 01 <client_id u16 LE> <vtime u32 LE> 00 00 00 00`
///
/// `vtime` is a [`crate::codec::CompressedTimestamp`] tick, i.e. microseconds
/// shifted right by 6 (64 µs resolution).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSyncMsg {
    pub client_id: u16,
    pub vtime: u32,
}

impl ClockSyncMsg {
    pub const LEN: usize = 16;

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::LEN);
        out.extend_from_slice(&[0x00, 0x00, 0x07, 0xf7, 0xff, 0x01]);
        out.extend_from_slice(&self.client_id.to_le_bytes());
        out.extend_from_slice(&self.vtime.to_le_bytes());
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        out
    }

    pub fn decode(b: &[u8]) -> Result<Self, ProtoError> {
        if b.len() != Self::LEN {
            return Err(malformed());
        }
        expect(b, 0, &[0x00, 0x00, 0x07, 0xf7, 0xff, 0x01])?;
        expect(b, 12, &[0x00, 0x00, 0x00, 0x00])?;
        Ok(Self {
            client_id: u16_at(b, 6)?,
            vtime: u32_at(b, 8)?,
        })
    }
}
