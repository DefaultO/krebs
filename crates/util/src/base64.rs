//! Standard base64 (RFC 4648 section 4) with padding.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes base64; padding is optional. Returns None on any character outside
/// the alphabet, misplaced padding, or a length that cannot be base64.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let b = text.as_bytes();
    let body = b.strip_suffix(b"==").or_else(|| b.strip_suffix(b"=")).unwrap_or(b);
    if b.len() != body.len() && !b.len().is_multiple_of(4) {
        return None; // padding only makes sense on a full final quantum
    }
    if body.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(body.len() / 4 * 3 + 2);
    for chunk in body.chunks(4) {
        let mut n: u32 = 0;
        for &c in chunk {
            n = n << 6 | value(c)? as u32;
        }
        n <<= 6 * (4 - chunk.len()) as u32;
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..chunk.len() - 1]);
    }
    Some(out)
}

fn value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4648_vectors() {
        let v = [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")];
        for (plain, enc) in v {
            assert_eq!(encode(plain.as_bytes()), enc);
            assert_eq!(decode(enc).unwrap(), plain.as_bytes());
            assert_eq!(decode(enc.trim_end_matches('=')).unwrap(), plain.as_bytes());
        }
    }

    #[test]
    fn binary_round_trip_and_rejects() {
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        for len in 0..data.len() {
            assert_eq!(decode(&encode(&data[..len])).unwrap(), &data[..len]);
        }
        for bad in ["Z", "Zm9vY", "Zm=9", "Zm9v=", "Z===", "Zm 9v", "Zm9v-_", "====", "Zg=", "Zm9vYg="] {
            assert_eq!(decode(bad), None, "{bad:?}");
        }
    }
}
