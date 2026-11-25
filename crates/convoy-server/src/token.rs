//! Session tokens the stock client validates during the join handshake.
//!
//! The client computes, for a peer's SteamID:
//!
//! ```text
//! CityHash64("server2client" || UPPER_HEX(SHA256(steam_id_le)))
//! ```
//!
//! and compares it with bytes 18..26 of the server's 26-byte mod request.
//! `client2server` is the mirror image, carried by the client's mod list.
//!
//! Both were confirmed against a capture of a real join: SteamID
//! `85568392937043937` yields `cd61c4c5b4fd5bc7` and `c6c14336b4789ded`.

use sha2::{Digest, Sha256};

const K0: u64 = 0xc3a5_c85c_97cb_3127;
const K1: u64 = 0xb492_b66f_be98_f273;
const K2: u64 = 0x9ae1_6a3b_2f90_404f;
const K3: u64 = 0xc949_d7c7_509e_6557;
const KMUL: u64 = 0x9ddf_ea08_eb38_2d69;

fn fetch64(s: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(s[i..i + 8].try_into().expect("8 bytes"))
}

fn fetch32(s: &[u8], i: usize) -> u64 {
    u32::from_le_bytes(s[i..i + 4].try_into().expect("4 bytes")) as u64
}

fn rotate(v: u64, shift: u32) -> u64 {
    if shift == 0 {
        v
    } else {
        v.rotate_right(shift)
    }
}

fn shift_mix(v: u64) -> u64 {
    v ^ (v >> 47)
}

fn hash_len16(u: u64, v: u64) -> u64 {
    hash_len16_mul(u, v, KMUL)
}

fn hash_len16_mul(u: u64, v: u64, mul: u64) -> u64 {
    let mut a = (u ^ v).wrapping_mul(mul);
    a ^= a >> 47;
    let mut b = (v ^ a).wrapping_mul(mul);
    b ^= b >> 47;
    b.wrapping_mul(mul)
}

fn hash_len0to16(s: &[u8]) -> u64 {
    let len = s.len();
    if len >= 8 {
        let a = fetch64(s, 0);
        let b = fetch64(s, len - 8);
        return hash_len16(a, rotate(b.wrapping_add(len as u64), (len as u32) & 63)) ^ b;
    }
    if len >= 4 {
        let a = fetch32(s, 0);
        return hash_len16(len as u64 + (a << 3), fetch32(s, len - 4));
    }
    if len > 0 {
        let a = s[0] as u64;
        let b = s[len >> 1] as u64;
        let c = s[len - 1] as u64;
        let y = a + (b << 8);
        let z = len as u64 + (c << 2);
        return shift_mix(y.wrapping_mul(K2) ^ z.wrapping_mul(K0)).wrapping_mul(K2);
    }
    K2
}

fn hash_len17to32(s: &[u8]) -> u64 {
    let len = s.len();
    let a = fetch64(s, 0).wrapping_mul(K1);
    let b = fetch64(s, 8);
    let c = fetch64(s, len - 8).wrapping_mul(K2);
    let d = fetch64(s, len - 16).wrapping_mul(K0);
    hash_len16(
        rotate(a.wrapping_sub(b), 43)
            .wrapping_add(rotate(c, 30))
            .wrapping_add(d),
        a.wrapping_add(rotate(b ^ K3, 20))
            .wrapping_sub(c)
            .wrapping_add(len as u64),
    )
}

fn hash_len33to64(s: &[u8]) -> u64 {
    let len = s.len();
    let z = fetch64(s, 24);
    let mut a = fetch64(s, 0)
        .wrapping_add((len as u64).wrapping_add(fetch64(s, len - 16)).wrapping_mul(K0));
    let mut b = rotate(a.wrapping_add(z), 52);
    let mut c = rotate(a, 37);
    a = a.wrapping_add(fetch64(s, 8));
    c = c.wrapping_add(rotate(a, 7));
    a = a.wrapping_add(fetch64(s, 16));
    let v1 = a.wrapping_add(z);
    let v2 = b.wrapping_add(rotate(a, 31)).wrapping_add(c);
    a = fetch64(s, 16).wrapping_add(fetch64(s, 32));
    let z2 = fetch64(s, 8);
    b = rotate(a.wrapping_add(z2), 52);
    c = rotate(a, 37);
    a = a.wrapping_add(fetch64(s, 40));
    c = c.wrapping_add(rotate(a, 7));
    a = a.wrapping_add(fetch64(s, 48));
    let v3 = a.wrapping_add(z2);
    let v4 = b.wrapping_add(rotate(a, 31)).wrapping_add(c);
    hash_len16(
        hash_len16(v1, v3)
            .wrapping_add(shift_mix(v2).wrapping_mul(K1))
            .wrapping_add(v4),
        hash_len16(v3, v4).wrapping_add(v2),
    )
}

