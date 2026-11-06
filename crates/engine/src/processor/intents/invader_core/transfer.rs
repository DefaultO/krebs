//! mirror: reference/engine/src/processor/intents/invader-core/transfer.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{to_js_string, truthy, Crash, O, V};
use crate::processor::global::obj;
use crate::processor::{eq_str, gt, js_add, loose_eq, prop, update, RoomScope};
use crate::utils::calc_resources;

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !eq_str(&object.get(atom!("type")), "invaderCore") {
        return Ok(());
    }
    let Some(target) = scope.room_objects.get(&to_js_string(&prop(intent, atom!("id"))?)) else { return Ok(()) };
    let ty = target.get(atom!("type"));
    if !(eq_str(&ty, "tower") || eq_str(&ty, "creep")) {
        return Ok(());
    }
    let tv = V::Obj(target.clone());
    let total = if eq_str(&ty, "creep") { V::Num(calc_resources(&tv)?) } else { prop(&prop(&tv, atom!("store"))?, atom!("energy"))? };
    let capacity = {
        let c = target.get(atom!("storeCapacity"));
        if truthy(&c) {
            c
        } else {
            prop(&prop(&tv, atom!("storeCapacityResource"))?, atom!("energy"))?
        }
    };
    if loose_eq(&total, &capacity) {
        return Ok(());
    }
    let mut amount = prop(intent, atom!("amount"))?;
    if gt(&js_add(&total, &amount), &capacity) {
        amount = V::Num(crate::live::to_number(&capacity) - crate::live::to_number(&total));
    }
    let store = prop(&tv, atom!("store"))?;
    let energy = js_add(&prop(&store, atom!("energy"))?, &amount);
    crate::processor::put(&store, atom!("energy"), energy.clone())?;

    let action_log = prop(&V::Obj(object.clone()), atom!("actionLog"))?;
    crate::processor::put(&action_log, atom!("transferEnergy"), obj(vec![("x", target.get(atom!("x"))), ("y", target.get(atom!("y")))]))?;
    update(scope, object, &obj(vec![("actionLog", action_log)]))?;
    update(scope, &target, &obj(vec![("store", obj(vec![("energy", energy)]))]))?;
    let data = obj(vec![("targetId", target.get(atom!("_id"))), ("resourceType", V::str(C::RESOURCE_ENERGY)), ("amount", amount)]);
    scope.event_log.push(obj(vec![("event", V::Num(C::EVENT_TRANSFER as f64)), ("objectId", object.get(atom!("_id"))), ("data", data)]));
    Ok(())
}
