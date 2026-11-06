//! mirror: reference/engine/src/processor/intents/power-creeps/usePower.js
//! parity: FAITHFUL
//! Verified against the official processor by the fuzz fixtures in crates/engine/tests/fixtures/oracle/pc.

use crate::atom;
use crate::constants::{self as C, PerLevel};
use crate::doc::{array_index, Atom};
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::{room_object, RoomScope};
use crate::utils;

/// `table[level-1]` of a per-level array, or the plain value (`_.isArray(x) ? x[level-1] : x`).
fn per_level(p: PerLevel, level: &V) -> V {
    match p {
        PerLevel::Fixed(v) => n(v as f64),
        PerLevel::Levels(a) => index(&a.map(|x| x as f64), to_number(level) - 1.0),
    }
}

/// `array[i]` for a numeric `i` (undefined outside the array or for a non-index).
fn index(a: &[f64], i: f64) -> V {
    array_index(&to_js_string(&n(i))).and_then(|i| a.get(i as usize)).map_or(V::Undef, |x| n(*x))
}

/// PC-USE-POWER.
pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = n(scope.game_time as f64);
    if let Some(ctrl) = &scope.room_controller {
        if !truthy(&ctrl.get(atom!("isPowerEnabled"))) {
            return Ok(());
        }
        if !loose_eq(&ctrl.get(atom!("user")), &object.get(atom!("user"))) && gt(&ctrl.get(atom!("safeMode")), &game_time) {
            return Ok(());
        }
    }

    let power = get(intent, atom!("power"))?;
    // `C.POWER_INFO[intent.power]`: keys "1".."19".
    let Some(info) = array_index(&to_js_string(&power)).and_then(C::power_info) else { return Ok(()) };
    let creep_power = get(&object.get(atom!("powers")), key(&power))?;
    let level = peek(&creep_power, atom!("level"));
    if !truthy(&creep_power) || loose_eq(&level, &n(0.0)) || gt(&peek(&creep_power, atom!("cooldownTime")), &game_time) {
        return Ok(());
    }

    let ops = info.ops.map_or(n(0.0), |p| per_level(p, &level));

    if !truthy(&object.get(atom!("store"))) {
        object.set(atom!("store"), obj(vec![]));
    }
    let store = || object.get(atom!("store"));
    let store_ops = || {
        let v = peek(&store(), atom!("ops"));
        if truthy(&v) { v } else { n(0.0) }
    };
    if lt(&store_ops(), &ops) {
        return Ok(());
    }

    let mut target: Option<O> = None;
    if let Some(range) = info.range {
        let Some(t) = room_object(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
        if utils::dist(&V::Obj(object.clone()), &V::Obj(t.clone())) > range as f64 {
            return Ok(());
        }
        let mut current = None;
        for i in values(&t.get(atom!("effects"))) {
            if loose_eq(&get(&i, atom!("power"))?, &power) {
                current = Some(i);
                break;
            }
        }
        if let Some(e) = current {
            if gt(&peek(&e, atom!("level")), &level) && gt(&peek(&e, atom!("endTime")), &game_time) {
                return Ok(());
            }
        }
        target = Some(t);
    }

    let effect_at = |level: &V| info.effect.map_or(V::Undef, |e| index(&e, to_number(level) - 1.0));
    let is = |t: &Option<O>, ty: &str| t.as_ref().is_some_and(|t| loose_eq(&t.get(atom!("type")), &s(ty)));

    // `switch(intent.power)` compares with `===`: only a number reaches a case.
    let pwr = match power {
        V::Num(x) if x.fract() == 0.0 && (1.0..=19.0).contains(&x) => x as u32,
        _ => 0,
    };
    // The cases that only check the target's type (every power without one has no range, so `target` is set).
    let wants: &[&str] = match pwr {
        C::PWR_OPERATE_SPAWN | C::PWR_DISRUPT_SPAWN => &["spawn"],
        C::PWR_OPERATE_TOWER | C::PWR_DISRUPT_TOWER => &["tower"],
        C::PWR_OPERATE_STORAGE => &["storage"],
        C::PWR_OPERATE_LAB => &["lab"],
        C::PWR_OPERATE_OBSERVER => &["observer"],
        C::PWR_OPERATE_TERMINAL | C::PWR_DISRUPT_TERMINAL => &["terminal"],
        C::PWR_DISRUPT_SOURCE | C::PWR_REGEN_SOURCE => &["source"],
        C::PWR_OPERATE_CONTROLLER => &["controller"],
        C::PWR_OPERATE_POWER => &["powerSpawn"],
        C::PWR_FORTIFY => &["rampart", "constructedWall"],
        _ => &[],
    };
    let mut apply = !wants.is_empty();
    if apply && !wants.iter().any(|w| is(&target, w)) {
        return Ok(());
    }
    match pwr {
        C::PWR_GENERATE_OPS => {
            let ops_now = add(&store_ops(), &effect_at(&level));
            update(&mut scope.bulk, object, vec![(atom!("store"), obj(vec![(Atom::intern(C::RESOURCE_OPS), ops_now)]))])?;
            let sum = calc_resources(object)?;
            let cap = object.get(atom!("storeCapacity"));
            if gt(&n(sum), &cap) {
                let amount = js_min(to_number(&peek(&store(), atom!("ops"))), sum - to_number(&cap));
                let drop = obj(vec![(atom!("amount"), n(amount)), (atom!("resourceType"), s(C::RESOURCE_OPS))]);
                super::drop::run(object, &drop, scope)?;
            }
        }
        C::PWR_REGEN_MINERAL => {
            let t = target.as_ref().unwrap();
            if !is(&target, "mineral") || loose_eq(&t.get(atom!("mineralAmount")), &n(0.0)) || truthy(&t.get(atom!("nextRegenerationTime"))) {
                return Ok(());
            }
            apply = true;
        }
        C::PWR_OPERATE_EXTENSION => {
            let t = target.clone().unwrap();
            let t_store = t.get(atom!("store"));
            if !truthy(&t_store) || !(is(&target, "storage") || is(&target, "terminal") || is(&target, "factory") || is(&target, "container")) {
                return Ok(());
            }
            let ctrl_user = || scope.room_controller.as_ref().map(|c| c.get(atom!("user"))).ok_or_else(|| crash_read(&V::Undef, "user"));
            if truthy(&t.get(atom!("user"))) && !loose_eq(&t.get(atom!("user")), &ctrl_user()?) {
                return Ok(());
            }
            if let Some(e) = find_power_effect(&t, C::PWR_DISRUPT_TERMINAL) {
                if gt(&peek(&e, atom!("endTime")), &game_time) {
                    return Ok(());
                }
            }
            let mut extensions = Vec::new();
            for i in room_values(scope) {
                if i.eq_str(atom!("type"), "extension") && loose_eq(&i.get(atom!("user")), &ctrl_user()?) && !truthy(&i.get(atom!("off"))) {
                    extensions.push(i);
                }
            }
            let ext_v: Vec<V> = extensions.iter().map(|o| V::Obj(o.clone())).collect();
            let capacity = sum(&ext_v, |i| Ok(peek(&peek(i, atom!("storeCapacityResource")), atom!("energy"))))?;
            let energy_limit = js_min(to_number(&peek(&t_store, atom!("energy"))), to_number(&effect_at(&level)) * capacity);
            stable_sort(&mut extensions, |a, b| comparator_distance(&t, a, b));
            let mut energy_sent = 0.0;
            for extension in &extensions {
                let cap = get(&extension.get(atom!("storeCapacityResource")), atom!("energy"))?;
                let ext_store = get(&extension.get(atom!("store")), atom!("energy"))?;
                let energy = js_min(energy_limit - energy_sent, to_number(&cap) - to_number(&ext_store));
                let filled = add(&ext_store, &n(energy));
                update(&mut scope.bulk, extension, vec![(atom!("store"), obj(vec![(atom!("energy"), filled)]))])?;
                energy_sent += energy;
                if !(energy_sent < energy_limit) {
                    break;
                }
            }
            if energy_sent == 0.0 {
                return Ok(());
            }
            let left = to_number(&peek(&t.get(atom!("store")), atom!("energy"))) - energy_sent;
            update(&mut scope.bulk, &t, vec![(atom!("store"), obj(vec![(atom!("energy"), n(left))]))])?;
        }
        C::PWR_SHIELD => {
            let (x, y) = (object.get(atom!("x")), object.get(atom!("y")));
            let site = room_find(scope, |i| loose_eq(&i.get(atom!("x")), &x) && loose_eq(&i.get(atom!("y")), &y) && i.eq_str(atom!("type"), "constructionSite"));
            if let Some(site) = site {
                remove_object(scope, &site);
            }
            let rampart = s("rampart");
            if !check_construction_site_objects(scope, &rampart, &x, &y) || !check_construction_site_terrain(scope.room_terrain, &rampart, &x, &y) {
                return Ok(());
            }
            let duration = info.duration.map_or(V::Undef, |p| per_level(p, &level));
            let end = add(&game_time, &duration);
            let doc = obj(vec![
                (atom!("type"), rampart),
                (atom!("room"), object.get(atom!("room"))),
                (atom!("x"), x),
                (atom!("y"), y),
                (atom!("user"), object.get(atom!("user"))),
                (atom!("hits"), effect_at(&level)),
                (atom!("hitsMax"), n(0.0)),
                (atom!("nextDecayTime"), end.clone()),
                (atom!("effects"), arr(vec![obj(vec![(atom!("power"), n(C::PWR_SHIELD as f64)), (atom!("level"), level.clone()), (atom!("endTime"), end)])])),
            ]);
            scope.bulk.insert(&doc, None);
        }
        C::PWR_OPERATE_FACTORY => {
            let t = target.as_ref().unwrap();
            if !is(&target, "factory") {
                return Ok(());
            }
            if !truthy(&t.get(atom!("level"))) {
                update(&mut scope.bulk, t, vec![(atom!("level"), level.clone())])?;
            } else if !loose_eq(&t.get(atom!("level")), &level) {
                return Ok(());
            }
            apply = true;
        }
        _ => {}
    }
    if apply {
        let t = target.as_ref().unwrap();
        let effects = t.get(atom!("effects"));
        let mut list = values(if truthy(&effects) { &effects } else { &V::Undef });
        list.retain(|e| !matches(e, &[(atom!("power"), power.clone())]));
        let duration = info.duration.map_or(V::Undef, |p| per_level(p, &level));
        list.push(obj(vec![
            (atom!("effect"), power.clone()),
            (atom!("power"), power.clone()),
            (atom!("level"), level.clone()),
            (atom!("endTime"), add(&game_time, &duration)),
        ]));
        update(&mut scope.bulk, t, vec![(atom!("effects"), V::Null)])?;
        update(&mut scope.bulk, t, vec![(atom!("effects"), arr(list))])?;
    }

    let cooldown = add(&game_time, &n(info.cooldown as f64));
    let ops_left = n(to_number(&store_ops()) - to_number(&ops));
    update(
        &mut scope.bulk,
        object,
        vec![
            (atom!("powers"), obj(vec![(key(&power), obj(vec![(atom!("cooldownTime"), cooldown)]))])),
            (atom!("store"), obj(vec![(atom!("ops"), ops_left)])),
        ],
    )?;

    let id = peek(intent, atom!("id"));
    event(scope, C::EVENT_POWER, object.get(atom!("_id")), Some(obj(vec![(atom!("power"), power.clone()), (atom!("targetId"), id)])));

    let at = target.as_ref().unwrap_or(object);
    let log = obj(vec![(atom!("id"), power), (atom!("x"), at.get(atom!("x"))), (atom!("y"), at.get(atom!("y")))]);
    put(&object.get(atom!("actionLog")), atom!("power"), log)
}
