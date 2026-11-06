//! mirror: reference/engine/src/processor/intents/power-creeps/tick.js
//! parity: FAITHFUL
//! Verified against the official processor by the fuzz fixtures in crates/engine/tests/fixtures/oracle/pc.
//! No death here: a power creep at 0 hits stays (vanilla kills it elsewhere only by age).

use crate::atom;
use crate::constants::EVENT_EXIT;
use crate::live::{is_equal, to_js_string, truthy, Crash, O, V};
use crate::processor::intents::movement;
use crate::processor::{eq_str, gt, js_add, loose_eq, obj, prop, push_event, type_is, update, RoomScope};
use crate::utils;

/// PC-TICK.
pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    if !type_is(object, "powerCreep") {
        return Ok(());
    }

    movement::execute(object, scope)?;

    let user = object.get(atom!("user"));
    if utils::is_at_edge(&V::Obj(object.clone())) && !eq_str(&user, "2") && !eq_str(&user, "3") {
        let (room_x, room_y) = utils::room_name_to_xy(&to_js_string(&object.get(atom!("room"))));
        let (mut x, mut y, mut room) = (object.get(atom!("x")), object.get(atom!("y")), object.get(atom!("room")));
        let at = |k, n: f64| loose_eq(&object.get(k), &V::Num(n));
        if at(atom!("x"), 0.0) {
            x = V::Num(49.0);
            room = V::str(&utils::get_room_name_from_xy(room_x - 1.0, room_y));
        } else if at(atom!("y"), 0.0) {
            y = V::Num(49.0);
            room = V::str(&utils::get_room_name_from_xy(room_x, room_y - 1.0));
        } else if at(atom!("x"), 49.0) {
            x = V::Num(0.0);
            room = V::str(&utils::get_room_name_from_xy(room_x + 1.0, room_y));
        } else if at(atom!("y"), 49.0) {
            y = V::Num(0.0);
            room = V::str(&utils::get_room_name_from_xy(room_x, room_y + 1.0));
        }
        update(scope, object, &obj! { "interRoom" => obj! { "room" => room.clone(), "x" => x.clone(), "y" => y.clone() } })?;
        push_event(scope, EVENT_EXIT, object.get(atom!("_id")), obj! { "room" => room, "x" => x, "y" => y });
    }

    let portal = scope
        .room_objects
        .iter()
        .find(|&i| type_is(i, "portal") && loose_eq(&i.get(atom!("x")), &object.get(atom!("x"))) && loose_eq(&i.get(atom!("y")), &object.get(atom!("y"))))
        .cloned();
    if let Some(portal) = portal {
        let destination = portal.get(atom!("destination"));
        if !truthy(&prop(&destination, atom!("shard"))?) {
            update(scope, object, &obj! { "interRoom" => destination })?;
        }
    }

    let mut hits = object.get(atom!("hits"));
    let damage = object.get(atom!("_damageToApply"));
    if truthy(&damage) {
        hits = V::Num(crate::live::to_number(&hits) - crate::live::to_number(&damage));
        object.delete(atom!("_damageToApply"));
    }
    let heal = object.get(atom!("_healToApply"));
    if truthy(&heal) {
        hits = js_add(&hits, &heal);
        object.delete(atom!("_healToApply"));
    }
    if gt(&hits, &object.get(atom!("hitsMax"))) {
        hits = object.get(atom!("hitsMax"));
    }
    if !loose_eq(&hits, &object.get(atom!("hits"))) {
        update(scope, object, &obj! { "hits" => hits })?;
    }

    if !is_equal(&object.get(atom!("actionLog")), &object.get(atom!("_actionLog"))) {
        update(scope, object, &obj! { "actionLog" => object.get(atom!("actionLog")) })?;
    }
    Ok(())
}
