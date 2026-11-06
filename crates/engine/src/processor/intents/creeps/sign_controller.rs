//! mirror: reference/engine/src/processor/intents/creeps/signController.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).
//! `datetime` is `vclock(T)` (CLK-SIGN).

use crate::atom;
use crate::live::{truthy, Crash, O, V};
use crate::processor::{obj, out_of_range, prop, room_object, type_is, update, RoomScope};

/// CREEP-SIGN-CONTROLLER.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let Some(target) = room_object(scope, &prop(intent, atom!("id"))?) else { return Ok(()) };
    if !type_is(&target, "controller") {
        return Ok(());
    }
    if out_of_range(object, &target, 1.0) {
        return Ok(());
    }
    let text = prop(intent, atom!("sign"))?;
    let sign = if truthy(&text) {
        obj! {
            "user" => object.get(atom!("user")),
            "text" => text,
            "time" => V::Num(scope.game_time as f64),
            "datetime" => V::Num(scope.ctx.vnow as f64),
        }
    } else {
        V::Null
    };
    update(scope, &target, &obj! { "sign" => sign })
}
