//! mirror: reference/engine/src/processor/intents/controllers/activateSafeMode.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(object: &O, _intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = n(scope.game_time as f64);

    if !truthy(&object.get(atom!("user"))) || !truthy(&object.get(atom!("level"))) {
        return Ok(());
    }
    if !gt(&object.get(atom!("safeModeAvailable")), &n(0.0)) {
        return Ok(());
    }
    if ge(&object.get(atom!("safeModeCooldown")), &game_time) {
        return Ok(());
    }
    if gt(&object.get(atom!("upgradeBlocked")), &game_time) {
        return Ok(());
    }
    let threshold = n(to_number(&game_time) + to_number(&by_index(&C::CONTROLLER_DOWNGRADE, &object.get(atom!("level")))) / 2.0
        - C::CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD as f64);
    if lt(&object.get(atom!("downgradeTime")), &threshold) {
        return Ok(());
    }

    object.set(atom!("_safeModeActivated"), n(1.0));
    Ok(())
}
