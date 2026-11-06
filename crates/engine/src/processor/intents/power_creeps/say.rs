//! mirror: reference/engine/src/processor/intents/power-creeps/say.js
//! parity: FAITHFUL
//! Verified against the official processor by the fuzz fixtures in crates/engine/tests/fixtures/oracle/pc.
//! Unlike creeps/say.js: no type or spawning check. `substring(0,10)` counts UTF-16 units.

use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

/// PC-SAY.
pub fn run(object: &O, intent: &V, _scope: &mut RoomScope) -> Result<(), Crash> {
    let V::Str(message) = get(intent, atom!("message"))? else { return Ok(()) };
    let units: Vec<u16> = message.encode_utf16().take(10).collect();
    let say = obj(vec![(atom!("message"), V::Str(String::from_utf16_lossy(&units).into())), (atom!("isPublic"), peek(intent, atom!("isPublic")))]);
    put(&object.get(atom!("actionLog")), atom!("say"), say)
}
