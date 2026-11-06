//! mirror: reference/engine/src/processor/intents/spawns/tick.js
//! parity: FAITHFUL

use super::_born_creep::born_creep_maybe;
use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !object.eq_str(atom!("type"), "spawn") {
        return Ok(());
    }

    if truthy(&object.get(atom!("spawning"))) {
        let effect = find_power_effect(object, C::PWR_DISRUPT_SPAWN);
        let disrupted = match &effect {
            Some(e) => gt(&get(e, atom!("endTime"))?, &n(game_time)),
            None => false,
        };
        if disrupted {
            let spawn_time = add(&n(1.0), &get(&object.get(atom!("spawning")), atom!("spawnTime"))?);
            update(&mut scope.bulk, object, vec![(atom!("spawning"), obj(vec![(atom!("spawnTime"), spawn_time)]))])?;
        } else if ge(&n(game_time), &n(to_number(&get(&object.get(atom!("spawning")), atom!("spawnTime"))?) - 1.0)) {
            let name = get(&object.get(atom!("spawning")), atom!("name"))?;
            let spawning_creep = room_find(scope, |i| {
                room_match(i, &[(atom!("type"), s("creep")), (atom!("name"), name.clone()), (atom!("x"), object.get(atom!("x"))), (atom!("y"), object.get(atom!("y")))])
            });

            let born_ok = born_creep_maybe(object, spawning_creep.as_ref(), scope)?;

            if born_ok {
                update(&mut scope.bulk, object, vec![(atom!("spawning"), V::Null)])?;
            } else {
                update(&mut scope.bulk, object, vec![(atom!("spawning"), obj(vec![(atom!("spawnTime"), n(1.0 + game_time))]))])?;
            }
        }
    }

    let Some(ctrl) = scope.room_controller.clone() else { return Ok(()) };
    if lt(&ctrl.get(atom!("level")), &n(1.0)) || !loose_eq(&ctrl.get(atom!("user")), &object.get(atom!("user"))) {
        return Ok(());
    }
    let is_spawn = |i: &O| i.is_str(atom!("type"), "spawn");
    let mut spawns = if scope.room_objects.version() == scope.prescan_version {
        scope.room_spawns.iter().filter(|i| is_spawn(i)).cloned().collect()
    } else {
        room_filter(scope, is_spawn)
    };
    let allowed = controller_structures("spawn", &ctrl.get(atom!("level")))?;
    if gt(&n(spawns.len() as f64), &allowed) {
        stable_sort(&mut spawns, |a, b| comparator_distance(&ctrl, a, b));
        let k = to_number(&allowed) as usize; // a number here: `length > undefined` is false
        if !spawns[..k.min(spawns.len())].iter().any(|i| i.same(object)) {
            return Ok(());
        }
    }

    let store = object.get(atom!("store"));
    if !truthy(&object.get(atom!("tutorial")))
        && scope.energy_available < C::SPAWN_ENERGY_CAPACITY as f64
        && lt(&get(&store, atom!("energy"))?, &n(C::SPAWN_ENERGY_CAPACITY as f64))
    {
        put(&store, atom!("energy"), n(to_number(&peek(&store, atom!("energy"))) + 1.0))?;
        update(&mut scope.bulk, object, vec![(atom!("store"), obj(vec![(atom!("energy"), peek(&store, atom!("energy")))]))])?;
    }

    Ok(())
}
