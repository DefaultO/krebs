//! mirror: reference/engine/src/processor/intents/creeps/rangedMassAttack.js
//! parity: FAITHFUL
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use super::attack::{in_foreign_safe_mode, rampart_at};
use crate::atom;
use crate::constants::{EFFECT_INVULNERABILITY, EVENT_ATTACK_TYPE_RANGED_MASS, PWR_FORTIFY, RANGED_ATTACK, RANGED_ATTACK_POWER};
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::{ge, get_or_undef, js_add, js_max, js_round, le, lodash_values, loose_eq, obj, prop, put, room_object_list, type_is, RoomScope};
use crate::utils;

/// CREEP-RANGED-MASS-ATTACK.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let _ = intent;
    if !type_is(object, "creep") {
        return Ok(());
    }
    if truthy(&object.get(atom!("spawning"))) {
        return Ok(());
    }
    let attack_power = utils::calc_body_effectiveness(&object.get(atom!("body")), RANGED_ATTACK, "rangedMassAttack", RANGED_ATTACK_POWER as f64, false)?;
    if attack_power == 0.0 {
        return Ok(());
    }
    if in_foreign_safe_mode(scope, object) {
        return Ok(());
    }

    let user = object.get(atom!("user"));
    let (ox, oy) = (object.get(atom!("x")), object.get(atom!("y")));
    // `i.x >= object.x - 3 && i.x <= object.x + 3` (`-` is numeric, `+` may concatenate).
    let within = |v: &V, c: &V| ge(v, &V::Num(to_number(c) - 3.0)) && le(v, &js_add(c, &V::Num(3.0)));
    let targets: Vec<O> = room_object_list(scope)
        .into_iter()
        .filter(|i| {
            (!matches!(i.get(atom!("user")), V::Undef) || type_is(i, "powerBank"))
                && !loose_eq(&i.get(atom!("user")), &user)
                && within(&i.get(atom!("x")), &ox)
                && within(&i.get(atom!("y")), &oy)
        })
        .collect();

    let game_time = V::Num(scope.game_time as f64);
    for target in targets {
        if !type_is(&target, "rampart") && rampart_at(scope, &target).is_some() {
            continue;
        }
        if !truthy(&target.get(atom!("hits"))) {
            continue;
        }
        if type_is(&target, "creep") && truthy(&target.get(atom!("spawning"))) {
            continue;
        }
        let mut shielded = false;
        for e in lodash_values(&target.get(atom!("effects"))) {
            if ge(&prop(&e, atom!("endTime"))?, &game_time)
                && (loose_eq(&get_or_undef(&e, atom!("power")), &V::Num(PWR_FORTIFY as f64)) || loose_eq(&get_or_undef(&e, atom!("effect")), &V::Num(EFFECT_INVULNERABILITY as f64)))
            {
                shielded = true;
                break;
            }
        }
        if shielded {
            continue;
        }
        let distance = js_max(
            (to_number(&object.get(atom!("x"))) - to_number(&target.get(atom!("x")))).abs(),
            (to_number(&object.get(atom!("y"))) - to_number(&target.get(atom!("y")))).abs(),
        );
        // distanceRate = {0: 1, 1: 1, 2: 0.4, 3: 0.1}
        let rate = if distance == 0.0 || distance == 1.0 {
            1.0
        } else if distance == 2.0 {
            0.4
        } else if distance == 3.0 {
            0.1
        } else {
            f64::NAN
        };
        let target_attack_power = js_round(attack_power * rate);
        super::super::_damage::damage(object, &target, target_attack_power, EVENT_ATTACK_TYPE_RANGED_MASS, scope)?;
    }
    put(&object.get(atom!("actionLog")), atom!("rangedMassAttack"), obj! {})
}
