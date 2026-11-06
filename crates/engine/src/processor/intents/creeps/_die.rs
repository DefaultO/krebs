//! mirror: reference/engine/src/processor/intents/creeps/_die.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::constants::{
    BodyPart, CLAIM, CREEP_CLAIM_LIFE_TIME, CREEP_CORPSE_RATE, CREEP_LIFE_TIME, CREEP_PART_MAX_ENERGY, EVENT_ATTACK_TYPE_NUKE, EVENT_OBJECT_DESTROYED, LAB_BOOST_ENERGY,
    LAB_BOOST_MINERAL, TOMBSTONE_DECAY_PER_PART,
};
use crate::doc::Atom;
use crate::live::{to_js_string, to_number, truthy, Crash, A, O, V};
use crate::processor::{
    bulk_remove, delete_room_object, get_or_undef, gt, js_add, js_min, lodash_match, lodash_match_str, lodash_values, obj, obj1, or0, prop, push_event, put, update, RoomScope,
};
use crate::utils;
use std::str::FromStr;

/// `amount` into the container on the tile first (`container.store[rt] += …`, bulk.update), the rest into
/// the tombstone (_die.js:240-253 and 258-271).
fn place(container: Option<&O>, tombstone_store: &O, rt: Atom, mut amount: V, scope: &mut RoomScope) -> Result<(), Crash> {
    if let Some(container) = container.filter(|c| gt(&c.get(atom!("hits")), &V::Num(0.0))) {
        if !truthy(&container.get(atom!("store"))) {
            container.set(atom!("store"), V::Obj(O::new()));
        }
        let target_total = utils::calc_resources(&V::Obj(container.clone()))?;
        let to_container = js_min(to_number(&amount), to_number(&container.get(atom!("storeCapacity"))) - target_total);
        if to_container > 0.0 {
            let store = container.get(atom!("store"));
            put(&store, rt, js_add(&or0(get_or_undef(&store, rt)), &V::Num(to_container)))?;
            update(scope, container, &obj1(atom!("store"), obj1(rt, get_or_undef(&container.get(atom!("store")), rt))))?;
            amount = V::Num(to_number(&amount) - to_container);
        }
    }
    if gt(&amount, &V::Num(0.0)) {
        tombstone_store.set(rt, js_add(&or0(tombstone_store.get(rt)), &amount));
    }
    Ok(())
}

