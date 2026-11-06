//! mirror: reference/engine/src/processor/intents/spawns/_charge-energy.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

/// _charge-energy.js:6 oldEnergyHandling.
fn old_energy_handling(spawn: &O, mut cost: f64, scope: &mut RoomScope) -> Result<bool, Crash> {
    let user = spawn.get(atom!("user"));
    let mut spawns = room_filter(scope, |i| i.eq_str(atom!("type"), "spawn") && loose_eq(&i.get(atom!("user")), &user) && !truthy(&i.get(atom!("off"))));
    let mut extensions = room_filter(scope, |i| i.eq_str(atom!("type"), "extension") && loose_eq(&i.get(atom!("user")), &user) && !truthy(&i.get(atom!("off"))));
    // `_.sum(list, 'store.energy')`: a property path, so a missing store reads as undefined.
    let energy = |i: &V| Ok(peek(&peek(i, atom!("store")), atom!("energy")));
    let as_v = |list: &[O]| list.iter().map(|o| V::Obj(o.clone())).collect::<Vec<_>>();
    let available_energy = sum(&as_v(&extensions), energy)? + sum(&as_v(&spawns), energy)?;

    if available_energy < cost {
        return Ok(false);
    }

    stable_sort(&mut spawns, |a, b| comparator_distance(spawn, a, b));
    for i in &spawns {
        cost = drain(i, cost, scope)?;
    }

    if cost <= 0.0 {
        return Ok(true);
    }

    stable_sort(&mut extensions, |a, b| comparator_distance(spawn, a, b));
    for extension in &extensions {
        if cost <= 0.0 {
            continue;
        }
        cost = drain(extension, cost, scope)?;
    }

    Ok(true)
}

/// `var neededEnergy = Math.min(cost, i.store.energy); i.store.energy -= neededEnergy; cost -= neededEnergy;
/// bulk.update(i, {store:{energy: i.store.energy}});` (the two variants differ only in statement order
/// around `cost`, which nothing in between reads). Returns the new cost.
fn drain(i: &O, cost: f64, scope: &mut RoomScope) -> Result<f64, Crash> {
    let store = i.get(atom!("store"));
    let needed = js_min(cost, to_number(&get(&store, atom!("energy"))?));
    put(&store, atom!("energy"), n(to_number(&peek(&store, atom!("energy"))) - needed))?;
    let left = peek(&store, atom!("energy"));
    update(&mut scope.bulk, i, vec![(atom!("store"), obj(vec![(atom!("energy"), left)]))])?;
    Ok(cost - needed)
}

/// _charge-energy.js:41 newEnergyHandling.
fn new_energy_handling(spawn: &O, mut cost: f64, energy_structures: &V, scope: &mut RoomScope) -> Result<bool, Crash> {
    let user = spawn.get(atom!("user"));
    let ids: Vec<V> = values(energy_structures)
        .into_iter()
        .filter(|id| match by_key(scope, id) {
            Some(o) => {
                let kind = o.get(atom!("type"));
                !truthy(&o.get(atom!("off"))) && strict_eq(&o.get(atom!("user")), &user) && (strict_eq(&kind, &s("spawn")) || strict_eq(&kind, &s("extension")))
            }
            None => false,
        })
        .collect();

    let ids = uniq(ids);

    let available_energy = sum(&ids, |id| get(&get(&V::Obj(by_key(scope, id).unwrap()), atom!("store"))?, atom!("energy")))?;
    if available_energy < cost {
        return Ok(false);
    }

    for id in &ids {
        let energy_structure = by_key(scope, id).unwrap();
        cost = drain(&energy_structure, cost, scope)?;
        if cost <= 0.0 {
            break;
        }
    }

    Ok(true)
}

/// `module.exports = function chargeEnergy(spawn, cost, energyStructures, scope)`.
pub fn charge_energy(spawn: &O, cost: f64, energy_structures: &V, scope: &mut RoomScope) -> Result<bool, Crash> {
    if matches!(energy_structures, V::Undef) {
        old_energy_handling(spawn, cost, scope)
    } else {
        new_energy_handling(spawn, cost, energy_structures, scope)
    }
}
