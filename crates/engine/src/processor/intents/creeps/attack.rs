//! mirror: reference/engine/src/processor/intents/creeps/attack.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::constants::{ATTACK, ATTACK_POWER, EVENT_ATTACK_TYPE_MELEE};
use crate::live::{truthy, Crash, O, V};
use crate::processor::{gt, lodash_match, lodash_match_str, loose_eq, out_of_range, prop, room_object, type_is, RoomScope};
use crate::utils;

/// The first rampart on `target`'s tile (DMG-RAMPART-REDIRECT): `_.find(roomObjects, {type:'rampart', x, y})`.
pub(crate) fn rampart_at(scope: &RoomScope, target: &O) -> Option<O> {
    let (x, y) = (target.get(atom!("x")), target.get(atom!("y")));
    scope.room_objects.near(&x, &y).find(|&o| {
        let v = V::Obj(o.clone());
        lodash_match_str(&v, atom!("type"), "rampart") && lodash_match(&v, atom!("x"), &x) && lodash_match(&v, atom!("y"), &y)
    }).cloned()
}

/// `roomController && roomController.user != object.user && roomController.safeMode > gameTime`.
pub(crate) fn in_foreign_safe_mode(scope: &RoomScope, object: &O) -> bool {
    scope.room_controller.as_ref().is_some_and(|c| !loose_eq(&c.get(atom!("user")), &object.get(atom!("user"))) && gt(&c.get(atom!("safeMode")), &V::Num(scope.game_time as f64)))
}

/// CREEP-ATTACK.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let Some(mut target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if target.same(object) {
        return Ok(());
    }
    if out_of_range(object, &target, 1.0) {
        return Ok(());
    }
    if type_is(&target, "creep") && truthy(&target.get(atom!("spawning"))) {
        return Ok(());
    }
    if !truthy(&target.get(atom!("hits"))) {
        return Ok(());
    }
    if in_foreign_safe_mode(scope, object) {
        return Ok(());
    }
    if let Some(rampart) = rampart_at(scope, &target) {
        target = rampart;
    }
    let attack_power = utils::calc_body_effectiveness(&object.get(atom!("body")), ATTACK, "attack", ATTACK_POWER as f64, false)?;
    super::super::_damage::damage(object, &target, attack_power, EVENT_ATTACK_TYPE_MELEE, scope)?;
    object.set(atom!("_attack"), V::Bool(true));
    Ok(())
}
