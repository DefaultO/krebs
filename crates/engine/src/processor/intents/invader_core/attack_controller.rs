//! mirror: reference/engine/src/processor/intents/invader-core/attackController.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{to_js_string, to_number, truthy, Crash, O, V};
use crate::processor::global::obj;
use crate::processor::{eq_str, gt, loose_eq, prop, put, update, RoomScope};

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !eq_str(&object.get(atom!("type")), "invaderCore") || truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let Some(target) = scope.room_objects.get(&to_js_string(&prop(intent, atom!("id"))?)) else { return Ok(()) };
    if !eq_str(&target.get(atom!("type")), "controller") {
        return Ok(());
    }
    if !truthy(&target.get(atom!("user"))) && !truthy(&target.get(atom!("reservation"))) {
        return Ok(());
    }
    let game_time = V::Num(scope.game_time as f64);
    // `rc && rc.user != object.user && rc.safeMode > gameTime || rc.upgradeBlocked > gameTime`
    let rc = scope.room_controller.clone().map_or(V::Undef, V::Obj);
    let safe = truthy(&rc) && !loose_eq(&prop(&rc, atom!("user"))?, &object.get(atom!("user"))) && gt(&prop(&rc, atom!("safeMode"))?, &game_time);
    if safe || gt(&prop(&rc, atom!("upgradeBlocked"))?, &game_time) {
        return Ok(());
    }
    let reservation = target.get(atom!("reservation"));
    if truthy(&reservation) {
        let effect = (C::INVADER_CORE_CONTROLLER_POWER as f64 * C::CONTROLLER_RESERVE as f64).floor();
        let end_time = to_number(&prop(&reservation, atom!("endTime"))?) - effect;
        update(scope, &target, &obj(vec![("reservation", obj(vec![("endTime", V::Num(end_time))]))]))?;
    }
    if truthy(&target.get(atom!("user"))) {
        let effect = (C::INVADER_CORE_CONTROLLER_POWER as f64 * C::CONTROLLER_CLAIM_DOWNGRADE as f64).floor();
        let downgrade = to_number(&target.get(atom!("downgradeTime"))) - effect;
        update(scope, &target, &obj(vec![("downgradeTime", V::Num(downgrade))]))?;
        target.set(crate::doc::Atom::intern("_upgradeBlocked"), V::Num(scope.game_time as f64 + C::CONTROLLER_ATTACK_BLOCKED_UPGRADE as f64));
    }
    let action_log = prop(&V::Obj(object.clone()), atom!("actionLog"))?;
    put(&action_log, atom!("reserveController"), obj(vec![("x", target.get(atom!("x"))), ("y", target.get(atom!("y")))]))?;
    scope.room_info.set(atom!("active"), V::Bool(true));
    scope.event_log.push(obj(vec![("event", V::Num(C::EVENT_ATTACK_CONTROLLER as f64)), ("objectId", object.get(atom!("_id")))]));
    Ok(())
}
