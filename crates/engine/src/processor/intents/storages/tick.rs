//! mirror: reference/engine/src/processor/intents/storages/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let Some(ctrl) = scope.room_controller.clone() else { return Ok(()) };
    let level = ctrl.get(atom!("level"));
    let mut store_capacity = if gt(&level, &n(0.0))
        && loose_eq(&ctrl.get(atom!("user")), &object.get(atom!("user")))
        && gt(&controller_structures("storage", &level)?, &n(0.0))
    {
        n(C::STORAGE_CAPACITY as f64)
    } else {
        n(0.0)
    };
    if gt(&store_capacity, &n(0.0)) {
        if let Some(effect) = find_power_effect(object, C::PWR_OPERATE_STORAGE) {
            if gt(&get(&effect, atom!("endTime"))?, &n(scope.game_time as f64)) {
                store_capacity = add(&store_capacity, &power_effect(C::PWR_OPERATE_STORAGE, &get(&effect, atom!("level"))?));
            }
        }
    }
    if !loose_eq(&store_capacity, &object.get(atom!("storeCapacity"))) {
        update(&mut scope.bulk, object, vec![(atom!("storeCapacity"), store_capacity)])?;
    }
    Ok(())
}
