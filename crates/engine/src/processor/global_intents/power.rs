//! mirror: reference/engine/src/processor/global-intents/power.js
//! parity: FAITHFUL

pub mod _die_power_creep;
pub mod create_power_creep;
pub mod delete_power_creep;
pub mod rename_power_creep;
pub mod spawn_power_creep;
pub mod suicide_power_creep;
pub mod upgrade_power_creep;

use crate::atom;
use crate::doc::Atom;
use crate::live::{to_js_string, to_number, O, V};
use crate::processor::global::Scope;
use crate::processor::{get_or_undef, le, loose_eq, or0};

/// `intentTypes` (power.js:8): per `users.intents` doc, every element of each, in this order.
pub const INTENT_TYPES: &[&str] =
    &["spawnPowerCreep", "suicidePowerCreep", "deletePowerCreep", "upgradePowerCreep", "createPowerCreep", "renamePowerCreep"];

type Handler = fn(&V, &O, &mut Scope);
const HANDLERS: [Handler; 6] = [
    spawn_power_creep::run,
    suicide_power_creep::run,
    delete_power_creep::run,
    upgrade_power_creep::run,
    create_power_creep::run,
    rename_power_creep::run,
];

/// power.js:12.
pub(crate) fn run(s: &mut Scope) {
    for doc in s.user_intents.clone() {
        // `usersById[iUserIntents.user]`; a missing user makes every handler throw (TICK-ERROR): skipped
        let Some(user) = s.users_by_id.get(&*to_js_string(&doc.get(atom!("user")))).cloned() else { continue };
        let intents = doc.get(atom!("intents"));
        for (name, handler) in INTENT_TYPES.iter().zip(HANDLERS) {
            if let V::Arr(list) = get_or_undef(&intents, Atom::intern(name)) {
                for intent in list.to_vec() {
                    handler(&intent, &user, s);
                }
            }
        }
    }
    for creep in s.power_creeps() {
        let gt_ = s.game_time;
        if gt_ >= to_number(&creep.get(atom!("ageTime"))) - 1.0 || le(&creep.get(atom!("hits")), &V::Num(0.0)) {
            _die_power_creep::die(&creep, s);
        }
    }
}

/// `_.filter(userPowerCreeps, i => i.user == user._id)`.
pub(crate) fn this_user_power_creeps(s: &Scope, user: &O) -> Vec<O> {
    let id = user.get(atom!("_id"));
    s.user_power_creeps.iter().filter(|pc| loose_eq(&pc.get(atom!("user")), &id)).cloned().collect()
}

/// `Math.floor(Math.pow((user.power || 0) / POWER_LEVEL_MULTIPLY, 1 / POWER_LEVEL_POW))` against
/// `pcs.length + _.sum(pcs, 'level') + (user._usedPowerLevels || 0)`: true when no level is left.
pub(crate) fn no_level_left(user: &O, pcs: &[O]) -> bool {
    use crate::constants::{POWER_LEVEL_MULTIPLY, POWER_LEVEL_POW};
    let power = to_number(&or0(user.get(atom!("power"))));
    let level = crate::processor::js_pow(power / POWER_LEVEL_MULTIPLY as f64, 1.0 / POWER_LEVEL_POW as f64).floor();
    // lodash 3 arraySum: from the last element, `+level || 0`
    let sum = pcs.iter().rev().fold(0.0, |acc, pc| {
        let x = to_number(&pc.get(atom!("level")));
        acc + if x.is_nan() || x == 0.0 { 0.0 } else { x }
    });
    let used = pcs.len() as f64 + sum + to_number(&or0(user.get(Atom::intern("_usedPowerLevels"))));
    used >= level
}

/// `intent.name.substring(0, 50)` (UTF-16 units); a non-string name throws in vanilla (None).
pub(crate) fn name50(v: &V) -> Option<V> {
    match v {
        V::Str(s) => {
            let units: Vec<u16> = s.encode_utf16().take(50).collect();
            Some(V::Str(String::from_utf16_lossy(&units).into()))
        }
        _ => None,
    }
}
