//! Mathematical quantization codecs discovered from binary analysis.

const QUAT_SCALE: f32 = 722.663_15;

/// Smallest-three unit quaternion representation.
///
/// Stores `[x, y, z, w]`. In wire format, packs into a 32-bit unsigned int:
/// - 2 bits: index of component with largest absolute value (`max_idx`)
/// - 3x 10 bits: sign bit + 9 magnitude bits for remaining 3 components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressedRotation(pub [f32; 4]);

impl CompressedRotation {
    #[inline]
    pub const fn new(q: [f32; 4]) -> Self {
        Self(q)
    }

    /// Encode unit quaternion [x, y, z, w] into 32-bit packed representation.
    pub fn encode(&self) -> u32 {
        let mut q = self.0;
        let mut max_idx = 0;
        let mut max_abs = q[0].abs();
        for i in 1..4 {
            let a = q[i].abs();
            if a > max_abs {
                max_abs = a;
                max_idx = i;
            }
        }

        if q[max_idx] < 0.0 {
            for v in &mut q {
                *v = -*v;
            }
        }

        let mut packed = (max_idx as u32) << 30;
        let mut shift = 20i32;
        for (i, &val) in q.iter().enumerate() {
            if i == max_idx {
                continue;
            }
            let sign_bit = if val < 0.0 { 1u32 << 9 } else { 0u32 };
            let mag = (val.abs() * QUAT_SCALE).round().clamp(0.0, 511.0) as u32;
            let comp = sign_bit | (mag & 0x1ff);
            packed |= comp << shift;
            shift -= 10;
        }

        packed
    }

    /// Decode 32-bit packed representation into unit quaternion [x, y, z, w].
    pub fn decode(packed: u32) -> Self {
        let max_idx = ((packed >> 30) & 0x3) as usize;
        let mut comps = [0.0f32; 3];
        let mut sum_sq = 0.0f32;

        for (i, comp_val) in comps.iter_mut().enumerate() {
            let shift = (2 - i) * 10;
            let raw = (packed >> shift) & 0x3ff;
            let sign = (raw >> 9) & 1;
            let mag = raw & 0x1ff;
            let mut val = (mag as f32) / QUAT_SCALE;
            if sign == 1 {
                val = -val;
            }
            *comp_val = val;
            sum_sq += val * val;
        }

        let c_max = (1.0f32 - sum_sq).max(0.0).sqrt();

        let mut q = [0.0f32; 4];
        let mut comp_idx = 0;
        for i in 0..4 {
            if i == max_idx {
                q[i] = c_max;
            } else {
                q[i] = comps[comp_idx];
                comp_idx += 1;
            }
        }

        Self(q)
    }
}

/// Convert unit quaternion `[x, y, z, w]` to Euler angles `(yaw, pitch, roll)` in degrees.
pub fn quat_to_euler(q: &[f32; 4]) -> (f32, f32, f32) {
    let [x, y, z, w] = *q;
    let rad_to_deg = 180.0 / std::f32::consts::PI;

    let yaw = (2.0 * (w * y + x * z)).atan2(1.0 - 2.0 * (y * y + z * z)) * rad_to_deg;
    let pitch = (2.0 * (w * x - y * z)).clamp(-1.0, 1.0).asin() * rad_to_deg;
    let roll = (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (x * x + z * z)) * rad_to_deg;

    (yaw, pitch, roll)
}

/// Local 2D player coordinates (X, Z).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerPos {
    pub x: f32,
    pub z: f32,
}

impl PlayerPos {
    pub const MIN: f32 = -4_193_792.0;
    pub const MAX: f32 = 4_193_792.0;

    #[inline]
    pub fn encode(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0..4].copy_from_slice(&self.x.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.z.to_le_bytes());
        bytes
    }

    #[inline]
    pub fn decode(bytes: [u8; 8]) -> Self {
        let x = f32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let z = f32::from_le_bytes(bytes[4..8].try_into().unwrap());
        Self { x, z }
    }
}

/// 3D Vector fixed-point scaling codec.
pub struct SmallFloatVec;

impl SmallFloatVec {
    /// Encode 3D vector to 3 signed 16-bit integers with scaling factor (e.g. 64.0 or 256.0).
    pub fn encode(v: &[f32; 3], scale: f32) -> [i16; 3] {
        [
            (v[0] * scale).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16,
            (v[1] * scale).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16,
            (v[2] * scale).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16,
        ]
    }

    /// Decode 3 signed 16-bit integers to 3D vector using inverse scaling factor.
    pub fn decode(packed: &[i16; 3], scale: f32) -> [f32; 3] {
        [
            (packed[0] as f32) / scale,
            (packed[1] as f32) / scale,
            (packed[2] as f32) / scale,
        ]
    }
}

/// Compressed timestamp with 64 microsecond resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CompressedTimestamp(pub u32);

impl CompressedTimestamp {
    #[inline]
    pub const fn from_micros(vtime_us: u64) -> Self {
        Self((vtime_us >> 6) as u32)
    }

    #[inline]
    pub const fn to_micros(&self) -> u64 {
        (self.0 as u64) << 6
    }
}
