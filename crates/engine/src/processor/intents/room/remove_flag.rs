//! mirror: reference/engine/src/processor/intents/room/remove-flag.js
//! parity: FAITHFUL

use super::create_flag::{escape_name, has_name};
use crate::atom;
use crate::live::{Crash, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;
use crate::world::UserIdx;

pub fn run(user: UserIdx, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let user_id = user_id(scope, user);

    let Some(flag_item) = scope.flags.iter().find(|i| room_match(i, &[(atom!("user"), user_id.clone())])).cloned() else {
        return Ok(());
    };

    let name = escape_name(intent)?;

    if !has_name(&flag_item, &name) {
        return Ok(());
    }
    flag_item.set(atom!("_modified"), V::Bool(true));
    // `_.remove(flagItem._parsed, i => i[0] == name)`; the array's identity is never observed, so a
    // filtered copy replaces it.
    let kept: Vec<V> = values(&flag_item.get(atom!("_parsed"))).into_iter().filter(|j| !loose_eq(&peek(j, atom!("0")), &name)).collect();
    flag_item.set(atom!("_parsed"), arr(kept));
    Ok(())
}
