//! mirror: reference/engine/src/processor/intents/spawns/renew-creep.js
//! parity: FAITHFUL

use super::_charge_energy::charge_energy;
use crate::atom;
use crate::bulk::Target;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::creeps::{_drop_resources_without_space::drop_resources_without_space, _recalc_body::recalc_body};
use crate::processor::RoomScope;

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !object.eq_str(atom!("type"), "spawn") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }

    let Some(target) = by_key(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    if !target.eq_str(atom!("type"), "creep") || !loose_eq(&target.get(atom!("user")), &object.get(atom!("user"))) || truthy(&target.get(atom!("spawning"))) {
        return Ok(());
    }
    if (target.num(atom!("x")) - object.num(atom!("x"))).abs() > 1.0 || (target.num(atom!("y")) - object.num(atom!("y"))).abs() > 1.0 {
        return Ok(());
    }
    let body = target.get(atom!("body"));
    if values(&body).iter().any(|i| loose_eq(&peek(i, atom!("type")), &s(C::CLAIM))) {
        return Ok(());
    }

    let parts = to_number(&get(&body, atom!("length"))?);
    let effect = (C::SPAWN_RENEW_RATIO * C::CREEP_LIFE_TIME as f64 / C::CREEP_SPAWN_TIME as f64 / parts).floor();
    if gt(&add(&target.get(atom!("ageTime")), &n(effect)), &n(game_time + C::CREEP_LIFE_TIME as f64)) {
        return Ok(());
    }

    let cost = (C::SPAWN_RENEW_RATIO * calc_creep_cost(&body)? / C::CREEP_SPAWN_TIME as f64 / parts).ceil();
    let result = charge_energy(object, cost, &V::Undef, scope)?;

    if !result {
        return Ok(());
    }

    stats_inc(scope, "energyCreeps", &object.get(atom!("user")), cost);

    put(&target.get(atom!("actionLog")), atom!("healed"), obj(vec![(atom!("x"), object.get(atom!("x"))), (atom!("y"), object.get(atom!("y")))]))?;
    scope.bulk.inc(Target::Obj(&target), &[atom!("ageTime")], effect);

    if values(&body).iter().any(|i| truthy(&peek(i, atom!("boost")))) {
        for i in values(&target.get(atom!("body"))) {
            put(&i, atom!("boost"), V::Null)?;
        }
        recalc_body(&target)?;
        // we may not be able to hold all of the resources we could before now.
        drop_resources_without_space(&target, scope)?;
        update(&mut scope.bulk, &target, vec![(atom!("body"), target.get(atom!("body"))), (atom!("storeCapacity"), target.get(atom!("storeCapacity")))])?;
    }

    Ok(())
}
