//! mirror: reference/engine/src/processor/intents/creeps/say.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).
//! `substring(0,10)` counts UTF-16 units; a surrogate pair cut in half becomes U+FFFD (J6).

use crate::atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::{obj, prop, put, type_is, RoomScope};

/// CREEP-SAY.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let _ = scope;
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let V::Str(message) = prop(intent, atom!("message"))? else { return Ok(()) };
    let units: Vec<u16> = message.encode_utf16().take(10).collect();
    let say = obj! { "message" => V::Str(String::from_utf16_lossy(&units).into()), "isPublic" => prop(intent, atom!("isPublic"))? };
    put(&object.get(atom!("actionLog")), atom!("say"), say)
}
