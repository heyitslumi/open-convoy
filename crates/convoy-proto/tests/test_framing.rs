use bytes::Bytes;
use convoy_proto::{parse_frame, FrameType, ProtoError, RawFrame};

fn frame(ftype: FrameType, payload: &[u8]) -> RawFrame {
    RawFrame { ftype, payload: Bytes::copy_from_slice(payload) }
}

#[test]
fn parses_flat_telemetry_frame() {
    // 00 00 prefix, type 0x04, telemetry subframe marker 0xf6ff.
    let buf = [0x00, 0x00, 0x04, 0xf6, 0xff, 0x01, 0x02, 0x03];
    let (f, consumed) = parse_frame(&buf).expect("valid frame").expect("complete frame");

    assert_eq!(f.ftype, FrameType::Telemetry);
    assert_eq!(f.payload, Bytes::from_static(&[0xf6, 0xff, 0x01, 0x02, 0x03]));
    assert_eq!(consumed, buf.len());
}

#[test]
fn parses_flat_auth_ticket_frame() {
    let buf = [0x00, 0x00, 0x0b, 0x16, 0x00, 0x01, 0x00];
    let (f, consumed) = parse_frame(&buf).expect("valid frame").expect("complete frame");

    assert_eq!(f.ftype, FrameType::AuthTicket);
    assert_eq!(f.payload, Bytes::from_static(&[0x16, 0x00, 0x01, 0x00]));
    assert_eq!(consumed, 7);
}

#[test]
fn parses_every_known_frame_type() {
    let cases = [
        (0x01u8, FrameType::Handshake),
        (0x02, FrameType::EntitySetup),
        (0x03, FrameType::Ping),
        (0x04, FrameType::Telemetry),
        (0x05, FrameType::PlayerMeta),
        (0x06, FrameType::ChatAdmin),
        (0x07, FrameType::ClockSync),
        (0x08, FrameType::Status),
        (0x0b, FrameType::AuthTicket),
        (0x0c, FrameType::SessionSetup),
    ];

    for (tag, expected) in cases {
        let buf = [0x00, 0x00, tag, 0xaa];
        let (f, _) = parse_frame(&buf).unwrap().unwrap();
        assert_eq!(f.ftype, expected, "tag 0x{tag:02x}");
    }
}

#[test]
fn parses_empty_payload() {
    let buf = [0x00, 0x00, 0x03];
    let (f, consumed) = parse_frame(&buf).unwrap().unwrap();

    assert_eq!(f.ftype, FrameType::Ping);
    assert!(f.payload.is_empty());
    assert_eq!(consumed, 3);
}

#[test]
fn parses_length_prefixed_frame() {
    // Total frame length 6 (u16 LE): 00 00 header, type 0x02, 3 payload bytes.
    let buf = [0x06, 0x00, 0x02, 0xde, 0xad, 0xbe, 0x99];
    let (f, consumed) = parse_frame(&buf).unwrap().unwrap();

    assert_eq!(f.ftype, FrameType::EntitySetup);
    assert_eq!(f.payload, Bytes::from_static(&[0xde, 0xad, 0xbe]));
    assert_eq!(consumed, 6);
}

#[test]
fn truncated_header_returns_none() {
    for buf in [&[][..], &[0x00][..], &[0x00, 0x00][..]] {
        assert!(parse_frame(buf).unwrap().is_none(), "buf {buf:?}");
    }
}

#[test]
fn incomplete_length_prefixed_frame_returns_none() {
    let buf = [0x0a, 0x00, 0x04, 0x01];
    assert!(parse_frame(&buf).unwrap().is_none());
}

#[test]
fn unknown_frame_type_is_an_error() {
    let buf = [0x00, 0x00, 0x0a, 0x00];
    match parse_frame(&buf) {
        Err(ProtoError::UnknownFrameType(0x0a)) => {}
        other => panic!("expected UnknownFrameType(0x0a), got {other:?}"),
    }
}

#[test]
fn malformed_length_prefix_is_an_error() {
    // Declared length 2 cannot hold the 2-byte header plus a type byte.
    let buf = [0x02, 0x00, 0x04];
    assert!(matches!(parse_frame(&buf), Err(ProtoError::MalformedHeader(_))));
}

#[test]
fn raw_frame_carries_bytes_without_copying_twice() {
    let buf = vec![0x00, 0x00, 0x05, 0x11, 0x22];
    let (f, _) = parse_frame(&buf).unwrap().unwrap();
    assert_eq!(f, frame(FrameType::PlayerMeta, &[0x11, 0x22]));
}
