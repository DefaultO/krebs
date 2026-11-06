//! mirror: reference/engine/src/processor/intents/nukers/launch-nuke.js
//! parity: FAITHFUL
//!
//! A nuke for another room goes into `RoomResult::foreign_inserts` (BULK-COLLECTION-WIDE) instead of this
//! room's `bulk`; one for this room stays in `bulk` as in vanilla; a name that is no room of the world
//! (vanilla: an orphan document no room ever loads) inserts nothing.

use crate::atom;
use crate::constants as C;
use crate::doc::Val;
use crate::live::{to_val, Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::terminal::send::is_room_name;
use crate::processor::RoomScope;

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !check_structure_against_controller(object, scope)? {
        return Ok(());
    }
    let (store, capacity) = (object.get(atom!("store")), object.get(atom!("storeCapacityResource")));
    if lt(&get(&store, atom!("G"))?, &get(&capacity, atom!("G"))?) || lt(&get(&store, atom!("energy"))?, &get(&capacity, atom!("energy"))?) {
        return Ok(());
    }
    if gt(&object.get(atom!("cooldownTime")), &n(game_time)) {
        return Ok(());
    }
    let (x, y) = (get(intent, atom!("x"))?, get(intent, atom!("y"))?);
    if lt(&x, &n(0.0)) || lt(&y, &n(0.0)) || gt(&x, &n(49.0)) || gt(&y, &n(49.0)) {
        return Ok(());
    }
    if novice_or_respawn(scope) {
        return Ok(());
    }

    let room_name = get(intent, atom!("roomName"))?;
    let V::Str(target) = &room_name else { return Ok(()) };
    if !is_room_name(target) {
        return Ok(());
    }

    let (tx, ty) = room_name_to_xy(target);
    let (ox, oy) = room_name_to_xy(&to_js_string(&object.get(atom!("room"))));

    if (tx - ox).abs() > C::NUKE_RANGE as f64 || (ty - oy).abs() > C::NUKE_RANGE as f64 {
        return Ok(());
    }

    update(
        &mut scope.bulk,
        object,
        vec![
            (atom!("store"), obj(vec![(atom!("energy"), n(0.0)), (atom!("G"), n(0.0))])),
            (atom!("cooldownTime"), n(game_time + C::NUKER_COOLDOWN as f64)),
        ],
    )?;

    let nuke = obj(vec![
        (atom!("type"), s("nuke")),
        (atom!("room"), room_name.clone()),
        (atom!("x"), x),
        (atom!("y"), y),
        (atom!("landTime"), n(game_time + C::NUKE_LAND_TIME as f64)),
        (atom!("launchRoomName"), object.get(atom!("room"))),
    ]);
    let meta = scope.ctx.meta;
    match meta.by_name(target) {
        Some(room) if *meta.rooms[room as usize].name != **target => {} // 'W01N5' is no room name
        // this room: vanilla's own `bulk.insert`, in counter order with the room's other inserts
        Some(room) if room == scope.room => {
            scope.bulk.insert(&nuke, None);
        }
        Some(room) => {
            if let Some(Val::Obj(doc)) = to_val(&nuke) {
                scope.out.foreign_inserts.push((room, doc));
            }
        }
        None => {}
    }
    Ok(())
}
