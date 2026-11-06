//! mirror: reference/engine/src/processor/intents/creeps/_drop-resources-without-space.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::constants::RESOURCES_ALL;
use crate::doc::Atom;
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::{js_min, le, obj, prop, RoomScope};
use crate::utils;

/// BODY-DROP-OVERFLOW: drop in RESOURCES_ALL order until the store fits.
pub fn drop_resources_without_space(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    for r in RESOURCES_ALL {
        let total = utils::calc_resources(&V::Obj(object.clone()))?;
        if le(&V::Num(total), &object.get(atom!("storeCapacity"))) {
            break;
        }
        let have = prop(&object.get(atom!("store")), Atom::intern(r.as_str()))?;
        if truthy(&have) {
            let amount = js_min(to_number(&have), total - to_number(&object.get(atom!("storeCapacity"))));
            super::drop::run(object, &obj! { "amount" => V::Num(amount), "resourceType" => V::str(r.as_str()) }, scope)?;
        }
    }
    Ok(())
}
