//! mirror: reference/engine/src/processor/intents/towers/repair.js
//! parity: FAITHFUL

use super::attack::{pos, tower_amount};
use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !object.eq_str(atom!("type"), "tower") || !truthy(&object.get(atom!("store"))) {
        return Ok(());
    }

    let Some(target) = by_key(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    if !truthy(&construction_cost(&target.get(atom!("type")))) || ge(&target.get(atom!("hits")), &target.get(atom!("hitsMax"))) {
        return Ok(());
    }
    let store = object.get(atom!("store"));
    if lt(&get(&store, atom!("energy"))?, &n(C::TOWER_ENERGY_COST as f64)) {
        return Ok(());
    }

    let amount = tower_amount(object, &target, C::TOWER_POWER_REPAIR, scope)?;

    if amount == 0.0 || amount.is_nan() {
        return Ok(());
    }

    target.set(atom!("hits"), add(&target.get(atom!("hits")), &n(amount)));
    if gt(&target.get(atom!("hits")), &target.get(atom!("hitsMax"))) {
        target.set(atom!("hits"), target.get(atom!("hitsMax")));
    }
    update(&mut scope.bulk, &target, vec![(atom!("hits"), target.get(atom!("hits")))])?;

    put(&store, atom!("energy"), n(to_number(&peek(&store, atom!("energy"))) - C::TOWER_ENERGY_COST as f64))?;
    put(&object.get(atom!("actionLog")), atom!("repair"), pos(&target))?;
    update(&mut scope.bulk, object, vec![(atom!("store"), obj(vec![(atom!("energy"), peek(&store, atom!("energy")))]))])?;

    stats_inc(scope, "energyConstruction", &object.get(atom!("user")), C::TOWER_ENERGY_COST as f64);

    let data = obj(vec![(atom!("targetId"), target.get(atom!("_id"))), (atom!("amount"), n(amount)), (atom!("energySpent"), n(C::TOWER_ENERGY_COST as f64))]);
    event(scope, C::EVENT_REPAIR, object.get(atom!("_id")), Some(data));
    Ok(())
}
