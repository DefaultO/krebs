//! mirror: reference/engine/src/processor/intents/creeps/generateSafeMode.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::constants::SAFE_MODE_COST;
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::{ge, get_or_undef, js_add, obj, or0, out_of_range, prop, room_object, type_is, update, RoomScope};

/// CREEP-GENERATE-SAFE-MODE (no creep type check, as vanilla).
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let Some(target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if !type_is(&target, "controller") {
        return Ok(());
    }
    if out_of_range(object, &target, 1.0) {
        return Ok(());
    }
    let store = object.get(atom!("store"));
    if !truthy(&store) || !ge(&get_or_undef(&store, atom!("G")), &V::Num(SAFE_MODE_COST as f64)) {
        return Ok(());
    }
    update(scope, &target, &obj! { "safeModeAvailable" => js_add(&or0(target.get(atom!("safeModeAvailable"))), &V::Num(1.0)) })?;
    let g = to_number(&get_or_undef(&object.get(atom!("store")), atom!("G"))) - SAFE_MODE_COST as f64;
    update(scope, object, &obj! { "store" => obj! { "G" => V::Num(g) } })
}
