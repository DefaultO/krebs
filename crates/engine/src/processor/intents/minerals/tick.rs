//! mirror: reference/engine/src/processor/intents/minerals/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !truthy(&object.get(atom!("mineralAmount"))) {
        if !truthy(&object.get(atom!("nextRegenerationTime"))) {
            object.set(atom!("nextRegenerationTime"), n(game_time + C::MINERAL_REGEN_TIME as f64));
            update(&mut scope.bulk, object, vec![(atom!("nextRegenerationTime"), object.get(atom!("nextRegenerationTime")))])?;
        }
        if game_time >= object.num(atom!("nextRegenerationTime")) - 1.0 {
            let update_data = O::new();
            update_data.set(atom!("nextRegenerationTime"), V::Null);
            update_data.set(atom!("mineralAmount"), by_index(&C::MINERAL_DENSITY, &object.get(atom!("density"))));
            let density = object.get(atom!("density"));
            if loose_eq(&density, &n(C::DENSITY_LOW as f64))
                || loose_eq(&density, &n(C::DENSITY_ULTRA as f64))
                || scope.rng.random() < C::MINERAL_DENSITY_CHANGE
            {
                let old_density = density;
                let mut new_density = V::Undef;
                loop {
                    let random = scope.rng.random();
                    // `for (var density in C.MINERAL_DENSITY_PROBABILITY)`: keys "1".."4" ascending
                    for (key, p) in C::MINERAL_DENSITY_PROBABILITY.iter().enumerate() {
                        if let Some(p) = p {
                            if random <= *p {
                                new_density = n(key as f64);
                                break;
                            }
                        }
                    }
                    if !loose_eq(&new_density, &old_density) {
                        break;
                    }
                }

                object.set(atom!("density"), new_density.clone());
                update_data.set(atom!("density"), new_density);
            }
            scope.bulk.update(crate::bulk::Target::Obj(object), &V::Obj(update_data))?;
        }
    }

    if let Some(effect) = find_power_effect(object, C::PWR_REGEN_MINERAL) {
        let end_time = get(&effect, atom!("endTime"))?;
        if gt(&end_time, &n(game_time)) && !truthy(&object.get(atom!("nextRegenerationTime"))) && truthy(&object.get(atom!("mineralAmount"))) {
            let period = C::power_info(C::PWR_REGEN_MINERAL).and_then(|p| p.period).unwrap() as f64;
            if (to_number(&end_time) - game_time - 1.0) % period == 0.0 {
                let amount = add(&object.get(atom!("mineralAmount")), &power_effect(C::PWR_REGEN_MINERAL, &get(&effect, atom!("level"))?));
                update(&mut scope.bulk, object, vec![(atom!("mineralAmount"), amount)])?;
            }
        }
    }

    Ok(())
}
