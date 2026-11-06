//! mirror: reference/engine/src/processor/intents/construction-sites/tick.js
//! parity: FAITHFUL
//!
//! CSITE-TICK: a no-op (sites never decay).

use crate::live::{Crash, O};
use crate::processor::RoomScope;

pub fn tick(_object: &O, _scope: &mut RoomScope) -> Result<(), Crash> {
    Ok(())
}
