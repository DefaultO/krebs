//! mirror: reference/engine/src/processor/intents/sources/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !object.eq_str(atom!("type"), "source") {
        return Ok(());
    }

    if lt(&object.get(atom!("energy")), &object.get(atom!("energyCapacity"))) {
        if !truthy(&object.get(atom!("nextRegenerationTime"))) {
            object.set(atom!("nextRegenerationTime"), n(game_time + C::ENERGY_REGEN_TIME as f64));
            update(&mut scope.bulk, object, vec![(atom!("nextRegenerationTime"), object.get(atom!("nextRegenerationTime")))])?;
        }

        if let Some(effect) = find_power_effect(object, C::PWR_DISRUPT_SOURCE) {
            if gt(&get(&effect, atom!("endTime"))?, &n(game_time)) {
                let t = add(&object.get(atom!("nextRegenerationTime")), &n(1.0));
                update(&mut scope.bulk, object, vec![(atom!("nextRegenerationTime"), t)])?;
            }
        }

        if ge(&n(game_time), &n(to_number(&object.get(atom!("nextRegenerationTime"))) - 1.0)) {
            update(&mut scope.bulk, object, vec![(atom!("nextRegenerationTime"), V::Null), (atom!("energy"), object.get(atom!("energyCapacity")))])?;
        }

        if let Some(effect) = find_power_effect(object, C::PWR_REGEN_SOURCE) {
            let end_time = get(&effect, atom!("endTime"))?;
            if gt(&end_time, &n(game_time)) {
                let period = C::power_info(C::PWR_REGEN_SOURCE).and_then(|p| p.period).unwrap() as f64;
                if (to_number(&end_time) - game_time - 1.0) % period == 0.0 {
                    let regen = add(&object.get(atom!("energy")), &power_effect(C::PWR_REGEN_SOURCE, &get(&effect, atom!("level"))?));
                    let energy = js_min(to_number(&object.get(atom!("energyCapacity"))), to_number(&regen));
                    update(&mut scope.bulk, object, vec![(atom!("energy"), n(energy))])?;
                }
            }
        }
    }

    match scope.room_controller.clone() {
        Some(ctrl) => {
            let (user, reservation) = (ctrl.get(atom!("user")), ctrl.get(atom!("reservation")));
            let neutral = C::SOURCE_ENERGY_NEUTRAL_CAPACITY as f64;
            if !truthy(&user) && !truthy(&reservation) && !loose_eq(&object.get(atom!("energyCapacity")), &n(neutral)) {
                let energy = js_min(to_number(&object.get(atom!("energy"))), neutral);
                update(&mut scope.bulk, object, vec![(atom!("energyCapacity"), n(neutral)), (atom!("energy"), n(energy))])?;
            }
            let full = C::SOURCE_ENERGY_CAPACITY as f64;
            if (truthy(&user) || truthy(&reservation)) && !loose_eq(&object.get(atom!("energyCapacity")), &n(full)) {
                update(&mut scope.bulk, object, vec![(atom!("energyCapacity"), n(full))])?;
            }
        }
        None => {
            let keeper = C::SOURCE_ENERGY_KEEPER_CAPACITY as f64;
            if !loose_eq(&object.get(atom!("energyCapacity")), &n(keeper)) {
                update(&mut scope.bulk, object, vec![(atom!("energyCapacity"), n(keeper))])?;
            }
        }
    }
    Ok(())
}
