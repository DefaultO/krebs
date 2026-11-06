//! mirror: reference/engine/src/processor/intents/creeps/repair.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::constants::{EVENT_REPAIR, REPAIR_COST, REPAIR_POWER, WORK};
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::{
    boost_effect, eq_str, ge, get_or_undef, gt, js_add, js_min, le, lodash_values, obj, out_of_range, prop, push_event, put, room_object, top_boosts_sum, type_is, update,
    xy_of, RoomScope,
};
use crate::utils;

/// CREEP-REPAIR.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    let store = object.get(atom!("store"));
    if truthy(&object.get(atom!("spawning"))) || !truthy(&store) || le(&get_or_undef(&store, atom!("energy")), &V::Num(0.0)) {
        return Ok(());
    }
    let Some(target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if !truthy(&target.get(atom!("hitsMax"))) || utils::construction_cost(&target.get(atom!("type"))).is_none() || ge(&target.get(atom!("hits")), &target.get(atom!("hitsMax"))) {
        return Ok(());
    }
    if out_of_range(object, &target, 3.0) {
        return Ok(());
    }

    let parts = lodash_values(&object.get(atom!("body")));
    let mut work = 0.0;
    for i in &parts {
        let alive = gt(&prop(i, atom!("hits"))?, &V::Num(0.0)) || gt(&get_or_undef(i, atom!("_oldHits")), &V::Num(0.0));
        if alive && eq_str(&get_or_undef(i, atom!("type")), WORK) {
            work += 1.0;
        }
    }
    let repair_power = {
        let p = work * REPAIR_POWER as f64;
        if truthy(&V::Num(p)) {
            p
        } else {
            0.0
        }
    };
    let energy = to_number(&get_or_undef(&object.get(atom!("store")), atom!("energy")));
    let repair_energy_remaining = energy / REPAIR_COST;
    let repair_hits_max = to_number(&target.get(atom!("hitsMax"))) - to_number(&target.get(atom!("hits")));
    let repair_effect = js_min(js_min(repair_power, repair_energy_remaining), repair_hits_max);
    let repair_cost = js_min(energy, (repair_effect * REPAIR_COST).ceil());
    let mut boosted_parts = Vec::new();
    for i in &parts {
        let boost = get_or_undef(i, atom!("boost"));
        let bonus = if eq_str(&prop(i, atom!("type"))?, WORK) && truthy(&boost) && gt(&boost_effect(WORK, &boost, "repair")?, &V::Num(0.0)) {
            (to_number(&boost_effect(WORK, &boost, "repair")?) - 1.0) * REPAIR_POWER as f64
        } else {
            0.0
        };
        boosted_parts.push(bonus);
    }
    let boosted_effect = js_min((repair_effect + top_boosts_sum(boosted_parts, repair_effect)).floor(), repair_hits_max);
    if !truthy(&V::Num(boosted_effect)) {
        return Ok(());
    }

    target.set(atom!("hits"), js_add(&target.get(atom!("hits")), &V::Num(boosted_effect)));
    let store = object.get(atom!("store"));
    put(&store, atom!("energy"), V::Num(to_number(&get_or_undef(&store, atom!("energy"))) - repair_cost))?;
    scope.stats_inc(atom!("energyConstruction"), &object.get(atom!("user")), repair_cost);
    if gt(&target.get(atom!("hits")), &target.get(atom!("hitsMax"))) {
        target.set(atom!("hits"), target.get(atom!("hitsMax")));
    }
    put(&object.get(atom!("actionLog")), atom!("repair"), xy_of(&target))?;
    update(scope, &target, &obj! { "hits" => target.get(atom!("hits")) })?;
    update(scope, object, &obj! { "store" => obj! { "energy" => get_or_undef(&object.get(atom!("store")), atom!("energy")) } })?;
    let data = obj! { "targetId" => target.get(atom!("_id")), "amount" => V::Num(boosted_effect), "energySpent" => V::Num(repair_cost) };
    push_event(scope, EVENT_REPAIR, object.get(atom!("_id")), data);
    Ok(())
}
