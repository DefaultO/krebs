//! mirror: reference/engine/src/processor/intents/labs/boost-creep.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::creeps::_recalc_body::recalc_body;
use crate::processor::RoomScope;
use std::str::FromStr;

/// `C.BOOSTS[part][mineral]` is truthy.
fn boostable(part: &V, mineral: &V) -> bool {
    let (Some(part), Some(mineral)) = (C::BodyPart::from_str(&to_js_string(part)).ok(), resource(mineral)) else { return false };
    C::BOOSTS.iter().any(|(p, row)| *p == part && row.iter().any(|(r, _)| *r == mineral))
}

/// `Array.prototype.slice(0, end)`'s end index for `end` = ToIntegerOrInfinity(v).
pub(crate) fn slice_end(len: usize, v: &V) -> usize {
    let e = to_number(v);
    let e = if e.is_nan() { 0.0 } else { e.trunc() };
    if e < 0.0 {
        (len as f64 + e).max(0.0) as usize
    } else {
        e.min(len as f64) as usize
    }
}

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let store = object.get(atom!("store"));
    if !truthy(&store) {
        return Ok(());
    }

    let mineral = mineral_type(&store);
    if !truthy(&mineral) {
        return Ok(());
    }
    if lt(&get(&store, key(&mineral))?, &n(C::LAB_BOOST_MINERAL as f64)) || lt(&get(&store, atom!("energy"))?, &n(C::LAB_BOOST_ENERGY as f64)) {
        return Ok(());
    }

    let Some(target) = by_key(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    if !target.eq_str(atom!("type"), "creep") || truthy(&target.get(atom!("spawning"))) {
        return Ok(());
    }
    if (target.num(atom!("x")) - object.num(atom!("x"))).abs() > 1.0 || (target.num(atom!("y")) - object.num(atom!("y"))).abs() > 1.0 {
        return Ok(());
    }

    let mut parts: Vec<V> = values(&target.get(atom!("body")))
        .into_iter()
        .filter(|i| !truthy(&peek(i, atom!("boost"))) && boostable(&peek(i, atom!("type")), &mineral))
        .collect();

    if parts.is_empty() {
        return Ok(());
    }

    if !loose_eq(&peek(&parts[0], atom!("type")), &s(C::TOUGH)) {
        parts.reverse();
    }

    let count = get(intent, atom!("bodyPartsCount"))?;
    if truthy(&count) {
        parts.truncate(slice_end(parts.len(), &count));
    }

    let mut parts = parts.into_iter();
    loop {
        let enough = ge(&peek(&store, key(&mineral)), &n(C::LAB_BOOST_MINERAL as f64)) && ge(&peek(&store, atom!("energy")), &n(C::LAB_BOOST_ENERGY as f64));
        if !enough {
            break;
        }
        let Some(part) = parts.next() else { break };
        put(&part, atom!("boost"), mineral.clone())?;
        put(&store, key(&mineral), n(to_number(&peek(&store, key(&mineral))) - C::LAB_BOOST_MINERAL as f64))?;
        put(&store, atom!("energy"), n(to_number(&peek(&store, atom!("energy"))) - C::LAB_BOOST_ENERGY as f64))?;
    }

    let left = peek(&store, key(&mineral));
    let diff = obj(vec![(key(&mineral), left.clone()), (atom!("energy"), peek(&store, atom!("energy")))]);
    if truthy(&left) {
        update(&mut scope.bulk, object, vec![(atom!("store"), diff)])?;
    } else {
        update(
            &mut scope.bulk,
            object,
            vec![
                (atom!("store"), diff),
                (atom!("storeCapacityResource"), obj(vec![(key(&mineral), V::Null)])),
                (atom!("storeCapacity"), n((C::LAB_ENERGY_CAPACITY + C::LAB_MINERAL_CAPACITY) as f64)),
            ],
        )?;
    }

    recalc_body(&target)?;

    update(&mut scope.bulk, &target, vec![(atom!("body"), target.get(atom!("body"))), (atom!("storeCapacity"), target.get(atom!("storeCapacity")))])?;
    let at = obj(vec![(atom!("x"), object.get(atom!("x"))), (atom!("y"), object.get(atom!("y")))]);
    put(&target.get(atom!("actionLog")), atom!("healed"), at)
}
