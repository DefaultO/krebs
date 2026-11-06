//! mirror: reference/engine/src/processor/intents/invader-core/intents.js
//! parity: FAITHFUL

use super::{attack_controller, create_creep, reserve_controller, transfer, upgrade_controller};
use crate::doc::Atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::intents::_calc_spawns::support::get;
use crate::processor::{IntentFn, RoomScope};

/// intents.js:2-15: each runs when truthy, in this order.
pub const ACTIONS: &[(&str, IntentFn)] = &[
    ("transfer", transfer::run),
    ("createCreep", create_creep::run),
    ("reserveController", reserve_controller::run),
    ("attackController", attack_controller::run),
    ("upgradeController", upgrade_controller::run),
];

pub fn run(object: &O, object_intents: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    for (name, f) in ACTIONS {
        let intent = get(object_intents, Atom::intern(name))?;
        if truthy(&intent) {
            f(object, &intent, scope)?;
        }
    }
    Ok(())
}
