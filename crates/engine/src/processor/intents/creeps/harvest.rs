//! mirror: reference/engine/src/processor/intents/creeps/harvest.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::bulk::Target;
use crate::constants::{DEPOSIT_DECAY_TIME, DEPOSIT_EXHAUST_MULTIPLY, DEPOSIT_EXHAUST_POW, EVENT_HARVEST, EXTRACTOR_COOLDOWN, HARVEST_DEPOSIT_POWER, HARVEST_MINERAL_POWER, HARVEST_POWER, WORK};
use crate::doc::Atom;
use crate::live::{to_js_string, to_number, truthy, Crash, O, V};
use crate::processor::{
    eq_str, get_or_undef, gt, js_add, js_min, loose_eq, obj, obj1, or0, out_of_range, prop, push_event, put, room_object, type_is, update, xy_of, RoomScope,
};
use crate::utils;

/// `require('./drop')(object, {amount, resourceType}, scope)`.
fn drop_overflow(object: &O, amount: f64, resource_type: V, scope: &mut RoomScope) -> Result<(), Crash> {
    super::drop::run(object, &obj! { "amount" => V::Num(amount), "resourceType" => resource_type }, scope)
}

/// CREEP-HARVEST-SOURCE / -MINERAL / -DEPOSIT.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let Some(target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if out_of_range(object, &target, 1.0) {
        return Ok(());
    }
    let game_time = scope.game_time as f64;
    let user = object.get(atom!("user"));

    if type_is(&target, "source") {
        if !truthy(&target.get(atom!("energy"))) {
            return Ok(());
        }
        if let Some(c) = &scope.room_controller {
            let c_user = c.get(atom!("user"));
            let reservation = c.get(atom!("reservation"));
            if truthy(&c_user) && !loose_eq(&c_user, &user) || truthy(&reservation) && !loose_eq(&prop(&reservation, atom!("user"))?, &user) {
                return Ok(());
            }
        }
        let harvest_amount = utils::calc_body_effectiveness(&object.get(atom!("body")), WORK, "harvest", HARVEST_POWER as f64, false)?;
        if truthy(&V::Num(harvest_amount)) {
            let amount = js_min(to_number(&target.get(atom!("energy"))), harvest_amount);
            target.set(atom!("energy"), V::Num(to_number(&target.get(atom!("energy"))) - amount));
            if !truthy(&object.get(atom!("store"))) {
                object.set(atom!("store"), V::Obj(O::new()));
            }
            let store = object.get(atom!("store"));
            put(&store, atom!("energy"), js_add(&or0(get_or_undef(&store, atom!("energy"))), &V::Num(amount)))?;
            let invader_harvested = js_add(&or0(target.get(atom!("invaderHarvested"))), &V::Num(amount));
            update(scope, object, &obj! { "store" => obj! { "energy" => get_or_undef(&object.get(atom!("store")), atom!("energy")) } })?;
            update(scope, &target, &obj! { "energy" => target.get(atom!("energy")), "invaderHarvested" => invader_harvested })?;

            let sum = utils::calc_resources(&V::Obj(object.clone()))?;
            if gt(&V::Num(sum), &object.get(atom!("storeCapacity"))) {
                let energy = to_number(&get_or_undef(&object.get(atom!("store")), atom!("energy")));
                drop_overflow(object, js_min(energy, sum - to_number(&object.get(atom!("storeCapacity")))), V::str("energy"), scope)?;
            }
            put(&object.get(atom!("actionLog")), atom!("harvest"), xy_of(&target))?;
            scope.stats_inc(atom!("energyHarvested"), &user, amount);
            push_event(scope, EVENT_HARVEST, object.get(atom!("_id")), obj! { "targetId" => target.get(atom!("_id")), "amount" => V::Num(amount) });
        }
    }

    if type_is(&target, "mineral") {
        if !truthy(&target.get(atom!("mineralAmount"))) {
            return Ok(());
        }
        let extractor = scope.room_objects.near(&target.get(atom!("x")), &target.get(atom!("y"))).find(|&i| {
            eq_str(&i.get(atom!("type")), "extractor") && loose_eq(&i.get(atom!("x")), &target.get(atom!("x"))) && loose_eq(&i.get(atom!("y")), &target.get(atom!("y")))
        }).cloned();
        let Some(extractor) = extractor else { return Ok(()) };
        let e_user = extractor.get(atom!("user"));
        if truthy(&e_user) && !loose_eq(&e_user, &user) {
            return Ok(());
        }
        if !utils::check_structure_against_controller(&extractor, &scope.room_objects, scope.room_controller.as_ref())? {
            return Ok(());
        }
        if truthy(&extractor.get(atom!("cooldown"))) {
            return Ok(());
        }
        let harvest_amount = utils::calc_body_effectiveness(&object.get(atom!("body")), WORK, "harvest", HARVEST_MINERAL_POWER as f64, false)?;
        if truthy(&V::Num(harvest_amount)) {
            let amount = js_min(to_number(&target.get(atom!("mineralAmount"))), harvest_amount);
            if !truthy(&object.get(atom!("store"))) {
                object.set(atom!("store"), V::Obj(O::new()));
            }
            let mineral_type = target.get(atom!("mineralType"));
            let mt = Atom::intern(&to_js_string(&mineral_type));
            update(scope, &target, &obj! { "mineralAmount" => V::Num(to_number(&target.get(atom!("mineralAmount"))) - amount) })?;
            let next = js_add(&or0(get_or_undef(&object.get(atom!("store")), mt)), &V::Num(amount));
            update(scope, object, &obj1(atom!("store"), obj1(mt, next)))?;

            let sum = utils::calc_resources(&V::Obj(object.clone()))?;
            if gt(&V::Num(sum), &object.get(atom!("storeCapacity"))) {
                let have = to_number(&get_or_undef(&object.get(atom!("store")), mt));
                drop_overflow(object, js_min(have, sum - to_number(&object.get(atom!("storeCapacity")))), mineral_type, scope)?;
            }
            put(&object.get(atom!("actionLog")), atom!("harvest"), xy_of(&target))?;
            extractor.set(atom!("_cooldown"), V::Num(EXTRACTOR_COOLDOWN as f64));
            push_event(scope, EVENT_HARVEST, object.get(atom!("_id")), obj! { "targetId" => target.get(atom!("_id")), "amount" => V::Num(harvest_amount) });
        }
    }

    if type_is(&target, "deposit") {
        let cooldown_time = target.get(atom!("cooldownTime"));
        if truthy(&cooldown_time) && gt(&cooldown_time, &V::Num(game_time)) {
            return Ok(());
        }
        let amount = utils::calc_body_effectiveness(&object.get(atom!("body")), WORK, "harvest", HARVEST_DEPOSIT_POWER as f64, false)?;
        let deposit_type = target.get(atom!("depositType"));
        let dt = Atom::intern(&to_js_string(&deposit_type));
        let next = js_add(&or0(prop(&object.get(atom!("store")), dt)?), &V::Num(amount));
        update(scope, object, &obj1(atom!("store"), obj1(dt, next)))?;

        let sum = utils::calc_resources(&V::Obj(object.clone()))?;
        if gt(&V::Num(sum), &object.get(atom!("storeCapacity"))) {
            let have = to_number(&get_or_undef(&object.get(atom!("store")), dt));
            drop_overflow(object, js_min(have, sum - to_number(&object.get(atom!("storeCapacity")))), deposit_type, scope)?;
        }
        put(&object.get(atom!("actionLog")), atom!("harvest"), xy_of(&target))?;
        scope.bulk.inc(Target::Obj(&target), &[atom!("harvested")], amount);
        let cooldown = (DEPOSIT_EXHAUST_MULTIPLY * crate::processor::js_pow(to_number(&target.get(atom!("harvested"))), DEPOSIT_EXHAUST_POW)).ceil();
        if cooldown > 1.0 {
            target.set(atom!("_cooldown"), V::Num(cooldown));
        }
        update(scope, &target, &obj! { "decayTime" => V::Num(DEPOSIT_DECAY_TIME as f64 + game_time) })?;
    }
    Ok(())
}
