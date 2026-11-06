//! mirror: reference/engine/src/processor/intents/roads/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !object.eq_str(atom!("type"), "road") {
        return Ok(());
    }

    let next = object.get(atom!("nextDecayTime"));
    if !truthy(&next) || game_time >= to_number(&next) - 1.0 {
        let (x, y) = (object.get(atom!("x")), object.get(atom!("y")));
        let legacy = |scope: &RoomScope, kind: &str| {
            room_any(scope, |i| loose_eq(&i.get(atom!("x")), &x) && loose_eq(&i.get(atom!("y")), &y) && loose_eq(&i.get(atom!("type")), &s(kind)))
        };
        let mut decay_amount = C::ROAD_DECAY_AMOUNT as f64;
        if legacy(scope, "swamp") || check_terrain(scope.room_terrain, &x, &y, C::TERRAIN_MASK_SWAMP) {
            decay_amount *= C::CONSTRUCTION_COST_ROAD_SWAMP_RATIO as f64;
        }
        if legacy(scope, "wall") || check_terrain(scope.room_terrain, &x, &y, C::TERRAIN_MASK_WALL) {
            decay_amount *= C::CONSTRUCTION_COST_ROAD_WALL_RATIO as f64;
        }
        object.set(atom!("hits"), n(object.num(atom!("hits")) - decay_amount));
        if le(&object.get(atom!("hits")), &n(0.0)) {
            remove_object(scope, object);
        } else {
            object.set(atom!("nextDecayTime"), n(game_time + C::ROAD_DECAY_TIME as f64));
            update(&mut scope.bulk, object, vec![(atom!("hits"), object.get(atom!("hits"))), (atom!("nextDecayTime"), object.get(atom!("nextDecayTime")))])?;
        }
    }
    Ok(())
}
