//! mirror: reference/engine/src/processor/intents/labs/intents.js
//! parity: FAITHFUL

use super::{boost_creep, reverse_reaction, run_reaction, unboost_creep};
use crate::doc::Atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::intents::_calc_spawns::support::get;
use crate::processor::{IntentFn, RoomScope};

/// intents.js:4-13 (LAB-INTENT-ORDER): boostCreep and unboostCreep each run when truthy; then runReaction,
/// else reverseReaction (only the first truthy of the last two).
pub const ACTIONS: &[(&str, IntentFn)] = &[
    ("boostCreep", boost_creep::run),
    ("unboostCreep", unboost_creep::run),
    ("runReaction", run_reaction::run),
    ("reverseReaction", reverse_reaction::run),
];

pub fn run(object: &O, object_intents: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    for (i, (name, f)) in ACTIONS.iter().enumerate() {
        let intent = get(object_intents, Atom::intern(name))?;
        if truthy(&intent) {
            f(object, &intent, scope)?;
            if i == 2 {
                break; // `else if (objectIntents.reverseReaction)`
            }
        }
    }
    Ok(())
}
