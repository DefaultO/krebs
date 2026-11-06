//! mirror: reference/engine/src/processor/intents/invader-core/stronghold/stronghold.js
//! parity: FAITHFUL
//!
//! Math.random draws come from the room stream (RNG-SITES): the deploy duration, the tower refill chance and the
//! population shuffles, in vanilla's call order.

use super::{creeps, defence};
use crate::atom;
use crate::constants as C;
use crate::doc::Atom;
use crate::live::{assign, from_val, strict_eq, to_js_string, to_number, truthy, Crash, O, V, A};
use crate::processor::common::fake_runtime::Intents;
use crate::processor::global::obj;
use crate::processor::intents::creeps::keepers::pretick::lodash_extremum;
use crate::processor::{get_or_undef, js_add, js_max, lodash_match, lodash_match_str, loose_eq, prop, room_object_list, update, RoomScope};
use crate::utils::{calc_body_effectiveness, calc_reward, dist};
use std::collections::BTreeMap;

/// The behavior context (invader-core/pretick.js:55).
pub struct Context {
    pub intents: Intents,
    pub defenders: Vec<O>,
    pub damaged_defenders: Vec<O>,
    pub hostiles: Vec<O>,
    pub towers: Vec<O>,
    pub ramparts: Vec<O>,
    pub damaged_roads: Vec<O>,
    pub core: O,
    /// `context.spots` (assignDefenders): `{50*x+y: creep id}`, ascending.
    pub spots: BTreeMap<u64, String>,
}

pub fn has_behavior(name: &str) -> bool {
    matches!(name, "deploy" | "default" | "bunker1" | "bunker2" | "bunker3" | "bunker4" | "bunker5")
}

pub fn run_behavior(name: &str, cx: &mut Context, scope: &mut RoomScope) -> Result<(), Crash> {
    match name {
        "deploy" => {
            handle_controller(cx, scope)?;
            deploy_stronghold(cx, scope)?;
        }
        "default" => {
            handle_controller(cx, scope)?;
            refill_towers(cx, scope)?;
            focus_closest(cx)?;
        }
        "bunker1" => {
            handle_controller(cx, scope)?;
            if !refill_towers(cx, scope)? {
                refill_creeps(cx)?;
            }
            focus_closest(cx)?;
        }
        "bunker2" | "bunker3" => {
            handle_controller(cx, scope)?;
            if name == "bunker2" {
                if !refill_towers(cx, scope)? {
                    refill_creeps(cx)?;
                }
            } else {
                refill_towers(cx, scope)?;
            }
            if !truthy(&cx.core.get(atom!("population"))) {
                let entry = |b: &str| obj(vec![("body", V::str(b)), ("behavior", V::str("simple-melee"))]);
                let population = if name == "bunker2" { vec![entry("weakDefender")] } else { vec![entry("fullDefender"), entry("fullDefender")] };
                let core = cx.core.clone();
                update(scope, &core, &obj(vec![("population", V::Arr(A::from_vec(population)))]))?;
            }
            maintain_population(cx, scope)?;
            focus_closest(cx)?;
        }
        "bunker4" | "bunker5" => {
            handle_controller(cx, scope)?;
            if !refill_towers(cx, scope)? {
                refill_creeps(cx)?;
            }
            if !truthy(&cx.core.get(atom!("population"))) {
                let entry = |b: &str, h: &str| obj(vec![("body", V::str(b)), ("behavior", V::str(h))]);
                let population = if name == "bunker4" {
                    let mut deck = vec![entry("fortifier", "fortifier")];
                    let defender = entry("boostedDefender", "coordinated");
                    let ranger = entry("boostedRanger", "coordinated");
                    deck.extend(std::iter::repeat_n(defender, 4));
                    deck.extend(std::iter::repeat_n(ranger, 3));
                    let shuffled = crate::policy::lodash_shuffle(scope.rng.rng(), &deck);
                    shuffled.into_iter().take(4).collect::<Vec<_>>()
                } else {
                    let melee = entry("fullBoostedMelee", "coordinated");
                    let ranger = entry("fullBoostedRanger", "coordinated");
                    let mut deck = vec![entry("fortifier", "fortifier")];
                    deck.extend(std::iter::repeat_n(melee, 7));
                    deck.extend(std::iter::repeat_n(ranger, 9));
                    let deck = crate::policy::lodash_shuffle(scope.rng.rng(), &deck);
                    let mut population = vec![entry("fortifier", "fortifier")];
                    population.extend(crate::policy::lodash_shuffle(scope.rng.rng(), &deck).into_iter().take(8));
                    population
                };
                let core = cx.core.clone();
                update(scope, &core, &obj(vec![("population", V::Arr(A::from_vec(population)))]))?;
            }
            if name == "bunker5" {
                antinuke(cx, scope)?;
            }
            cx.spots = assign_defenders(cx)?;
            maintain_population(cx, scope)?;
            towers_maintenance(cx);
            focus_max(cx, scope)?;
        }
        _ => {}
    }
    Ok(())
}

