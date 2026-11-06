//! mirror: reference/engine/src/processor/intents/creeps/move.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::intents::movement;
use crate::processor::{eq_str, gt, is_obstacle_type, js_add, lodash_match, loose_eq, lt, prop, room_object, type_is, RoomScope};
use crate::utils;

/// CREEP-MOVE: a tentative destination (MOVE-PREFILTER, MOVE-ADD); shared with power creeps.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    object.set(atom!("_oldFatigue"), object.get(atom!("fatigue")));

    let mut d = None;
    let direction = prop(intent, atom!("direction"))?;
    if truthy(&direction) {
        d = utils::get_offsets_by_direction(&direction);
    }
    let id = prop(intent, atom!("id"))?;
    if truthy(&id) {
        if let Some(creep) = room_object(scope, &id) {
            if type_is(&creep, "creep") && utils::dist(&V::Obj(object.clone()), &V::Obj(creep.clone())) == 1.0 {
                let c = |k| to_number(&creep.get(k)) - to_number(&object.get(k));
                d = Some((c(atom!("x")), c(atom!("y"))));
            }
        }
    }
    let Some((dx, dy)) = d else { return Ok(()) };

    let nx = js_add(&object.get(atom!("x")), &V::Num(dx));
    let ny = js_add(&object.get(atom!("y")), &V::Num(dy));
    let (zero, edge) = (V::Num(0.0), V::Num(49.0));
    if lt(&nx, &zero) || gt(&nx, &edge) || lt(&ny, &zero) || gt(&ny, &edge) {
        return Ok(());
    }

    let is_pc = type_is(object, "powerCreep");
    let user = object.get(atom!("user"));
    let mut blocked = false;
    for target in scope.room_objects.near(&nx, &ny) {
        let t = V::Obj(target.clone());
        if !(lodash_match(&t, atom!("x"), &nx) && lodash_match(&t, atom!("y"), &ny)) {
            continue;
        }
        let ty = target.get(atom!("type"));
        if is_obstacle_type(&ty) && !eq_str(&ty, "creep") && !eq_str(&ty, "powerCreep")
            || eq_str(&ty, "rampart") && !truthy(&target.get(atom!("isPublic"))) && !loose_eq(&user, &target.get(atom!("user")))
            || is_pc && eq_str(&ty, "portal") && truthy(&prop(&target.get(atom!("destination")), atom!("shard"))?)
        {
            blocked = true;
            break;
        }
    }
    if !blocked {
        movement::add(scope, object, dx, dy);
    }
    Ok(())
}
