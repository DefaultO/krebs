//! mirror: reference/engine/src/processor/global-intents/power/spawnPowerCreep.js
//! parity: FAITHFUL

use crate::atom;
use crate::bulk::Target;
use crate::constants::POWER_CREEP_LIFE_TIME;
use crate::doc::Atom;
use crate::live::{assign, is_equal, to_js_string, truthy, O, V};
use crate::processor::global::{obj, Scope};
use crate::processor::{get_or_undef, gt, loose_eq};

pub(crate) fn run(intent: &V, user: &O, s: &mut Scope) {
    let uid = user.get(atom!("_id"));
    let Some(power_spawn) = s.power_spawn(&get_or_undef(intent, atom!("id"))) else { return };
    let just_spawned = Atom::intern("_justSpawned");
    if !loose_eq(&power_spawn.get(atom!("user")), &uid) || truthy(&power_spawn.get(just_spawned)) {
        return;
    }
    let name = get_or_undef(intent, atom!("name"));
    let Some(pc) = s.user_power_creeps.iter().find(|pc| loose_eq(&pc.get(atom!("user")), &uid) && loose_eq(&pc.get(atom!("name")), &name)).cloned() else { return };
    let cooldown = pc.get(atom!("spawnCooldownTime"));
    if matches!(cooldown, V::Null) || gt(&cooldown, &V::Num(s.now)) {
        return;
    }
    let (room, x, y) = (power_spawn.get(atom!("room")), power_spawn.get(atom!("x")), power_spawn.get(atom!("y")));
    // _.any(roomObjectsByType.powerCreep, {room, x, y})
    if s.power_creeps().iter().any(|c| is_equal(&c.get(atom!("room")), &room) && is_equal(&c.get(atom!("x")), &x) && is_equal(&c.get(atom!("y")), &y)) {
        return;
    }

    let update = obj(vec![("shard", V::str(s.shard_name)), ("spawnCooldownTime", V::Null), ("deleteTime", V::Null)]);
    let _ = s.bulk_users_power_creeps.update(Target::Obj(&pc), &update);

    // Object.assign({}, powerCreep, {...}) after the update merged into it
    let creep = O::new();
    assign(&creep, &pc);
    let fields = obj(vec![
        ("type", V::str("powerCreep")),
        ("room", room),
        ("x", x),
        ("y", y),
        ("hits", pc.get(atom!("hitsMax"))),
        ("ageTime", V::Num(s.game_time + POWER_CREEP_LIFE_TIME as f64)),
        ("actionLog", obj(vec![("spawned", V::Bool(true))])),
        ("notifyWhenAttacked", V::Bool(true)),
    ]);
    if let V::Obj(f) = &fields {
        assign(&creep, f);
    }
    let id = to_js_string(&pc.get(atom!("_id")));
    s.bulk_objects.insert(&V::Obj(creep), Some(&id));
    power_spawn.set(just_spawned, V::Bool(true));
}
