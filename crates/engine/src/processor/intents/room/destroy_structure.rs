//! mirror: reference/engine/src/processor/intents/room/destroy-structure.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::creeps::_clear_newbie_walls::clear_newbie_walls;
use crate::processor::intents::structures::_destroy::destroy;
use crate::processor::RoomScope;
use crate::world::UserIdx;

pub fn run(user: UserIdx, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let user_id = user_id(scope, user);

    let Some(object) = by_key(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    if !truthy(&construction_cost(&object.get(atom!("type")))) {
        return Ok(());
    }

    match &scope.room_controller {
        Some(c) if loose_eq(&c.get(atom!("user")), &user_id) => {}
        _ => return Ok(()),
    }

    if loose_eq(&object.get(atom!("type")), &s(C::STRUCTURE_WALL)) && truthy(&object.get(atom!("decayTime"))) && !truthy(&object.get(atom!("user"))) {
        return Ok(());
    }

    if room_any(scope, |i| {
        let kind = i.get(atom!("type"));
        (loose_eq(&kind, &s("creep")) || loose_eq(&kind, &s("powerCreep"))) && !loose_eq(&i.get(atom!("user")), &user_id)
    }) {
        return Ok(());
    }

    destroy(&object, scope, None)?;

    if object.eq_str(atom!("type"), "constructedWall") && truthy(&object.get(atom!("decayTime"))) && truthy(&object.get(atom!("user"))) {
        clear_newbie_walls(scope)?;
    }
    Ok(())
}
