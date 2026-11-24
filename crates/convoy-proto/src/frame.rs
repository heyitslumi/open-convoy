//! Wire framing: splits a byte stream into typed [`RawFrame`]s.
//!
//! Two layouts are accepted on the wire:
//!
//! * **Flat frame** — `00 00 <type> <payload...>`. The frame ends at the end of
//!   the buffer handed to [`parse_frame`], so a flat frame must be one packet.
//! * **Length-prefixed frame** — `<len: u16 LE> <type> <payload...>`, where
//!   `len` counts the whole frame including the 2-byte header.

use bytes::Bytes;

/// Message type carried by the third byte of a frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameType {
    Handshake = 0x01,
    EntitySetup = 0x02,
    Ping = 0x03,
    Telemetry = 0x04,
    PlayerMeta = 0x05,
    ChatAdmin = 0x06,
    ClockSync = 0x07,
    Status = 0x08,
    AuthTicket = 0x0b,
    SessionSetup = 0x0c,
}

impl FrameType {
    /// Returns `None` for tag bytes the protocol does not define.
    pub const fn from_u8(tag: u8) -> Option<Self> {
        Some(match tag {
            0x01 => Self::Handshake,
            0x02 => Self::EntitySetup,
            0x03 => Self::Ping,
            0x04 => Self::Telemetry,
            0x05 => Self::PlayerMeta,
            0x06 => Self::ChatAdmin,
            0x07 => Self::ClockSync,
            0x08 => Self::Status,
            0x0b => Self::AuthTicket,
            0x0c => Self::SessionSetup,
            _ => return None,
        })
    }

    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl TryFrom<u8> for FrameType {
    type Error = ProtoError;

    fn try_from(tag: u8) -> Result<Self, Self::Error> {
        Self::from_u8(tag).ok_or(ProtoError::UnknownFrameType(tag))
    }
}

/// A decoded frame header plus its payload, before any payload-level decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawFrame {
    pub ftype: FrameType,
    pub payload: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtoError {
    #[error("unknown frame type: 0x{0:02x}")]
    UnknownFrameType(u8),
    #[error("malformed frame header: {0}")]
    MalformedHeader(&'static str),
    #[error("malformed message payload: {0}")]
    MalformedMessage(&'static str),
}

/// Parses one frame from the front of `buf`.
///
/// Returns `Ok(None)` when more bytes are needed, `Ok(Some((frame, consumed)))`
/// with the number of bytes the frame occupied, or [`ProtoError`] when the
/// header cannot be valid at all (unknown type, impossible length).
pub fn parse_frame(buf: &[u8]) -> Result<Option<(RawFrame, usize)>, ProtoError> {
    if buf.len() < 3 {
        return Ok(None);
    }

    // Flat frame: `00 00` prefix, whole remaining buffer is the frame.
    // shortcut: a flat frame cannot be split out of a buffer holding several
    // frames; switch to the length-prefixed layout if that is ever needed.
    if buf[0] == 0x00 && buf[1] == 0x00 {
        let ftype = FrameType::try_from(buf[2])?;
        let payload = Bytes::copy_from_slice(&buf[3..]);
        return Ok(Some((RawFrame { ftype, payload }, buf.len())));
    }

    // Length-prefixed frame: u16 LE total length, header included.
    let total = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    if total < 3 {
        return Err(ProtoError::MalformedHeader("length shorter than header"));
    }
    if buf.len() < total {
        return Ok(None);
    }

    let ftype = FrameType::try_from(buf[2])?;
    let payload = Bytes::copy_from_slice(&buf[3..total]);
    Ok(Some((RawFrame { ftype, payload }, total)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_type_roundtrips_through_u8() {
        for tag in [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0b, 0x0c] {
            let ftype = FrameType::try_from(tag).unwrap();
            assert_eq!(ftype.as_u8(), tag);
            assert_eq!(FrameType::from_u8(tag), Some(ftype));
        }
        assert_eq!(FrameType::from_u8(0x00), None);
        assert_eq!(FrameType::from_u8(0x09), None);
    }
}
