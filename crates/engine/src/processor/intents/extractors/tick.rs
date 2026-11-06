//! mirror: reference/engine/src/processor/intents/extractors/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    if gt(&object.get(atom!("cooldown")), &n(0.0)) {
        object.set(atom!("cooldown"), n(object.num(atom!("cooldown")) - 1.0));

        if lt(&object.get(atom!("cooldown")), &n(0.0)) {
            object.set(atom!("cooldown"), n(0.0));
        }

        update(&mut scope.bulk, object, vec![(atom!("cooldown"), object.get(atom!("cooldown")))])?;
    }

    let cooldown = object.get(atom!("_cooldown"));
    if truthy(&cooldown) {
        update(&mut scope.bulk, object, vec![(atom!("cooldown"), cooldown)])?;
    }

    Ok(())
}
