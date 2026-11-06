//! mirror: reference/engine/src/processor/intents/energy/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    if !object.eq_str(atom!("type"), "energy") {
        return Ok(());
    }

    let resource_type = object.get(atom!("resourceType"));
    let k = key(&if truthy(&resource_type) { resource_type } else { s(C::RESOURCE_ENERGY) });

    let amount = object.num(k);
    object.set(k, n(amount - (amount / C::ENERGY_DECAY as f64).ceil()));

    let left = object.get(k);
    if le(&left, &n(0.0)) || !truthy(&left) {
        // vanilla logs "Energy NaN: dropped" for a NaN amount
        remove_object(scope, object);
    } else {
        update(&mut scope.bulk, object, vec![(k, left)])?;
    }
    Ok(())
}
