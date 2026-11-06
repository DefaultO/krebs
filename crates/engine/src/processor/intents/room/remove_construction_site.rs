//! mirror: reference/engine/src/processor/intents/room/remove-construction-site.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::_create_energy::create_energy;
use crate::processor::RoomScope;
use crate::world::UserIdx;

pub fn run(user: UserIdx, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let user_id = user_id(scope, user);

    let Some(object) = by_key(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    if !object.eq_str(atom!("type"), "constructionSite") {
        return Ok(());
    }

    let controller_owner = scope.room_controller.as_ref().is_some_and(|c| loose_eq(&c.get(atom!("user")), &user_id));
    if !loose_eq(&object.get(atom!("user")), &user_id) && !controller_owner {
        return Ok(());
    }

    // the site stays in roomObjects (ROOM-REMOVE-CSITE)
    bulk_remove(&mut scope.bulk, &object.get(atom!("_id")));
    if gt(&object.get(atom!("progress")), &n(1.0)) {
        let room = to_js_string(&object.get(atom!("room")));
        let refund = (object.num(atom!("progress")) / 2.0).floor();
        create_energy(object.num(atom!("x")), object.num(atom!("y")), &room, refund, C::RESOURCE_ENERGY, scope)?;
    }
    Ok(())
}