/// stronghold.js:13 range: Infinity unless both have x, y, room and share the room.
fn range(a: &V, b: &V) -> f64 {
    let undef = |o: &V, k| matches!(get_or_undef(o, k), V::Undef);
    if matches!(a, V::Undef) || matches!(b, V::Undef) || [atom!("x"), atom!("y"), atom!("room")].iter().any(|&k| undef(a, k) || undef(b, k)) {
        return f64::INFINITY;
    }
    if !loose_eq(&get_or_undef(a, atom!("room")), &get_or_undef(b, atom!("room"))) {
        return f64::INFINITY;
    }
    let c = |o: &V, k| to_number(&get_or_undef(o, k));
    js_max((c(a, atom!("x")) - c(b, atom!("x"))).abs(), (c(a, atom!("y")) - c(b, atom!("y"))).abs())
}

/// `_.some(o.body, {type})`.
fn has_part(o: &O, part: &str) -> bool {
    match o.get(atom!("body")) {
        V::Arr(b) => b.to_vec().iter().any(|p| lodash_match_str(p, atom!("type"), part)),
        _ => false,
    }
}

fn at_same_tile(list: &[O], o: &O) -> bool {
    list.iter().any(|r| {
        let rv = V::Obj(r.clone());
        lodash_match(&rv, atom!("x"), &o.get(atom!("x"))) && lodash_match(&rv, atom!("y"), &o.get(atom!("y")))
    })
}

