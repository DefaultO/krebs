//! mirror: reference/engine/src/processor/intents/controllers/unclaim.js
//! parity: FAITHFUL

use super::tick::safe_mode_cooldown;
use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(object: &O, _intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !object.eq_str(atom!("type"), "controller") {
        return Ok(());
    }

    if !truthy(&object.get(atom!("user"))) || !truthy(&object.get(atom!("level"))) {
        return Ok(());
    }

    remove_room_from_user(scope, &object.get(atom!("room")), &object.get(atom!("user")))?;

    let cooldown = safe_mode_cooldown(scope);
    update(
        &mut scope.bulk,
        object,
        vec![
            (atom!("user"), V::Null),
            (atom!("level"), n(0.0)),
            (atom!("progress"), n(0.0)),
            (atom!("downgradeTime"), V::Null),
            (atom!("safeMode"), V::Null),
            (atom!("safeModeAvailable"), n(0.0)),
            (atom!("safeModeCooldown"), cooldown),
            (atom!("isPowerEnabled"), V::Bool(false)),
        ],
    )
}
