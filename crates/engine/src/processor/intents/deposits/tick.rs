//! mirror: reference/engine/src/processor/intents/deposits/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    let cooldown = object.get(atom!("_cooldown"));
    if truthy(&cooldown) {
        update(&mut scope.bulk, object, vec![(atom!("cooldownTime"), add(&n(game_time), &cooldown))])?;
    }

    let decay_time = object.get(atom!("decayTime"));
    if truthy(&decay_time) && gt(&n(game_time), &decay_time) {
        remove_object(scope, object);
    }
    Ok(())
}