fn deploy_stronghold(cx: &mut Context, scope: &mut RoomScope) -> Result<(), Crash> {
    let core = cx.core.clone();
    let game_time = scope.game_time as f64;
    let deploy = core.get(atom!("deployTime"));
    if !(truthy(&deploy) && to_number(&deploy) <= 1.0 + game_time) {
        return Ok(());
    }
    let duration = crate::processor::js_round(C::STRONGHOLD_DECAY_TICKS as f64 * (0.9 + scope.rng.random() * 0.2));
    let decay_time = game_time + duration;
    let collapse = || {
        obj(vec![
            ("effect", V::Num(C::EFFECT_COLLAPSE_TIMER as f64)),
            ("power", V::Num(C::EFFECT_COLLAPSE_TIMER as f64)),
            ("endTime", V::Num(game_time + duration)),
            ("duration", V::Num(duration)),
        ])
    };
    let V::Arr(effects) = core.get(atom!("effects")) else { return Err(Crash("core.effects.push is not a function".into())) };
    effects.push(collapse());
    let data = obj(vec![
        ("deployTime", V::Null),
        ("decayTime", V::Num(decay_time)),
        ("hits", V::Num(C::INVADER_CORE_HITS as f64)),
        ("hitsMax", V::Num(C::INVADER_CORE_HITS as f64)),
        ("effects", V::Arr(effects)),
    ]);
    update(scope, &core, &data)?;
    for r in &cx.ramparts {
        let id = to_js_string(&r.get(atom!("_id")));
        scope.bulk.remove(&id);
        scope.room_objects.remove(&id);
    }

    let data = from_val(crate::strongholds::data());
    let template = get_or_undef(&get_or_undef(&data, Atom::intern("templates")), Atom::intern(&to_js_string(&core.get(atom!("templateName")))));
    let reward_level = prop(&template, Atom::intern("rewardLevel"))?;
    let level_of = |table: &[u32], key: &V| {
        let k = to_js_string(key);
        k.parse::<usize>().ok().filter(|i| i.to_string() == *k).and_then(|i| table.get(i)).map_or(V::Undef, |&v| V::Num(v as f64))
    };
    let rampart_max = C::RAMPART_HITS_MAX[8].map_or(V::Undef, |v| V::Num(v as f64));
    let options_rampart = obj(vec![
        ("hits", level_of(&C::STRONGHOLD_RAMPART_HITS, &reward_level)),
        ("hitsMax", rampart_max),
        ("hitsTarget", level_of(&C::STRONGHOLD_RAMPART_HITS, &core.get(atom!("level")))),
        ("nextDecayTime", V::Num(decay_time)),
    ]);
    let options_tower = obj(vec![
        ("hits", V::Num(C::TOWER_HITS as f64)),
        ("hitsMax", V::Num(C::TOWER_HITS as f64)),
        ("store", obj(vec![("energy", V::Num(C::TOWER_CAPACITY as f64))])),
        ("storeCapacityResource", obj(vec![("energy", V::Num(C::TOWER_CAPACITY as f64))])),
        ("actionLog", obj(vec![("attack", V::Null), ("heal", V::Null), ("repair", V::Null)])),
    ]);
    let options_container = obj(vec![
        ("notifyWhenAttacked", V::Bool(false)),
        ("hits", V::Num(C::CONTAINER_HITS as f64)),
        ("hitsMax", V::Num(C::CONTAINER_HITS as f64)),
        ("nextDecayTime", V::Num(decay_time)),
        ("store", obj(vec![])),
        ("storeCapacity", V::Num(0.0)),
    ]);
    let options_road = obj(vec![
        ("notifyWhenAttacked", V::Bool(false)),
        ("hits", V::Num(C::ROAD_HITS as f64)),
        ("hitsMax", V::Num(C::ROAD_HITS as f64)),
        ("nextDecayTime", V::Num(decay_time)),
    ]);
    let options = |ty: &str| match ty {
        "rampart" => Some(&options_rampart),
        "tower" => Some(&options_tower),
        "container" => Some(&options_container),
        "road" => Some(&options_road),
        _ => None,
    };

    let mut counter = 1;
    let structures = match get_or_undef(&template, Atom::intern("structures")) {
        V::Arr(a) => a.to_vec(),
        _ => Vec::new(),
    };
    for i in structures {
        let x = js_add(&js_add(&V::Num(0.0), &core.get(atom!("x"))), &get_or_undef(&i, Atom::intern("dx")));
        let y = js_add(&js_add(&V::Num(0.0), &core.get(atom!("y"))), &get_or_undef(&i, Atom::intern("dy")));
        for key in scope.room_objects.keys() {
            let Some(o) = scope.room_objects.get(&key) else { continue };
            if truthy(&o.get(atom!("strongholdId"))) || !loose_eq(&o.get(atom!("x")), &x) || !loose_eq(&o.get(atom!("y")), &y) {
                continue;
            }
            let ty = to_js_string(&o.get(atom!("type")));
            if &*ty == "creep" || &*ty == "powerCreep" {
                crate::processor::intents::creeps::_die::die(&o, None, true, scope, None)?;
            }
            if &*ty == "constructionSite" {
                let id = to_js_string(&o.get(atom!("_id")));
                scope.room_objects.remove(&id);
                scope.bulk.remove(&id);
                let progress = o.get(atom!("progress"));
                if crate::processor::gt(&progress, &V::Num(1.0)) {
                    let room = to_js_string(&o.get(atom!("room")));
                    crate::processor::intents::_create_energy::create_energy(to_number(&o.get(atom!("x"))), to_number(&o.get(atom!("y"))), &room, (to_number(&progress) / 2.0).floor(), C::RESOURCE_ENERGY, scope)?;
                }
            }
            if ty.parse::<C::StructureType>().ok().and_then(C::construction_cost).is_some_and(|c| c > 0) {
                crate::processor::intents::structures::_destroy::destroy(&o, scope, None)?;
            }
        }
        let ty = to_js_string(&get_or_undef(&i, atom!("type")));
        if &*ty == C::STRUCTURE_INVADER_CORE {
            continue;
        }
        let s = O::new();
        if let V::Obj(io) = &i {
            assign(&s, io);
        }
        let effects = V::Arr(A::from_vec(vec![collapse()]));
        let extra = obj(vec![("x", x), ("y", y), ("room", core.get(atom!("room"))), ("strongholdId", core.get(atom!("strongholdId"))), ("decayTime", V::Num(decay_time)), ("effects", effects)]);
        if let V::Obj(e) = &extra {
            assign(&s, e);
        }
        if let Some(V::Obj(opts)) = options(&ty) {
            assign(&s, opts);
        }
        s.delete(Atom::intern("dx"));
        s.delete(Atom::intern("dy"));
        if &*ty == C::STRUCTURE_TOWER || &*ty == C::STRUCTURE_RAMPART {
            s.set(atom!("user"), core.get(atom!("user")));
        }
        if &*ty == C::STRUCTURE_CONTAINER {
            let rewards = match get_or_undef(&data, Atom::intern("containerRewards")) {
                V::Obj(o) => o,
                _ => O::new(),
            };
            let amount = to_number(&get_or_undef(&get_or_undef(&data, Atom::intern("containerAmounts")), Atom::intern(&to_js_string(&reward_level))));
            s.set(atom!("store"), calc_reward(&rewards, amount, Some(3.0), scope.rng.rng()));
        }
        scope.bulk.insert(&V::Obj(s.clone()), None);
        scope.room_objects.insert(format!("deployedStructure{counter}").into(), s);
        counter += 1;
    }
    Ok(())
}

