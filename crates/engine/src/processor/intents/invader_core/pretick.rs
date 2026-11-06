//! mirror: reference/engine/src/processor/intents/invader-core/pretick.js
//! parity: FAITHFUL
//!
//! The context's `creeps` list is never read by the behaviors, so it is not built.

use super::stronghold::stronghold::{has_behavior, run_behavior, Context};
use crate::atom;
use crate::constants as C;
use crate::doc::Atom;
use crate::live::{to_js_string, truthy, Crash, O, V};
use crate::processor::common::fake_runtime::Intents;
use crate::processor::{eq_str, lt, loose_eq, room_object_list, RoomScope};
use std::collections::BTreeMap;

/// Returns `{objectId: intents}` (PROC-NPC-PRETICK), or undefined for an unknown behavior.
pub fn pretick(object: &O, scope: &mut RoomScope) -> Result<V, Crash> {
    let behavior = if truthy(&object.get(atom!("deployTime"))) {
        "deploy".to_string()
    } else {
        let b = object.get(Atom::intern("strongholdBehavior"));
        if truthy(&b) {
            to_js_string(&b).to_string()
        } else {
            "default".to_string()
        }
    };
    if !has_behavior(&behavior) {
        return Ok(V::Undef);
    }
    let user = object.get(atom!("user"));
    let mut cx = Context {
        intents: Intents::new(),
        defenders: Vec::new(),
        damaged_defenders: Vec::new(),
        hostiles: Vec::new(),
        towers: Vec::new(),
        ramparts: Vec::new(),
        damaged_roads: Vec::new(),
        core: object.clone(),
        spots: BTreeMap::new(),
    };
    for o in room_object_list(scope) {
        let ty = o.get(atom!("type"));
        if (eq_str(&ty, "creep") || eq_str(&ty, "powerCreep")) && !truthy(&o.get(atom!("spawning"))) {
            if loose_eq(&o.get(atom!("user")), &user) {
                cx.defenders.push(o.clone());
                if lt(&o.get(atom!("hits")), &o.get(atom!("hitsMax"))) {
                    cx.damaged_defenders.push(o);
                }
            } else if !loose_eq(&o.get(atom!("user")), &V::str("3")) {
                cx.hostiles.push(o);
            }
            continue;
        }
        if eq_str(&ty, C::STRUCTURE_TOWER) && loose_eq(&o.get(atom!("user")), &user) {
            cx.towers.push(o);
            continue;
        }
        if eq_str(&ty, C::STRUCTURE_RAMPART) && loose_eq(&o.get(atom!("user")), &user) {
            cx.ramparts.push(o);
            continue;
        }
        let sid = Atom::intern("strongholdId");
        if loose_eq(&object.get(sid), &o.get(sid)) && eq_str(&ty, C::STRUCTURE_ROAD) && lt(&o.get(atom!("hits")), &o.get(atom!("hitsMax"))) {
            cx.damaged_roads.push(o);
        }
    }
    run_behavior(&behavior, &mut cx, scope)?;
    Ok(cx.intents.list())
}
