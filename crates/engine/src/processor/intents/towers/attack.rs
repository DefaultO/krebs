//! mirror: reference/engine/src/processor/intents/towers/attack.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::_damage::damage;
use crate::processor::RoomScope;

/// attack.js:32-46, heal.js:23-37, repair.js:20-34 (TOWER-FALLOFF): the amount at `target`'s range
/// after the operate/disrupt effects, floored.
pub(crate) fn tower_amount(object: &O, target: &O, base: u32, scope: &RoomScope) -> Result<f64, Crash> {
    let mut range = cheb(target, object);
    let mut amount = base as f64;
    let (optimal, falloff_range) = (C::TOWER_OPTIMAL_RANGE as f64, C::TOWER_FALLOFF_RANGE as f64);
    if range > optimal {
        if range > falloff_range {
            range = falloff_range;
        }
        amount -= amount * C::TOWER_FALLOFF * (range - optimal) / (falloff_range - optimal);
    }
    for power in [C::PWR_OPERATE_TOWER, C::PWR_DISRUPT_TOWER] {
        if let Some(effect) = find_power_effect(object, power) {
            if gt(&get(&effect, atom!("endTime"))?, &n(scope.game_time as f64)) {
                amount *= to_number(&power_effect(power, &get(&effect, atom!("level"))?));
            }
        }
    }
    Ok(amount.floor())
}

/// `object.store.energy -= C.TOWER_ENERGY_COST; bulk.update(object, {store:{energy: object.store.energy}});`
pub(crate) fn spend_energy(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let store = object.get(atom!("store"));
    put(&store, atom!("energy"), n(to_number(&get(&store, atom!("energy"))?) - C::TOWER_ENERGY_COST as f64))?;
    update(&mut scope.bulk, object, vec![(atom!("store"), obj(vec![(atom!("energy"), peek(&store, atom!("energy")))]))])
}

/// `{x: o.x, y: o.y}`
pub(crate) fn pos(o: &O) -> V {
    obj(vec![(atom!("x"), o.get(atom!("x"))), (atom!("y"), o.get(atom!("y")))])
}

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !object.eq_str(atom!("type"), "tower") || !truthy(&object.get(atom!("store"))) {
        return Ok(());
    }

    let Some(mut target) = by_key(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    if target.same(object) {
        return Ok(());
    }
    if target.eq_str(atom!("type"), "creep") && truthy(&target.get(atom!("spawning"))) {
        return Ok(());
    }
    if !truthy(&target.get(atom!("hits"))) {
        return Ok(());
    }
    if lt(&get(&object.get(atom!("store")), atom!("energy"))?, &n(C::TOWER_ENERGY_COST as f64)) {
        return Ok(());
    }
    let src = [(atom!("type"), s("rampart")), (atom!("x"), target.get(atom!("x"))), (atom!("y"), target.get(atom!("y")))];
    if let Some(rampart) = room_find(scope, |i| room_match(i, &src)) {
        target = rampart;
    }

    let amount = tower_amount(object, &target, C::TOWER_POWER_ATTACK, scope)?;

    if amount == 0.0 || amount.is_nan() {
        return Ok(());
    }

    damage(object, &target, amount, C::EVENT_ATTACK_TYPE_RANGED, scope)?;

    spend_energy(object, scope)?;

    put(&object.get(atom!("actionLog")), atom!("attack"), pos(&target))?;
    let target_log = target.get(atom!("actionLog"));
    if truthy(&target_log) {
        put(&target_log, atom!("attacked"), pos(object))?;
    }

    if truthy(&target.get(atom!("notifyWhenAttacked"))) {
        send_attacking_notification(scope, &target);
    }

    Ok(())
}
