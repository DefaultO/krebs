//! mirror: reference/engine/src/processor/intents/power-creeps/renew.js
//! parity: FAITHFUL
//! Verified against the official processor by the fuzz fixtures in crates/engine/tests/fixtures/oracle/pc.

use crate::atom;
use crate::constants::POWER_CREEP_LIFE_TIME;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::{room_object, RoomScope};
use crate::utils;

/// PC-RENEW.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let Some(target) = room_object(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    let ty = target.get(atom!("type"));
    if !loose_eq(&ty, &s("powerBank")) && !loose_eq(&ty, &s("powerSpawn")) {
        return Ok(());
    }
    if utils::dist(&V::Obj(object.clone()), &V::Obj(target.clone())) > 1.0 {
        return Ok(());
    }
    if loose_eq(&ty, &s("powerSpawn")) && !check_structure_against_controller(&target, scope)? {
        return Ok(());
    }
    update(&mut scope.bulk, object, vec![(atom!("ageTime"), n(scope.game_time as f64 + POWER_CREEP_LIFE_TIME as f64))])?;
    put(&object.get(atom!("actionLog")), atom!("healed"), obj(vec![(atom!("x"), object.get(atom!("x"))), (atom!("y"), object.get(atom!("y")))]))
}
