//! mirror: reference/engine/src/processor/intents/creeps/invaders/flee.js
//! parity: FAITHFUL

use super::pretick::Context;
use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::common::fake_runtime;
use crate::processor::global::obj;
use crate::processor::RoomScope;
use crate::utils::dist;

/// Moves away from hostiles closer than `range`; true when it did.
pub fn run(creep: &O, range: f64, cx: &Context, scope: &RoomScope) -> Result<bool, Crash> {
    let cv = V::Obj(creep.clone());
    let near: Vec<O> = cx.hostiles.iter().filter(|c| dist(&cv, &V::Obj((*c).clone())) < range).cloned().collect();
    if !near.is_empty() {
        let direction = fake_runtime::flee(creep, &near, range, scope)?;
        if direction != 0.0 {
            cx.intents.set(&creep.get(atom!("_id")), "move", obj(vec![("direction", V::Num(direction))]));
            return Ok(true);
        }
    }
    Ok(false)
}
