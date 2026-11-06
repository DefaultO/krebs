//! mirror: reference/engine/src/processor/intents/towers/heal.js
//! parity: FAITHFUL

use super::attack::{pos, spend_energy, tower_amount};
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
    if !target.eq_str(atom!("type"), "creep") && !strict_eq(&target.get(atom!("type")), &s("powerCreep")) {
        return Ok(());
    }
    if truthy(&target.get(atom!("spawning"))) {
        return Ok(());
    }
    if lt(&get(&object.get(atom!("store")), atom!("energy"))?, &n(C::TOWER_ENERGY_COST as f64)) {
        return Ok(());
    }

    let amount = tower_amount(object, &target, C::TOWER_POWER_HEAL, scope)?;

    if amount == 0.0 || amount.is_nan() {
        return Ok(());
    }

    let before = target.get(atom!("_healToApply"));
    target.set(atom!("_healToApply"), add(&if truthy(&before) { before } else { n(0.0) }, &n(amount)));

    spend_energy(object, scope)?;

    put(&object.get(atom!("actionLog")), atom!("heal"), pos(&target))?;
    put(&target.get(atom!("actionLog")), atom!("healed"), pos(object))?;

    let data = obj(vec![(atom!("targetId"), target.get(atom!("_id"))), (atom!("amount"), n(amount)), (atom!("healType"), n(C::EVENT_HEAL_TYPE_RANGED as f64))]);
    event(scope, C::EVENT_HEAL, object.get(atom!("_id")), Some(data));
    Ok(())
}
