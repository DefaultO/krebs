//! mirror: reference/engine/src/processor/intents/invader-core/reserveController.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{to_js_string, truthy, Crash, O, V};
use crate::processor::global::obj;
use crate::processor::{eq_str, gt, js_add, loose_eq, prop, put, update, RoomScope};

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !eq_str(&object.get(atom!("type")), "invaderCore") {
        return Ok(());
    }
    let Some(target) = scope.room_objects.get(&to_js_string(&prop(intent, atom!("id"))?)) else { return Ok(()) };
    if !eq_str(&target.get(atom!("type")), "controller") {
        return Ok(());
    }
    let reservation = target.get(atom!("reservation"));
    if truthy(&target.get(atom!("user"))) || (truthy(&reservation) && !loose_eq(&prop(&reservation, atom!("user"))?, &object.get(atom!("user")))) {
        return Ok(());
    }
    let game_time = scope.game_time as f64;
    if !truthy(&reservation) {
        target.set(atom!("reservation"), obj(vec![("user", object.get(atom!("user"))), ("endTime", V::Num(game_time + 1.0))]));
    }
    let effect = (C::INVADER_CORE_CONTROLLER_POWER * C::CONTROLLER_RESERVE) as f64;
    let reservation = target.get(atom!("reservation"));
    let end = prop(&reservation, atom!("endTime"))?;
    if gt(&js_add(&end, &V::Num(effect)), &V::Num(game_time + C::CONTROLLER_RESERVE_MAX as f64)) {
        return Ok(());
    }
    let action_log = prop(&V::Obj(object.clone()), atom!("actionLog"))?;
    put(&action_log, atom!("reserveController"), obj(vec![("x", target.get(atom!("x"))), ("y", target.get(atom!("y")))]))?;
    put(&reservation, atom!("endTime"), js_add(&end, &V::Num(effect)))?;
    update(scope, &target, &obj(vec![("reservation", reservation)]))?;
    let data = obj(vec![("amount", V::Num(effect))]);
    scope.event_log.push(obj(vec![("event", V::Num(C::EVENT_RESERVE_CONTROLLER as f64)), ("objectId", object.get(atom!("_id"))), ("data", data)]));
    Ok(())
}
