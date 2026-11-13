//! Doc reuse (ARCHITECTURE.md §6.6, J11): a VM keeps the room docs it parsed and reparses only the ones that
//! changed. Each run the host sends one ASCII JSON array,
//! `[generation, [blob index, rooms doc, [op, …], changed doc, …], …]`, holding a plan for each room the VM also had
//! in its previous run. The ops rebuild the room's docs in blob order: `k, n` (k ≥ 0) takes docs k..k+n of the VM's
//! copy of that room with their seqs, `-1 - seq` takes the next changed doc. A doc is taken over when its seq and
//! its JSON are unchanged (the same `Arc` is: a doc held here is shared, so it is never mutated in place). A room the
//! VM did not have, or a plan it does not take, is parsed whole from the blob's text (`natives.blob`).
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
use std::sync::Arc;

const FRESH_KINDS: [&str; 5] = ["creep", "powerCreep", "spawn", "invaderCore", "tombstone"];

/// Whether a VM may keep this doc from one run to the next while it does not change.
fn takes_over(d: &DocRef) -> bool {
    !FRESH_KINDS.contains(&d.kind()) && d.obj.get(engine::atom!("effects")).is_none()
}

/// One room as the VM's JS side holds it: its docs in blob order.
struct Held {
    docs: Arc<[DocRef]>,
    /// `takes_over` of each doc. A late room is a few hundred roads that never change, and asking each of them again
    /// every tick was most of a plan's time, so the answer stays with the doc for as long as the VM holds it.
    keeps: Vec<bool>,
}

/// What the VM's JS side holds: for each room of its previous run, that room's docs in blob order.
#[derive(Default)]
pub(crate) struct DocCache {
    /// The generation the JS side stores once it has taken a run's input.
    generation: u64,
    rooms: HashMap<RoomIdx, Held>,
    /// The last plan's length; the next one is about as long.
    size: usize,
}

impl DocCache {
    /// The plans for this run's blobs (parallel to `rooms`) against the previous run's.
    pub fn plan(&mut self, blobs: &[Option<RoomBlob>], rooms: &[Option<RoomIdx>]) -> String {
        let mut out = String::with_capacity(self.size + self.size / 8 + 64);
        out.push('[');
        int(&mut out, self.generation as f64);
        self.generation += 1;
        let mut next: HashMap<RoomIdx, Held> = HashMap::with_capacity(self.rooms.len());
        for (i, (b, r)) in blobs.iter().zip(rooms).enumerate() {
            let (Some(b), Some(r)) = (b, r) else { continue };
            // A blob that is only its text, or a room listed twice: parsed whole.
            if b.is_text_only() || next.contains_key(r) {
                continue;
            }
            let mut keeps = Vec::with_capacity(b.docs.len());
            let Some(prev) = self.rooms.get(r) else {
                keeps.extend(b.docs.iter().map(takes_over));
                next.insert(*r, Held { docs: b.docs.clone(), keeps });
                continue;
            };
            out.push_str(",[");
            int(&mut out, i as f64);
            out.push(',');
            out.push_str(&b.info);
            out.push_str(",[");
            let mut fresh = Vec::new();
            let (mut k, mut run) = (0, None::<(usize, usize)>);
            let held = &prev.docs;
            for d in b.docs.iter() {
                while k < held.len() && held[k].seq < d.seq {
                    k += 1;
                }
                let at = k < held.len() && held[k].seq == d.seq;
                let (keep, same) = if at && Arc::ptr_eq(&held[k], d) {
                    (prev.keeps[k], prev.keeps[k])
                } else {
                    let keep = takes_over(d);
                    (keep, keep && at && held[k].json() == d.json())
                };
                keeps.push(keep);
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
                    int(&mut out, -1.0 - d.seq as f64);
                    out.push(',');
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
            next.insert(*r, Held { docs: b.docs.clone(), keeps });
        }
        out.push(']');
        self.rooms = next;
        self.size = out.len();
        out
    }
}

fn flush(out: &mut String, run: Option<(usize, usize)>) {
    if let Some((k, n)) = run {
        int(out, k as f64);
        out.push(',');
        int(out, n as f64);
        out.push(',');
    }
}

/// A whole number as JSON has it.
fn int(out: &mut String, n: f64) {
    util::num::write_js_string(out, n);
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
        RoomBlob::new(info.into(), docs.into())
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

    /// A doc that stays the same `Arc` is not asked again whether it may be kept, and the answer it was given holds:
    /// the same docs give the same plan run after run, creeps and docs with effects fresh every time.
    #[test]
    fn the_same_docs_plan_the_same_every_run() {
        let docs = [
            doc(1, r#"{"_id":"a","type":"road"}"#),
            doc(2, r#"{"_id":"c","type":"creep"}"#),
            doc(3, r#"{"_id":"b","type":"road"}"#),
            doc(4, r#"{"_id":"s","type":"spawn"}"#),
            doc(5, r#"{"_id":"g","type":"source","effects":[]}"#),
            doc(6, r#"{"_id":"d","type":"road"}"#),
        ];
        let blobs = [Some(blob("{}", &docs))];
        let mut cache = DocCache::default();
        assert_eq!(cache.plan(&blobs, &[Some(1)]), "[0]");
        for generation in 1..4 {
            let want = format!("[{generation},[0,{{}},[0,1,-3,2,1,-5,-6,5,1],{},{},{}]]", docs[1].json(), docs[3].json(), docs[4].json());
            assert_eq!(cache.plan(&blobs, &[Some(1)]), want);
        }
        // a blob that is only its text is left to the VM to parse whole, whatever room it names, and forgets the room
        assert_eq!(cache.plan(&[Some(RoomBlob::from_text(Arc::from(&b"{}"[..])))], &[Some(1)]), "[4]");
        assert_eq!(cache.plan(&blobs, &[Some(1)]), "[5]");
    }
}
