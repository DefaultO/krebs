//! mirror: reference/engine/src/processor/intents/links/intents.js
//! parity: FAITHFUL

use super::transfer;
use crate::doc::Atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::intents::_calc_spawns::support::get;
use crate::processor::{IntentFn, RoomScope};

/// intents.js:4-5: each runs when truthy.
pub const ACTIONS: &[(&str, IntentFn)] = &[("transfer", transfer::run)];

pub fn run(object: &O, object_intents: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    for (name, f) in ACTIONS {
        let intent = get(object_intents, Atom::intern(name))?;
        if truthy(&intent) {
            f(object, &intent, scope)?;
        }
    }
    Ok(())
}
