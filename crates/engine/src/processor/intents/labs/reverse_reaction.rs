//! mirror: reference/engine/src/processor/intents/labs/reverse-reaction.js
//! parity: FAITHFUL

use super::run_reaction::{cooling_down, reaction_amount, reaction_time, update_drained, update_filled};
use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

/// reverse-reaction.js:28-42 for one input lab: `labN`, its mineral, then the rejection test. BUG-CRASH-REVERSE-LAB:
/// `labN.store` of a lab destroyed earlier this tick throws (private) or rejects (the null check first).
fn input_lab(id: &V, amount: &V, scope: &RoomScope) -> Result<Option<(O, V)>, Crash> {
    let lab = by_key(scope, id);
    let Some(lab) = lab else {
        // an Object.prototype member: `.store` reads undefined, then `lab.type != 'lab'` rejects
        if inherited(id) || scope.ctx.compat.fix("BUG-CRASH-REVERSE-LAB") {
            return Ok(None);
        }
        return Err(crash_read(&V::Undef, "store"));
    };
    let mineral = mineral_type(&lab.get(atom!("store")));
    if !lab.eq_str(atom!("type"), "lab") {
        return Ok(None);
    }
    if truthy(&mineral) {
        let held = add(&get(&lab.get(atom!("store")), key(&mineral))?, amount);
        if gt(&held, &get(&lab.get(atom!("storeCapacityResource")), key(&mineral))?) {
            return Ok(None);
        }
    }
    Ok(Some((lab, mineral)))
}

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !object.eq_str(atom!("type"), "lab") || cooling_down(object, scope) {
        return Ok(());
    }

    let (id1, id2) = (get(intent, atom!("lab1"))?, get(intent, atom!("lab2"))?);
    if loose_eq(&id1, &id2) {
        return Ok(());
    }

    let reaction_amount = reaction_amount(object, scope)?;

    let store = object.get(atom!("store"));
    let mineral = mineral_type(&store);
    let have = get(&store, key(&mineral))?;
    if lt(&if truthy(&have) { have } else { n(0.0) }, &reaction_amount) {
        return Ok(());
    }

    let Some((lab1, lab1_mineral)) = input_lab(&id1, &reaction_amount, scope)? else { return Ok(()) };
    let Some((lab2, lab2_mineral)) = input_lab(&id2, &reaction_amount, scope)? else { return Ok(()) };

    let variants = get_reaction_variants(&mineral);
    let variant = variants.into_iter().find(|(a, b)| {
        (!truthy(&lab1_mineral) || loose_eq(&lab1_mineral, &s(a.as_str()))) && (!truthy(&lab2_mineral) || loose_eq(&lab2_mineral, &s(b.as_str())))
    });
    let Some((v0, v1)) = variant else { return Ok(()) };

    put(&store, key(&mineral), n(to_number(&peek(&store, key(&mineral))) - to_number(&reaction_amount)))?;
    let cooldown = add(&n(scope.game_time as f64), &reaction_time(&mineral));
    update_drained(object, &mineral, vec![(atom!("cooldownTime"), cooldown)], scope)?;

    update_filled(&lab1, &s(v0.as_str()), &reaction_amount, Vec::new(), scope)?;
    update_filled(&lab2, &s(v1.as_str()), &reaction_amount, Vec::new(), scope)?;

    let log = obj(vec![(atom!("x1"), lab1.get(atom!("x"))), (atom!("y1"), lab1.get(atom!("y"))), (atom!("x2"), lab2.get(atom!("x"))), (atom!("y2"), lab2.get(atom!("y")))]);
    put(&object.get(atom!("actionLog")), atom!("reverseReaction"), log)
}
