//! mirror: reference/engine/src/processor/intents/invader-core/upgradeController.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::doc::Atom;
use crate::live::{to_js_string, truthy, Crash, O, V};
use crate::processor::global::obj;
use crate::processor::{eq_str, gt, js_add, lodash_find, lodash_match, loose_eq, prop, put, update, RoomScope};

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if !eq_str(&object.get(atom!("type")), "invaderCore") {
        return Ok(());
    }
    let Some(target) = scope.room_objects.get(&to_js_string(&prop(intent, atom!("id"))?)) else { return Ok(()) };
    if !eq_str(&target.get(atom!("type")), "controller") {
        return Ok(());
    }
    if loose_eq(&target.get(atom!("level")), &V::Num(0.0)) || !loose_eq(&target.get(atom!("user")), &object.get(atom!("user"))) {
        return Ok(());
    }
    let game_time = scope.game_time as f64;
    let blocked = target.get(atom!("upgradeBlocked"));
    if truthy(&blocked) && gt(&blocked, &V::Num(game_time)) {
        return Ok(());
    }
    let downgrade = C::INVADER_CORE_CONTROLLER_DOWNGRADE as f64;
    let invulnerability = V::Num(C::EFFECT_INVULNERABILITY as f64);
    match lodash_find(&target.get(atom!("effects")), |e| lodash_match(e, atom!("effect"), &invulnerability)) {
        Some(effect) => put(&effect, atom!("endTime"), V::Num(game_time + downgrade))?,
        None => target.set(
            atom!("effects"),
            V::Arr(crate::live::A::from_vec(vec![obj(vec![("effect", invulnerability.clone()), ("endTime", V::Num(game_time + downgrade)), ("duration", V::Num(downgrade))])])),
        ),
    }
    let upgrade_power = 1.0;
    target.set(atom!("downgradeTime"), V::Num(game_time + downgrade));
    let upgraded = Atom::intern("_upgraded");
    target.set(upgraded, js_add(&target.get(upgraded), &V::Num(upgrade_power)));
    let action_log = prop(&V::Obj(object.clone()), atom!("actionLog"))?;
    put(&action_log, atom!("upgradeController"), obj(vec![("x", target.get(atom!("x"))), ("y", target.get(atom!("y")))]))?;
    let effects = target.get(atom!("effects"));
    update(scope, &target, &obj(vec![("effects", V::Null)]))?;
    update(scope, &target, &obj(vec![("downgradeTime", target.get(atom!("downgradeTime"))), ("effects", effects)]))?;
    let data = obj(vec![("amount", V::Num(upgrade_power)), ("energySpent", V::Num(0.0))]);
    scope.event_log.push(obj(vec![("event", V::Num(C::EVENT_UPGRADE_CONTROLLER as f64)), ("objectId", object.get(atom!("_id"))), ("data", data)]));
    Ok(())
}
