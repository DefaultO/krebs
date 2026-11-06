//! Fast seedable deterministic PRNG: xoshiro256** seeded through splitmix64
//! (<https://prng.di.unimi.it/>). Not cryptographic.

#[derive(Clone, Debug)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut sm = seed;
        let mut next = || {
            sm = sm.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = sm;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            z ^ (z >> 31)
        };
        // splitmix64 never yields four zeros in a row, so the state is valid.
        Rng { s: [next(), next(), next(), next()] }
    }

    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform in [0, 1) with 53 random bits.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in [0, n); n must be > 0. Unbiased (Lemire's method).
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "below(0)");
        let mut m = self.next_u64() as u128 * n as u128;
        // Reject the few low products that would over-represent some values.
        // The threshold 2^64 mod n is below n, so the division is only needed
        // when the low half is below n (rarely, for small n).
        if (m as u64) < n {
            let threshold = n.wrapping_neg() % n;
            while (m as u64) < threshold {
                m = self.next_u64() as u128 * n as u128;
            }
        }
        (m >> 64) as u64
    }

    /// Uniform in [lo, hi); requires lo < hi.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        assert!(lo < hi, "empty range");
        lo.wrapping_add(self.below(hi.wrapping_sub(lo) as u64) as i64)
    }

    /// Uniform in [lo, hi); requires lo < hi with a finite span.
    pub fn range_f64(&mut self, lo: f64, hi: f64) -> f64 {
        // Rounding can land exactly on hi (lo = 1, hi = 2 and a draw of
        // 1 - 2^-53 gives 2 - 2^-53, a tie that rounds to even, i.e. 2).
        (lo + (hi - lo) * self.next_f64()).min(hi.next_down())
    }

    /// Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            v.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_output() {
        // xoshiro256** from state {1, 2, 3, 4} (reference implementation).
        let mut r = Rng { s: [1, 2, 3, 4] };
        let expected = [11520u64, 0, 1509978240, 1215971899390074240];
        for e in expected {
            assert_eq!(r.next_u64(), e);
        }
    }

    #[test]
    fn below_sequence_is_stable() {
        // Pinned outputs: callers rely on a seed reproducing the same draws.
        let mut r = Rng::new(42);
        let n = [1u64, 2, 3, 7, 10, 1000, 1 << 40, u64::MAX, (1 << 63) + 1, 6];
        let got: Vec<u64> = n.iter().map(|&n| r.below(n)).collect();
        assert_eq!(got, [0, 0, 2, 6, 9, 769, 790833169754, 15679888225317814406, 7022439175346172479, 3]);
    }

    #[test]
    fn deterministic_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        assert_eq!((0..100).map(|_| a.next_u64()).collect::<Vec<_>>(), (0..100).map(|_| b.next_u64()).collect::<Vec<_>>());
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
        let mut counts = [0u32; 6];
        for _ in 0..60_000 {
            let f = a.next_f64();
            assert!((0.0..1.0).contains(&f));
            let x = a.range(-3, 3);
            assert!((-3..3).contains(&x));
            counts[(x + 3) as usize] += 1;
        }
        assert!(counts.iter().all(|&c| (9_000..11_000).contains(&c)), "{counts:?}");
        let _ = a.range(i64::MIN, i64::MAX); // full span must not overflow
        // A one-ulp span used to return hi for about half the draws.
        let hi = 1.0 + f64::EPSILON;
        assert!((0..10_000).all(|_| a.range_f64(1.0, hi) == 1.0));
        let mut v: Vec<u32> = (0..100).collect();
        a.shuffle(&mut v);
        assert_ne!(v, (0..100).collect::<Vec<_>>());
        v.sort();
        assert_eq!(v, (0..100).collect::<Vec<_>>());
    }
}
