//! mirror: reference/engine/src/processor/intents/power-creeps/enableRoom.js
//! parity: FAITHFUL
//! Verified against the official processor by the fuzz fixtures in crates/engine/tests/fixtures/oracle/pc.

use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::{room_object, RoomScope};
use crate::utils;

/// PC-ENABLE-ROOM.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let Some(target) = room_object(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    if !target.eq_str(atom!("type"), "controller") {
        return Ok(());
    }
    if !loose_eq(&target.get(atom!("user")), &object.get(atom!("user"))) && gt(&target.get(atom!("safeMode")), &n(scope.game_time as f64)) {
        return Ok(());
    }
    if utils::dist(&V::Obj(object.clone()), &V::Obj(target.clone())) > 1.0 {
        return Ok(());
    }
    update(&mut scope.bulk, &target, vec![(atom!("isPowerEnabled"), V::Bool(true))])?;
    put(&object.get(atom!("actionLog")), atom!("attack"), obj(vec![(atom!("x"), target.get(atom!("x"))), (atom!("y"), target.get(atom!("y")))]))
}
