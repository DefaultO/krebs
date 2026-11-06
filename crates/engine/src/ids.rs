//! Object/user/order ids (ARCHITECTURE §3.7, ID-LENGTH): lowercase hex of a keyed
//! 4-round Feistel permutation of a counter. Deterministic (no clock, no RNG), unique,
//! 15 hex chars (`private`, like Loki) or 24 (`mmo`, like ObjectIds). Ids that long are
//! never canonical array indices, so they never reorder JS objects keyed by id.

use crate::doc::Id;
use crate::policy::{self, Profile};
use std::collections::HashSet;

pub struct IdGen {
    counter: u64,
    len: u8,
    key: u64,
    imported: Option<HashSet<Id>>,
}

impl IdGen {
    pub fn new(world_seed: u64, profile: Profile) -> IdGen {
        let len = match profile {
            Profile::Private => 15,
            Profile::Mmo => 24,
        };
        IdGen { counter: 0, len, key: policy::seed(world_seed, "ids", &[]), imported: None }
    }

    /// Called only from serial code (P4 id pass, P5, CRON-STAGE, P0, import).
    #[allow(clippy::should_implement_trait)] // FROZEN name; an endless generator, not an iterator
    pub fn next(&mut self) -> Id {
        loop {
            let id = self.format(self.counter);
            self.counter += 1;
            if !self.imported.as_ref().is_some_and(|s| s.contains(&id)) {
                return id;
            }
        }
    }

    /// An id loaded from imported data: `next` never produces it.
    pub fn mark_imported(&mut self, id: Id) {
        self.imported.get_or_insert_with(HashSet::new).insert(id);
    }

    /// Persisted with checkpoints.
    pub fn counter(&self) -> u64 {
        self.counter
    }

    pub fn set_counter(&mut self, counter: u64) {
        self.counter = counter;
    }

    fn format(&self, counter: u64) -> Id {
        let half = self.len as u32 * 2; // bits per half: 30 or 48
        let mask = (1u128 << half) - 1;
        let (mut l, mut r) = ((counter as u128 >> half) & mask, counter as u128 & mask);
        for round in 1..=4u64 {
            let f = policy::mix64(r as u64 ^ policy::mix64(self.key ^ round)) as u128 & mask;
            (l, r) = (r, l ^ f);
        }
        Id::new(&format!("{:0w$x}", (l << half) | r, w = self.len as usize)).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_deterministic_and_sized() {
        let mut a = IdGen::new(42, Profile::Private);
        let mut b = IdGen::new(42, Profile::Private);
        let mut seen = HashSet::new();
        for _ in 0..200_000 {
            let id = a.next();
            assert_eq!(id, b.next());
            assert_eq!(id.as_str().len(), 15);
            assert!(id.as_str().bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
            assert!(seen.insert(id));
        }
        assert_ne!(IdGen::new(43, Profile::Private).next(), IdGen::new(42, Profile::Private).next());
        let mmo = IdGen::new(42, Profile::Mmo).next();
        assert_eq!(mmo.as_str().len(), 24);
        assert!(crate::doc::array_index(mmo.as_str()).is_none());
    }

    #[test]
    fn skips_imported_and_resumes() {
        let mut probe = IdGen::new(1, Profile::Private);
        let (first, second) = (probe.next(), probe.next());
        let mut g = IdGen::new(1, Profile::Private);
        g.mark_imported(first);
        assert_eq!(g.next(), second);
        assert_eq!(g.counter(), 2);
        let mut resumed = IdGen::new(1, Profile::Private);
        resumed.set_counter(1);
        assert_eq!(resumed.next(), second);
    }
}
