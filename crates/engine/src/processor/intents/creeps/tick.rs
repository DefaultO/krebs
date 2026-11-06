//! mirror: reference/engine/src/processor/intents/creeps/tick.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).
//! A spawning creep on a spawn delegates to spawns/_born-creep (another package), which the fixtures do not reach.
//! BUG-DIE-DOUBLE (R) is replicated: nothing returns after a death.

use crate::atom;
use crate::constants::{CLAIM, CREEP_CLAIM_LIFE_TIME, CREEP_LIFE_TIME, EVENT_EXIT};
use crate::live::{is_equal, to_js_string, to_number, truthy, Crash, O, V};
use crate::processor::intents::movement;
use crate::processor::{
    boosts_entry, bulk_remove, delete_room_object, eq_str, get_or_undef, gt, js_add, js_min, js_round, lodash_match_str, lodash_values, loose_eq, lt, obj, prop, push_event,
    read_crash, room_object_list, type_is, update, RoomScope,
};
use crate::utils;

/// tick.js:7 _applyDamage: TOUGH boosts reduce the summed damage (DMG-TOUGH).
fn apply_damage(object: &O, damage: f64) -> Result<(), Crash> {
    let mut damage = damage;
    let mut damage_reduce = 0.0;
    let mut damage_effective = damage;
    let body = object.get(atom!("body"));
    let mut boosted = false;
    for i in lodash_values(&body) {
        if truthy(&prop(&i, atom!("boost"))?) {
            boosted = true;
            break;
        }
    }
    if boosted {
        let len = to_number(&prop(&body, atom!("length"))?);
        let mut i = 0.0;
        while i < len {
            if damage_effective <= 0.0 {
                break;
            }
            let part = crate::processor::get_index(&body, i);
            i += 1.0;
            let mut damage_ratio = 1.0;
            let boost = prop(&part, atom!("boost"))?;
            if truthy(&boost) {
                let ty = to_js_string(&get_or_undef(&part, atom!("type")));
                if let Some(d) = boosts_entry(&ty, &boost)?.and_then(|e| e.iter().find(|(m, _)| *m == "damage").map(|(_, d)| *d)) {
                    damage_ratio = d;
                }
            }
            let hits_effective = to_number(&get_or_undef(&part, atom!("hits"))) / damage_ratio;
            damage_reduce += js_min(hits_effective, damage_effective) * (1.0 - damage_ratio);
            damage_effective -= js_min(hits_effective, damage_effective);
        }
    }
    damage -= js_round(damage_reduce);
    object.set(atom!("hits"), V::Num(to_number(&object.get(atom!("hits"))) - damage));
    Ok(())
}

