//! Replays the room-processor oracle fixtures (lines `{name, scenario, result}` of kind "room" under
//! crates/engine/tests/fixtures/, e.g. fixtures/oracle/<area>/cases.ndjson made by
//! `node tools/oracle/processor.js gen …` from the official processor.js) through `process_room` via
//! `engine::scenario`, printing the first difference of each failing case with its tags. Enforcement rules
//! and the CHITIN_ORACLE* environment: oracle_support/mod.rs.

mod oracle_support;

use engine::scenario::{first_diff, RoomScenario};
use oracle_support::{cases, check_all, fuzz, verdict, Case, Verdict};
use util::json::Json;

fn replay(c: &Case) -> Verdict {
    verdict(c, || RoomScenario::from_json(&c.scenario)?.run())
}

/// The scenario adapter reads and writes every fixture scenario losslessly (FROZEN format).
#[test]
fn scenarios_round_trip() {
    let cases = cases("room");
    assert!(!cases.is_empty(), "no oracle fixtures found");
    for c in &cases {
        let s = RoomScenario::from_json(&c.scenario).unwrap_or_else(|e| panic!("{} {}: {e}", c.file, c.name));
        assert_eq!(s.to_json(), c.scenario, "{} {}: scenario does not round-trip", c.file, c.name);
        assert_eq!(RoomScenario::from_json(&s.to_json()).unwrap(), s);
    }
}

#[test]
fn processor_oracle() {
    check_all("room", replay);
}

/// The same fixtures with the history mod off, the only way they reach `quiet_tick`.
#[test]
fn processor_oracle_without_history() {
    check_all("room", |c| verdict(c, || RoomScenario::from_json(&c.scenario)?.run_without_history()));
}

/// CHITIN_ORACLE=1: random room scenarios from the live oracle.
#[test]
fn processor_fuzz() {
    fuzz("room", "processor.js", replay);
}

/// CHITIN_ORACLE=1: recorded real-server rooms with random intents.
#[test]
fn processor_fuzz_recorded() {
    fuzz("recorded", "processor.js", replay);
}

// ---- adapter self-check ----

use engine::bulk::DbOp;
use engine::doc::{Atom, Doc, Id, Obj, Val};
use engine::live::Crash;
use engine::processor::{RoomDraft, RoomResult};
use std::sync::Arc;

fn obj_of(j: &Json) -> Obj {
    match Val::from_json(j) {
        Val::Obj(o) => o,
        v => panic!("not an object: {v:?}"),
    }
}

/// The oracle's captured `users` ops (after the storage RPC) as engine DbOps.
fn db_ops(ops: Option<&Json>) -> Vec<DbOp> {
    let mut out = Vec::new();
    for op in ops.and_then(Json::as_array).into_iter().flatten() {
        let id = Id::new(op.get("id").and_then(Json::as_str).unwrap_or("")).unwrap();
        match op.get("op").and_then(Json::as_str) {
            Some("remove") => out.push(DbOp::Remove(id)),
            Some("update") => {
                for (k, v) in op.get("update").and_then(Json::as_object).unwrap() {
                    for (key, val) in v.as_object().unwrap() {
                        let a = Atom::intern(key);
                        out.push(match k.as_str() {
                            "$set" => DbOp::Set(id, obj_of(&Json::Obj(vec![(key.clone(), val.clone())]))),
                            "$inc" => DbOp::Inc(id, vec![a], val.as_f64().unwrap_or(f64::NAN)),
                            "$addToSet" => DbOp::AddToSet(id, a, Val::from_json(val)),
                            "$pull" => DbOp::Pull(id, a, Val::from_json(val)),
                            other => panic!("unexpected users op {other}"),
                        });
                    }
                }
            }
            other => panic!("unexpected users op {other:?}"),
        }
    }
    out
}