fn handle_controller(cx: &Context, scope: &RoomScope) -> Result<(), Crash> {
    let Some(rc) = &scope.room_controller else { return Ok(()) };
    let core = &cx.core;
    let id = core.get(atom!("_id"));
    let game_time = scope.game_time as f64;
    let rc_id = obj(vec![("id", rc.get(atom!("_id")))]);
    if strict_eq(&rc.get(atom!("user")), &core.get(atom!("user"))) {
        if to_number(&rc.get(atom!("downgradeTime"))) - game_time < (C::INVADER_CORE_CONTROLLER_DOWNGRADE - 25) as f64 {
            cx.intents.set(&id, "upgradeController", rc_id);
        }
    } else {
        let reservation = rc.get(atom!("reservation"));
        if !truthy(&reservation) || strict_eq(&prop(&reservation, atom!("user"))?, &core.get(atom!("user"))) {
            cx.intents.set(&id, "reserveController", rc_id);
        } else {
            cx.intents.set(&id, "attackController", rc_id);
        }
    }
    Ok(())
}

fn energy_of(o: &O) -> Result<V, Crash> {
    prop(&prop(&V::Obj(o.clone()), atom!("store"))?, atom!("energy"))
}

fn refill_towers(cx: &Context, scope: &mut RoomScope) -> Result<bool, Crash> {
    const CHANCE: [f64; 6] = [0.0, 0.01, 0.1, 0.3, 1.0, 1.0];
    let level = to_js_string(&cx.core.get(atom!("level")));
    let chance = level.parse::<usize>().ok().filter(|i| i.to_string() == *level).and_then(|i| CHANCE.get(i).copied());
    let draw = scope.rng.random();
    if chance.is_some_and(|c| c < draw) {
        return Ok(false);
    }
    let mut under = Vec::new();
    for t in &cx.towers {
        if crate::processor::le(&energy_of(t)?, &V::Num(2.0 * C::TOWER_ENERGY_COST as f64)) && at_same_tile(&cx.ramparts, t) {
            under.push(t.clone());
        }
    }
    if under.is_empty() {
        return Ok(false);
    }
    let t = lodash_extremum(&under, |o| to_number(&get_or_undef(&o.get(atom!("store")), atom!("energy"))), false);
    if !truthy(&t) {
        return Ok(false);
    }
    let capacity = prop(&prop(&t, atom!("storeCapacityResource"))?, atom!("energy"))?;
    let amount = to_number(&capacity) - to_number(&prop(&prop(&t, atom!("store"))?, atom!("energy"))?);
    let intent = obj(vec![("id", get_or_undef(&t, atom!("_id"))), ("amount", V::Num(amount)), ("resourceType", V::str(C::RESOURCE_ENERGY))]);
    cx.intents.set(&cx.core.get(atom!("_id")), "transfer", intent);
    Ok(true)
}

