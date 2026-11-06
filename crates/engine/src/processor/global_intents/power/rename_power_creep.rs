//! mirror: reference/engine/src/processor/global-intents/power/renamePowerCreep.js
//! parity: FAITHFUL

use crate::atom;
use crate::bulk::Target;
use crate::live::{is_equal, to_js_string, O, V};
use crate::processor::global::{obj, Scope};
use crate::processor::global_intents::power::{name50, this_user_power_creeps};
use crate::processor::{get_or_undef, loose_eq};

pub(crate) fn run(intent: &V, user: &O, s: &mut Scope) {
    let pcs = this_user_power_creeps(s, user);
    let id = get_or_undef(intent, atom!("id"));
    let Some(pc) = pcs.iter().find(|pc| loose_eq(&pc.get(atom!("_id")), &id)) else { return };
    if matches!(pc.get(atom!("spawnCooldownTime")), V::Null) {
        return;
    }
    let Some(name) = name50(&get_or_undef(intent, atom!("name"))) else { return };
    if pcs.iter().any(|p| is_equal(&p.get(atom!("name")), &name)) {
        return;
    }
    let pc_id = to_js_string(&pc.get(atom!("_id")));
    let _ = s.bulk_users_power_creeps.update(Target::Id(&pc_id), &obj(vec![("name", name)]));
}
