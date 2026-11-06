//! mirror: reference/engine/src/processor/intents/power-creeps/withdraw.js
//! parity: FAITHFUL
//! Verified against the official processor by the fuzz fixtures in crates/engine/tests/fixtures/oracle/pc.

use crate::live::{Crash, O, V};
use crate::processor::RoomScope;

/// Delegates to creeps/withdraw.js.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    crate::processor::intents::creeps::withdraw::run(object, intent, scope)
}