fn refill_creeps(cx: &Context) -> Result<bool, Crash> {
    let mut under = Vec::new();
    for c in &cx.defenders {
        let cap = c.get(atom!("storeCapacity"));
        if crate::processor::gt(&cap, &V::Num(0.0)) && 2.0 * to_number(&energy_of(c)?) <= to_number(&cap) {
            under.push(c.clone());
        }
    }
    if under.is_empty() {
        return Ok(false);
    }
    let c = lodash_extremum(&under, |o| to_number(&get_or_undef(&o.get(atom!("store")), atom!("energy"))), false);
    if !truthy(&c) {
        return Ok(false);
    }
    let amount = to_number(&prop(&c, atom!("storeCapacity"))?) - to_number(&prop(&prop(&c, atom!("store"))?, atom!("energy"))?);
    let intent = obj(vec![("id", get_or_undef(&c, atom!("_id"))), ("amount", V::Num(amount)), ("resourceType", V::str(C::RESOURCE_ENERGY))]);
    cx.intents.set(&cx.core.get(atom!("_id")), "transfer", intent);
    Ok(true)
}

fn towers_maintenance(cx: &mut Context) {
    if cx.towers.is_empty() || (cx.damaged_defenders.is_empty() && cx.damaged_roads.is_empty()) {
        return;
    }
    if let Some(creep) = cx.damaged_defenders.iter().find(|d| at_same_tile(&cx.ramparts, d)) {
        let tower = cx.towers[0].clone();
        cx.intents.set(&tower.get(atom!("_id")), "heal", obj(vec![("id", creep.get(atom!("_id")))]));
        cx.towers.retain(|t| !t.same(&tower));
        return;
    }
    if cx.damaged_roads.iter().any(|r| at_same_tile(&cx.ramparts, r)) {
        // vanilla repairs `_.first(damagedRoads)`, not the protected one
        let road = cx.damaged_roads[0].clone();
        let tower = cx.towers[0].clone();
        cx.intents.set(&tower.get(atom!("_id")), "repair", obj(vec![("id", road.get(atom!("_id")))]));
        cx.towers.retain(|t| !t.same(&tower));
    }
}

/// Melee defenders next to `target` attack it; rangers within 3 shoot (mass attack when adjacent).
fn defenders_engage(cx: &Context, target: &V) {
    for d in &cx.defenders {
        if range(&V::Obj(d.clone()), target) == 1.0 && has_part(d, C::ATTACK) {
            cx.intents.set(&d.get(atom!("_id")), "attack", obj(vec![("id", get_or_undef(target, atom!("_id"))), ("x", get_or_undef(target, atom!("x"))), ("y", get_or_undef(target, atom!("y")))]));
        }
    }
    for r in &cx.defenders {
        let d = range(&V::Obj(r.clone()), target);
        if d <= 3.0 && has_part(r, C::RANGED_ATTACK) {
            if d == 1.0 {
                cx.intents.set(&r.get(atom!("_id")), "rangedMassAttack", obj(vec![]));
            } else {
                cx.intents.set(&r.get(atom!("_id")), "rangedAttack", obj(vec![("id", get_or_undef(target, atom!("_id")))]));
            }
        }
    }
}

fn focus_closest(cx: &Context) -> Result<bool, Crash> {
    if cx.hostiles.is_empty() {
        return Ok(false);
    }
    let core = V::Obj(cx.core.clone());
    let target = lodash_extremum(&cx.hostiles, |c| dist(&V::Obj(c.clone()), &core), false);
    if !truthy(&target) {
        return Ok(false);
    }
    for t in &cx.towers {
        cx.intents.set(&t.get(atom!("_id")), "attack", obj(vec![("id", get_or_undef(&target, atom!("_id")))]));
    }
    defenders_engage(cx, &target);
    Ok(true)
}

