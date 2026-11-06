//! mirror: reference/engine/src/processor/intents/spawns/set-spawn-directions.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(spawn: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !spawn.eq_str(atom!("type"), "spawn") || !truthy(&spawn.get(atom!("spawning"))) {
        return Ok(());
    }
    let directions = get(intent, atom!("directions"))?;
    if let V::Arr(list) = &directions {
        if list.is_empty() {
            return Ok(());
        }
        // convert directions to numbers, eliminate duplicates
        let list = uniq(list.to_vec().iter().map(|e| n(to_number(e))).collect());
        // bail if any numbers are out of bounds or non-integers
        if !list.iter().any(|d| {
            let x = to_number(d);
            !(1.0..=8.0).contains(&x) || x != to_int32(d) as f64
        }) {
            // _.clone(spawn.spawning): a shallow copy
            let spawning = O::new();
            if let V::Obj(old) = spawn.get(atom!("spawning")) {
                crate::live::assign(&spawning, &old);
            }
            spawning.set(atom!("directions"), arr(list));
            update(&mut scope.bulk, spawn, vec![(atom!("spawning"), V::Null)])?;
            update(&mut scope.bulk, spawn, vec![(atom!("spawning"), V::Obj(spawning))])?;
        }
    }
    Ok(())
}
