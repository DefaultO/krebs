//! FxHash (rustc's hasher): a multiply-rotate over 8-byte words, several times faster than std's SipHash on
//! short keys. Not DoS-resistant: only for maps whose keys the server makes (object ids, room-local keys).

use std::hash::{BuildHasherDefault, Hasher};

pub type FxBuild = BuildHasherDefault<Fx>;
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuild>;

#[derive(Default, Clone, Copy)]
pub struct Fx(u64);

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl Fx {
    #[inline]
    fn add(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(SEED);
    }
}

impl Hasher for Fx {
    #[inline]
    fn write(&mut self, mut b: &[u8]) {
        while b.len() >= 8 {
            self.add(u64::from_le_bytes(b[..8].try_into().unwrap()));
            b = &b[8..];
        }
        if b.len() >= 4 {
            self.add(u32::from_le_bytes(b[..4].try_into().unwrap()) as u64);
            b = &b[4..];
        }
        for &x in b {
            self.add(x as u64);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_ids_hash_apart() {
        let h = |s: &str| {
            let mut f = Fx::default();
            f.write(s.as_bytes());
            f.finish()
        };
        assert_ne!(h("5bbcac4b9099fc012e635f6a"), h("5bbcac4b9099fc012e635f6b"));
        assert_ne!(h("ab"), h("ba"));
        let m: FxHashMap<&str, u32> = [("a", 1), ("b", 2)].into_iter().collect();
        assert_eq!(m["b"], 2);
    }
}
