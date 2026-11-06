//! mirror: reference/engine/src/processor/intents/creeps/upgradeController.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).
//! `config.ptr` is false.

use crate::atom;
use crate::bulk::Target;
use crate::constants::{
    power_info, CONTROLLER_DOWNGRADE, CONTROLLER_DOWNGRADE_RESTORE, CONTROLLER_LEVELS, CONTROLLER_MAX_UPGRADE_PER_TICK, EVENT_UPGRADE_CONTROLLER, PWR_OPERATE_CONTROLLER,
    UPGRADE_CONTROLLER_POWER, WORK,
};
use crate::live::{strict_eq, to_js_string, to_number, truthy, Crash, O, V};
use crate::processor::{
    boost_effect, by_level, eq_str, ge, get_or_undef, gt, js_add, js_min, le, lodash_find, lodash_match, lodash_values, loose_eq, lt, obj, or0, out_of_range, prop, push_event,
    put, room_object, top_boosts_sum, type_is, update, xy_of, RoomScope,
};

/// CREEP-UPGRADE-CONTROLLER.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let store = object.get(atom!("store"));
    if !type_is(object, "creep") || truthy(&object.get(atom!("spawning"))) || !truthy(&store) || le(&get_or_undef(&store, atom!("energy")), &V::Num(0.0)) {
        return Ok(());
    }
    let user = object.get(atom!("user"));
    let user_doc = scope.user_doc(&user);
    if truthy(&user_doc) && strict_eq(&get_or_undef(&user_doc, atom!("shardAccess")), &V::Bool(false)) {
        return Ok(());
    }
    let Some(target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if !type_is(&target, "controller") {
        return Ok(());
    }
    if out_of_range(object, &target, 3.0) {
        return Ok(());
    }
    if loose_eq(&target.get(atom!("level")), &V::Num(0.0)) || !loose_eq(&target.get(atom!("user")), &user) {
        return Ok(());
    }
    let game_time = scope.game_time as f64;
    let blocked = target.get(atom!("upgradeBlocked"));
    if truthy(&blocked) && gt(&blocked, &V::Num(game_time)) {
        return Ok(());
    }

    target.set(atom!("_upgraded"), or0(target.get(atom!("_upgraded"))));

    let body = object.get(atom!("body"));
    let mut work = 0.0;
    for i in lodash_values(&body) {
        let alive = gt(&prop(&i, atom!("hits"))?, &V::Num(0.0)) || gt(&get_or_undef(&i, atom!("_oldHits")), &V::Num(0.0));
        if alive && eq_str(&get_or_undef(&i, atom!("type")), WORK) {
            work += 1.0;
        }
    }
    let build_power = {
        let p = work * UPGRADE_CONTROLLER_POWER as f64;
        if truthy(&V::Num(p)) {
            p
        } else {
            0.0
        }
    };
    let mut build_effect = js_min(build_power, to_number(&get_or_undef(&object.get(atom!("store")), atom!("energy"))));
    let mut boosted_parts = Vec::new();
    for i in lodash_values(&body) {
        let boost = get_or_undef(&i, atom!("boost"));
        let bonus = if eq_str(&prop(&i, atom!("type"))?, WORK) && truthy(&boost) && gt(&boost_effect(WORK, &boost, "upgradeController")?, &V::Num(0.0)) {
            to_number(&boost_effect(WORK, &boost, "upgradeController")?) - 1.0
        } else {
            0.0
        };
        boosted_parts.push(bonus);
    }

    if loose_eq(&target.get(atom!("level")), &V::Num(8.0)) {
        let mut limit = CONTROLLER_MAX_UPGRADE_PER_TICK as f64;
        let effect = lodash_find(&target.get(atom!("effects")), |e| lodash_match(e, atom!("power"), &V::Num(PWR_OPERATE_CONTROLLER as f64)));
        if let Some(effect) = effect {
            if ge(&prop(&effect, atom!("endTime"))?, &V::Num(game_time)) {
                let effects = power_info(PWR_OPERATE_CONTROLLER).and_then(|p| p.effect).expect("PWR_OPERATE_CONTROLLER has effects");
                let table: Vec<Option<f64>> = effects.iter().map(|&x| Some(x)).collect();
                limit += by_level(&table, &V::Num(to_number(&prop(&effect, atom!("level"))?) - 1.0));
            }
        }
        if ge(&target.get(atom!("_upgraded")), &V::Num(limit)) {
            return Ok(());
        }
        build_effect = js_min(build_effect, limit - to_number(&target.get(atom!("_upgraded"))));
    }

    let boosted_effect = (build_effect + top_boosts_sum(boosted_parts, build_effect)).floor();

    if lt(&target.get(atom!("level")), &V::Num(8.0)) {
        let mut next_level_progress = by_level(&CONTROLLER_LEVELS, &target.get(atom!("level")));
        if truthy(&target.get(atom!("tutorial"))) && loose_eq(&target.get(atom!("level")), &V::Num(1.0)) {
            next_level_progress = 4.0;
        }
        let progress_after = js_add(&target.get(atom!("progress")), &V::Num(boosted_effect));
        let restore = js_add(&target.get(atom!("downgradeTime")), &V::Num(CONTROLLER_DOWNGRADE_RESTORE as f64));
        let downgrade_now = game_time + by_level(&CONTROLLER_DOWNGRADE, &target.get(atom!("level")));
        if ge(&progress_after, &V::Num(next_level_progress)) && ge(&restore, &V::Num(downgrade_now)) {
            target.set(atom!("progress"), V::Num(to_number(&progress_after) - next_level_progress));
            target.set(atom!("level"), V::Num(to_number(&target.get(atom!("level"))) + 1.0));
            target.set(atom!("downgradeTime"), V::Num(game_time + by_level(&CONTROLLER_DOWNGRADE, &target.get(atom!("level"))) / 2.0));
            let message = format!(
                "Your Controller in room {} has been upgraded to level {}.",
                to_js_string(&target.get(atom!("room"))),
                to_js_string(&target.get(atom!("level")))
            );
            scope.send_notification(&target.get(atom!("user")), message);
            if loose_eq(&target.get(atom!("level")), &V::Num(8.0)) {
                target.set(atom!("progress"), V::Num(0.0));
            }
            target.set(atom!("safeModeAvailable"), js_add(&or0(target.get(atom!("safeModeAvailable"))), &V::Num(1.0)));
        } else {
            target.set(atom!("progress"), js_add(&target.get(atom!("progress")), &V::Num(boosted_effect)));
        }
    }

    let target_user = target.get(atom!("user"));
    if truthy(&target_user) {
        scope.bulk_users.inc(Target::Id(&to_js_string(&target_user)), &[atom!("gcl")], boosted_effect);
    }
    target.set(atom!("_upgraded"), js_add(&target.get(atom!("_upgraded")), &V::Num(build_effect)));
    scope.stats_inc(atom!("energyControl"), &user, boosted_effect);
    let store = object.get(atom!("store"));
    put(&store, atom!("energy"), V::Num(to_number(&get_or_undef(&store, atom!("energy"))) - build_effect))?;
    put(&object.get(atom!("actionLog")), atom!("upgradeController"), xy_of(&target))?;
    update(scope, object, &obj! { "store" => obj! { "energy" => get_or_undef(&object.get(atom!("store")), atom!("energy")) } })?;
    let data = obj! {
        "level" => target.get(atom!("level")),
        "progress" => target.get(atom!("progress")),
        "safeModeAvailable" => target.get(atom!("safeModeAvailable")),
        "downgradeTime" => target.get(atom!("downgradeTime")),
    };
    update(scope, &target, &data)?;
    push_event(scope, EVENT_UPGRADE_CONTROLLER, object.get(atom!("_id")), obj! { "amount" => V::Num(boosted_effect), "energySpent" => V::Num(build_effect) });
    Ok(())
}