fn focus_max(cx: &Context, scope: &RoomScope) -> Result<bool, Crash> {
    if cx.hostiles.is_empty() {
        return Ok(false);
    }
    let game_time = scope.game_time as f64;
    let mut active = Vec::new();
    for t in &cx.towers {
        if crate::processor::ge(&energy_of(t)?, &V::Num(C::TOWER_ENERGY_COST as f64)) {
            active.push(t.clone());
        }
    }
    let tower_damage = |creep: &V, tower: &O| -> f64 {
        let mut r = dist(creep, &V::Obj(tower.clone()));
        let mut amount = C::TOWER_POWER_ATTACK as f64;
        if r > C::TOWER_OPTIMAL_RANGE as f64 {
            if r > C::TOWER_FALLOFF_RANGE as f64 {
                r = C::TOWER_FALLOFF_RANGE as f64;
            }
            amount -= amount * C::TOWER_FALLOFF * (r - C::TOWER_OPTIMAL_RANGE as f64) / (C::TOWER_FALLOFF_RANGE - C::TOWER_OPTIMAL_RANGE) as f64;
        }
        for power in [C::PWR_OPERATE_TOWER, C::PWR_DISRUPT_TOWER] {
            let p = V::Num(power as f64);
            if let Some(effect) = crate::processor::lodash_find(&tower.get(atom!("effects")), |e| lodash_match(e, atom!("power"), &p)) {
                if crate::processor::gt(&get_or_undef(&effect, atom!("endTime")), &V::Num(game_time)) {
                    let i = to_number(&get_or_undef(&effect, atom!("level"))) - 1.0;
                    let m = C::power_info(power).and_then(|pi| pi.effect).filter(|_| (0.0..5.0).contains(&i) && i.fract() == 0.0).map_or(f64::NAN, |e| e[i as usize]);
                    amount *= m;
                }
            }
        }
        amount.floor()
    };
    // lodash arraySum: from the last element, `+x || 0`
    let sum = |xs: Vec<f64>| xs.into_iter().rev().fold(0.0, |a, x| a + if x.is_nan() { 0.0 } else { x });
    let target = lodash_extremum(
        &cx.hostiles,
        |creep| {
            let cv = V::Obj(creep.clone());
            let mut damage = sum(active.iter().map(|t| tower_damage(&cv, t)).collect());
            damage += sum(cx
                .defenders
                .iter()
                .map(|d| {
                    let dv = V::Obj(d.clone());
                    let mut x = 0.0;
                    if range(&dv, &cv) <= 3.0 && has_part(d, C::RANGED_ATTACK) {
                        x += calc_body_effectiveness(&d.get(atom!("body")), C::RANGED_ATTACK, "rangedAttack", C::RANGED_ATTACK_POWER as f64, false).unwrap_or(f64::NAN);
                    }
                    if range(&dv, &cv) <= 1.0 && has_part(d, C::ATTACK) {
                        x += calc_body_effectiveness(&d.get(atom!("body")), C::ATTACK, "attack", C::ATTACK_POWER as f64, false).unwrap_or(f64::NAN);
                    }
                    x
                })
                .collect());
            damage
        },
        true,
    );
    defenders_engage(cx, &target);
    for t in &active {
        cx.intents.set(&t.get(atom!("_id")), "attack", obj(vec![("id", get_or_undef(&target, atom!("_id")))]));
    }
    Ok(true)
}

fn maintain_population(cx: &mut Context, scope: &mut RoomScope) -> Result<(), Crash> {
    // `for (let i in core.population)`: an array, or the object bulk.update made of one (`id[key] || {}` merged
    // with the array), both enumerate index keys ascending
    let population: Vec<(String, V)> = match cx.core.get(atom!("population")) {
        V::Arr(a) => a.to_vec().into_iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
        V::Obj(o) => o.keys().into_iter().map(|k| (k.as_str().to_string(), o.get(k))).collect(),
        _ => return Ok(()),
    };
    for (i, entry) in population.iter() {
        let name = V::str(&format!("defender{i}"));
        let body_name = to_js_string(&get_or_undef(entry, atom!("body")));
        let behavior = to_js_string(&get_or_undef(entry, Atom::intern("behavior")));
        // _.find(defenders, {name})
        let creep = cx.defenders.iter().find(|d| lodash_match(&V::Obj((*d).clone()), atom!("name"), &name)).cloned();
        if let Some(creep) = creep {
            if creeps::has_behavior(&behavior) {
                creeps::run_behavior(&behavior, &creep, cx, scope)?;
                continue;
            }
        }
        let spawning = Atom::intern("_spawning");
        if !truthy(&cx.core.get(atom!("spawning"))) && !truthy(&cx.core.get(spawning)) {
            let Some((body, boosts)) = creeps::body(&body_name) else { return Err(Crash("Cannot read properties of undefined (reading 'body')".into())) };
            let body = V::Arr(A::from_vec(body.into_iter().map(V::str).collect()));
            let boosts = V::Arr(A::from_vec(boosts.into_iter().map(|b| b.map_or(V::Undef, V::str)).collect()));
            cx.intents.set(&cx.core.get(atom!("_id")), "createCreep", obj(vec![("name", name), ("body", body), ("boosts", boosts)]));
            cx.core.set(spawning, V::Bool(true));
        }
    }
    Ok(())
}

