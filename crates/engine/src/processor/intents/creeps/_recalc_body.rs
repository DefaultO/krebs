//! mirror: reference/engine/src/processor/intents/creeps/_recalc-body.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::constants::{CARRY, CARRY_CAPACITY};
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::{get_or_undef, gt, prop, put};
use crate::utils;

/// BODY-RECALC: hits spread from the last part to the first; `storeCapacity` from the live CARRY parts.
pub fn recalc_body(object: &O) -> Result<(), Crash> {
    let mut hits = object.get(atom!("hits"));
    let body = object.get(atom!("body"));
    let mut i = to_number(&prop(&body, atom!("length"))?) - 1.0;
    while i >= 0.0 {
        let part = crate::processor::get_index(&body, i);
        let old = prop(&part, atom!("_oldHits"))?;
        put(&part, atom!("_oldHits"), if truthy(&old) { old } else { get_or_undef(&part, atom!("hits")) })?;
        put(&part, atom!("hits"), if gt(&hits, &V::Num(100.0)) { V::Num(100.0) } else { hits.clone() })?;
        hits = V::Num(to_number(&hits) - 100.0);
        if to_number(&hits) < 0.0 {
            hits = V::Num(0.0);
        }
        i -= 1.0;
    }
    if !truthy(&object.get(atom!("noCapacityRecalc"))) {
        let capacity = utils::calc_body_effectiveness(&object.get(atom!("body")), CARRY, "capacity", CARRY_CAPACITY as f64, true)?;
        object.set(atom!("storeCapacity"), V::Num(capacity));
    }
    Ok(())
}
