//! mirror: reference/engine/src/processor/intents/creeps/_add-fatigue.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::live::{to_js_string, to_number, truthy, Crash, O, V};
use crate::processor::{js_add, js_max, js_min, loose_eq, obj, update, RoomScope};

/// MOVE-FATIGUE-CHAIN: recovery clears the creep's own fatigue first; the rest (and any cost) goes to the
/// head of its pull train.
pub fn add_fatigue(object: &O, d_fatigue: f64, scope: &mut RoomScope) -> Result<(), Crash> {
    let mut object = object.clone();
    let mut d = d_fatigue;
    if matches!(object.get(atom!("_fatigue")), V::Undef) {
        object.set(atom!("_fatigue"), object.get(atom!("fatigue")));
    }

    // MOVES contribution removes own fatigue first
    if to_number(&object.get(atom!("_fatigue"))) > 0.0 && d < 0.0 {
        let resting = js_min(to_number(&object.get(atom!("_fatigue"))), -d);
        object.set(atom!("_fatigue"), V::Num(to_number(&object.get(atom!("_fatigue"))) - resting));
        d += resting;
        let fatigue = V::Num(js_max(0.0, to_number(&object.get(atom!("_fatigue")))));
        if !loose_eq(&object.get(atom!("fatigue")), &fatigue) {
            update(scope, &object, &obj! { "fatigue" => fatigue })?;
        }
        if d == 0.0 {
            return Ok(());
        }
    }

    let mut steps = 0;
    loop {
        let pulled = object.get(atom!("_pulled"));
        let Some(next) = (if truthy(&pulled) { scope.room_objects.get(&to_js_string(&pulled)) } else { None }) else { break };
        object = next;
        steps += 1;
        // ponytail: a `_pulled` cycle would hang vanilla's processor; it is reported as a crash here.
        if steps > scope.room_objects.len() {
            return Err(Crash("_add-fatigue: cyclic pull chain".into()));
        }
    }

    if matches!(object.get(atom!("_fatigue")), V::Undef) {
        object.set(atom!("_fatigue"), object.get(atom!("fatigue")));
    }
    object.set(atom!("_fatigue"), js_add(&object.get(atom!("_fatigue")), &V::Num(d)));
    let fatigue = V::Num(js_max(0.0, to_number(&object.get(atom!("_fatigue")))));
    if !loose_eq(&object.get(atom!("fatigue")), &fatigue) {
        update(scope, &object, &obj! { "fatigue" => fatigue })?;
    }
    Ok(())
}
