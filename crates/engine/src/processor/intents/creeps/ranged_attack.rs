//! mirror: reference/engine/src/processor/intents/creeps/rangedAttack.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use super::attack::{in_foreign_safe_mode, rampart_at};
use crate::atom;
use crate::constants::{EVENT_ATTACK_TYPE_RANGED, RANGED_ATTACK, RANGED_ATTACK_POWER};
use crate::live::{truthy, Crash, O, V};
use crate::processor::{out_of_range, prop, room_object, type_is, RoomScope};
use crate::utils;

/// CREEP-RANGED-ATTACK.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let Some(mut target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if target.same(object) {
        return Ok(());
    }
    if out_of_range(object, &target, 3.0) {
        return Ok(());
    }
    if type_is(&target, "creep") && truthy(&target.get(atom!("spawning"))) {
        return Ok(());
    }
    if !truthy(&target.get(atom!("hits"))) {
        return Ok(());
    }
    if in_foreign_safe_mode(scope, object) {
        return Ok(());
    }
    if let Some(rampart) = rampart_at(scope, &target) {
        target = rampart;
    }
    let attack_power = utils::calc_body_effectiveness(&object.get(atom!("body")), RANGED_ATTACK, "rangedAttack", RANGED_ATTACK_POWER as f64, false)?;
    super::super::_damage::damage(object, &target, attack_power, EVENT_ATTACK_TYPE_RANGED, scope)
}
