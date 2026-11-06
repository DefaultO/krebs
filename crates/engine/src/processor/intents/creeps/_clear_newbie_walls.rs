//! mirror: reference/engine/src/processor/intents/creeps/_clear-newbie-walls.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::live::{truthy, Crash, V};
use crate::processor::{bulk_remove, delete_room_object, read_crash, type_is, RoomScope};

/// Removes every newbie wall (an owned `constructedWall` with `decayTime`).
pub fn clear_newbie_walls(scope: &mut RoomScope) -> Result<(), Crash> {
    for key in scope.room_objects.keys() {
        // _.forEach reads each key live: an entry deleted meanwhile is `undefined` and `i.type` throws.
        let Some(i) = scope.room_objects.get(&key) else { return Err(read_crash(&V::Undef, "type")) };
        if type_is(&i, "constructedWall") && truthy(&i.get(atom!("decayTime"))) && truthy(&i.get(atom!("user"))) {
            bulk_remove(&mut scope.bulk, &i.get(atom!("_id")));
            delete_room_object(scope, &i.get(atom!("_id")));
        }
    }
    Ok(())
}
