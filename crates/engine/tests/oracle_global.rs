//! Replays the global-stage oracle fixtures (kind "global", made by `node tools/oracle/global.js gen …` from
//! the official processor/global.js) through `processor::global::run` via `engine::scenario`. A vanilla
//! global-stage throw is TICK-ERROR, which chitin never reproduces (COMPAT-CRASH-GLOBAL): such cases are
//! skipped. Enforcement rules and the CHITIN_ORACLE* environment: oracle_support/mod.rs.

mod oracle_support;

use engine::scenario::{first_diff, GlobalScenario, GLOBAL_COLLECTIONS};
use oracle_support::{cases, check_all, fuzz, verdict, Case, Verdict};
use util::json::Json;

fn replay(c: &Case) -> Verdict {
    if let Some(crash) = c.result.get("crash").and_then(Json::as_str) {
        return Verdict::Skip(format!("vanilla global stage threw ({crash}): COMPAT-CRASH-GLOBAL"));
    }
    if let Ok(Err(e)) = GlobalScenario::from_json(&c.scenario).map(|s| s.intent_owners().map(drop)) {
        return Verdict::Skip(e);
    }
    verdict(c, || GlobalScenario::from_json(&c.scenario)?.run())
}

#[test]
fn global_scenarios_round_trip() {
    let cases = cases("global");
    assert!(!cases.is_empty(), "no global oracle fixtures found");
    for c in &cases {
        let s = GlobalScenario::from_json(&c.scenario).unwrap_or_else(|e| panic!("{} {}: {e}", c.file, c.name));
        assert_eq!(s.to_json(), c.scenario, "{} {}: scenario does not round-trip", c.file, c.name);
    }
}

/// The world built from a scenario maps back to exactly its collections when nothing runs: checks the
/// World assembly (rooms, seq order, power creeps, market) and the outcome conversion.
#[test]
fn global_adapter_round_trips_the_world() {
    for c in cases("global") {
        let s = GlobalScenario::from_json(&c.scenario).unwrap();
        let w = s.world().unwrap_or_else(|e| panic!("{}: {e}", c.name));
        let mut expected: Vec<(String, Json)> = GLOBAL_COLLECTIONS
            .iter()
            .map(|k| {
                let docs = s.coll(k).iter().map(|o| engine::doc::Val::Obj(o.clone()).to_json());
                // users.intents docs come back as {user, intents}.
                let docs: Vec<Json> = if *k == "users.intents" {
                    docs.map(|d| Json::Obj(vec![("user".into(), d.get("user").unwrap().clone()), ("intents".into(), d.get("intents").unwrap().clone())])).collect()
                } else {
                    docs.collect()
                };
                (k.to_string(), Json::Arr(docs))
            })
            .collect();
        expected.push(("activated".into(), Json::Arr(Vec::new())));
        expected.push(("crashed".into(), Json::Bool(false)));
        if let Some(d) = first_diff(&Json::Obj(expected), &s.outcome(&w, &[])) {
            panic!("{} :: {}: {d}", c.file, c.name);
        }
    }
}

#[test]
fn global_oracle() {
    check_all("global", replay);
}

/// CHITIN_ORACLE=1: random global stages from the live oracle.
#[test]
fn global_fuzz() {
    fuzz("global", "global.js", replay);
}
