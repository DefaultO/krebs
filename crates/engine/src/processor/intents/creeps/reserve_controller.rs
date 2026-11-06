//! mirror: reference/engine/src/processor/intents/creeps/reserveController.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use super::claim_controller::live_claim_parts;
use crate::atom;
use crate::constants::{CONTROLLER_RESERVE, CONTROLLER_RESERVE_MAX, EVENT_RESERVE_CONTROLLER};
use crate::live::{strict_eq, truthy, Crash, O, V};
use crate::processor::{get_or_undef, gt, js_add, loose_eq, obj, out_of_range, prop, push_event, put, room_object, type_is, update, xy_of, RoomScope};

/// CREEP-RESERVE-CONTROLLER.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
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
    if out_of_range(object, &target, 1.0) {
        return Ok(());
    }
    let reservation = target.get(atom!("reservation"));
    if truthy(&target.get(atom!("user"))) || truthy(&reservation) && !loose_eq(&get_or_undef(&reservation, atom!("user")), &user) {
        return Ok(());
    }

    let effect = live_claim_parts(object)? * CONTROLLER_RESERVE as f64;
    if !truthy(&V::Num(effect)) {
        return Ok(());
    }
    let game_time = scope.game_time as f64;
    if !truthy(&target.get(atom!("reservation"))) {
        target.set(atom!("reservation"), obj! { "user" => user.clone(), "endTime" => V::Num(game_time + 1.0) });
    }
    let reservation = target.get(atom!("reservation"));
    if gt(&js_add(&get_or_undef(&reservation, atom!("endTime")), &V::Num(effect)), &V::Num(game_time + CONTROLLER_RESERVE_MAX as f64)) {
        return Ok(());
    }

    put(&object.get(atom!("actionLog")), atom!("reserveController"), xy_of(&target))?;
    put(&reservation, atom!("endTime"), js_add(&get_or_undef(&reservation, atom!("endTime")), &V::Num(effect)))?;
    update(scope, &target, &obj! { "reservation" => target.get(atom!("reservation")) })?;
    push_event(scope, EVENT_RESERVE_CONTROLLER, object.get(atom!("_id")), obj! { "amount" => V::Num(effect) });
    Ok(())
}
