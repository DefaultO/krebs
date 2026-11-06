//! mirror: reference/engine/src/processor/intents/towers/intents.js
//! parity: FAITHFUL

use super::{attack, heal, repair};
use crate::doc::Atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::intents::_calc_spawns::support::get;
use crate::processor::{IntentFn, RoomScope};

/// intents.js:4-9 (TOWER-INTENT-ORDER): an `if / else if` chain, only the first truthy one runs.
pub const ACTIONS: &[(&str, IntentFn)] = &[("heal", heal::run), ("repair", repair::run), ("attack", attack::run)];

pub fn run(object: &O, object_intents: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    for (name, f) in ACTIONS {
        let intent = get(object_intents, Atom::intern(name))?;
        if truthy(&intent) {
            return f(object, &intent, scope);
        }
    }
    Ok(())
}
