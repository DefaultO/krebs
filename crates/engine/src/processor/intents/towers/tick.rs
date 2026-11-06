//! mirror: reference/engine/src/processor/intents/towers/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O};
use crate::processor::intents::factories::tick::persist_action_log;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    if !object.eq_str(atom!("type"), "tower") {
        return Ok(());
    }

    persist_action_log(object, scope)
}
