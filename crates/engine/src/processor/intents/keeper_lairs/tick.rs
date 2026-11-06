//! mirror: reference/engine/src/processor/intents/keeper-lairs/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !object.eq_str(atom!("type"), "keeperLair") {
        return Ok(());
    }

    let name = add(&s("Keeper"), &object.get(atom!("_id")));
    let keeper_of = |scope: &RoomScope| {
        room_find(scope, |i| i.eq_str(atom!("type"), "creep") && i.eq_str(atom!("user"), "3") && loose_eq(&i.get(atom!("name")), &name))
    };

    if !truthy(&object.get(atom!("nextSpawnTime"))) {
        let keeper = keeper_of(scope);
        if keeper.as_ref().is_none_or(|k| lt(&k.get(atom!("hits")), &n(5000.0))) {
            update(&mut scope.bulk, object, vec![(atom!("nextSpawnTime"), n(game_time + C::ENERGY_REGEN_TIME as f64))])?;
        }
    }

    let next = object.get(atom!("nextSpawnTime"));
    if truthy(&next) && game_time >= to_number(&next) - 1.0 {
        if let Some(keeper) = keeper_of(scope) {
            bulk_remove(&mut scope.bulk, &keeper.get(atom!("_id")));
        }

        let part = |t: &str| obj(vec![(atom!("type"), s(t)), (atom!("hits"), n(100.0))]);
        let mut body = Vec::new();
        for _ in 0..17 {
            body.push(part(C::TOUGH));
        }
        for _ in 0..13 {
            body.push(part(C::MOVE));
        }
        for _ in 0..10 {
            body.push(part(C::ATTACK));
            body.push(part(C::RANGED_ATTACK));
        }

        let keeper = obj(vec![
            (atom!("name"), name.clone()),
            (atom!("x"), object.get(atom!("x"))),
            (atom!("y"), object.get(atom!("y"))),
            (atom!("body"), arr(body)),
            (atom!("store"), obj(vec![(atom!("energy"), n(0.0))])),
            (atom!("storeCapacity"), n(0.0)),
            (atom!("type"), s("creep")),
            (atom!("room"), object.get(atom!("room"))),
            (atom!("user"), s("3")),
            (atom!("hits"), n(5000.0)),
            (atom!("hitsMax"), n(5000.0)),
            (atom!("spawning"), V::Bool(false)),
            (atom!("fatigue"), n(0.0)),
        ]);
        scope.bulk.insert(&keeper, None);

        update(&mut scope.bulk, object, vec![(atom!("nextSpawnTime"), V::Null)])?;
    }

    Ok(())
}
