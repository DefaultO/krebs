//! mirror: reference/engine/src/processor/intents/nukes/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::_damage::damage;
use crate::processor::intents::creeps::_die::die;
use crate::processor::RoomScope;

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if novice_or_respawn(scope) {
        remove_object(scope, object);
        return Ok(());
    }

    if game_time == object.num(atom!("landTime")) - 1.0 {
        // `_.forEach(roomObjects, …)`: a key snapshot; objects removed meanwhile read as undefined
        for k in scope.room_objects.keys() {
            let Some(target) = scope.room_objects.get(&k) else { continue };
            let kind = target.get(atom!("type"));
            if loose_eq(&kind, &s("creep")) {
                die(&target, Some(0.0), true, scope, Some(C::EVENT_ATTACK_TYPE_NUKE))?;
            }
            if loose_eq(&kind, &s("powerCreep")) {
                update(&mut scope.bulk, &target, vec![(atom!("hits"), n(0.0))])?;
            }
            if ["constructionSite", "energy", "tombstone", "ruin"].iter().any(|t| loose_eq(&kind, &s(t))) {
                remove_object(scope, &target);
            }
            if loose_eq(&kind, &s("spawn")) && !loose_eq(&target.get(atom!("spawning")), &V::Null) {
                update(&mut scope.bulk, &target, vec![(atom!("spawning"), V::Null)])?;
            }
        }

        for dx in -2i32..=2 {
            for dy in -2i32..=2 {
                let x = add(&object.get(atom!("x")), &n(dx as f64));
                let y = add(&object.get(atom!("y")), &n(dy as f64));
                let range = dx.abs().max(dy.abs());
                let mut amount = if range == 0 { C::NUKE_DAMAGE[0] } else { C::NUKE_DAMAGE[2] }.map_or(f64::NAN, |d| d as f64);

                let mut objects = room_filter(scope, |i| room_match(i, &[(atom!("x"), x.clone()), (atom!("y"), y.clone())]));
                let rampart = objects.iter().find(|i| room_match(i, &[(atom!("type"), s("rampart"))])).cloned();
                if let Some(rampart) = rampart {
                    let rampart_hits = rampart.get(atom!("hits"));
                    objects.retain(|i| !i.same(&rampart));
                    damage(object, &rampart, amount, C::EVENT_ATTACK_TYPE_NUKE, scope)?;
                    amount -= to_number(&rampart_hits);
                }
                if amount > 0.0 {
                    for target in &objects {
                        damage(object, target, amount, C::EVENT_ATTACK_TYPE_NUKE, scope)?;
                    }
                }
            }
        }

        if let Some(ctrl) = scope.room_controller.clone() {
            if gt(&ctrl.get(atom!("safeMode")), &n(game_time)) {
                update(&mut scope.bulk, &ctrl, vec![(atom!("safeMode"), n(game_time)), (atom!("safeModeCooldown"), V::Null)])?;
            }

            let invulnerable = values(&ctrl.get(atom!("effects")))
                .iter()
                .any(|e| loose_eq(&peek(e, atom!("effect")), &n(C::EFFECT_INVULNERABILITY as f64)) && gt(&peek(e, atom!("endTime")), &n(game_time)));
            let blocked = ctrl.get(atom!("upgradeBlocked"));
            // `user && !invulnerable && !upgradeBlocked || upgradeBlocked < gameTime` (sic)
            if truthy(&ctrl.get(atom!("user"))) && !invulnerable && !truthy(&blocked) || lt(&blocked, &n(game_time)) {
                update(&mut scope.bulk, &ctrl, vec![(atom!("upgradeBlocked"), n(game_time + C::CONTROLLER_NUKE_BLOCKED_UPGRADE as f64))])?;
            }
        }
    }

    if game_time >= object.num(atom!("landTime")) {
        remove_object(scope, object);
    }

    Ok(())
}
