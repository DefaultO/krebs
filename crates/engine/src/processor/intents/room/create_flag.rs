//! mirror: reference/engine/src/processor/intents/room/create-flag.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;
use crate::world::UserIdx;

/// `intent.name.replace(/\|/g,"$VLINE$").replace(/~/g,"$TILDE$")`.
pub(crate) fn escape_name(intent: &V) -> Result<V, Crash> {
    match get(intent, atom!("name"))? {
        V::Str(name) => Ok(s(&name.replace('|', "$VLINE$").replace('~', "$TILDE$"))),
        v @ (V::Undef | V::Null) => Err(crash_read(&v, "replace")),
        _ => Err(Crash("intent.name.replace is not a function".into())),
    }
}

/// `_.any(i._parsed, j => j[0] == name)`.
pub(crate) fn has_name(flag: &O, name: &V) -> bool {
    values(&flag.get(atom!("_parsed"))).iter().any(|j| loose_eq(&peek(j, atom!("0")), name))
}

pub fn run(user: UserIdx, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let user_id = user_id(scope, user);

    let name = escape_name(intent)?;

    if scope.flags.iter().any(|i| loose_eq(&i.get(atom!("user")), &user_id) && has_name(i, &name)) {
        return Ok(());
    }
    let colors: Vec<V> = C::COLORS_ALL.iter().map(|&c| n(c as f64)).collect();
    let color = get(intent, atom!("color"))?;
    if !truthy(&color) || !contains(&colors, &color) {
        return Ok(());
    }
    let secondary_color = get(intent, atom!("secondaryColor"))?;
    if !truthy(&secondary_color) || !contains(&colors, &secondary_color) {
        return Ok(());
    }

    let (x, y) = (get(intent, atom!("x"))?, get(intent, atom!("y"))?);
    if lt(&x, &n(0.0)) || gt(&x, &n(49.0)) || lt(&y, &n(0.0)) || gt(&y, &n(49.0)) {
        return Ok(());
    }

    let flag_item = match scope.flags.iter().find(|i| room_match(i, &[(atom!("user"), user_id.clone())])) {
        Some(f) => f.clone(),
        None => {
            let f = O::new();
            f.set(atom!("user"), user_id.clone());
            f.set(atom!("room"), get(intent, atom!("roomName"))?);
            f.set(atom!("_parsed"), arr(vec![]));
            scope.flags.push(f.clone());
            f
        }
    };

    flag_item.set(atom!("_modified"), V::Bool(true));
    match flag_item.get(atom!("_parsed")) {
        V::Arr(parsed) => parsed.push(arr(vec![name, color, secondary_color, x, y])),
        v => return Err(crash_read(&v, "push")),
    }
    Ok(())
}
