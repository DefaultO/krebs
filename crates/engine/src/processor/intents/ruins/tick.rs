//! mirror: reference/engine/src/processor/intents/ruins/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::containers::tick::drop_store;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let decay_time = object.get(atom!("decayTime"));
    if !truthy(&decay_time) || scope.game_time as f64 >= to_number(&decay_time) - 1.0 {
        drop_store(object, |_| true, scope)?;

        remove_object(scope, object);
    }
    Ok(())
}
