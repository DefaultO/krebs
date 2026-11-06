//! mirror: reference/engine/src/processor/intents/links/transfer.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !object.eq_str(atom!("type"), "link") {
        return Ok(());
    }

    let store = object.get(atom!("store"));
    let intent_amount = get(intent, atom!("amount"))?;
    if !truthy(&store) || lt(&get(&store, atom!("energy"))?, &intent_amount) || lt(&intent_amount, &n(0.0)) {
        return Ok(());
    }

    let Some(target) = by_key(scope, &get(intent, atom!("id"))?) else { return Ok(()) };

    if !target.eq_str(atom!("type"), "link") {
        return Ok(());
    }
    let mut amount = intent_amount;

    if gt(&object.get(atom!("cooldown")), &n(0.0)) {
        return Ok(());
    }
    if !check_structure_against_controller(object, scope)? {
        return Ok(());
    }
    let target_store = target.get(atom!("store"));
    let target_total = get(&target_store, atom!("energy"))?;

    let capacity = target.get(atom!("storeCapacityResource"));
    if !truthy(&capacity) || !truthy(&get(&capacity, atom!("energy"))?) || loose_eq(&target_total, &get(&capacity, atom!("energy"))?) {
        return Ok(());
    }
    let capacity = get(&capacity, atom!("energy"))?;

    if gt(&add(&target_total, &amount), &capacity) {
        amount = n(to_number(&capacity) - to_number(&target_total));
    }
    put(&target_store, atom!("energy"), add(&peek(&target_store, atom!("energy")), &amount))?;

    put(&store, atom!("energy"), n(to_number(&peek(&store, atom!("energy"))) - to_number(&amount)))?;

    let loss = (to_number(&amount) * C::LINK_LOSS_RATIO).ceil();
    put(&target_store, atom!("energy"), n(to_number(&peek(&target_store, atom!("energy"))) - loss))?;
    let cooldown = C::LINK_COOLDOWN as f64 * cheb(&target, object);
    object.set(atom!("cooldown"), add(&object.get(atom!("cooldown")), &n(cooldown)));
    let to = obj(vec![(atom!("x"), target.get(atom!("x"))), (atom!("y"), target.get(atom!("y")))]);
    put(&object.get(atom!("actionLog")), atom!("transferEnergy"), to)?;
    update(&mut scope.bulk, &target, vec![(atom!("store"), obj(vec![(atom!("energy"), peek(&target_store, atom!("energy")))]))])?;

    update(
        &mut scope.bulk,
        object,
        vec![
            (atom!("store"), obj(vec![(atom!("energy"), peek(&store, atom!("energy")))])),
            (atom!("cooldown"), object.get(atom!("cooldown"))),
            (atom!("actionLog"), object.get(atom!("actionLog"))),
        ],
    )?;

    let data = obj(vec![(atom!("targetId"), target.get(atom!("_id"))), (atom!("resourceType"), s(C::RESOURCE_ENERGY)), (atom!("amount"), amount)]);
    event(scope, C::EVENT_TRANSFER, object.get(atom!("_id")), Some(data));
    Ok(())
}
