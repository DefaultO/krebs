//! mirror: reference/engine/src/processor/intents/spawns/cancel-spawning.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(spawn: &O, _intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !spawn.eq_str(atom!("type"), "spawn") || !truthy(&spawn.get(atom!("spawning"))) {
        return Ok(());
    }
    let name = get(&spawn.get(atom!("spawning")), atom!("name"))?;
    let spawning_creep = room_find(scope, |i| {
        room_match(i, &[(atom!("type"), s("creep")), (atom!("name"), name.clone()), (atom!("x"), spawn.get(atom!("x"))), (atom!("y"), spawn.get(atom!("y")))])
    });
    match spawning_creep {
        Some(creep) => remove_object(scope, &creep),
        // BUG-CRASH-CANCEL-SPAWN: `spawningCreep._id` of undefined throws (private) or only clears spawning.
        None if scope.ctx.compat.fix("BUG-CRASH-CANCEL-SPAWN") => {}
        None => return Err(crash_read(&V::Undef, "_id")),
    }
    update(&mut scope.bulk, spawn, vec![(atom!("spawning"), V::Null)])
}
