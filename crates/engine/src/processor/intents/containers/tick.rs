//! mirror: reference/engine/src/processor/intents/containers/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::_create_energy::create_energy;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    let next = object.get(atom!("nextDecayTime"));
    if !truthy(&next) || game_time >= to_number(&next) - 1.0 {
        let hits = object.get(atom!("hits"));
        object.set(atom!("hits"), if truthy(&hits) { hits } else { n(0.0) });
        object.set(atom!("hits"), n(object.num(atom!("hits")) - C::CONTAINER_DECAY as f64));
        if le(&object.get(atom!("hits")), &n(0.0)) {
            drop_store(object, |amount| gt(amount, &n(0.0)), scope)?;

            remove_object(scope, object);
        } else {
            let owned = matches!(&scope.room_controller, Some(c) if gt(&c.get(atom!("level")), &n(0.0)));
            let time = if owned { C::CONTAINER_DECAY_TIME_OWNED } else { C::CONTAINER_DECAY_TIME };
            object.set(atom!("nextDecayTime"), n(game_time + time as f64));
            update(&mut scope.bulk, object, vec![(atom!("hits"), object.get(atom!("hits"))), (atom!("nextDecayTime"), object.get(atom!("nextDecayTime")))])?;
        }
    }
    Ok(())
}

/// `if(object.store) _.forEach(object.store, (amount, resourceType) => { if(<filter>) createEnergy(object.x,
/// object.y, object.room, amount, resourceType, scope); })` (containers, tombstones, ruins).
pub(crate) fn drop_store(object: &O, filter: impl Fn(&crate::live::V) -> bool, scope: &mut RoomScope) -> Result<(), Crash> {
    let crate::live::V::Obj(store) = object.get(atom!("store")) else {
        // ponytail: lodash would walk an array or string store too; stores are objects.
        return Ok(());
    };
    for resource_type in store.keys() {
        let amount = store.get(resource_type);
        if filter(&amount) {
            let room = to_js_string(&object.get(atom!("room")));
            create_energy(object.num(atom!("x")), object.num(atom!("y")), &room, to_number(&amount), resource_type.as_str(), scope)?;
        }
    }
    Ok(())
}
