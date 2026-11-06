//! mirror: reference/engine/src/processor/intents/creeps/build.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).
//! `config.ptr` is false (private server); `createdStructureCounter` is per processing thread, like vanilla's
//! per processor process.

use crate::atom;
use crate::constants::*;
use crate::live::{assign, to_number, truthy, Crash, O, V};
use crate::processor::{
    boost_effect, bulk_remove, by_level, delete_room_object, eq_str, get_or_undef, gt, is_obstacle_type, js_add, js_min, lodash_match, lodash_match_str, loose_eq, lt, obj, out_of_range, prop,
    push_event, put, room_object, top_boosts_sum, type_is, update, xy_of, RoomScope,
};
use crate::utils;
use std::cell::Cell;

thread_local! {
    /// build.js:7 `createdStructureCounter` (module-global, never reset).
    static CREATED_STRUCTURE_COUNTER: Cell<u64> = const { Cell::new(0) };
}

/// CREEP-BUILD and CREEP-BUILD-COMPLETE.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "creep") {
        return Ok(());
    }
    let store = object.get(atom!("store"));
    if truthy(&object.get(atom!("spawning"))) || !truthy(&store) || crate::processor::le(&get_or_undef(&store, atom!("energy")), &V::Num(0.0)) {
        return Ok(());
    }
    let intent_id = prop(intent, atom!("id"))?;
    let Some(target) = room_object(scope, &intent_id) else { return Ok(()) };
    let structure_type = target.get(atom!("structureType"));
    if !type_is(&target, "constructionSite") || utils::construction_cost(&structure_type).is_none() {
        return Ok(());
    }
    if out_of_range(object, &target, 3.0) {
        return Ok(());
    }

    let user = object.get(atom!("user"));
    let (mut objects_in_tile, mut creeps_in_tile, mut my_creeps_in_tile) = (Vec::new(), Vec::new(), Vec::new());
    let mut structure = None;
    for obj in scope.room_objects.near(&target.get(atom!("x")), &target.get(atom!("y"))).cloned().collect::<Vec<O>>() {
        if loose_eq(&obj.get(atom!("x")), &target.get(atom!("x"))) && loose_eq(&obj.get(atom!("y")), &target.get(atom!("y"))) {
            if loose_eq(&obj.get(atom!("type")), &structure_type) {
                structure = Some(obj);
                continue;
            }
            if type_is(&obj, "creep") {
                if loose_eq(&obj.get(atom!("user")), &user) {
                    my_creeps_in_tile.push(obj.clone());
                }
                creeps_in_tile.push(obj);
            } else {
                objects_in_tile.push(obj);
            }
        }
    }
    if structure.is_some() {
        return Ok(());
    }

    let game_time = V::Num(scope.game_time as f64);
    if is_obstacle_type(&structure_type) {
        if objects_in_tile.iter().any(|i| is_obstacle_type(&i.get(atom!("type")))) {
            return Ok(());
        }
        let my_safe_mode = match &scope.room_controller {
            Some(c) => loose_eq(&c.get(atom!("user")), &user) && gt(&c.get(atom!("safeMode")), &game_time),
            None => false,
        };
        // _.any(blockingCreeps): any element truthy (creep objects always are).
        let blocking_creeps = if my_safe_mode { &my_creeps_in_tile } else { &creeps_in_tile };
        if !blocking_creeps.is_empty() {
            return Ok(());
        }
    }

    let (tx, ty) = (to_number(&target.get(atom!("x"))), to_number(&target.get(atom!("y"))));
    if !eq_str(&structure_type, "extractor") && !eq_str(&structure_type, "road") && utils::check_terrain(scope.room_terrain, tx, ty, TERRAIN_MASK_WALL) {
        return Ok(());
    }

    let body = object.get(atom!("body"));
    let mut work = 0.0;
    let mut boosted_parts = Vec::new();
    for i in crate::processor::lodash_values(&body) {
        let alive = gt(&prop(&i, atom!("hits"))?, &V::Num(0.0)) || gt(&get_or_undef(&i, atom!("_oldHits")), &V::Num(0.0));
        if alive && eq_str(&get_or_undef(&i, atom!("type")), WORK) {
            work += 1.0;
        }
    }
    let build_power = {
        let p = work * BUILD_POWER as f64;
        if truthy(&V::Num(p)) {
            p
        } else {
            0.0
        }
    };
    let build_remaining = to_number(&target.get(atom!("progressTotal"))) - to_number(&target.get(atom!("progress")));
    let build_effect = js_min(js_min(build_power, build_remaining), to_number(&get_or_undef(&object.get(atom!("store")), atom!("energy"))));
    for i in crate::processor::lodash_values(&body) {
        let boost = get_or_undef(&i, atom!("boost"));
        let bonus = if eq_str(&prop(&i, atom!("type"))?, WORK) && truthy(&boost) && gt(&boost_effect(WORK, &boost, "build")?, &V::Num(0.0)) {
            (to_number(&boost_effect(WORK, &boost, "build")?) - 1.0) * BUILD_POWER as f64
        } else {
            0.0
        };
        boosted_parts.push(bonus);
    }
    let boosted_effect = js_min((build_effect + top_boosts_sum(boosted_parts, build_effect)).floor(), build_remaining);

    target.set(atom!("progress"), js_add(&target.get(atom!("progress")), &V::Num(boosted_effect)));
    let store = object.get(atom!("store"));
    put(&store, atom!("energy"), V::Num(to_number(&get_or_undef(&store, atom!("energy"))) - build_effect))?;
    scope.stats_inc(atom!("energyConstruction"), &user, build_effect);
    put(&object.get(atom!("actionLog")), atom!("build"), xy_of(&target))?;
    update(scope, object, &obj! { "store" => obj! { "energy" => get_or_undef(&object.get(atom!("store")), atom!("energy")) } })?;

    let incomplete = lt(&target.get(atom!("progress")), &target.get(atom!("progressTotal")));
    let data = obj! {
        "targetId" => target.get(atom!("_id")),
        "amount" => V::Num(boosted_effect),
        "structureType" => structure_type.clone(),
        "x" => target.get(atom!("x")),
        "y" => target.get(atom!("y")),
        "incomplete" => V::Bool(incomplete),
    };
    push_event(scope, EVENT_BUILD, object.get(atom!("_id")), data);

    if incomplete {
        return update(scope, &target, &obj! { "progress" => target.get(atom!("progress")) });
    }

    bulk_remove(&mut scope.bulk, &target.get(atom!("_id")));
    let new_object = O::new();
    new_object.set(atom!("type"), structure_type.clone());
    new_object.set(atom!("x"), target.get(atom!("x")));
    new_object.set(atom!("y"), target.get(atom!("y")));
    new_object.set(atom!("room"), target.get(atom!("room")));
    new_object.set(atom!("notifyWhenAttacked"), V::Bool(true));
    let extend = |data: V| {
        if let V::Obj(d) = data {
            assign(&new_object, &d);
        }
    };
    let n = |x: u32| V::Num(x as f64);
    let t_user = || target.get(atom!("user"));
    let energy0 = || obj! { "energy" => V::Num(0.0) };
    let st = |s: &str| eq_str(&structure_type, s);

    if st("spawn") {
        extend(obj! {
            "name" => target.get(atom!("name")),
            "user" => t_user(),
            "store" => energy0(),
            "storeCapacityResource" => obj! { "energy" => n(SPAWN_ENERGY_CAPACITY) },
            "hits" => n(SPAWN_HITS),
            "hitsMax" => n(SPAWN_HITS),
        });
    }
    if st("extension") {
        extend(obj! {
            "user" => t_user(),
            "store" => energy0(),
            "storeCapacityResource" => obj! { "energy" => V::Num(0.0) },
            "hits" => n(EXTENSION_HITS),
            "hitsMax" => n(EXTENSION_HITS),
        });
    }
    if st("link") {
        extend(obj! {
            "user" => t_user(),
            "store" => energy0(),
            "storeCapacityResource" => obj! { "energy" => n(LINK_CAPACITY) },
            "cooldown" => V::Num(0.0),
            "hits" => n(LINK_HITS),
            "hitsMax" => n(LINK_HITS_MAX),
        });
    }
    if st("storage") {
        extend(obj! {
            "user" => t_user(),
            "store" => energy0(),
            "storeCapacity" => n(STORAGE_CAPACITY),
            "hits" => n(STORAGE_HITS),
            "hitsMax" => n(STORAGE_HITS),
        });
    }
    let hits_max = match &scope.room_controller {
        Some(c) if loose_eq(&c.get(atom!("user")), &user) => {
            let h = by_level(&RAMPART_HITS_MAX, &c.get(atom!("level")));
            if truthy(&V::Num(h)) {
                h
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
    if st("rampart") {
        extend(obj! {
            "user" => t_user(),
            "hits" => n(RAMPART_HITS),
            "hitsMax" => V::Num(hits_max),
            "nextDecayTime" => V::Num(scope.game_time as f64 + RAMPART_DECAY_TIME as f64),
        });
    }
    if st("road") {
        let mut hits = ROAD_HITS as f64;
        let (vx, vy) = (target.get(atom!("x")), target.get(atom!("y")));
        let legacy = |t: &str| {
            scope.room_objects.near(&vx, &vy).any(|o| {
                let o = V::Obj(o.clone());
                lodash_match(&o, atom!("x"), &vx) && lodash_match(&o, atom!("y"), &vy) && lodash_match_str(&o, atom!("type"), t)
            })
        };
        if legacy("swamp") || utils::check_terrain(scope.room_terrain, tx, ty, TERRAIN_MASK_SWAMP) {
            hits *= CONSTRUCTION_COST_ROAD_SWAMP_RATIO as f64;
        }
        if legacy("wall") || utils::check_terrain(scope.room_terrain, tx, ty, TERRAIN_MASK_WALL) {
            hits *= CONSTRUCTION_COST_ROAD_WALL_RATIO as f64;
        }
        extend(obj! {
            "hits" => V::Num(hits),
            "hitsMax" => V::Num(hits),
            "nextDecayTime" => V::Num(scope.game_time as f64 + ROAD_DECAY_TIME as f64),
        });
    }
    if st("constructedWall") {
        extend(obj! { "hits" => n(WALL_HITS), "hitsMax" => n(WALL_HITS_MAX) });
    }
    if st("tower") {
        extend(obj! {
            "user" => t_user(),
            "store" => energy0(),
            "storeCapacityResource" => obj! { "energy" => n(TOWER_CAPACITY) },
            "hits" => n(TOWER_HITS),
            "hitsMax" => n(TOWER_HITS),
        });
    }
    if st("observer") {
        extend(obj! { "user" => t_user(), "hits" => n(OBSERVER_HITS), "hitsMax" => n(OBSERVER_HITS) });
    }
    if st("extractor") {
        extend(obj! { "user" => t_user(), "hits" => n(EXTRACTOR_HITS), "hitsMax" => n(EXTRACTOR_HITS) });
    }
    if st("lab") {
        extend(obj! {
            "user" => t_user(),
            "hits" => n(LAB_HITS),
            "hitsMax" => n(LAB_HITS),
            "mineralAmount" => V::Num(0.0),
            "cooldown" => V::Num(0.0),
            "store" => energy0(),
            "storeCapacity" => n(LAB_ENERGY_CAPACITY + LAB_MINERAL_CAPACITY),
            "storeCapacityResource" => obj! { "energy" => n(LAB_ENERGY_CAPACITY) },
        });
    }
    if st("powerSpawn") {
        extend(obj! {
            "user" => t_user(),
            "store" => energy0(),
            "storeCapacityResource" => obj! { "energy" => n(POWER_SPAWN_ENERGY_CAPACITY), "power" => n(POWER_SPAWN_POWER_CAPACITY) },
            "hits" => n(POWER_SPAWN_HITS),
            "hitsMax" => n(POWER_SPAWN_HITS),
        });
    }
    if st("terminal") {
        extend(obj! {
            "user" => t_user(),
            "store" => energy0(),
            "storeCapacity" => n(TERMINAL_CAPACITY),
            "hits" => n(TERMINAL_HITS),
            "hitsMax" => n(TERMINAL_HITS),
        });
    }
    if st("container") {
        extend(obj! {
            "store" => energy0(),
            "storeCapacity" => n(CONTAINER_CAPACITY),
            "hits" => n(CONTAINER_HITS),
            "hitsMax" => n(CONTAINER_HITS),
            "nextDecayTime" => V::Num(scope.game_time as f64 + CONTAINER_DECAY_TIME as f64),
        });
    }
    if st("nuker") {
        extend(obj! {
            "user" => t_user(),
            "store" => energy0(),
            "storeCapacityResource" => obj! { "energy" => n(NUKER_ENERGY_CAPACITY), "G" => n(NUKER_GHODIUM_CAPACITY) },
            "hits" => n(NUKER_HITS),
            "hitsMax" => n(NUKER_HITS),
            "cooldownTime" => V::Num(scope.game_time as f64 + NUKER_COOLDOWN as f64),
        });
    }
    if st("factory") {
        extend(obj! {
            "user" => t_user(),
            "store" => energy0(),
            "storeCapacity" => n(FACTORY_CAPACITY),
            "hits" => n(FACTORY_HITS),
            "hitsMax" => n(FACTORY_HITS),
            "cooldown" => V::Num(0.0),
        });
    }

    scope.bulk.insert(&V::Obj(new_object.clone()), None);
    let counter = CREATED_STRUCTURE_COUNTER.with(|c| {
        let n = c.get();
        c.set(n + 1);
        n
    });
    scope.room_objects.insert(format!("createdStructure{counter}").into(), new_object);
    delete_room_object(scope, &intent_id);
    Ok(())
}
