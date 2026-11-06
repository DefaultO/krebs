//! mirror: reference/engine/src/processor/intents/power-creeps/intents.js
//! parity: FAITHFUL
//! Verified against the official processor by the fuzz fixtures in crates/engine/tests/fixtures/oracle/pc.

use super::{drop, enable_room, pickup, r#move, renew, say, transfer, use_power, withdraw};
use crate::doc::Atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::{prop, IntentFn, RoomScope};

/// `creepActions` (intents.js:3, PC-INTENT-ORDER): run in this order when truthy; no priorities.
pub const CREEP_ACTIONS: &[(&str, IntentFn)] = &[
    ("move", r#move::run),
    ("usePower", use_power::run),
    ("withdraw", withdraw::run),
    ("transfer", transfer::run),
    ("say", say::run),
    ("drop", drop::run),
    ("pickup", pickup::run),
    ("enableRoom", enable_room::run),
    ("renew", renew::run),
];

pub fn run(object: &O, object_intents: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    for &(name, f) in CREEP_ACTIONS {
        let intent = prop(object_intents, Atom::intern(name))?;
        if truthy(&intent) {
            f(object, &intent, scope)?;
        }
    }
    Ok(())
}
