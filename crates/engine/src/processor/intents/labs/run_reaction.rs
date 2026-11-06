//! mirror: reference/engine/src/processor/intents/labs/run-reaction.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;
use std::str::FromStr;

/// `reactionAmount`: LAB_REACTION_AMOUNT plus an active PWR_OPERATE_LAB effect.
pub(crate) fn reaction_amount(object: &O, scope: &RoomScope) -> Result<V, Crash> {
    let mut amount = n(C::LAB_REACTION_AMOUNT as f64);
    if let Some(effect) = find_power_effect(object, C::PWR_OPERATE_LAB) {
        if gt(&get(&effect, atom!("endTime"))?, &n(scope.game_time as f64)) {
            amount = add(&amount, &power_effect(C::PWR_OPERATE_LAB, &get(&effect, atom!("level"))?));
        }
    }
    Ok(amount)
}

/// `!!object.cooldownTime && object.cooldownTime > gameTime`
pub(crate) fn cooling_down(object: &O, scope: &RoomScope) -> bool {
    let t = object.get(atom!("cooldownTime"));
    truthy(&t) && gt(&t, &n(scope.game_time as f64))
}

/// `C.REACTION_TIME[m]`
pub(crate) fn reaction_time(m: &V) -> V {
    resource(m).and_then(C::reaction_time).map_or(V::Undef, |t| n(t as f64))
}

/// `(store[k]||0) + amount`
pub(crate) fn plus(store: &V, k: &V, amount: &V) -> Result<V, Crash> {
    let have = get(store, key(k))?;
    Ok(add(&if truthy(&have) { have } else { n(0.0) }, amount))
}

/// The mineral switch after a lab's mineral dropped: `store[m]` persisted, and when it is now 0 the
/// mineral capacity is removed and `storeCapacity` restored (LAB-STORE-MODEL).
pub(crate) fn update_drained(lab: &O, m: &V, extra: Vec<(crate::doc::Atom, V)>, scope: &mut RoomScope) -> Result<(), Crash> {
    let left = peek(&lab.get(atom!("store")), key(m));
    let mut data = vec![(atom!("store"), obj(vec![(key(m), left.clone())]))];
    if truthy(&left) {
        data.extend(extra);
    } else {
        data.push((atom!("storeCapacityResource"), obj(vec![(key(m), V::Null)])));
        data.push((atom!("storeCapacity"), n((C::LAB_ENERGY_CAPACITY + C::LAB_MINERAL_CAPACITY) as f64)));
        data.extend(extra);
    }
    update(&mut scope.bulk, lab, data)
}

/// `store: {[m]: (store[m]||0) + amount}` plus the mineral capacity when the lab had none for `m`.
pub(crate) fn update_filled(lab: &O, m: &V, amount: &V, extra: Vec<(crate::doc::Atom, V)>, scope: &mut RoomScope) -> Result<(), Crash> {
    let has_capacity = truthy(&get(&lab.get(atom!("storeCapacityResource")), key(m))?);
    let filled = plus(&lab.get(atom!("store")), m, amount)?;
    let mut data = vec![(atom!("store"), obj(vec![(key(m), filled)]))];
    data.extend(extra);
    if !has_capacity {
        data.push((atom!("storeCapacityResource"), obj(vec![(key(m), n(C::LAB_MINERAL_CAPACITY as f64))])));
        data.push((atom!("storeCapacity"), V::Null));
    }
    update(&mut scope.bulk, lab, data)
}

/// `C.REACTIONS[a][b]` (TypeError when `a` has no row).
fn reactions(a: &V, b: &V) -> Result<V, Crash> {
    let row = resource(a).and_then(|a| C::REACTIONS.iter().find(|(r, _)| *r == a));
    let Some((_, row)) = row else { return Err(crash_read(&V::Undef, &to_js_string(b))) };
    Ok(match b {
        V::Str(st) => C::ResourceType::from_str(st).ok().and_then(|b| row.iter().find(|(r, _)| *r == b)).map_or(V::Undef, |(_, p)| s(p.as_str())),
        _ => V::Undef,
    })
}

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if cooling_down(object, scope) {
        return Ok(());
    }

    let reaction_amount = reaction_amount(object, scope)?;

    let Some(lab1) = by_key(scope, &get(intent, atom!("lab1"))?).filter(|l| l.eq_str(atom!("type"), "lab")) else { return Ok(()) };
    let lab1_mineral = mineral_type(&lab1.get(atom!("store")));
    if !truthy(&lab1_mineral) || lt(&get(&lab1.get(atom!("store")), key(&lab1_mineral))?, &reaction_amount) {
        return Ok(());
    }
    if (lab1.num(atom!("x")) - object.num(atom!("x"))).abs() > 2.0 || (lab1.num(atom!("y")) - object.num(atom!("y"))).abs() > 2.0 {
        return Ok(());
    }

    let Some(lab2) = by_key(scope, &get(intent, atom!("lab2"))?).filter(|l| l.eq_str(atom!("type"), "lab")) else { return Ok(()) };
    let lab2_mineral = mineral_type(&lab2.get(atom!("store")));
    if !truthy(&lab2_mineral) || lt(&get(&lab2.get(atom!("store")), key(&lab2_mineral))?, &reaction_amount) {
        return Ok(());
    }
    if (lab2.num(atom!("x")) - object.num(atom!("x"))).abs() > 2.0 || (lab2.num(atom!("y")) - object.num(atom!("y"))).abs() > 2.0 {
        return Ok(());
    }

    let store = object.get(atom!("store"));
    let mineral = mineral_type(&store);
    if gt(&plus(&store, &mineral, &reaction_amount)?, &n(C::LAB_MINERAL_CAPACITY as f64)) {
        return Ok(());
    }

    let product = reactions(&lab1_mineral, &lab2_mineral)?;

    if !truthy(&product) || truthy(&mineral) && !loose_eq(&mineral, &product) {
        return Ok(());
    }

    let cooldown = add(&n(scope.game_time as f64), &reaction_time(&product));
    update_filled(object, &product, &reaction_amount, vec![(atom!("cooldownTime"), cooldown)], scope)?;

    for (lab, m) in [(&lab1, &lab1_mineral), (&lab2, &lab2_mineral)] {
        let lab_store = lab.get(atom!("store"));
        put(&lab_store, key(m), n(to_number(&peek(&lab_store, key(m))) - to_number(&reaction_amount)))?;
        update_drained(lab, m, Vec::new(), scope)?;
    }

    let log = obj(vec![(atom!("x1"), lab1.get(atom!("x"))), (atom!("y1"), lab1.get(atom!("y"))), (atom!("x2"), lab2.get(atom!("x"))), (atom!("y2"), lab2.get(atom!("y")))]);
    put(&object.get(atom!("actionLog")), atom!("runReaction"), log)
}