fn weak_hash_len32_with_seeds(s: &[u8], i: usize, a_in: u64, b_in: u64) -> (u64, u64) {
    let w = fetch64(s, i);
    let x = fetch64(s, i + 8);
    let y = fetch64(s, i + 16);
    let z = fetch64(s, i + 24);
    let mut a = a_in.wrapping_add(w);
    let mut b = rotate(b_in.wrapping_add(a).wrapping_add(z), 21);
    let c = a;
    a = a.wrapping_add(x).wrapping_add(y);
    b = b.wrapping_add(rotate(a, 44));
    (a.wrapping_add(z), b.wrapping_add(c))
}

/// Google CityHash64, including the >64-byte loop.
pub fn city_hash64(s: &[u8]) -> u64 {
    let len = s.len();
    if len <= 32 {
        return if len <= 16 { hash_len0to16(s) } else { hash_len17to32(s) };
    }
    if len <= 64 {
        return hash_len33to64(s);
    }

    let mut x = fetch64(s, len - 40);
    let mut y = fetch64(s, len - 16).wrapping_add(fetch64(s, len - 56));
    let mut z = hash_len16(
        fetch64(s, len - 48).wrapping_add(len as u64),
        fetch64(s, len - 24),
    );
    let mut v = weak_hash_len32_with_seeds(s, len - 64, len as u64, z);
    let mut w = weak_hash_len32_with_seeds(s, len - 32, y.wrapping_add(K1), x);
    x = x.wrapping_mul(K1).wrapping_add(fetch64(s, 0));

    let mut pos = 0usize;
    let mut remaining = (len - 1) & !63usize;
    loop {
        x = rotate(
            x.wrapping_add(y).wrapping_add(v.0).wrapping_add(fetch64(s, pos + 8)),
            37,
        )
        .wrapping_mul(K1);
        y = rotate(y.wrapping_add(v.1).wrapping_add(fetch64(s, pos + 48)), 42).wrapping_mul(K1);
        x ^= w.1;
        y = y.wrapping_add(v.0).wrapping_add(fetch64(s, pos + 40));
        z = rotate(z.wrapping_add(w.0), 33).wrapping_mul(K1);
        v = weak_hash_len32_with_seeds(s, pos, v.1.wrapping_mul(K1), x.wrapping_add(w.0));
        w = weak_hash_len32_with_seeds(
            s,
            pos + 32,
            z.wrapping_add(w.1),
            y.wrapping_add(fetch64(s, pos + 16)),
        );
        std::mem::swap(&mut z, &mut x);
        pos += 64;
        remaining -= 64;
        if remaining == 0 {
            break;
        }
    }

    hash_len16(
        hash_len16(v.0, w.0)
            .wrapping_add(shift_mix(y).wrapping_mul(K1))
            .wrapping_add(z),
        hash_len16(v.1, w.1).wrapping_add(x),
    )
}

fn salted(domain: &str, steam_id: u64) -> u64 {
    let digest = Sha256::digest(steam_id.to_le_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02X}")).collect();
    city_hash64(format!("{domain}{hex}").as_bytes())
}

/// Token the client compares against bytes 18..26 of the server's mod request.
pub fn server2client(steam_id: u64) -> [u8; 8] {
    salted("server2client", steam_id).to_le_bytes()
}

/// Token the client sends back in its mod list.
pub fn client2server(steam_id: u64) -> [u8; 8] {
    salted("client2server", steam_id).to_le_bytes()
}


/// Entity id for a joining client. The official server uses opaque 64-bit ids;
/// any value works as long as the session uses it consistently.
pub fn entity_id(client_id: u16) -> u64 {
    0x0a00_0000_0000_0000u64 | client_id as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cityhash_reference_vectors() {
        assert_eq!(city_hash64(b""), 0x9ae1_6a3b_2f90_404f);
        assert_eq!(city_hash64(b"abc"), 0x24a5_b3a0_74e7_f369);
    }

    #[test]
    fn tokens_match_the_official_capture() {
        let sid = 85_568_392_937_043_937u64;
        assert_eq!(server2client(sid), [0xcd, 0x61, 0xc4, 0xc5, 0xb4, 0xfd, 0x5b, 0xc7]);
        assert_eq!(client2server(sid), [0xc6, 0xc1, 0x43, 0x36, 0xb4, 0x78, 0x9d, 0xed]);
    }
}