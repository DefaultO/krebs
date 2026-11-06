//! mirror: reference/engine/src/processor/global-intents/power/upgradePowerCreep.js
//! parity: FAITHFUL

use crate::atom;
use crate::bulk::Target;
use crate::constants::{power_info, POWER_CREEP_MAX_LEVEL};
use crate::doc::Atom;
use crate::live::{strict_eq, to_js_string, to_number, truthy, O, V};
use crate::processor::global::{obj, Scope};
use crate::processor::global_intents::power::{no_level_left, this_user_power_creeps};
use crate::processor::{ge, get_or_undef, js_add, loose_eq, lt, put};

pub(crate) fn run(intent: &V, user: &O, s: &mut Scope) {
    let pcs = this_user_power_creeps(s, user);
    if no_level_left(user, &pcs) {
        return;
    }
    let id = get_or_undef(intent, atom!("id"));
    let Some(pc) = pcs.iter().find(|pc| loose_eq(&pc.get(atom!("_id")), &id)).cloned() else { return };
    if ge(&pc.get(atom!("level")), &V::Num(POWER_CREEP_MAX_LEVEL as f64)) {
        return;
    }
    // C.POWER_INFO[intent.power]: keys "1".."19"
    let key = to_js_string(&get_or_undef(intent, atom!("power")));
    let Some(info) = key.parse::<u32>().ok().filter(|p| p.to_string() == *key).and_then(power_info) else { return };
    if !strict_eq(&V::str(info.class_name), &pc.get(atom!("className"))) {
        return;
    }
    let pk = Atom::intern(&key);

    let level = pc.get(atom!("level"));
    let powers = pc.get(atom!("powers"));
    if matches!(powers, V::Undef | V::Null) {
        return; // vanilla reads a property of undefined: throws
    }
    if !truthy(&get_or_undef(&powers, pk)) {
        let _ = put(&powers, pk, obj(vec![("level", V::Num(0.0))]));
    }
    let power = get_or_undef(&powers, pk);
    let power_level = get_or_undef(&power, atom!("level"));
    if loose_eq(&power_level, &V::Num(5.0)) {
        return;
    }
    // powerInfo.level[powers[p].level]: undefined outside 0..4 (then `level < undefined` is false)
    let needed = match to_js_string(&power_level).parse::<usize>() {
        Ok(i) if i < 5 && i.to_string() == *to_js_string(&power_level) => V::Num(info.level[i] as f64),
        _ => V::Undef,
    };
    if lt(&level, &needed) {
        return;
    }

    let level = V::Num(to_number(&level) + 1.0);
    let store_capacity = js_add(&pc.get(atom!("storeCapacity")), &V::Num(100.0));
    let hits_max = js_add(&pc.get(atom!("hitsMax")), &V::Num(1000.0));
    let _ = put(&power, atom!("level"), V::Num(to_number(&power_level) + 1.0));

    let data = || obj(vec![("level", level.clone()), ("hitsMax", hits_max.clone()), ("storeCapacity", store_capacity.clone()), ("powers", powers.clone())]);
    if let Some(room_pc) = s.power_creeps().into_iter().find(|c| loose_eq(&c.get(atom!("_id")), &id)) {
        let _ = s.bulk_objects.update(Target::Obj(&room_pc), &data());
    }
    let _ = s.bulk_users_power_creeps.update(Target::Obj(&pc), &data());
}
