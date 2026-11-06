//! mirror: reference/engine/src/processor/intents/ramparts/set-public.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let is_public = truthy(&get(intent, atom!("isPublic"))?);
    update(&mut scope.bulk, object, vec![(atom!("isPublic"), V::Bool(is_public))])
}
