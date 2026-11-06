//! mirror: reference/engine/src/processor/intents/factories/tick.js
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
            && truthy(&controller_structures("factory", &level)?)
        {
            C::FACTORY_CAPACITY
        } else {
            0
        };
        if !loose_eq(&n(store_capacity as f64), &object.get(atom!("storeCapacity"))) {
            update(&mut scope.bulk, object, vec![(atom!("storeCapacity"), n(store_capacity as f64))])?;
        }
    }

    persist_action_log(object, scope)
}

/// `if(!_.isEqual(object._actionLog, object.actionLog)) bulk.update(object, {actionLog: object.actionLog});`
/// (factories, labs, towers).
pub(crate) fn persist_action_log(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    if !is_equal(&object.get(atom!("_actionLog")), &object.get(atom!("actionLog"))) {
        update(&mut scope.bulk, object, vec![(atom!("actionLog"), object.get(atom!("actionLog")))])?;
    }
    Ok(())
}