/// CREEP-TICK-ORDER.
pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    let game_time = scope.game_time as f64;

    if truthy(&object.get(atom!("spawning"))) {
        let spawn = scope.room_objects.near(&object.get(atom!("x")), &object.get(atom!("y"))).find(|&o| {
            loose_eq(&o.get(atom!("x")), &object.get(atom!("x"))) && loose_eq(&o.get(atom!("y")), &object.get(atom!("y"))) && (type_is(o, "spawn") || type_is(o, "invaderCore"))
        }).cloned();
        match spawn {
            None => {
                bulk_remove(&mut scope.bulk, &object.get(atom!("_id")));
                delete_room_object(scope, &object.get(atom!("_id")));
            }
            Some(spawn) => {
                let spawning = spawn.get(atom!("spawning"));
                if !truthy(&spawning) || !loose_eq(&get_or_undef(&spawning, atom!("name")), &object.get(atom!("name"))) {
                    crate::processor::intents::spawns::_born_creep::born_creep(&spawn, object, scope)?;
                }
            }
        }
    } else {
        movement::execute(object, scope)?;

        let user = object.get(atom!("user"));
        if utils::is_at_edge(&V::Obj(object.clone())) && !eq_str(&user, "2") && !eq_str(&user, "3") {
            let (room_x, room_y) = utils::room_name_to_xy(&to_js_string(&object.get(atom!("room"))));
            let (mut x, mut y, mut room) = (object.get(atom!("x")), object.get(atom!("y")), object.get(atom!("room")));
            let at = |k, n: f64| loose_eq(&object.get(k), &V::Num(n));
            if at(atom!("x"), 0.0) {
                x = V::Num(49.0);
                room = V::str(&utils::get_room_name_from_xy(room_x - 1.0, room_y));
            } else if at(atom!("y"), 0.0) {
                y = V::Num(49.0);
                room = V::str(&utils::get_room_name_from_xy(room_x, room_y - 1.0));
            } else if at(atom!("x"), 49.0) {
                x = V::Num(0.0);
                room = V::str(&utils::get_room_name_from_xy(room_x + 1.0, room_y));
            } else if at(atom!("y"), 49.0) {
                y = V::Num(0.0);
                room = V::str(&utils::get_room_name_from_xy(room_x, room_y + 1.0));
            }
            update(scope, object, &obj! { "interRoom" => obj! { "room" => room.clone(), "x" => x.clone(), "y" => y.clone() } })?;
            push_event(scope, EVENT_EXIT, object.get(atom!("_id")), obj! { "room" => room, "x" => x, "y" => y });
        }

        // since NPC creeps may appear right on portals without `ageTime` defined at the first tick
        if truthy(&object.get(atom!("ageTime"))) {
            let portal = scope
                .room_objects
                .near(&object.get(atom!("x")), &object.get(atom!("y")))
                .find(|&i| type_is(i, "portal") && loose_eq(&i.get(atom!("x")), &object.get(atom!("x"))) && loose_eq(&i.get(atom!("y")), &object.get(atom!("y"))))
                .cloned();
            if let Some(portal) = portal {
                update(scope, object, &obj! { "interRoom" => portal.get(atom!("destination")) })?;
            }
        }

        if !truthy(&object.get(atom!("tutorial"))) {
            if !truthy(&object.get(atom!("ageTime"))) {
                let claim = lodash_values(&object.get(atom!("body"))).iter().any(|b| lodash_match_str(b, atom!("type"), CLAIM));
                let life = if claim { CREEP_CLAIM_LIFE_TIME } else { CREEP_LIFE_TIME } as f64;
                object.set(atom!("ageTime"), V::Num(game_time + life));
                update(scope, object, &obj! { "ageTime" => object.get(atom!("ageTime")) })?;
            }
            if game_time >= to_number(&object.get(atom!("ageTime"))) - 1.0 {
                super::_die::die(object, None, false, scope, None)?;
            }
        }

        if !is_equal(&object.get(atom!("actionLog")), &object.get(atom!("_actionLog"))) {
            update(scope, object, &obj! { "actionLog" => object.get(atom!("actionLog")) })?;
        }
    }

    let moves = movement::move_power(&object.get(atom!("body")))?;
    if moves > 0.0 {
        super::_add_fatigue::add_fatigue(object, -2.0 * moves, scope)?;
    }

    // _.isNaN: only the number NaN.
    let hits = object.get(atom!("hits"));
    if matches!(hits, V::Num(n) if n.is_nan()) || crate::processor::le(&hits, &V::Num(0.0)) {
        super::_die::die(object, None, true, scope, None)?;
    }

    if truthy(&object.get(atom!("userSummoned"))) {
        let mut foreign = false;
        for i in room_object_list(scope) {
            if type_is(&i, "creep") && !eq_str(&i.get(atom!("user")), "2") {
                let Some(c) = &scope.room_controller else { return Err(read_crash(&V::Undef, "user")) };
                if !loose_eq(&i.get(atom!("user")), &c.get(atom!("user"))) {
                    foreign = true;
                    break;
                }
            }
        }
        if foreign {
            super::_die::die(object, None, false, scope, None)?;
        }
    }

    let old_hits = object.get(atom!("hits"));
    let damage = object.get(atom!("_damageToApply"));
    if truthy(&damage) {
        apply_damage(object, to_number(&damage))?;
        object.delete(atom!("_damageToApply"));
    }
    let heal = object.get(atom!("_healToApply"));
    if truthy(&heal) {
        object.set(atom!("hits"), js_add(&object.get(atom!("hits")), &heal));
        object.delete(atom!("_healToApply"));
    }
    if gt(&object.get(atom!("hits")), &object.get(atom!("hitsMax"))) {
        object.set(atom!("hits"), object.get(atom!("hitsMax")));
    }

    if crate::processor::le(&object.get(atom!("hits")), &V::Num(0.0)) {
        super::_die::die(object, None, true, scope, None)?;
    } else if !loose_eq(&object.get(atom!("hits")), &old_hits) {
        super::_recalc_body::recalc_body(object)?;
        if lt(&object.get(atom!("hits")), &old_hits) {
            super::_drop_resources_without_space::drop_resources_without_space(object, scope)?;
        }
        let data = obj! {
            "hits" => object.get(atom!("hits")),
            "body" => object.get(atom!("body")),
            "storeCapacity" => object.get(atom!("storeCapacity")),
        };
        update(scope, object, &data)?;
    }
    Ok(())
}
