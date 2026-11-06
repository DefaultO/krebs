//! mirror: reference/engine/src/processor/intents/ramparts/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !object.eq_str(atom!("type"), "rampart") {
        return Ok(());
    }

    if let Some(effect) = find_power_effect(object, C::PWR_SHIELD) {
        if le(&get(&effect, atom!("endTime"))?, &n(game_time)) {
            remove_object(scope, object);
        }
        return Ok(());
    }

    if let Some(ctrl) = &scope.room_controller {
        if !object.eq_str(atom!("user"), "2") {
            let hits_max = if loose_eq(&object.get(atom!("user")), &ctrl.get(atom!("user"))) {
                let max = by_index(&C::RAMPART_HITS_MAX, &ctrl.get(atom!("level")));
                if truthy(&max) {
                    max
                } else {
                    n(0.0)
                }
            } else {
                n(0.0)
            };
            if !loose_eq(&hits_max, &object.get(atom!("hitsMax"))) {
                update(&mut scope.bulk, object, vec![(atom!("hitsMax"), hits_max)])?;
            }
        }
    }

    let next = object.get(atom!("nextDecayTime"));
    if !truthy(&next) || game_time >= to_number(&next) - 1.0 {
        let hits = object.get(atom!("hits"));
        object.set(atom!("hits"), if truthy(&hits) { hits } else { n(0.0) });
        object.set(atom!("hits"), n(object.num(atom!("hits")) - C::RAMPART_DECAY_AMOUNT as f64));
        if le(&object.get(atom!("hits")), &n(0.0)) {
            remove_object(scope, object);
        } else {
            object.set(atom!("nextDecayTime"), n(game_time + C::RAMPART_DECAY_TIME as f64));
            update(&mut scope.bulk, object, vec![(atom!("hits"), object.get(atom!("hits"))), (atom!("nextDecayTime"), object.get(atom!("nextDecayTime")))])?;
        }
    }

    Ok(())
}
