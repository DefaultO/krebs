//! mirror: reference/engine/src/processor/intents/creeps/dismantle.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).
//! BUG-DISMANTLE-NAN (R) is replicated.

use super::attack::{in_foreign_safe_mode, rampart_at};
use crate::atom;
use crate::constants::{DISMANTLE_COST, DISMANTLE_POWER, EFFECT_INVULNERABILITY, EVENT_ATTACK_TYPE_DISMANTLE, PWR_FORTIFY, PWR_SHIELD, WORK};
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::{ge, get_or_undef, gt, js_add, js_min, lodash_values, loose_eq, obj, out_of_range, prop, put, room_object, type_is, update, RoomScope};
use crate::utils;

/// CREEP-DISMANTLE.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let Some(mut target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if utils::construction_cost(&target.get(atom!("type"))).is_none() {
        return Ok(());
    }
    if out_of_range(object, &target, 1.0) {
        return Ok(());
    }
    if in_foreign_safe_mode(scope, object) {
        return Ok(());
    }
    if let Some(rampart) = rampart_at(scope, &target) {
        target = rampart;
    }

    let power = utils::calc_body_effectiveness(&object.get(atom!("body")), WORK, "dismantle", DISMANTLE_POWER as f64, false)?;
    let amount = js_min(power, to_number(&target.get(atom!("hits"))));
    let mut energy_gain = (amount * DISMANTLE_COST).floor();

    let game_time = V::Num(scope.game_time as f64);
    for e in lodash_values(&target.get(atom!("effects"))) {
        let power = |p| loose_eq(&get_or_undef(&e, atom!("power")), &V::Num(p as f64));
        if ge(&prop(&e, atom!("endTime"))?, &game_time) && (power(PWR_SHIELD) || power(PWR_FORTIFY) || loose_eq(&get_or_undef(&e, atom!("effect")), &V::Num(EFFECT_INVULNERABILITY as f64))) {
            energy_gain = 0.0;
            break;
        }
    }

    if truthy(&V::Num(amount)) {
        if !truthy(&object.get(atom!("store"))) {
            object.set(atom!("store"), V::Obj(crate::live::O::new()));
        }
        let store = object.get(atom!("store"));
        put(&store, atom!("energy"), js_add(&get_or_undef(&store, atom!("energy")), &V::Num(energy_gain)))?;
        update(scope, object, &obj! { "store" => obj! { "energy" => get_or_undef(&object.get(atom!("store")), atom!("energy")) } })?;

        let used_space = utils::calc_resources(&V::Obj(object.clone()))?;
        if gt(&V::Num(used_space), &object.get(atom!("storeCapacity"))) {
            let over = used_space - to_number(&object.get(atom!("storeCapacity")));
            super::drop::run(object, &obj! { "amount" => V::Num(over), "resourceType" => V::str("energy") }, scope)?;
        }
        super::super::_damage::damage(object, &target, amount, EVENT_ATTACK_TYPE_DISMANTLE, scope)?;
    }
    Ok(())
}
