//! mirror: reference/engine/src/processor/intents/portals/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    let unstable = object.get(atom!("unstableDate"));
    if truthy(&unstable) && gt(&date_now(scope), &unstable) {
        update(&mut scope.bulk, object, vec![(atom!("decayTime"), n(game_time + C::PORTAL_DECAY as f64)), (atom!("unstableDate"), V::Null)])?;
    }

    let decay_time = object.get(atom!("decayTime"));
    if truthy(&decay_time) && gt(&n(game_time), &decay_time) {
        remove_object(scope, object);

        let (x, y) = (add(&object.get(atom!("x")), &n(1.0)), add(&object.get(atom!("y")), &n(1.0)));
        let wall = room_find(scope, |i| i.eq_str(atom!("type"), "constructedWall") && loose_eq(&i.get(atom!("x")), &x) && loose_eq(&i.get(atom!("y")), &y));
        if let Some(wall) = wall {
            remove_object(scope, &wall);
        }
    }
    Ok(())
}
