//! mirror: reference/engine/src/processor/intents/spawns/_born-creep.js
//! parity: FAITHFUL
//!
//! `movement.isTileBusy` is `movement::is_tile_busy` over the room's movement matrix (WP09).

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::creeps::_die::die;
use crate::processor::intents::movement::is_tile_busy;
use crate::processor::RoomScope;

/// `module.exports = function(spawn, creep, scope)` with a creep (the skeleton's signature).
pub fn born_creep(spawn: &O, creep: &O, scope: &mut RoomScope) -> Result<bool, Crash> {
    born_creep_maybe(spawn, Some(creep), scope)
}

/// The same; `creep` is undefined when the spawning creep is gone (the update is then a no-op, the
/// stomp still kills). ponytail: becomes `born_creep` itself once the interface request lands.
pub fn born_creep_maybe(spawn: &O, creep: Option<&O>, scope: &mut RoomScope) -> Result<bool, Crash> {
    let user = spawn.get(atom!("user"));
    let (sx, sy) = (spawn.get(atom!("x")), spawn.get(atom!("y")));

    // `i.x == newX && i.y == newY` and an unwalkable object or an unwalkable site.
    let check_obstacle = |i: &O, x: &V, y: &V| {
        loose_eq(&i.get(atom!("x")), x)
            && loose_eq(&i.get(atom!("y")), y)
            && (is_obstacle_type(&i.get(atom!("type")))
                || i.eq_str(atom!("type"), "constructionSite") && is_obstacle_type(&i.get(atom!("structureType"))))
    };

    let mut directions: Vec<V> = (1..=8).map(|d| n(d as f64)).collect();
    let spawning = spawn.get(atom!("spawning"));
    if truthy(&spawning) && truthy(&peek(&spawning, atom!("directions"))) {
        // ponytail: `for…of` over a string direction list is not modelled; directions are arrays.
        directions = match peek(&spawning, atom!("directions")) {
            V::Arr(a) => a.to_vec(),
            _ => return Err(Crash("directions is not iterable".into())),
        };
    }
    let other_directions: Vec<V> = (1..=8).map(|d| n(d as f64)).filter(|d| !contains(&directions, d)).collect();

    let (mut new_x, mut new_y) = (V::Undef, V::Undef);
    let mut is_occupied = false;
    let mut hostile_occupied: Option<O> = None;
    // find the first direction where the creep can spawn
    for direction in &directions {
        let (dx, dy) = offsets_by_direction(direction).ok_or_else(|| Crash("undefined is not iterable (cannot read property Symbol(Symbol.iterator))".into()))?;

        new_x = add(&sx, &n(dx));
        new_y = add(&sy, &n(dy));
        is_occupied = room_any(scope, |i| check_obstacle(i, &new_x, &new_y))
            || is_tile_busy(scope, &new_x, &new_y)
            || (check_terrain(scope.room_terrain, &new_x, &new_y, C::TERRAIN_MASK_WALL)
                && !room_any(scope, |i| room_match(i, &[(atom!("type"), s("road")), (atom!("x"), new_x.clone()), (atom!("y"), new_y.clone())])));

        if !is_occupied {
            break;
        }

        // remember the first direction where we found a hostile creep
        if hostile_occupied.is_none() {
            hostile_occupied = room_find(scope, |i| {
                loose_eq(&i.get(atom!("x")), &new_x)
                    && loose_eq(&i.get(atom!("y")), &new_y)
                    && i.eq_str(atom!("type"), "creep")
                    && !loose_eq(&i.get(atom!("user")), &user)
            });
        }
    }

    // if we found a place to spawn, do so
    if !is_occupied {
        if let Some(creep) = creep {
            update(&mut scope.bulk, creep, vec![(atom!("x"), new_x), (atom!("y"), new_y), (atom!("spawning"), V::Bool(false))])?;
        }
        return Ok(true);
    }

    // spawn is surrounded, spawnstomp the first hostile we found above, unless...
    if let Some(hostile) = hostile_occupied {
        // bail if there's an opening we could spawn to but chose not to
        for direction in &other_directions {
            let (dx, dy) = offsets_by_direction(direction).expect("1..8");

            let (x, y) = (add(&sx, &n(dx)), add(&sy, &n(dy)));
            let occupied = room_any(scope, |i| check_obstacle(i, &x, &y))
                || check_terrain(scope.room_terrain, &x, &y, C::TERRAIN_MASK_WALL)
                || is_tile_busy(scope, &x, &y);

            if !occupied {
                return Ok(false);
            }
        }

        die(&hostile, None, true, scope, None)?;
        if let Some(creep) = creep {
            update(
                &mut scope.bulk,
                creep,
                vec![(atom!("x"), hostile.get(atom!("x"))), (atom!("y"), hostile.get(atom!("y"))), (atom!("spawning"), V::Bool(false))],
            )?;
        }
        return Ok(true);
    }

    Ok(false)
}