fn antinuke(cx: &Context, scope: &mut RoomScope) -> Result<(), Crash> {
    if scope.game_time % 10 != 0 {
        return Ok(());
    }
    let objects = room_object_list(scope);
    let nukes: Vec<O> = objects.iter().filter(|o| o.is_str(atom!("type"), "nuke")).cloned().collect();
    let level = to_js_string(&cx.core.get(atom!("level")));
    let base = level.parse::<usize>().ok().filter(|i| i.to_string() == *level).and_then(|i| C::STRONGHOLD_RAMPART_HITS.get(i)).map_or(f64::NAN, |&v| v as f64);
    let containers: Vec<O> = objects.iter().filter(|o| o.is_str(atom!("type"), C::STRUCTURE_CONTAINER)).cloned().collect();
    for rampart in cx.ramparts.clone() {
        if at_same_tile(&containers, &rampart) {
            continue;
        }
        let rv = V::Obj(rampart.clone());
        let mut hits_target = base;
        for n in &nukes {
            let r = dist(&rv, &V::Obj(n.clone()));
            if r == 0.0 {
                hits_target += C::NUKE_DAMAGE[0].unwrap_or(0) as f64;
                continue;
            }
            if r <= 2.0 {
                hits_target += C::NUKE_DAMAGE[2].unwrap_or(0) as f64;
            }
        }
        if !loose_eq(&rampart.get(atom!("hitsTarget")), &V::Num(hits_target)) {
            update(scope, &rampart, &obj(vec![("hitsTarget", V::Num(hits_target))]))?;
        }
    }
    Ok(())
}

fn assign_defenders(cx: &Context) -> Result<BTreeMap<u64, String>, Crash> {
    let (mut melee_spots, mut ranger_spots): (Vec<O>, Vec<O>) = (Vec::new(), Vec::new());
    for h in &cx.hostiles {
        let hv = V::Obj(h.clone());
        melee_spots.extend(cx.ramparts.iter().filter(|r| dist(&hv, &V::Obj((*r).clone())) <= 1.0).cloned());
        ranger_spots.extend(cx.ramparts.iter().filter(|r| dist(&hv, &V::Obj((*r).clone())) <= 3.0).cloned());
    }
    let unique = |list: Vec<O>| -> Vec<O> {
        let mut out: Vec<O> = Vec::new();
        for o in list {
            if !out.iter().any(|x| x.same(&o)) {
                out.push(o);
            }
        }
        out
    };
    let melee_spots = unique(melee_spots);
    let ranger_spots = unique(ranger_spots.into_iter().filter(|r| !melee_spots.iter().any(|m| m.same(r))).collect());
    let (mut rangers, mut melees) = (Vec::new(), Vec::new());
    for d in &cx.defenders {
        if has_part(d, C::ATTACK) {
            melees.push(to_js_string(&d.get(atom!("_id"))).to_string());
        }
        if has_part(d, C::RANGED_ATTACK) {
            rangers.push(to_js_string(&d.get(atom!("_id"))).to_string());
        }
    }
    let mut spots = BTreeMap::new();
    if !melee_spots.is_empty() && !melees.is_empty() {
        spots = defence::distribute(&melee_spots, &melees)?;
    }
    if !ranger_spots.is_empty() && !rangers.is_empty() {
        spots.extend(defence::distribute(&ranger_spots, &rangers)?);
    }
    Ok(spots)
}