/// CREEP-DIE: removal, tombstone (unless nuked), corpse resources, destroy event, `creepsLost`.
pub fn die(object: &O, drop_rate: Option<f64>, violent_death: bool, scope: &mut RoomScope, attack_type: Option<u32>) -> Result<(), Crash> {
    let drop_rate = drop_rate.unwrap_or(CREEP_CORPSE_RATE);
    let game_time = scope.game_time as f64;
    bulk_remove(&mut scope.bulk, &object.get(atom!("_id")));
    delete_room_object(scope, &object.get(atom!("_id")));

    let body = object.get(atom!("body"));
    let mut decay_time = V::Num(to_number(&prop(&body, atom!("length"))?) * TOMBSTONE_DECAY_PER_PART as f64);
    if truthy(&object.get(atom!("tombstoneDecay"))) {
        decay_time = object.get(atom!("tombstoneDecay"));
    }

    if attack_type.is_none_or(|t| t != EVENT_ATTACK_TYPE_NUKE) {
        let mut creep_body = Vec::new();
        for b in lodash_values(&body) {
            creep_body.push(prop(&b, atom!("type"))?);
        }
        let log = object.get(atom!("actionLog"));
        let say = if truthy(&log) { get_or_undef(&log, atom!("say")) } else { V::Undef };
        let creep_saying = if truthy(&log) && truthy(&say) && truthy(&get_or_undef(&say, atom!("isPublic"))) { get_or_undef(&say, atom!("message")) } else { V::Undef };
        let tombstone_store = O::new();
        let tombstone = obj! {
            "type" => V::str("tombstone"),
            "room" => object.get(atom!("room")),
            "x" => object.get(atom!("x")),
            "y" => object.get(atom!("y")),
            "user" => object.get(atom!("user")),
            "deathTime" => V::Num(game_time),
            "decayTime" => js_add(&V::Num(game_time), &decay_time),
            "creepId" => V::Str(to_js_string(&object.get(atom!("_id")))),
            "creepName" => object.get(atom!("name")),
            "creepTicksToLive" => V::Num(to_number(&object.get(atom!("ageTime"))) - game_time),
            "creepBody" => V::Arr(A::from_vec(creep_body)),
            "creepSaying" => creep_saying,
            "store" => V::Obj(tombstone_store.clone()),
        };

        let (vx, vy) = (object.get(atom!("x")), object.get(atom!("y")));
        let container = scope.room_objects.near(&vx, &vy).find(|&o| {
            let v = V::Obj(o.clone());
            lodash_match_str(&v, atom!("type"), "container") && lodash_match(&v, atom!("x"), &vx) && lodash_match(&v, atom!("y"), &vy)
        }).cloned();

        if drop_rate > 0.0 && !truthy(&object.get(atom!("userSummoned"))) && !truthy(&object.get(atom!("strongholdId"))) {
            let claim = lodash_values(&body).iter().any(|b| lodash_match_str(b, atom!("type"), CLAIM));
            let life_time = if claim { CREEP_CLAIM_LIFE_TIME } else { CREEP_LIFE_TIME } as f64;
            let life_rate = drop_rate * to_number(&object.get(atom!("_ticksToLive"))) / life_time;
            let body_resources = O::new();
            body_resources.set(atom!("energy"), V::Num(0.0));
            let add = |k: Atom, n: f64| body_resources.set(k, V::Num(to_number(&body_resources.get(k)) + n));
            for i in utils::for_each_items(&body, "object.body.forEach")? {
                let boost = prop(&i, atom!("boost"))?;
                if truthy(&boost) {
                    let b = Atom::intern(&to_js_string(&boost));
                    body_resources.set(b, or0(body_resources.get(b)));
                    add(b, LAB_BOOST_MINERAL as f64 * life_rate);
                    add(atom!("energy"), LAB_BOOST_ENERGY as f64 * life_rate);
                }
                let cost = BodyPart::from_str(&to_js_string(&get_or_undef(&i, atom!("type")))).map_or(f64::NAN, |p| p.cost() as f64);
                add(atom!("energy"), js_min(CREEP_PART_MAX_ENERGY as f64, cost * life_rate));
            }
            for rt in body_resources.keys() {
                let amount = to_number(&body_resources.get(rt)).floor();
                if amount > 0.0 {
                    place(container.as_ref(), &tombstone_store, rt, V::Num(amount), scope)?;
                }
            }
            // _.forEach(object.store, (amount, resourceType) => …); not floored.
            let store = object.get(atom!("store"));
            let entries: Vec<(Atom, V)> = match &store {
                V::Obj(s) => s.keys().into_iter().map(|k| (k, s.get(k))).collect(),
                // ponytail: array/string stores never occur; lodash would key them by index.
                other => lodash_values(other).into_iter().enumerate().map(|(i, v)| (Atom::intern(&i.to_string()), v)).collect(),
            };
            for (rt, amount) in entries {
                if gt(&amount, &V::Num(0.0)) {
                    place(container.as_ref(), &tombstone_store, rt, amount, scope)?;
                }
            }
        }
        scope.bulk.insert(&tombstone, None);
    }

    push_event(scope, EVENT_OBJECT_DESTROYED, object.get(atom!("_id")), obj! { "type" => V::str("creep") });

    let user = object.get(atom!("user"));
    if violent_death && !crate::processor::eq_str(&user, "3") && !crate::processor::eq_str(&user, "2") {
        let parts = to_number(&prop(&body, atom!("length"))?);
        scope.stats_inc(atom!("creepsLost"), &user, parts);
    }
    Ok(())
}
