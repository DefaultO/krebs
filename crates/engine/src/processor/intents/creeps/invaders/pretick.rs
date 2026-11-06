//! mirror: reference/engine/src/processor/intents/creeps/invaders/pretick.js
//! parity: FAITHFUL
//!
//! `creeps` and `defenders` of the vanilla context are never read by the AI files, so they are not built.

use super::{find_attack, healer, shoot_at_will};
use crate::atom;
use crate::constants as C;
use crate::live::{truthy, Crash, O, V};
use crate::processor::common::fake_runtime::{has_active_bodyparts, Intents};
use crate::processor::{eq_str, get_or_undef, loose_eq, RoomScope};

/// The invader AI's shared context (pretick.js:37).
pub struct Context {
    pub intents: Intents,
    pub invaders: Vec<O>,
    pub healers: Vec<O>,
    pub hostiles: Vec<O>,
    pub fortifications: Vec<O>,
}

/// Returns `{objectId: intents}` (PROC-NPC-PRETICK).
pub fn pretick(creep: &O, scope: &mut RoomScope) -> Result<V, Crash> {
    let mut cx = Context { intents: Intents::new(), invaders: Vec::new(), healers: Vec::new(), hostiles: Vec::new(), fortifications: Vec::new() };
    let user = creep.get(atom!("user"));
    for key in scope.room_objects.keys() {
        let Some(o) = scope.room_objects.get(&key) else { continue };
        let ty = o.get(atom!("type"));
        // `!object.spawning && object.type == 'creep' || object.type == 'powerCreep'`
        if (!truthy(&o.get(atom!("spawning"))) && eq_str(&ty, "creep")) || eq_str(&ty, "powerCreep") {
            if loose_eq(&user, &o.get(atom!("user"))) {
                cx.invaders.push(o.clone());
                if has_active_bodyparts(&o, C::HEAL) {
                    cx.healers.push(o.clone());
                }
            } else if !loose_eq(&o.get(atom!("user")), &V::Num(3.0)) {
                cx.hostiles.push(o.clone());
            }
        }
        if eq_str(&ty, C::STRUCTURE_RAMPART) || eq_str(&ty, C::STRUCTURE_WALL) {
            cx.fortifications.push(o);
        }
    }
    // _.some(creep.body, {type: HEAL}): any heal part, damaged or not
    let has_heal = match creep.get(atom!("body")) {
        V::Arr(b) => b.to_vec().iter().any(|p| crate::live::is_equal(&get_or_undef(p, atom!("type")), &V::str(C::HEAL))),
        _ => false,
    };
    if has_heal {
        healer::run(creep, &mut cx, scope)?;
    } else {
        find_attack::run(creep, &mut cx, scope)?;
    }
    shoot_at_will::run(creep, &cx)?;
    Ok(cx.intents.list())
}
