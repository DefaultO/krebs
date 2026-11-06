//! mirror: reference/engine/src/processor/intents/nukes/pretick.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

/// `module.exports = function(object, intents, scope)`; `intents` is processor.js's room intents object
/// `{users: {<userId>: {objects: {<objectId>: {<intent>: …}}, objectsManual}}}`.
pub fn pretick(object: &O, intents: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if loose_eq(&object.get(atom!("landTime")), &n(1.0 + scope.game_time as f64)) {
        for user_intents in values(&get(intents, atom!("users"))?) {
            for i in values(&get(&user_intents, atom!("objects"))?) {
                if truthy(&get(&i, atom!("createCreep"))?) {
                    put(&i, atom!("createCreep"), V::Null)?;
                }
            }
        }
    }
    Ok(())
}
