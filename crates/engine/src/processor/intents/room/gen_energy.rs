//! mirror: reference/engine/src/processor/intents/room/gen-energy.js
//! parity: FAITHFUL
//!
//! ROOM-GEN-ENERGY: not an intent type of system.js, so only mods (custom intent types) reach it.

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;
use crate::world::UserIdx;

pub fn run(user: UserIdx, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !loose_eq(&user_id(scope, user), &s("3")) {
        return Ok(());
    }

    let (mut x, mut y);
    loop {
        x = n((scope.rng.random() * 48.0).floor() + 1.0);
        y = n((scope.rng.random() * 48.0).floor() + 1.0);
        let blocked = room_any(scope, |i| is_obstacle_type(&i.get(atom!("type"))) && loose_eq(&i.get(atom!("x")), &x) && loose_eq(&i.get(atom!("y")), &y))
            || check_terrain(scope.room_terrain, &x, &y, C::TERRAIN_MASK_WALL);
        if !blocked {
            break;
        }
    }

    let energy = obj(vec![
        (atom!("x"), x),
        (atom!("y"), y),
        (atom!("type"), s("energy")),
        (atom!("energy"), n(300.0)),
        (atom!("room"), get(intent, atom!("roomName"))?),
    ]);
    scope.bulk.insert(&energy, None);
    Ok(())
}
