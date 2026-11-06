//! mirror: reference/engine/src/processor/intents/power-creeps/move.js
//! parity: FAITHFUL
//! Verified against the official processor by the fuzz fixtures in crates/engine/tests/fixtures/oracle/pc.

use crate::live::{Crash, O, V};
use crate::processor::RoomScope;

/// Delegates to creeps/move.js.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    crate::processor::intents::creeps::r#move::run(object, intent, scope)
}
