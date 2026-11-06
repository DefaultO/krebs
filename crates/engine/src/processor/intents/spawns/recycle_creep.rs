//! mirror: reference/engine/src/processor/intents/spawns/recycle-creep.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::creeps::_die::die;
use crate::processor::RoomScope;

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !object.eq_str(atom!("type"), "spawn") {
        return Ok(());
    }

    let Some(target) = by_key(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    if !target.eq_str(atom!("type"), "creep") || !loose_eq(&target.get(atom!("user")), &object.get(atom!("user"))) || truthy(&target.get(atom!("spawning"))) {
        return Ok(());
    }
    if (target.num(atom!("x")) - object.num(atom!("x"))).abs() > 1.0 || (target.num(atom!("y")) - object.num(atom!("y"))).abs() > 1.0 {
        return Ok(());
    }

    die(&target, Some(1.0), false, scope, None)
}
