//! mirror: reference/engine/src/processor/intents/creeps/pull.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::intents::movement;
use crate::processor::{out_of_range, prop, room_object, type_is, RoomScope};

/// CREEP-PULL: a pull link to an adjacent creep (MOVE-PULL-LINK).
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") || truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let Some(target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if !type_is(&target, "creep") || truthy(&target.get(atom!("spawning"))) {
        return Ok(());
    }
    if out_of_range(object, &target, 1.0) {
        return Ok(());
    }
    movement::add_pulling(scope, object, &target)
}
