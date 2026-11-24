use convoy_proto::codec::{quat_to_euler, CompressedRotation, CompressedTimestamp, PlayerPos, SmallFloatVec};

#[test]
fn test_identity_quaternion_roundtrip() {
    let identity = CompressedRotation::new([0.0, 0.0, 0.0, 1.0]);
    let encoded = identity.encode();
    assert_eq!((encoded >> 30) & 0x3, 3);
    let decoded = CompressedRotation::decode(encoded);
    for i in 0..4 {
        assert!((identity.0[i] - decoded.0[i]).abs() < 0.002);
    }
}

#[test]
fn test_yaw_rotation_roundtrips() {
    // 90 deg yaw around Y: [0, sin(45 deg), 0, cos(45 deg)]
    let angle_90 = std::f32::consts::FRAC_PI_4;
    let q_90 = CompressedRotation::new([0.0, angle_90.sin(), 0.0, angle_90.cos()]);
    let enc_90 = q_90.encode();
    let dec_90 = CompressedRotation::decode(enc_90);
    let (yaw_90, pitch_90, roll_90) = quat_to_euler(&dec_90.0);
    assert!((yaw_90 - 90.0).abs() < 0.2);
    assert!(pitch_90.abs() < 0.2);
    assert!(roll_90.abs() < 0.2);

    // 180 deg yaw around Y: [0, sin(90 deg), 0, cos(90 deg)] = [0, 1, 0, 0]
    let q_180 = CompressedRotation::new([0.0, 1.0, 0.0, 0.0]);
    let enc_180 = q_180.encode();
    let dec_180 = CompressedRotation::decode(enc_180);
    let (yaw_180, pitch_180, roll_180) = quat_to_euler(&dec_180.0);
    assert!((yaw_180.abs() - 180.0).abs() < 0.2);
    assert!(pitch_180.abs() < 0.2);
    assert!(roll_180.abs() < 0.2);
}

#[test]
fn test_known_captured_quaternion() {
    let dec = CompressedRotation::decode(0x403804a7);
    let (yaw, pitch, roll) = quat_to_euler(&dec.0);
    assert!((yaw - 153.28).abs() < 0.5, "yaw was {}", yaw);
    assert!((pitch - 0.26).abs() < 0.5, "pitch was {}", pitch);
    assert!((roll - 0.43).abs() < 0.5, "roll was {}", roll);
}

#[test]
fn test_player_pos_roundtrip() {
    let test_cases = [
        (0.0, 0.0),
        (123.456, -789.012),
        (-4193792.0, 4193792.0),
        (42.0, -0.001),
    ];
    for (x, z) in test_cases {
        let pos = PlayerPos { x, z };
        let encoded = pos.encode();
        let decoded = PlayerPos::decode(encoded);
        assert_eq!(pos.x.to_bits(), decoded.x.to_bits());
        assert_eq!(pos.z.to_bits(), decoded.z.to_bits());
    }
}

#[test]
fn test_vector_scaling_roundtrip() {
    let v: [f32; 3] = [1.5, -2.25, 0.125];

    // scale = 64.0
    let encoded_64 = SmallFloatVec::encode(&v, 64.0);
    let decoded_64 = SmallFloatVec::decode(&encoded_64, 64.0);
    for i in 0..3 {
        assert!((v[i] - decoded_64[i]).abs() <= (1.0 / 64.0));
    }

    // scale = 256.0
    let encoded_256 = SmallFloatVec::encode(&v, 256.0);
    let decoded_256 = SmallFloatVec::decode(&encoded_256, 256.0);
    for i in 0..3 {
        assert!((v[i] - decoded_256[i]).abs() <= (1.0 / 256.0));
    }
}

#[test]
fn test_compressed_timestamp_truncation() {
    let vtime_us = 1_000_000u64; // 1 second
    let ts = CompressedTimestamp::from_micros(vtime_us);
    assert_eq!(ts.0, (vtime_us >> 6) as u32);
    assert_eq!(ts.to_micros(), (vtime_us >> 6) * 64);
}