fn without_placeholder(j: &Json) -> (Obj, bool) {
    let mut o = obj_of(j);
    let inserted = o.get(Atom::intern("_id")).and_then(Val::as_str).is_some_and(|id| id.starts_with('#'));
    if inserted {
        o.remove(Atom::intern("_id"));
    }
    (o, inserted)
}

/// For every fixture, the RoomResult a perfect port would return (built from the oracle's own outcome and
/// ops) must map back to exactly that outcome: checks the adapter's assembly, placeholder ids, key order,
/// and `bulk::apply` on the users collection against the oracle's Loki mirror.
#[test]
fn adapter_reproduces_oracle_outcomes() {
    for c in cases("room") {
        let s = RoomScenario::from_json(&c.scenario).unwrap();
        let o = c.result.get("outcome").unwrap();
        let get = |k: &str| o.get(k).unwrap_or_else(|| panic!("{}: outcome.{k} missing", c.name));
        let uidx = |u: &Json| s.users.iter().position(|(id, _)| Some(id.as_str()) == u.as_str()).unwrap() as u32;
        let crashed = get("crashed").as_bool().unwrap();
        let (mut objects, mut inserts) = (Vec::new(), Vec::new());
        for d in get("objects").as_array().unwrap() {
            match without_placeholder(d) {
                (d, true) => inserts.push(d),
                (d, false) => objects.push(Doc::new(objects.len() as u64 + 1, d)),
            }
        }
        let flags: Vec<Arc<Obj>> = get("flags").as_array().unwrap().iter().map(|f| Arc::new(without_placeholder(f).0)).collect();
        let next = (!crashed).then(|| RoomDraft {
            info: Arc::new(obj_of(get("roomInfo"))),
            objects,
            flags: flags.into(),
            event_log: get("eventLog").stringify().into(),
            map_view: get("mapView").stringify().into(),
            presence: Vec::new(),
            user_refs: Vec::new(),
            has_movers: false,
            has_terminal_send: false,
            has_power_creeps: false,
        });
        let r = RoomResult {
            room: 0,
            next,
            inserts: if crashed { Vec::new() } else { inserts },
            crashed: crashed.then(|| Crash("vanilla crash".into())),
            activate_next: get("activated").as_bool().unwrap(),
            user_ops: if crashed { Vec::new() } else { db_ops(c.result.get("ops").and_then(|o| o.get("users"))) },
            foreign_inserts: Vec::new(),
            notifications: get("notifications").as_array().unwrap().iter().map(|n| (uidx(n.at(0).unwrap()), n.at(1).unwrap().as_str().unwrap().into())).collect(),
            history: (!get("history").is_null()).then(|| get("history").stringify().into()),
            stats: get("stats")
                .as_array()
                .unwrap()
                .iter()
                .map(|x| (uidx(x.at(0).unwrap()), Atom::intern(x.at(1).unwrap().as_str().unwrap()), x.at(2).unwrap().as_f64().unwrap_or(f64::NAN)))
                .collect(),
            cost_ns: 0,
            index: None,
        };
        assert!(get("foreignInserts").as_array().unwrap().is_empty(), "{}: foreign inserts are not covered here", c.name);
        if let Some(d) = first_diff(o, &s.outcome(&r)) {
            panic!("{} :: {}: {d}", c.file, c.name);
        }
    }
}

/// CHITIN_ORACLE=1: random NPC rooms (keepers, invaders) from the live oracle.
#[test]
fn processor_fuzz_npc() {
    fuzz("npc", "processor.js", replay);
}

/// CHITIN_ORACLE=1: random stronghold rooms (invader cores of every level) from the live oracle.
#[test]
fn processor_fuzz_stronghold() {
    fuzz("stronghold", "processor.js", replay);
}

/// CHITIN_ORACLE=1: random power creep rooms (usePower on every target type, enableRoom, renew, ...).
#[test]
fn processor_fuzz_pc() {
    fuzz("pc", "processor.js", replay);
}
