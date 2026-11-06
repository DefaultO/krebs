//! mirror: reference/engine/src/processor/intents/creeps/claimController.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::bulk::Target;
use crate::constants::CLAIM;
use crate::live::{strict_eq, to_number, truthy, Crash, O, V};
use crate::processor::{eq_str, get_or_undef, gt, lodash_values, loose_eq, lt, obj, out_of_range, prop, room_object, type_is, update, RoomScope};
use crate::utils;

/// `_.filter(object.body, (i) => i.hits > 0 && i.type == C.CLAIM).length`.
pub(crate) fn live_claim_parts(object: &O) -> Result<f64, Crash> {
    let mut n = 0.0;
    for i in lodash_values(&object.get(atom!("body"))) {
        if gt(&prop(&i, atom!("hits"))?, &V::Num(0.0)) && eq_str(&get_or_undef(&i, atom!("type")), CLAIM) {
            n += 1.0;
        }
    }
    Ok(n)
}

/// CREEP-CLAIM-CONTROLLER.
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
    let user = object.get(atom!("user"));
    let bind_user = target.get(atom!("bindUser"));
    if truthy(&bind_user) && !loose_eq(&user, &bind_user) {
        return Ok(());
    }
    if gt(&target.get(atom!("level")), &V::Num(0.0)) {
        return Ok(());
    }
    if live_claim_parts(object)? == 0.0 {
        return Ok(());
    }
    let reservation = target.get(atom!("reservation"));
    if truthy(&reservation) && !loose_eq(&get_or_undef(&reservation, atom!("user")), &user) {
        return Ok(());
    }
    let user_doc = scope.user_doc(&user);
    let rooms = prop(&user_doc, atom!("rooms"))?;
    let claimed_rooms = if truthy(&rooms) { to_number(&get_or_undef(&rooms, atom!("length"))) } else { 0.0 };
    if strict_eq(&get_or_undef(&user_doc, atom!("shardAccess")), &V::Bool(false)) {
        return Ok(());
    }
    if lt(&get_or_undef(&user_doc, atom!("gcl")), &V::Num(utils::calc_needed_gcl(claimed_rooms + 1.0))) {
        return Ok(());
    }

    let level = 1.0;
    update(scope, &target, &obj! { "user" => user.clone(), "level" => V::Num(level), "progress" => V::Num(0.0), "downgradeTime" => V::Null, "reservation" => V::Null })?;

    // driver.addRoomToUser(object.room, users[object.user], bulkUsers)
    let room = object.get(atom!("room"));
    let V::Obj(u) = &user_doc else { return Ok(()) };
    let rooms = u.get(atom!("rooms"));
    let has = match &rooms {
        V::Arr(a) => a.to_vec().iter().any(|r| strict_eq(r, &room)),
        _ => false,
    };
    if !truthy(&rooms) || !has {
        scope.bulk_users.add_to_set(Target::Obj(u), atom!("rooms"), &room);
    }
    Ok(())
}
