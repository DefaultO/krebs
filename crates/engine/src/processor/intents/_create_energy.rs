//! mirror: reference/engine/src/processor/intents/_create-energy.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::doc::Atom;
use crate::live::{to_number, Crash, O, V};
use crate::processor::{js_add, js_min, js_round, lodash_match, lodash_match_str, obj1, update, RoomScope};
use crate::utils;

/// RES-CREATE-DROP: a container on the tile is filled first; the rest joins a pile or becomes a new one.
pub fn create_energy(x: f64, y: f64, room: &str, amount: f64, resource_type: &str, scope: &mut RoomScope) -> Result<(), Crash> {
    // resourceType = resourceType || 'energy'
    let resource_type = if resource_type.is_empty() { "energy" } else { resource_type };
    let rt = Atom::intern(resource_type);
    let mut amount = js_round(amount);
    if amount <= 0.0 {
        return Ok(());
    }
    let (vx, vy) = (V::Num(x), V::Num(y));
    let at = |o: &O, ty: &str| {
        let v = V::Obj(o.clone());
        lodash_match_str(&v, atom!("type"), ty) && lodash_match(&v, atom!("x"), &vx) && lodash_match(&v, atom!("y"), &vy)
    };

    let objects: Vec<O> = scope.room_objects.near(&vx, &vy).cloned().collect();
    let container = objects.iter().find(|o| at(o, "container")).cloned();
    if let Some(container) = container.filter(|c| to_number(&c.get(atom!("hits"))) > 0.0) {
        if !crate::live::truthy(&container.get(atom!("store"))) {
            container.set(atom!("store"), V::Obj(O::new()));
        }
        let target_total = utils::calc_resources(&V::Obj(container.clone()))?;
        let to_container = js_min(amount, to_number(&container.get(atom!("storeCapacity"))) - target_total);
        if to_container > 0.0 {
            let store = container.get(atom!("store"));
            let next = js_add(&crate::processor::or0(crate::processor::get_or_undef(&store, rt)), &V::Num(to_container));
            crate::processor::put(&store, rt, next.clone())?;
            update(scope, &container, &obj1(atom!("store"), obj1(rt, next)))?;
            amount -= to_container;
        }
    }

    if amount > 0.0 {
        let rt_v = V::str(resource_type);
        let existing = objects.iter().find(|o| at(o, "energy") && lodash_match(&V::Obj((*o).clone()), atom!("resourceType"), &rt_v)).cloned();
        match existing {
            Some(drop) => {
                let sum = js_add(&drop.get(rt), &V::Num(amount));
                update(scope, &drop, &obj1(rt, sum))?;
            }
            None => {
                let o = O::new();
                o.set(atom!("_id"), V::Undef);
                o.set(atom!("type"), V::str("energy"));
                o.set(atom!("x"), vx);
                o.set(atom!("y"), vy);
                o.set(atom!("room"), V::str(room));
                o.set(rt, V::Num(amount));
                o.set(atom!("resourceType"), rt_v);
                let id = scope.bulk.insert(&V::Obj(o.clone()), None).to_string();
                o.set(atom!("_id"), V::str(&id));
                scope.room_objects.insert(id.into(), o);
            }
        }
    }
    Ok(())
}
