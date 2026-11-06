//! mirror: reference/engine/src/processor/intents/_damage.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).
//! The destruction branch delegates to structures/_destroy (another package), which the fixtures do not reach.
//! BUG-CRASH-NUKE-PB (C): an attacker without `actionLog` (a nuke) crashes the hit-back block under `private`
//! (as vanilla), and skips it where the row is fixed.

use crate::atom;
use crate::constants::{
    ATTACK, ATTACK_POWER, EFFECT_INVULNERABILITY, EVENT_ATTACK, EVENT_ATTACK_TYPE_DISMANTLE, EVENT_ATTACK_TYPE_HIT_BACK, EVENT_ATTACK_TYPE_MELEE,
    EVENT_ATTACK_TYPE_NUKE, EVENT_ATTACK_TYPE_RANGED, EVENT_OBJECT_DESTROYED, POWER_BANK_HIT_BACK, PWR_FORTIFY,
};
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::{eq_str, get_or_undef, gt, js_add, lodash_match, lodash_match_str, lodash_values, loose_eq, obj, or0, prop, push_event, put, type_is, update, xy_of, RoomScope};
use crate::utils;

/// DMG-QUEUE: creeps and power creeps queue `_damageToApply`; structures lose hits now.
pub fn damage(object: &O, target: &O, damage: f64, attack_type: u32, scope: &mut RoomScope) -> Result<(), Crash> {
    if !truthy(&target.get(atom!("_id"))) || !truthy(&target.get(atom!("hits"))) {
        return Ok(());
    }
    let game_time = V::Num(scope.game_time as f64);
    let mut attack_back_power = 0.0;
    let is_creep = type_is(target, "creep");
    let is_pc = type_is(target, "powerCreep");

    if is_creep {
        let (ox, oy) = (object.get(atom!("x")), object.get(atom!("y")));
        let under_rampart = scope.room_objects.near(&ox, &oy).any(|i| {
            let i = V::Obj(i.clone());
            lodash_match_str(&i, atom!("type"), "rampart") && lodash_match(&i, atom!("x"), &ox) && lodash_match(&i, atom!("y"), &oy)
        });
        if attack_type == EVENT_ATTACK_TYPE_MELEE && !under_rampart {
            attack_back_power = utils::calc_body_effectiveness(&target.get(atom!("body")), ATTACK, "attack", ATTACK_POWER as f64, false)?;
        }
        target.set(atom!("_damageToApply"), js_add(&or0(target.get(atom!("_damageToApply"))), &V::Num(damage)));
    } else if is_pc {
        target.set(atom!("_damageToApply"), js_add(&or0(target.get(atom!("_damageToApply"))), &V::Num(damage)));
    } else {
        if attack_type != EVENT_ATTACK_TYPE_NUKE && (type_is(target, "constructedWall") || type_is(target, "rampart")) {
            // _.find(target.effects, e => (e.power == PWR_FORTIFY || e.effect == EFFECT_INVULNERABILITY) && e.endTime > gameTime)
            for e in lodash_values(&target.get(atom!("effects"))) {
                let fortified = loose_eq(&prop(&e, atom!("power"))?, &V::Num(PWR_FORTIFY as f64)) || loose_eq(&get_or_undef(&e, atom!("effect")), &V::Num(EFFECT_INVULNERABILITY as f64));
                if fortified && gt(&get_or_undef(&e, atom!("endTime")), &game_time) {
                    return Ok(());
                }
            }
        }
        target.set(atom!("hits"), V::Num(to_number(&target.get(atom!("hits"))) - damage));
    }

    if type_is(target, "powerBank") {
        attack_back_power = damage * POWER_BANK_HIT_BACK;
    }
    if let Some(c) = &scope.room_controller {
        if loose_eq(&c.get(atom!("user")), &object.get(atom!("user"))) && gt(&c.get(atom!("safeMode")), &game_time) {
            attack_back_power = 0.0;
        }
    }

    if type_is(target, "constructedWall") && truthy(&target.get(atom!("decayTime"))) {
        super::creeps::_clear_newbie_walls::clear_newbie_walls(scope)?;
    } else if crate::processor::le(&target.get(atom!("hits")), &V::Num(0.0)) {
        if !is_creep && !is_pc {
            super::structures::_destroy::destroy(target, scope, Some(attack_type))?;
            push_event(scope, EVENT_OBJECT_DESTROYED, target.get(atom!("_id")), obj! { "type" => target.get(atom!("type")) });
        }
    } else if !is_creep && !is_pc {
        update(scope, target, &obj! { "hits" => target.get(atom!("hits")) })?;
    }

    let log = object.get(atom!("actionLog"));
    if truthy(&log) && type_is(object, "creep") {
        if attack_type == EVENT_ATTACK_TYPE_MELEE || attack_type == EVENT_ATTACK_TYPE_DISMANTLE {
            put(&log, atom!("attack"), xy_of(target))?;
        }
        if attack_type == EVENT_ATTACK_TYPE_RANGED {
            put(&log, atom!("rangedAttack"), xy_of(target))?;
        }
    }
    let target_log = target.get(atom!("actionLog"));
    if truthy(&target_log) {
        put(&target_log, atom!("attacked"), xy_of(object))?;
    }

    let user = object.get(atom!("user"));
    if !eq_str(&user, "2") && !eq_str(&user, "3") {
        if truthy(&target.get(atom!("notifyWhenAttacked"))) {
            let controller = scope.room_controller.clone();
            utils::send_attacking_notification(target, controller.as_ref(), scope);
        }
        let t_user = target.get(atom!("user"));
        if truthy(&user) && truthy(&t_user) && !loose_eq(&user, &t_user) && !eq_str(&t_user, "2") && !eq_str(&t_user, "3") {
            scope.room_info.set(atom!("lastPvpTime"), game_time.clone());
        }
    }

    if truthy(&V::Num(attack_back_power)) {
        let skip = !truthy(&object.get(atom!("actionLog"))) && scope.ctx.compat.fix("BUG-CRASH-NUKE-PB");
        if !skip {
            object.set(atom!("_damageToApply"), js_add(&or0(object.get(atom!("_damageToApply"))), &V::Num(attack_back_power)));
            put(&object.get(atom!("actionLog")), atom!("attacked"), xy_of(target))?;
            let data = obj! { "targetId" => object.get(atom!("_id")), "damage" => V::Num(attack_back_power), "attackType" => V::Num(EVENT_ATTACK_TYPE_HIT_BACK as f64) };
            push_event(scope, EVENT_ATTACK, target.get(atom!("_id")), data);
        }
    }

    let data = obj! { "targetId" => target.get(atom!("_id")), "damage" => V::Num(damage), "attackType" => V::Num(attack_type as f64) };
    push_event(scope, EVENT_ATTACK, object.get(atom!("_id")), data);
    Ok(())
}
