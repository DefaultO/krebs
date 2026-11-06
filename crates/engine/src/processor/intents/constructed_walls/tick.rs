//! mirror: reference/engine/src/processor/intents/constructedWalls/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !object.eq_str(atom!("type"), "constructedWall") {
        return Ok(());
    }

    if let Some(ctrl) = &scope.room_controller {
        if !truthy(&object.get(atom!("newbieWall"))) {
            let hits_max = if truthy(&controller_structures("constructedWall", &ctrl.get(atom!("level")))?) { C::WALL_HITS_MAX } else { 0 };
            if !loose_eq(&n(hits_max as f64), &object.get(atom!("hitsMax"))) {
                update(&mut scope.bulk, object, vec![(atom!("hitsMax"), n(hits_max as f64))])?;
            }
        }
    }

    if gt(&object.get(atom!("ticksToLive")), &n(0.0)) {
        let decay_time = add(&n(game_time), &object.get(atom!("ticksToLive")));
        update(&mut scope.bulk, object, vec![(atom!("decayTime"), decay_time), (atom!("ticksToLive"), V::Null)])?;
    }

    let decay_time = object.get(atom!("decayTime"));
    if !truthy(&decay_time) {
        return Ok(());
    }

    let is_object = matches!(decay_time, V::Obj(_) | V::Arr(_));
    if !is_object {
        let unowned_room = matches!(&scope.room_controller, Some(c) if !truthy(&c.get(atom!("user"))));
        if game_time >= to_number(&decay_time) - 1.0 || unowned_room {
            remove_object(scope, object);
        }

        let user = object.get(atom!("user"));
        if truthy(&user) && game_time == to_number(&decay_time) - 5000.0 {
            send_notification(scope, &user, "Attention! Your room protection will be removed soon.\nLearn how to defend your room against intruders from <a href='http://support.screeps.com/hc/en-us/articles/203339002-Defending-your-room'>this article</a>.".into());
        }
    }

    if is_object && gt(&date_now(scope), &peek(&decay_time, atom!("timestamp"))) {
        remove_object(scope, object);
    }

    Ok(())
}
