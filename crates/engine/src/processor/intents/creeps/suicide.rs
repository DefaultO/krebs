//! mirror: reference/engine/src/processor/intents/creeps/suicide.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::{eq_str, type_is, RoomScope};

/// CREEP-SUICIDE: invaders ("2") drop nothing.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let _ = intent;
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let drop_rate = if eq_str(&object.get(atom!("user")), "2") { Some(0.0) } else { None };
    super::_die::die(object, drop_rate, false, scope, None)
}
