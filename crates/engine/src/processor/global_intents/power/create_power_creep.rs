//! mirror: reference/engine/src/processor/global-intents/power/createPowerCreep.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants::POWER_CLASS_OPERATOR;
use crate::doc::Atom;
use crate::live::{is_equal, to_js_string, to_number, O, V};
use crate::processor::global::{obj, Scope};
use crate::processor::global_intents::power::{name50, no_level_left, this_user_power_creeps};
use crate::processor::{get_or_undef, or0};

pub(crate) fn run(intent: &V, user: &O, s: &mut Scope) {
    let pcs = this_user_power_creeps(s, user);
    if no_level_left(user, &pcs) {
        return;
    }
    // Object.values(C.POWER_CLASS).indexOf(intent.className) === -1
    let class_name = get_or_undef(intent, atom!("className"));
    if !matches!(&class_name, V::Str(c) if &**c == POWER_CLASS_OPERATOR) {
        return;
    }
    let Some(name) = name50(&get_or_undef(intent, atom!("name"))) else { return };
    if pcs.iter().any(|pc| is_equal(&pc.get(atom!("name")), &name)) {
        return;
    }
    let doc = obj(vec![
        ("name", name),
        ("className", class_name),
        ("user", V::Str(to_js_string(&user.get(atom!("_id"))))),
        ("level", V::Num(0.0)),
        ("hitsMax", V::Num(1000.0)),
        ("store", obj(vec![])),
        ("storeCapacity", V::Num(100.0)),
        ("spawnCooldownTime", V::Num(0.0)),
        ("powers", obj(vec![])),
    ]);
    s.bulk_users_power_creeps.insert(&doc, None);
    let used = Atom::intern("_usedPowerLevels");
    user.set(used, V::Num(to_number(&or0(user.get(used))) + 1.0));
}
