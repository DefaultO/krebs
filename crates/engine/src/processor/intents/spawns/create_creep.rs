//! mirror: reference/engine/src/processor/intents/spawns/create-creep.js
//! parity: FAITHFUL

use super::_charge_energy::charge_energy;
use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(spawn: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if truthy(&spawn.get(atom!("spawning"))) {
        return Ok(());
    }
    if !spawn.eq_str(atom!("type"), "spawn") {
        return Ok(());
    }

    if !check_structure_against_controller(spawn, scope)? {
        return Ok(());
    }

    let mut directions = get(intent, atom!("directions"))?;
    if !matches!(directions, V::Undef) {
        let V::Arr(list) = &directions else { return Ok(()) };
        // convert directions to numbers, eliminate duplicates
        let list = uniq(list.to_vec().iter().map(|e| n(to_number(e))).collect());
        if !list.is_empty() {
            // bail if any numbers are out of bounds or non-integers
            if !list.iter().all(|d| {
                let x = to_number(d);
                (1.0..=8.0).contains(&x) && x == to_int32(d) as f64
            }) {
                return Ok(());
            }
        }
        directions = arr(list);
    }

    // intent.body = intent.body.slice(0, C.MAX_CREEP_SIZE); (an array intent from add-object-intent has
    // no body; a string body slices and then fails in calcCreepCost)
    let body = match get(intent, atom!("body"))? {
        V::Arr(a) => arr(a.to_vec().into_iter().take(C::MAX_CREEP_SIZE as usize).collect()),
        v @ (V::Undef | V::Null) => return Err(crash_read(&v, "slice")),
        V::Str(_) => return Err(Crash("body.forEach is not a function".into())),
        _ => return Err(Crash("intent.body.slice is not a function".into())),
    };
    put(intent, atom!("body"), body.clone())?;

    let cost = calc_creep_cost(&body)?;
    let result = charge_energy(spawn, cost, &get(intent, atom!("energyStructures"))?, scope)?;

    if !result {
        return Ok(());
    }

    let user = spawn.get(atom!("user"));
    stats_inc(scope, "energyCreeps", &user, cost);

    let parts = values(&body);
    stats_inc(scope, "creepsProduced", &user, parts.len() as f64);

    let mut need_time = (C::CREEP_SPAWN_TIME as usize * parts.len()) as f64;

    if let Some(effect) = find_power_effect(spawn, C::PWR_OPERATE_SPAWN) {
        if gt(&get(&effect, atom!("endTime"))?, &n(game_time)) {
            need_time = (need_time * to_number(&power_effect(C::PWR_OPERATE_SPAWN, &get(&effect, atom!("level"))?))).ceil();
        }
    }

    update(
        &mut scope.bulk,
        spawn,
        vec![(
            atom!("spawning"),
            obj(vec![
                (atom!("name"), get(intent, atom!("name"))?),
                (atom!("needTime"), n(need_time)),
                (atom!("spawnTime"), n(game_time + need_time)),
                (atom!("directions"), directions),
            ]),
        )],
    )?;

    let all_parts: Vec<V> = C::BODYPARTS_ALL.iter().map(|p| s(p.as_str())).collect();
    let mut creep_body = Vec::new();
    let mut store_capacity = 0.0;

    for i in &parts {
        if contains(&all_parts, i) {
            creep_body.push(obj(vec![(atom!("type"), i.clone()), (atom!("hits"), n(100.0))]));
        }

        if loose_eq(i, &s(C::CARRY)) {
            store_capacity += C::CARRY_CAPACITY as f64;
        }
    }

    let hits = n((creep_body.len() * 100) as f64);
    let creep = obj(vec![
        (atom!("name"), get(intent, atom!("name"))?),
        (atom!("x"), spawn.get(atom!("x"))),
        (atom!("y"), spawn.get(atom!("y"))),
        (atom!("body"), arr(creep_body)),
        (atom!("store"), obj(vec![(atom!("energy"), n(0.0))])),
        (atom!("storeCapacity"), n(store_capacity)),
        (atom!("type"), s("creep")),
        (atom!("room"), spawn.get(atom!("room"))),
        (atom!("user"), spawn.get(atom!("user"))),
        (atom!("hits"), hits.clone()),
        (atom!("hitsMax"), hits),
        (atom!("spawning"), V::Bool(true)),
        (atom!("fatigue"), n(0.0)),
        (atom!("notifyWhenAttacked"), V::Bool(true)),
    ]);

    if truthy(&spawn.get(atom!("tutorial"))) {
        put(&creep, atom!("tutorial"), V::Bool(true))?;
    }

    scope.bulk.insert(&creep, None);
    Ok(())
}
