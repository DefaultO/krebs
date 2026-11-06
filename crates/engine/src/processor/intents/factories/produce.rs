//! mirror: reference/engine/src/processor/intents/factories/produce.js
//! parity: DIVERGENT(PROTO-KEYS)
//!
//! `C.COMMODITIES[intent.resourceType]` with an Object.prototype member name (`constructor`, `toString`,
//! …; reachable through add-object-intent) finds a function in vanilla and writes its source text into
//! the store; here such names are not commodities.

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;
    let store = object.get(atom!("store"));
    let resource_type = get(intent, atom!("resourceType"))?;

    let Some(commodity) = resource(&resource_type).and_then(C::commodity) else { return Ok(()) };
    if !truthy(&store) {
        return Ok(());
    }
    let level = commodity.level.map(|l| n(l as f64));
    if let Some(level) = &level {
        if truthy(level) && !loose_eq(&object.get(atom!("level")), level) {
            return Ok(());
        }
    }

    let cooldown_time = object.get(atom!("cooldownTime"));
    if truthy(&cooldown_time) && gt(&cooldown_time, &n(game_time)) {
        return Ok(());
    }

    if !check_structure_against_controller(object, scope)? {
        return Ok(());
    }

    if let Some(level) = &level {
        if truthy(level) && gt(&object.get(atom!("level")), &n(0.0)) {
            let operated = values(&object.get(atom!("effects"))).iter().any(|e| {
                loose_eq(&peek(e, atom!("power")), &n(C::PWR_OPERATE_FACTORY as f64))
                    && loose_eq(&peek(e, atom!("level")), level)
                    && ge(&peek(e, atom!("endTime")), &n(game_time))
            });
            if !operated {
                return Ok(());
            }
        }
    }

    for (part, need) in commodity.components {
        let have = get(&store, crate::doc::Atom::intern(part.as_str()))?;
        if lt(&if truthy(&have) { have } else { n(0.0) }, &n(*need as f64)) {
            return Ok(());
        }
    }

    let target_total = calc_resources(object)?;
    let components_total: f64 = commodity.components.iter().rev().map(|(_, k)| *k as f64).sum();
    let amount = commodity.amount as f64;
    if gt(&n(target_total - components_total + amount), &object.get(atom!("storeCapacity"))) {
        return Ok(());
    }

    for (part, need) in commodity.components {
        let k = crate::doc::Atom::intern(part.as_str());
        put(&store, k, n(to_number(&peek(&store, k)) - *need as f64))?;
    }
    let k = key(&resource_type);
    let have = peek(&store, k);
    put(&store, k, add(&if truthy(&have) { have } else { n(0.0) }, &n(amount)))?;
    update(&mut scope.bulk, object, vec![(atom!("store"), store.clone())])?;

    let log = obj(vec![(atom!("x"), object.get(atom!("x"))), (atom!("y"), object.get(atom!("y"))), (atom!("resourceType"), resource_type)]);
    put(&object.get(atom!("actionLog")), atom!("produce"), log)?;

    update(&mut scope.bulk, object, vec![(atom!("cooldownTime"), n(commodity.cooldown as f64 + game_time))])
}
