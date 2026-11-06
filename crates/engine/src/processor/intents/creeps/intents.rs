//! mirror: reference/engine/src/processor/intents/creeps/intents.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use super::{
    attack, attack_controller, build, claim_controller, dismantle, drop, generate_safe_mode, harvest, heal, pickup, pull,
    r#move, ranged_attack, ranged_heal, ranged_mass_attack, repair, reserve_controller, say, sign_controller, suicide,
    transfer, upgrade_controller, withdraw,
};
use crate::doc::Atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::{prop, IntentFn, RoomScope};
use std::sync::OnceLock;

/// `creepActions` (intents.js:15-17, CREEP-INTENT-ORDER): one creep's intents run in this order.
pub const CREEP_ACTIONS: &[(&str, IntentFn)] = &[
    ("drop", drop::run),
    ("transfer", transfer::run),
    ("withdraw", withdraw::run),
    ("pickup", pickup::run),
    ("heal", heal::run),
    ("rangedHeal", ranged_heal::run),
    ("dismantle", dismantle::run),
    ("attack", attack::run),
    ("harvest", harvest::run),
    ("move", r#move::run),
    ("repair", repair::run),
    ("build", build::run),
    ("rangedMassAttack", ranged_mass_attack::run),
    ("rangedAttack", ranged_attack::run),
    ("say", say::run),
    ("suicide", suicide::run),
    ("claimController", claim_controller::run),
    ("upgradeController", upgrade_controller::run),
    ("reserveController", reserve_controller::run),
    ("attackController", attack_controller::run),
    ("generateSafeMode", generate_safe_mode::run),
    ("signController", sign_controller::run),
    ("pull", pull::run),
];

/// `priorities` (intents.js:3-13, CREEP-INTENT-PRIORITY): an intent is skipped when any of these is present.
pub const PRIORITIES: &[(&str, &[&str])] = &[
    ("rangedHeal", &["heal"]),
    ("attackController", &["rangedHeal", "heal"]),
    ("dismantle", &["attackController", "rangedHeal", "heal"]),
    ("repair", &["dismantle", "attackController", "rangedHeal", "heal"]),
    ("build", &["repair", "dismantle", "attackController", "rangedHeal", "heal"]),
    ("attack", &["build", "repair", "dismantle", "attackController", "rangedHeal", "heal"]),
    ("harvest", &["attack", "build", "repair", "dismantle", "attackController", "rangedHeal", "heal"]),
    ("rangedMassAttack", &["build", "repair", "rangedHeal"]),
    ("rangedAttack", &["rangedMassAttack", "build", "repair", "rangedHeal"]),
];

/// `CREEP_ACTIONS` as atoms, each with its `PRIORITIES` entry.
struct Action {
    name: Atom,
    run: IntentFn,
    blockers: Vec<Atom>,
}

fn actions() -> &'static [Action] {
    static TABLE: OnceLock<Vec<Action>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let blockers = |name: &str| PRIORITIES.iter().find(|(n, _)| *n == name).map_or(Vec::new(), |(_, b)| b.iter().map(|b| Atom::intern(b)).collect());
        CREEP_ACTIONS.iter().map(|&(name, run)| Action { name: Atom::intern(name), run, blockers: blockers(name) }).collect()
    })
}

/// intents.js:21 checkPriorities.
fn check_priorities(intents: &V, action: &Action) -> Result<bool, Crash> {
    if !truthy(&prop(intents, action.name)?) {
        return Ok(false);
    }
    for &b in &action.blockers {
        if truthy(&prop(intents, b)?) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn run(object: &O, object_intents: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    for action in actions() {
        if check_priorities(object_intents, action)? {
            (action.run)(object, &prop(object_intents, action.name)?, scope)?;
        }
    }
    Ok(())
}
