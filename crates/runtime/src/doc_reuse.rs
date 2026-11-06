//! Doc reuse (ARCHITECTURE.md §6.6, J11): a VM keeps the room docs it parsed and reparses only the ones that
//! changed. Beside the shared room blobs the host sends each run one ASCII JSON array,
//! `[generation, [blob index, rooms doc, [op, …], changed doc, …], …]`, holding a plan for each room the VM also had
//! in its previous run. The ops rebuild the room's docs in blob order: `k, n` (k ≥ 0) takes docs k..k+n of the VM's
//! copy of that room with their seqs, `-1 - seq` takes the next changed doc. A doc is taken over when its seq and
//! its JSON are unchanged (the same `Arc` is: a doc held here is shared, so it is never mutated in place).
//!
//! Never taken over: docs whose getters hand a piece of the doc itself to the player (`creep.body`, a spawn's or
//! invader core's `spawning.directions`, a tombstone's power creep `powers`), and docs with `effects`, which engine
//! code passes to `RoomObject.call` (a property the player can shadow on that function); vanilla's fresh parse
//! makes those new objects every tick. The JS side ignores plans made for another generation than the one it holds, and switches
//! reuse off for good (`docsOff`) when a mod defines object types or the player has replaced a builtin that engine
//! code hands raw docs to (prelude.js).

use engine::doc::DocRef;
use engine::world::{RoomBlob, RoomIdx};
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

const FRESH_KINDS: [&str; 5] = ["creep", "powerCreep", "spawn", "invaderCore", "tombstone"];

/// What the VM's JS side holds: for each room of its previous run, that room's docs in blob order.
#[derive(Default)]
pub(crate) struct DocCache {
    /// The generation the JS side stores once it has taken a run's input.
    generation: u64,
    rooms: HashMap<RoomIdx, Arc<[DocRef]>>,
}

impl DocCache {
    /// The plans for this run's blobs (parallel to `rooms`) against the previous run's.
    pub fn plan(&mut self, blobs: &[Option<RoomBlob>], rooms: &[Option<RoomIdx>]) -> String {
        let mut out = format!("[{}", self.generation);
        self.generation += 1;
        let mut next = HashMap::with_capacity(self.rooms.len());
        for (i, (b, r)) in blobs.iter().zip(rooms).enumerate() {
            let (Some(b), Some(r)) = (b, r) else { continue };
            // A hand-made blob without its docs, or a room listed twice: parsed whole.
            if b.docs.len() + 1 != b.offsets.len() || next.insert(*r, b.docs.clone()).is_some() {
                continue;
            }
            let Some(prev) = self.rooms.get(r) else { continue };
            let _ = write!(out, ",[{i},");
            push_ascii(&mut out, &b.json[8..b.info_end as usize]);
            out.push_str(",[");
            let mut fresh = Vec::new();
            let (mut k, mut run) = (0, None::<(usize, usize)>);
            for d in b.docs.iter() {
                while k < prev.len() && prev[k].seq < d.seq {
                    k += 1;
                }
                let same = k < prev.len()
                    && prev[k].seq == d.seq
                    && (Arc::ptr_eq(&prev[k], d) || prev[k].json() == d.json())
                    && !FRESH_KINDS.contains(&d.kind())
                    && d.obj.get(engine::atom!("effects")).is_none();
                if same {
                    run = match run {
                        Some((s, n)) if s + n == k => Some((s, n + 1)),
                        _ => {
                            flush(&mut out, run);
                            Some((k, 1))
                        }
                    };
                } else {
                    flush(&mut out, run.take());
                    let _ = write!(out, "{},", -1.0 - d.seq as f64);
                    fresh.push(d);
                }
            }
            flush(&mut out, run);
            if out.ends_with(',') {
                out.pop();
            }
            out.push(']');
            for d in fresh {
                out.push(',');
                out.push_str(d.json());
            }
            out.push(']');
        }
        out.push(']');
        self.rooms = next;
        out
    }
}

fn flush(out: &mut String, run: Option<(usize, usize)>) {
    if let Some((k, n)) = run {
        let _ = write!(out, "{k},{n},");
    }
}

fn push_ascii(out: &mut String, json: &[u8]) {
    // Blobs are ASCII (§3.4).
    out.push_str(std::str::from_utf8(json).expect("ASCII blob"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::doc::{Doc, Val};
    use util::json::Json;

    fn doc(seq: u64, json: &str) -> DocRef {
        let Val::Obj(o) = Val::from_json(&Json::parse(json).unwrap()) else { panic!() };
        Doc::new(seq, o)
    }

    fn blob(info: &str, docs: &[DocRef]) -> RoomBlob {
        let mut s = format!("{{\"room\":{info}");
        let info_end = s.len() as u32;
        s.push_str(",\"seq\":[],\"objects\":[");
        let mut offsets = Vec::new();
        for (i, d) in docs.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            offsets.push(s.len() as u32);
            s.push_str(d.json());
        }
        s.push(']');
        offsets.push(s.len() as u32);
        s.push('}');
        RoomBlob { json: Arc::from(s.into_bytes()), offsets: offsets.into(), info_end, docs: docs.into() }
    }

    #[test]
    fn plans_take_over_unchanged_docs_by_seq() {
        let road = |seq, id: &str| doc(seq, &format!(r#"{{"_id":"{id}","type":"road"}}"#));
        let (a, b, c, e) = (road(1, "a"), road(2, "b"), doc(3, r#"{"_id":"c","type":"creep"}"#), road(4, "e"));
        let g = doc(10, r#"{"_id":"g","type":"source","effects":[]}"#);
        let mut cache = DocCache::default();
        assert_eq!(cache.plan(&[Some(blob("{}", &[a.clone(), b, c.clone(), e, g.clone()]))], &[Some(7)]), "[0]");

        // a unchanged (same Arc), b changed, c a creep and g with effects (never taken over), e equal JSON, f new;
        // a blob without a room and a room new to this VM are parsed whole.
        let b2 = doc(2, r#"{"_id":"b","type":"road","hits":1}"#);
        let f = road(9, "f");
        let blobs = [Some(blob(r#"{"_id":"W1N1"}"#, &[a.clone(), b2.clone(), c.clone(), road(4, "e"), f.clone(), g.clone()])), Some(blob("{}", &[])), Some(blob("{}", &[a.clone()]))];
        let plan = cache.plan(&blobs, &[Some(7), None, Some(8)]);
        assert_eq!(plan, format!(r#"[1,[0,{{"_id":"W1N1"}},[0,1,-3,-4,3,1,-10,-11],{},{},{},{}]]"#, b2.json(), c.json(), f.json(), g.json()));

        // Runs merge; a room absent from a run is forgotten.
        assert_eq!(cache.plan(&[Some(blob("{}", &[a.clone()]))], &[Some(8)]), "[2,[0,{},[0,1]]]");
        assert_eq!(cache.plan(&[Some(blob("{}", &[a.clone(), b2, road(4, "e"), f]))], &[Some(7)]), "[3]");
        assert_eq!(cache.plan(&[Some(blob("{}", &[a, road(4, "e")]))], &[Some(7)]), "[4,[0,{},[0,1,2,1]]]");
    }
}
