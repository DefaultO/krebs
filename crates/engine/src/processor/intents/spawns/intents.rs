//! mirror: reference/engine/src/processor/intents/spawns/intents.js
//! parity: FAITHFUL

use super::{cancel_spawning, create_creep, recycle_creep, renew_creep, set_spawn_directions};
use crate::doc::Atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::intents::_calc_spawns::support::get;
use crate::processor::{IntentFn, RoomScope};

/// intents.js:3-16 (SPAWN-INTENT-ORDER): each runs when truthy, in this order.
pub const ACTIONS: &[(&str, IntentFn)] = &[
    ("createCreep", create_creep::run),
    ("renewCreep", renew_creep::run),
    ("recycleCreep", recycle_creep::run),
    ("setSpawnDirections", set_spawn_directions::run),
    ("cancelSpawning", cancel_spawning::run),
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
