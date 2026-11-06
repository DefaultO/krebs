//! mirror: reference/engine/src/processor/intents/terminal/send.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

/// `/^(W|E)\d+(N|S)\d+$/` (also nukers' `/^(W|E)\d+(S|N)\d+$/`).
pub(crate) fn is_room_name(name: &str) -> bool {
    let b = name.as_bytes();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    if !matches!(b.first(), Some(b'W' | b'E')) {
        return false;
    }
    let dx = digits(1);
    if dx == 0 || !matches!(b.get(1 + dx), Some(b'N' | b'S')) {
        return false;
    }
    let dy = digits(2 + dx);
    dy > 0 && 2 + dx + dy == b.len()
}

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let target_room = to_js_string(&get(intent, atom!("targetRoomName"))?);
    if !is_room_name(&target_room) {
        return Ok(());
    }

    let resource_type = get(intent, atom!("resourceType"))?;
    if resource(&resource_type).is_none() {
        return Ok(());
    }
    let amount = get(intent, atom!("amount"))?;
    let store = object.get(atom!("store"));
    if !truthy(&amount) || !truthy(&store) || !ge(&get(&store, key(&resource_type))?, &amount) {
        return Ok(());
    }

    let range = calc_rooms_distance(scope, &to_js_string(&object.get(atom!("room"))), &target_room, true);
    let mut cost = calc_terminal_energy_cost(to_number(&amount), range);

    if let Some(effect) = find_power_effect(object, C::PWR_OPERATE_TERMINAL) {
        if ge(&get(&effect, atom!("endTime"))?, &n(scope.game_time as f64)) {
            cost = (cost * to_number(&power_effect(C::PWR_OPERATE_TERMINAL, &get(&effect, atom!("level"))?))).ceil();
        }
    }

    let energy = get(&store, atom!("energy"))?;
    let is_energy = loose_eq(&resource_type, &s(C::RESOURCE_ENERGY));
    if !is_energy && lt(&energy, &n(cost)) || is_energy && lt(&energy, &add(&amount, &n(cost))) {
        return Ok(());
    }

    update(&mut scope.bulk, object, vec![(atom!("send"), intent.clone())])
}
