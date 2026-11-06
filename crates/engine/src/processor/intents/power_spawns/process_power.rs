//! mirror: reference/engine/src/processor/intents/power-spawns/process-power.js
//! parity: FAITHFUL

use crate::atom;
use crate::bulk::Target;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

pub fn run(object: &O, _intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let store = object.get(atom!("store"));
    if !object.eq_str(atom!("type"), "powerSpawn") || !truthy(&store) {
        return Ok(());
    }

    if !check_structure_against_controller(object, scope)? {
        return Ok(());
    }

    let mut amount = 1.0;
    if let Some(effect) = find_power_effect(object, C::PWR_OPERATE_POWER) {
        if ge(&get(&effect, atom!("endTime"))?, &n(scope.game_time as f64)) {
            let boosted = amount + to_number(&power_effect(C::PWR_OPERATE_POWER, &get(&effect, atom!("level"))?));
            amount = js_min(to_number(&get(&store, atom!("power"))?), boosted);
        }
    }

    if lt(&get(&store, atom!("power"))?, &n(amount)) || lt(&get(&store, atom!("energy"))?, &n(amount * C::POWER_SPAWN_ENERGY_RATIO as f64)) {
        return Ok(());
    }

    put(&store, atom!("power"), n(to_number(&peek(&store, atom!("power"))) - amount))?;
    put(&store, atom!("energy"), n(to_number(&peek(&store, atom!("energy"))) - amount * C::POWER_SPAWN_ENERGY_RATIO as f64))?;

    let user = object.get(atom!("user"));
    stats_inc(scope, "powerProcessed", &user, amount);

    let diff = obj(vec![(atom!("energy"), peek(&store, atom!("energy"))), (atom!("power"), peek(&store, atom!("power")))]);
    update(&mut scope.bulk, object, vec![(atom!("store"), diff)])?;

    // `if(bulkUsers.inc) bulkUsers.inc(object.user, 'power', amount)`: by id, memory untouched
    if truthy(&user) {
        scope.bulk_users.inc(Target::Id(&to_js_string(&user)), &[atom!("power")], amount);
    }
    Ok(())
}
