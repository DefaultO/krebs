//! mirror: reference/engine/src/processor/global-intents/power/deletePowerCreep.js
//! parity: FAITHFUL
//!
//! Vanilla also `console.log`s `user.powerExperimentationTime` to the engine's stdout; not reproduced.

use crate::atom;
use crate::bulk::Target;
use crate::constants::POWER_CREEP_DELETE_COOLDOWN;
use crate::live::{to_js_string, truthy, O, V};
use crate::processor::global::{obj, Scope};
use crate::processor::{get_or_undef, gt, loose_eq};

pub(crate) fn run(intent: &V, user: &O, s: &mut Scope) {
    let uid = user.get(atom!("_id"));
    let id = get_or_undef(intent, atom!("id"));
    let Some(pc) = s.user_power_creeps.iter().find(|pc| loose_eq(&pc.get(atom!("user")), &uid) && loose_eq(&pc.get(atom!("_id")), &id)).cloned() else { return };
    if matches!(pc.get(atom!("spawnCooldownTime")), V::Null) {
        return;
    }
    let pc_id = to_js_string(&pc.get(atom!("_id")));
    if truthy(&get_or_undef(intent, atom!("cancel"))) {
        let _ = s.bulk_users_power_creeps.update(Target::Id(&pc_id), &obj(vec![("deleteTime", V::Null)]));
    } else {
        if gt(&user.get(atom!("powerExperimentationTime")), &V::Num(s.now)) {
            s.bulk_users_power_creeps.remove(&pc_id);
            return;
        }
        if truthy(&pc.get(atom!("deleteTime"))) {
            return;
        }
        let delete_time = V::Num(s.now + POWER_CREEP_DELETE_COOLDOWN as f64);
        let _ = s.bulk_users_power_creeps.update(Target::Id(&pc_id), &obj(vec![("deleteTime", delete_time)]));
    }
}
