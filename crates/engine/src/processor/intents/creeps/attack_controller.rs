//! mirror: reference/engine/src/processor/intents/creeps/attackController.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use super::claim_controller::live_claim_parts;
use crate::atom;
use crate::constants::{CONTROLLER_ATTACK_BLOCKED_UPGRADE, CONTROLLER_CLAIM_DOWNGRADE, CONTROLLER_RESERVE, EFFECT_INVULNERABILITY, EVENT_ATTACK_CONTROLLER};
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::{get_or_undef, gt, lodash_values, loose_eq, obj, out_of_range, prop, put, read_crash, room_object, type_is, update, xy_of, RoomScope};

/// CREEP-ATTACK-CONTROLLER.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
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
    if !truthy(&target.get(atom!("user"))) && !truthy(&target.get(atom!("reservation"))) {
        return Ok(());
    }
    let game_time = V::Num(scope.game_time as f64);
    // roomController && roomController.user != object.user && roomController.safeMode > gameTime ||
    //     roomController.upgradeBlocked > gameTime
    let foreign_safe_mode = super::attack::in_foreign_safe_mode(scope, object);
    if foreign_safe_mode {
        return Ok(());
    }
    let Some(controller) = &scope.room_controller else { return Err(read_crash(&V::Undef, "upgradeBlocked")) };
    if gt(&controller.get(atom!("upgradeBlocked")), &game_time) {
        return Ok(());
    }
    for e in lodash_values(&target.get(atom!("effects"))) {
        if loose_eq(&prop(&e, atom!("effect"))?, &V::Num(EFFECT_INVULNERABILITY as f64)) && gt(&get_or_undef(&e, atom!("endTime")), &game_time) {
            return Ok(());
        }
    }

    if truthy(&target.get(atom!("reservation"))) {
        let effect = (live_claim_parts(object)? * CONTROLLER_RESERVE as f64).floor();
        if !truthy(&V::Num(effect)) {
            return Ok(());
        }
        let end_time = to_number(&get_or_undef(&target.get(atom!("reservation")), atom!("endTime"))) - effect;
        update(scope, &target, &obj! { "reservation" => obj! { "endTime" => V::Num(end_time) } })?;
    }
    if truthy(&target.get(atom!("user"))) {
        let effect = (live_claim_parts(object)? * CONTROLLER_CLAIM_DOWNGRADE as f64).floor();
        if !truthy(&V::Num(effect)) {
            return Ok(());
        }
        let downgrade_time = to_number(&target.get(atom!("downgradeTime"))) - effect;
        update(scope, &target, &obj! { "downgradeTime" => V::Num(downgrade_time) })?;
        target.set(atom!("_upgradeBlocked"), V::Num(scope.game_time as f64 + CONTROLLER_ATTACK_BLOCKED_UPGRADE as f64));
    }
    put(&object.get(atom!("actionLog")), atom!("attack"), xy_of(&target))?;
    scope.event_log.push(obj! { "event" => V::Num(EVENT_ATTACK_CONTROLLER as f64), "objectId" => object.get(atom!("_id")) });
    Ok(())
}
