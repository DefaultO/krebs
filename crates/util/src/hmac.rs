//! HMAC-SHA1 (RFC 2104) and PBKDF2-HMAC-SHA1 (RFC 8018), for auth tokens and
//! password hashes.

use crate::sha1::sha1;

pub fn hmac_sha1(key: &[u8], msg: &[u8]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..20].copy_from_slice(&sha1(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    // ponytail: rehashes the padded key per call; caching the two pad states would
    // halve PBKDF2's cost (~100 ms for 100k iterations) if login latency matters.
    let mut inner = Vec::with_capacity(64 + msg.len());
    inner.extend(k.iter().map(|b| b ^ 0x36));
    inner.extend_from_slice(msg);
    let mut outer = [0u8; 84];
    for (o, b) in outer.iter_mut().zip(k) {
        *o = b ^ 0x5c;
    }
    outer[64..].copy_from_slice(&sha1(&inner));
    sha1(&outer)
}

/// Fills `out` with PBKDF2-HMAC-SHA1(password, salt, iterations).
pub fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    for (i, chunk) in out.chunks_mut(20).enumerate() {
        let mut msg = salt.to_vec();
        msg.extend_from_slice(&(i as u32 + 1).to_be_bytes());
        let mut u = hmac_sha1(password, &msg);
        let mut t = u;
        for _ in 1..iterations {
            u = hmac_sha1(password, &u);
            for (a, b) in t.iter_mut().zip(u) {
                *a ^= b;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(d: &[u8]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn rfc2202() {
        let cases: [(&[u8], &[u8], &str); 7] = [
            (&[0x0b; 20], b"Hi There", "b617318655057264e28bc0b6fb378c8ef146be00"),
            (b"Jefe", b"what do ya want for nothing?", "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79"),
            (&[0xaa; 20], &[0xdd; 50], "125d7342b9ac11cd91a39af48aa17b4f63f175d3"),
            (&(1..=25).collect::<Vec<u8>>(), &[0xcd; 50], "4c9007f4026250c6bc8414f9bf50c86c2d7235da"),
            (&[0x0c; 20], b"Test With Truncation", "4c1a03424b55e07fe7f27be1d58bb9324a9a5a04"),
            (&[0xaa; 80], b"Test Using Larger Than Block-Size Key - Hash Key First", "aa4ae5e15272d00e95705637ce8a3b55ed402112"),
            (&[0xaa; 80], b"Test Using Larger Than Block-Size Key and Larger Than One Block-Size Data", "e8e99d0f45237d786d6bbaa7965c7808bbff1a91"),
        ];
        for (key, msg, want) in cases {
            assert_eq!(hex(&hmac_sha1(key, msg)), want);
        }
    }

    #[test]
    fn rfc6070() {
        let check = |p: &[u8], s: &[u8], c: u32, want: &str| {
            let mut out = vec![0u8; want.len() / 2];
            pbkdf2_sha1(p, s, c, &mut out);
            assert_eq!(hex(&out), want);
        };
        check(b"password", b"salt", 1, "0c60c80f961f0e71f3a9b524af6012062fe037a6");
        check(b"password", b"salt", 2, "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957");
        check(b"password", b"salt", 4096, "4b007901b765489abead49d926f721d065a429c1");
        check(b"passwordPASSWORDpassword", b"saltSALTsaltSALTsaltSALTsaltSALTsalt", 4096, "3d2eec4fe41c849b80c8d83662c0e44a8b291a964cf2f07038");
        check(b"pass\0word", b"sa\0lt", 4096, "56fa6aa75548099dcc37d7f03425e0c3");
    }

    #[test]
    #[ignore = "16.7M iterations; run with --release --ignored"]
    fn rfc6070_long() {
        let mut out = [0u8; 20];
        pbkdf2_sha1(b"password", b"salt", 16_777_216, &mut out);
        assert_eq!(hex(&out), "eefe3d61cd4da4e4e9945b3d6ba2158c2634e984");
    }
}
