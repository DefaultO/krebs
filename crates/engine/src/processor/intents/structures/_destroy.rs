//! mirror: reference/engine/src/processor/intents/structures/_destroy.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::invader_core;
use crate::processor::RoomScope;

/// `module.exports = function(object, scope, attackType)`.
pub fn destroy(object: &O, scope: &mut RoomScope, attack_type: Option<u32>) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    let spawning = object.get(atom!("spawning"));
    if object.eq_str(atom!("type"), "spawn") && truthy(&spawning) {
        let src = [(atom!("user"), object.get(atom!("user"))), (atom!("name"), get(&spawning, atom!("name"))?)];
        if let Some(creep) = room_find(scope, |i| room_match(i, &src)) {
            remove_object(scope, &creep);
        }
    }

    if object.eq_str(atom!("type"), "invaderCore") {
        invader_core::destroy::destroy(object, scope)?;
    }

    if attack_type != Some(C::EVENT_ATTACK_TYPE_NUKE) {
        let id = match object.get(atom!("_id")) {
            v @ (V::Undef | V::Null) => return Err(crash_read(&v, "toString")),
            v => V::Str(to_js_string(&v)),
        };
        let kind = object.get(atom!("type"));
        let ruin_decay = C::RUIN_DECAY_STRUCTURES
            .iter()
            .find(|(t, _)| loose_eq(&kind, &s(t.as_str())))
            .map_or(C::RUIN_DECAY, |&(_, d)| d);
        let ruin = obj(vec![
            (atom!("type"), s("ruin")),
            (atom!("room"), object.get(atom!("room"))),
            (atom!("x"), object.get(atom!("x"))),
            (atom!("y"), object.get(atom!("y"))),
            (
                atom!("structure"),
                obj(vec![
                    (atom!("id"), id),
                    (atom!("type"), kind),
                    (atom!("hits"), n(0.0)),
                    (atom!("hitsMax"), object.get(atom!("hitsMax"))),
                    (atom!("user"), object.get(atom!("user"))),
                ]),
            ),
            (atom!("destroyTime"), n(game_time)),
            (atom!("decayTime"), n(game_time + ruin_decay as f64)),
        ]);
        if truthy(&object.get(atom!("user"))) {
            put(&ruin, atom!("user"), object.get(atom!("user")))?;
        }
        let store = object.get(atom!("store"));
        put(&ruin, atom!("store"), if truthy(&store) { store } else { obj(vec![]) })?;

        if truthy(&object.get(atom!("effects"))) {
            if let Some(collapse) = find_by(&object.get(atom!("effects")), &[(atom!("effect"), n(C::EFFECT_COLLAPSE_TIMER as f64))]) {
                // `_.max([ruin.decayTime, collapseEffect.endTime])` keeps the first maximal value as is.
                let end_time = get(&collapse, atom!("endTime"))?;
                if to_number(&end_time) > game_time + ruin_decay as f64 {
                    put(&ruin, atom!("decayTime"), end_time)?;
                }
            }
        }

        scope.bulk.insert(&ruin, None);
    }

    remove_object(scope, object);
    Ok(())
}
