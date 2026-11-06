//! mirror: reference/engine/src/processor/intents/creeps/rangedHeal.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).
//! The statements are heal.js's with range 3, the `rangedHeal` power and log key and EVENT_HEAL_TYPE_RANGED,
//! so both share `heal::heal`.

use crate::live::{Crash, O, V};
use crate::processor::RoomScope;

/// CREEP-RANGED-HEAL.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    super::heal::heal(object, intent, scope, true)
}
