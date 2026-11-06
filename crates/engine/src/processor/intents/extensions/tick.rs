//! mirror: reference/engine/src/processor/intents/extensions/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    if !object.eq_str(atom!("type"), "extension") {
        return Ok(());
    }

    if let Some(ctrl) = &scope.room_controller {
        let capacities = C::EXTENSION_ENERGY_CAPACITY.map(Some);
        let cap = by_index(&capacities, &ctrl.get(atom!("level")));
        let store_capacity = if truthy(&cap) { cap } else { n(0.0) };
        let current = object.get(atom!("storeCapacityResource"));
        if !truthy(&current) || !truthy(&get(&current, atom!("energy"))?) || !loose_eq(&store_capacity, &get(&current, atom!("energy"))?) {
            update(&mut scope.bulk, object, vec![(atom!("storeCapacityResource"), obj(vec![(atom!("energy"), store_capacity)]))])?;
        }
    }
    Ok(())
}
