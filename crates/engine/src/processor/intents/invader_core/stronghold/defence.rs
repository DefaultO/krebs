//! mirror: reference/engine/src/processor/intents/invader-core/stronghold/defence.js
//! parity: FAITHFUL

use super::stronghold::Context;
use crate::atom;
use crate::constants as C;
use crate::live::{to_number, Crash, O, V};
use crate::processor::{eq_str, room_object_list, RoomScope};
use crate::utils::dist;
use std::collections::BTreeMap;
use std::rc::Rc;

/// defence.js:8 createSafeMatrixCallback: None without hostiles; else the matrix the callback returns (every
/// tile 255, the core's ramparts 1, non-creep obstacles 255), which replaces the default one.
pub fn create_safe_matrix(cx: &Context, scope: &RoomScope) -> Option<Rc<Vec<u8>>> {
    if cx.hostiles.is_empty() {
        return None;
    }
    let mut m = vec![255u8; 2500];
    let mut set = |o: &O, v: u8| {
        let (x, y) = (to_number(&o.get(atom!("x"))) as i32, to_number(&o.get(atom!("y"))) as i32);
        if (0..2500).contains(&(x * 50 + y)) {
            m[(x * 50 + y) as usize] = v;
        }
    };
    for r in &cx.ramparts {
        set(r, 1);
    }
    for o in room_object_list(scope) {
        let ty = o.get(atom!("type"));
        if !eq_str(&ty, "creep") && matches!(&ty, V::Str(t) if C::OBSTACLE_OBJECT_TYPES.contains(&&**t)) {
            set(&o, 255);
        }
    }
    Some(Rc::new(m))
}

/// defence.js:36 distribute: `{50*x+y: agent}` (integer keys, so ascending when enumerated).
pub fn distribute(positions: &[O], agents: &[String]) -> Result<BTreeMap<u64, String>, Crash> {
    let mut result = BTreeMap::new();
    if agents.is_empty() {
        return Ok(result);
    }
    let mut agents: Vec<String> = agents.iter().take(positions.len()).cloned().collect();
    let mut weights: Vec<(O, f64)> = positions.iter().map(|p| (p.clone(), 100.0)).collect();
    while !agents.is_empty() {
        let creep = agents.remove(0);
        // _.max(weights, 'weight'): the first greatest
        let mut best = 0;
        for (i, w) in weights.iter().enumerate() {
            if w.1 > weights[best].1 {
                best = i;
            }
        }
        let (place, _) = weights.remove(best);
        let key = 50.0 * to_number(&place.get(atom!("x"))) + to_number(&place.get(atom!("y")));
        result.insert(key as u64, creep);
        let size = weights.len() as f64;
        let pv = V::Obj(place.clone());
        for w in weights.iter_mut() {
            w.1 -= crate::processor::js_max(0.0, size - dist(&V::Obj(w.0.clone()), &pv));
        }
    }
    Ok(result)
}
