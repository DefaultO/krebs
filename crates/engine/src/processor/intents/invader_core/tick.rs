//! mirror: reference/engine/src/processor/intents/invader-core/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::doc::Atom;
use crate::live::{is_equal, to_number, truthy, Crash, O, V};
use crate::processor::global::obj;
use crate::processor::intents::spawns::_born_creep::born_creep_maybe;
use crate::processor::{eq_str, get_or_undef, lodash_find, lodash_match, lodash_match_str, update, RoomScope};

/// The controller reset of tick.js:16 and destroy.js:12.
pub(crate) fn release_controller(scope: &mut RoomScope) -> Result<(), Crash> {
    if let Some(rc) = scope.room_controller.clone() {
        let data = obj(vec![
            ("user", V::Null),
            ("level", V::Num(0.0)),
            ("progress", V::Num(0.0)),
            ("downgradeTime", V::Null),
            ("safeMode", V::Null),
            ("safeModeAvailable", V::Num(0.0)),
            ("safeModeCooldown", V::Null),
            ("isPowerEnabled", V::Bool(false)),
            ("effects", V::Null),
        ]);
        update(scope, &rc, &data)?;
    }
    Ok(())
}

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    if !eq_str(&object.get(atom!("type")), "invaderCore") {
        return Ok(());
    }
    let game_time = scope.game_time as f64;
    let collapse = V::Num(C::EFFECT_COLLAPSE_TIMER as f64);
    if let Some(effect) = lodash_find(&object.get(atom!("effects")), |e| lodash_match(e, atom!("effect"), &collapse)) {
        if to_number(&get_or_undef(&effect, atom!("endTime"))) <= game_time {
            return release_controller(scope);
        }
    }

    let spawning = object.get(atom!("spawning"));
    if truthy(&spawning) && game_time >= to_number(&get_or_undef(&spawning, atom!("spawnTime"))) - 1.0 {
        let name = get_or_undef(&spawning, atom!("name"));
        // _.find(roomObjects, {type: 'creep', name, x, y})
        let creep = scope.room_objects.iter().find(|&o| {
            let v = V::Obj(o.clone());
            lodash_match_str(&v, atom!("type"), "creep")
                && lodash_match(&v, atom!("name"), &name)
                && lodash_match(&v, atom!("x"), &object.get(atom!("x")))
                && lodash_match(&v, atom!("y"), &object.get(atom!("y")))
        }).cloned();
        if born_creep_maybe(object, creep.as_ref(), scope)? {
            update(scope, object, &obj(vec![("spawning", V::Null)]))?;
        } else {
            update(scope, object, &obj(vec![("spawning", obj(vec![("spawnTime", V::Num(1.0 + game_time))]))]))?;
        }
    }

    let action_log = object.get(atom!("actionLog"));
    if !is_equal(&action_log, &object.get(Atom::intern("_actionLog"))) {
        scope.room_info.set(atom!("active"), V::Bool(true));
        update(scope, object, &obj(vec![("actionLog", action_log)]))?;
    }
    Ok(())
}
