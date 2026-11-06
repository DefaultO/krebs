//! mirror: reference/engine/src/processor/intents/room/create-construction-site.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;
use crate::world::UserIdx;
use std::sync::atomic::{AtomicU64, Ordering};

/// `createdConstructionSiteCounter`: module-global, never reset. Only the uniqueness of the synthetic
/// `roomObjects` keys depends on it (they are never integer-like, so their position is insertion order).
static CREATED_CONSTRUCTION_SITE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn run(user: UserIdx, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let user_id = user_id(scope, user);
    let (x, y) = (get(intent, atom!("x"))?, get(intent, atom!("y"))?);
    let structure_type = get(intent, atom!("structureType"))?;
    let room_name = get(intent, atom!("roomName"))?;

    if le(&x, &n(0.0)) || ge(&x, &n(49.0)) || le(&y, &n(0.0)) || ge(&y, &n(49.0)) {
        return Ok(());
    }

    if !truthy(&construction_cost(&structure_type)) {
        return Ok(());
    }

    // `/^(W|E)/.test(intent.roomName)`
    if to_js_string(&room_name).starts_with(['W', 'E']) {
        if let Some(ctrl) = &scope.room_controller {
            let owner = ctrl.get(atom!("user"));
            let reservation = ctrl.get(atom!("reservation"));
            if truthy(&owner) && !loose_eq(&owner, &user_id) || truthy(&reservation) && !loose_eq(&get(&reservation, atom!("user"))?, &user_id) {
                return Ok(());
            }
        }

        if !check_controller_availability(&structure_type, scope)? {
            return Ok(());
        }
    }

    if !check_construction_site_objects(scope, &structure_type, &x, &y) || !check_construction_site_terrain(scope.room_terrain, &structure_type, &x, &y) {
        return Ok(());
    }

    let mut progress_total = to_number(&construction_cost(&structure_type));

    if loose_eq(&structure_type, &s("road")) {
        let legacy = |kind: &str| room_any(scope, |i| room_match(i, &[(atom!("x"), x.clone()), (atom!("y"), y.clone()), (atom!("type"), s(kind))]));
        if legacy("swamp") || check_terrain(scope.room_terrain, &x, &y, C::TERRAIN_MASK_SWAMP) {
            progress_total *= C::CONSTRUCTION_COST_ROAD_SWAMP_RATIO as f64;
        }
        if legacy("wall") || check_terrain(scope.room_terrain, &x, &y, C::TERRAIN_MASK_WALL) {
            progress_total *= C::CONSTRUCTION_COST_ROAD_WALL_RATIO as f64;
        }
    }

    // `config.ptr` (PTR env) is never set in chitin.

    if loose_eq(&room_name, &s("sim")) && loose_eq(&structure_type, &s("tower")) {
        progress_total = 100.0;
    }

    let site = O::new();
    site.set(atom!("structureType"), structure_type.clone());
    site.set(atom!("x"), x);
    site.set(atom!("y"), y);
    site.set(atom!("type"), s("constructionSite"));
    site.set(atom!("room"), room_name);
    site.set(atom!("user"), user_id);
    site.set(atom!("progress"), n(0.0));
    site.set(atom!("progressTotal"), n(progress_total));

    if loose_eq(&structure_type, &s("spawn")) {
        site.set(atom!("name"), get(intent, atom!("name"))?);
    }

    scope.bulk.insert(&V::Obj(site.clone()), None);

    let counter = CREATED_CONSTRUCTION_SITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    scope.room_objects.insert(format!("_createdConstructionSite{counter}").into(), site);
    Ok(())
}
