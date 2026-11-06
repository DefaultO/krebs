//! mirror: reference/engine/src/processor/intents/terminal/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    if let Some(ctrl) = scope.room_controller.clone() {
        let level = ctrl.get(atom!("level"));
        let store_capacity = if gt(&level, &n(0.0))
            && loose_eq(&ctrl.get(atom!("user")), &object.get(atom!("user")))
            && truthy(&controller_structures("terminal", &level)?)
        {
            C::TERMINAL_CAPACITY
        } else {
            0
        };
        if !loose_eq(&n(store_capacity as f64), &object.get(atom!("storeCapacity"))) {
            update(&mut scope.bulk, object, vec![(atom!("storeCapacity"), n(store_capacity as f64))])?;
        }
    }
    Ok(())
}
