//! Cron oracle fixtures (kind "cron", made by `node tools/oracle/cron.js gen …` from the official
//! backend/lib/cronjobs.js over the real storage db.js): the scenario format round-trips, and every case
//! is replayed through `CronScenario::run` (one built-in job of engine::cron::JOBS on the scenario world).
//! Enforcement rules and the CHITIN_ORACLE* environment: oracle_support/mod.rs.

mod oracle_support;

use engine::scenario::CronScenario;
use oracle_support::{cases, check_all, verdict, Case, Verdict};

fn replay(c: &Case) -> Verdict {
    verdict(c, || CronScenario::from_json(&c.scenario)?.run())
}

#[test]
fn cron_scenarios_round_trip() {
    let cases = cases("cron");
    assert!(!cases.is_empty(), "no cron oracle fixtures found");
    for c in &cases {
        let s = CronScenario::from_json(&c.scenario).unwrap_or_else(|e| panic!("{} {}: {e}", c.file, c.name));
        assert_eq!(s.to_json(), c.scenario, "{} {}: scenario does not round-trip", c.file, c.name);
        s.world.world().unwrap_or_else(|e| panic!("{} {}: {e}", c.file, c.name));
    }
}

/// Cron cases reach backend/lib/utils.js, which backend/src/png.rs mirrors for the map; only engine mirrors
/// may keep an engine case pending.
#[test]
fn only_engine_mirrors_gate_cases() {
    let m = oracle_support::mirrors();
    assert!(m.contains_key("reference/engine/src/processor.js"));
    assert!(m.values().all(|(f, _)| f.starts_with("crates/engine/src/")), "{m:?}");
}

#[test]
fn cron_oracle() {
    check_all("cron", replay);
}

/// CHITIN_ORACLE=1: random cron runs (every job) from the live oracle.
#[test]
fn cron_fuzz() {
    oracle_support::fuzz("cron", "cron.js", replay);
}
