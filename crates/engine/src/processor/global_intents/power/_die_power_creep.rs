//! mirror: reference/engine/src/processor/global-intents/power/_diePowerCreep.js
//! parity: FAITHFUL
//!
//! The global stage's scope has no `roomObjects`, so vanilla's container lookup always finds nothing and every
//! resource goes into the tombstone.

use crate::atom;
use crate::bulk::Target;
use crate::constants::{POWER_CREEP_SPAWN_COOLDOWN, TOMBSTONE_DECAY_POWER_CREEP};
use crate::live::{to_js_string, to_number, truthy, O, V};
use crate::processor::global::{obj, Scope};
use crate::processor::{get_or_undef, gt, js_add, le, or0};

pub(crate) fn die(object: &O, s: &mut Scope) {
    let g = |k| object.get(k);
    let say = get_or_undef(&g(atom!("actionLog")), atom!("say"));
    let saying = if truthy(&g(atom!("actionLog"))) && truthy(&say) && truthy(&get_or_undef(&say, atom!("isPublic"))) { get_or_undef(&say, atom!("message")) } else { V::Undef };
    // _.mapValues(object.powers, i => ({level: i.level}))
    let powers = O::new();
    if let V::Obj(p) = g(atom!("powers")) {
        for k in p.keys() {
            powers.set(k, obj(vec![("level", get_or_undef(&p.get(k), atom!("level")))]));
        }
    }
    let store = O::new();
    let tombstone = obj(vec![
        ("type", V::str("tombstone")),
        ("room", g(atom!("room"))),
        ("x", g(atom!("x"))),
        ("y", g(atom!("y"))),
        ("user", g(atom!("user"))),
        ("deathTime", V::Num(s.game_time)),
        ("decayTime", V::Num(s.game_time + TOMBSTONE_DECAY_POWER_CREEP as f64)),
        ("store", V::Obj(store.clone())),
        ("powerCreepId", V::Str(to_js_string(&g(atom!("_id"))))),
        ("powerCreepName", g(atom!("name"))),
        ("powerCreepTicksToLive", V::Num(to_number(&g(atom!("ageTime"))) - s.game_time)),
        ("powerCreepClassName", g(atom!("className"))),
        ("powerCreepLevel", g(atom!("level"))),
        ("powerCreepPowers", V::Obj(powers)),
        ("powerCreepSaying", saying),
    ]);

    if let V::Obj(st) = g(atom!("store")) {
        for k in st.keys() {
            let amount = st.get(k);
            if le(&amount, &V::Num(0.0)) {
                continue;
            }
            if gt(&amount, &V::Num(0.0)) {
                store.set(k, js_add(&or0(store.get(k)), &amount));
            }
        }
    }

    s.bulk_objects.insert(&tombstone, None);
    let id = to_js_string(&g(atom!("_id")));
    s.bulk_objects.remove(&id);
    let update = obj(vec![("shard", V::Null), ("spawnCooldownTime", V::Num(s.now + POWER_CREEP_SPAWN_COOLDOWN as f64))]);
    let _ = s.bulk_users_power_creeps.update(Target::Id(&id), &update);
}
