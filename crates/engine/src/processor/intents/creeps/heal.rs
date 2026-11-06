//! mirror: reference/engine/src/processor/intents/creeps/heal.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use super::attack::in_foreign_safe_mode;
use crate::atom;
use crate::constants::{EVENT_HEAL, EVENT_HEAL_TYPE_MELEE, EVENT_HEAL_TYPE_RANGED, HEAL, HEAL_POWER, RANGED_HEAL_POWER};
use crate::live::{truthy, Crash, O, V};
use crate::processor::{js_add, obj, or0, out_of_range, prop, push_event, put, room_object, type_is, xy_of, RoomScope};
use crate::utils;

/// CREEP-HEAL (`ranged == false`) and CREEP-RANGED-HEAL (rangedHeal.js, the same statements with range 3).
pub(crate) fn heal(object: &O, intent: &V, scope: &mut RoomScope, ranged: bool) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let Some(target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if !type_is(&target, "creep") && !type_is(&target, "powerCreep") || truthy(&target.get(atom!("spawning"))) {
        return Ok(());
    }
    if out_of_range(object, &target, if ranged { 3.0 } else { 1.0 }) {
        return Ok(());
    }
    if in_foreign_safe_mode(scope, object) {
        return Ok(());
    }
    let (method, base, log_key, heal_type) = if ranged {
        ("rangedHeal", RANGED_HEAL_POWER, atom!("rangedHeal"), EVENT_HEAL_TYPE_RANGED)
    } else {
        ("heal", HEAL_POWER, atom!("heal"), EVENT_HEAL_TYPE_MELEE)
    };
    let heal_power = utils::calc_body_effectiveness(&object.get(atom!("body")), HEAL, method, base as f64, false)?;
    target.set(atom!("_healToApply"), js_add(&or0(target.get(atom!("_healToApply"))), &V::Num(heal_power)));
    put(&object.get(atom!("actionLog")), log_key, xy_of(&target))?;
    put(&target.get(atom!("actionLog")), atom!("healed"), xy_of(object))?;
    let data = obj! { "targetId" => target.get(atom!("_id")), "amount" => V::Num(heal_power), "healType" => V::Num(heal_type as f64) };
    push_event(scope, EVENT_HEAL, object.get(atom!("_id")), data);
    Ok(())
}

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    heal(object, intent, scope, false)
}
