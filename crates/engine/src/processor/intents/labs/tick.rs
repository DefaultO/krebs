//! mirror: reference/engine/src/processor/intents/labs/tick.js
//! parity: FAITHFUL

use crate::live::{Crash, O};
use crate::processor::intents::factories::tick::persist_action_log;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    persist_action_log(object, scope)
}
