//! mirror: reference/engine/src/processor/intents/controllers/tick.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;

/// `roomInfo.novice > Date.now() ? null : gameTime + C.SAFE_MODE_COOLDOWN`.
pub(crate) fn safe_mode_cooldown(scope: &RoomScope) -> V {
    if gt(&scope.room_info.get(atom!("novice")), &date_now(scope)) {
        V::Null
    } else {
        n(scope.game_time as f64 + C::SAFE_MODE_COOLDOWN as f64)
    }
}

pub fn tick(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if !object.eq_str(atom!("type"), "controller") {
        return Ok(());
    }

    let reservation = object.get(atom!("reservation"));
    if truthy(&reservation) && (ge(&n(game_time), &n(to_number(&get(&reservation, atom!("endTime"))?) - 1.0)) || truthy(&object.get(atom!("user")))) {
        update(&mut scope.bulk, object, vec![(atom!("reservation"), V::Null)])?;
    }

    let user = object.get(atom!("user"));
    if !truthy(&user) || loose_eq(&user, &s("2")) {
        return Ok(());
    }

    add_room_to_user(scope, &object.get(atom!("room")), &user)?;

    let upgrade_blocked = object.get(atom!("_upgradeBlocked"));
    if truthy(&upgrade_blocked) {
        update(&mut scope.bulk, object, vec![(atom!("upgradeBlocked"), upgrade_blocked)])?;
        object.delete(atom!("_upgradeBlocked"));
    }

    if truthy(&object.get(atom!("_safeModeActivated"))) && !gt(&object.get(atom!("upgradeBlocked")), &n(game_time)) {
        let cooldown = safe_mode_cooldown(scope);
        update(
            &mut scope.bulk,
            object,
            vec![
                (atom!("safeModeAvailable"), n(to_number(&object.get(atom!("safeModeAvailable"))) - 1.0)),
                (atom!("safeMode"), n(game_time + C::SAFE_MODE_DURATION as f64)),
                (atom!("safeModeCooldown"), cooldown),
            ],
        )?;
    }

    if !truthy(&object.get(atom!("downgradeTime"))) || truthy(&object.get(atom!("tutorial"))) {
        let t = add(&add(&n(game_time), &by_index(&C::CONTROLLER_DOWNGRADE, &object.get(atom!("level")))), &n(1.0));
        update(&mut scope.bulk, object, vec![(atom!("downgradeTime"), t)])?;
        return Ok(());
    }

    if truthy(&object.get(atom!("_upgraded"))) && !gt(&object.get(atom!("upgradeBlocked")), &n(game_time)) {
        let restored = add(&add(&object.get(atom!("downgradeTime")), &n(C::CONTROLLER_DOWNGRADE_RESTORE as f64)), &n(1.0));
        let cap = add(&add(&n(game_time), &by_index(&C::CONTROLLER_DOWNGRADE, &object.get(atom!("level")))), &n(1.0));
        update(&mut scope.bulk, object, vec![(atom!("downgradeTime"), n(js_min(to_number(&restored), to_number(&cap))))])?;
        return Ok(());
    }

    if game_time == to_number(&object.get(atom!("downgradeTime"))) - 3000.0 {
        let msg = format!(
            "Attention! Your Controller in room {} will be downgraded to level {} in 3000 ticks (~2 hours)! Upgrade it to prevent losing of this room. <a href='http://support.screeps.com/hc/en-us/articles/203086021-Territory-control'>Learn more</a>",
            to_js_string(&object.get(atom!("room"))),
            num_str(to_number(&object.get(atom!("level"))) - 1.0)
        );
        send_notification(scope, &user, msg);
    }

    while ge(&n(game_time), &n(to_number(&object.get(atom!("downgradeTime"))) - 1.0)) && gt(&object.get(atom!("level")), &n(0.0)) {
        object.set(atom!("level"), n(to_number(&object.get(atom!("level"))) - 1.0));
        let msg = format!(
            "Your Controller in room {} has been downgraded to level {} due to absence of upgrading activity!",
            to_js_string(&object.get(atom!("room"))),
            to_js_string(&object.get(atom!("level")))
        );
        send_notification(scope, &object.get(atom!("user")), msg);
        if loose_eq(&object.get(atom!("level")), &n(0.0)) {
            remove_room_from_user(scope, &object.get(atom!("room")), &object.get(atom!("user")))?;

            object.set(atom!("progress"), n(0.0));
            object.set(atom!("user"), V::Null);
            object.set(atom!("downgradeTime"), V::Null);
            object.set(atom!("upgradeBlocked"), V::Null);
            object.set(atom!("safeMode"), V::Null);
            object.set(atom!("safeModeAvailable"), n(0.0));
            object.set(atom!("safeModeCooldown"), safe_mode_cooldown(scope));
            object.set(atom!("isPowerEnabled"), V::Bool(false));
        } else {
            let level = object.get(atom!("level"));
            let step = n(to_number(&by_index(&C::CONTROLLER_DOWNGRADE, &level)) / 2.0 + 1.0);
            object.set(atom!("downgradeTime"), add(&object.get(atom!("downgradeTime")), &step));
            let progress = n(js_round(to_number(&by_index(&C::CONTROLLER_LEVELS, &level)) * 0.9));
            object.set(atom!("progress"), add(&object.get(atom!("progress")), &progress));
            object.set(atom!("safeModeAvailable"), n(0.0));
            object.set(atom!("safeModeCooldown"), safe_mode_cooldown(scope));
        }

        let keys = [
            atom!("downgradeTime"),
            atom!("level"),
            atom!("progress"),
            atom!("user"),
            atom!("upgradeBlocked"),
            atom!("safeMode"),
            atom!("safeModeCooldown"),
            atom!("safeModeAvailable"),
            atom!("isPowerEnabled"),
        ];
        update(&mut scope.bulk, object, keys.iter().map(|&k| (k, object.get(k))).collect())?;
    }

    Ok(())
}
