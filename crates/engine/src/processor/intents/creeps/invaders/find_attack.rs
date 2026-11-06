//! mirror: reference/engine/src/processor/intents/creeps/invaders/findAttack.js
//! parity: FAITHFUL

use super::{flee, pretick::Context};
use crate::atom;
use crate::constants as C;
use crate::live::{to_js_string, to_number, truthy, Crash, O, V};
use crate::processor::common::fake_runtime::{find_closest_by_path, find_path, has_active_bodyparts, move_to, position, unpack_local, Opts, Target};
use crate::processor::global::obj;
use crate::processor::{eq_str, get_or_undef, loose_eq, lt, RoomScope};
use crate::utils::dist;

/// findAttack.js:8 checkPath: does the path end exactly on `pos`?
fn check_path(from: &O, pos: &V, scope: &RoomScope) -> Result<bool, Crash> {
    let path = find_path(from, &Target::Pos(pos.clone()), &Opts::default(), scope)?;
    let Some(last) = path.last() else { return Ok(false) };
    Ok(loose_eq(&get_or_undef(last, atom!("x")), &get_or_undef(pos, atom!("x"))) && loose_eq(&get_or_undef(last, atom!("y")), &get_or_undef(pos, atom!("y"))))
}

pub fn run(creep: &O, cx: &mut Context, scope: &mut RoomScope) -> Result<(), Crash> {
    let cv = V::Obj(creep.clone());
    let id = creep.get(atom!("_id"));
    let set_move = |cx: &Context, d: f64| {
        if d != 0.0 {
            cx.intents.set(&id, "move", obj(vec![("direction", V::Num(d))]));
        }
    };
    let have_attack = has_active_bodyparts(creep, C::ATTACK);
    if !have_attack && has_active_bodyparts(creep, C::RANGED_ATTACK) && flee::run(creep, 3.0, cx, scope)? {
        return Ok(());
    }

    let mut target: Option<O> = None;
    if lt(&creep.get(atom!("hits")), &V::Num(to_number(&creep.get(atom!("hitsMax"))) / 2.0)) && !cx.healers.is_empty() && !have_attack {
        target = find_closest_by_path(creep, &cx.healers, Opts { ignore_roads: true, ..Opts::default() }, scope)?;
        if let Some(t) = target.clone() {
            let d = move_to(creep, &t, Opts { ignore_roads: true, ..Opts::default() }, scope)?;
            if d != 0.0 {
                set_move(cx, d);
            } else {
                target = None;
            }
        }
    }

    if have_attack {
        if let Some(near) = cx.hostiles.iter().find(|c| dist(&cv, &V::Obj((*c).clone())) <= 1.0) {
            cx.intents.set(&id, "attack", obj(vec![("id", near.get(atom!("_id"))), ("x", near.get(atom!("x"))), ("y", near.get(atom!("y")))]));
        }
    }

    let far_or_melee = |t: &O| have_attack || dist(&cv, &V::Obj(t.clone())) > 3.0;
    let hostiles = cx.hostiles.clone();
    let attempts = [
        Opts { ignore_roads: true, ignore_creeps: true, ..Opts::default() },
        Opts { ignore_roads: true, zero_tiles: Some(cx.fortifications.clone()), ..Opts::default() },
        Opts { ignore_destructible: true, ignore_roads: true, ..Opts::default() },
    ];
    for opts in attempts {
        if target.is_some() {
            break;
        }
        target = find_closest_by_path(creep, &hostiles, opts.clone(), scope)?;
        if let Some(t) = target.clone().filter(|t| far_or_melee(t)) {
            let d = move_to(creep, &t, opts, scope)?;
            set_move(cx, d);
        }
    }

    let Some(target) = target else {
        let mut unreachable = Vec::new();
        for key in scope.room_objects.keys() {
            let Some(o) = scope.room_objects.get(&key) else { continue };
            if eq_str(&o.get(atom!("type")), "spawn") && !check_path(creep, &position(&o.get(atom!("x")), &o.get(atom!("y")), &o.get(atom!("room")))?, scope)? {
                unreachable.push(o);
            }
        }
        if unreachable.is_empty() && scope.room_controller.as_ref().is_some_and(|c| truthy(&c.get(atom!("user")))) {
            cx.intents.set(&id, "suicide", obj(vec![]));
            return Ok(());
        }
        if let Some(t) = unreachable.first() {
            let d = move_to(creep, t, Opts { ignore_destructible: true, ignore_roads: true, ..Opts::default() }, scope)?;
            set_move(cx, d);
        }
        return Ok(());
    };

    cx.intents.set(&id, "attack", obj(vec![("id", target.get(atom!("_id"))), ("x", target.get(atom!("x"))), ("y", target.get(atom!("y")))]));
    let mm = creep.get(atom!("memory_move"));
    let path = get_or_undef(&mm, atom!("path"));
    if (have_attack || has_active_bodyparts(creep, C::WORK)) && truthy(&mm) && truthy(&path) {
        let units: Vec<u16> = to_js_string(&path).encode_utf16().collect();
        let Some(&first) = units.first() else { return Ok(()) };
        let pos = unpack_local(first, &creep.get(atom!("room")))?;
        let structures: Vec<O> = scope
            .room_objects
            .keys()
            .into_iter()
            .filter_map(|k| scope.room_objects.get(&k))
            .filter(|o| {
                let ty = to_js_string(&o.get(atom!("type")));
                ty.parse::<C::StructureType>().is_ok_and(|t| C::CONTROLLER_STRUCTURES.iter().any(|(s, _)| *s == t))
                    && &*ty != "spawn"
                    && loose_eq(&o.get(atom!("x")), &get_or_undef(&pos, atom!("x")))
                    && loose_eq(&o.get(atom!("y")), &get_or_undef(&pos, atom!("y")))
            })
            .collect();
        if let Some(s) = structures.first() {
            if has_active_bodyparts(creep, C::RANGED_ATTACK) {
                cx.intents.set(&id, "rangedAttack", obj(vec![("id", s.get(atom!("_id")))]));
            }
            if has_active_bodyparts(creep, C::WORK) {
                cx.intents.set(&id, "dismantle", obj(vec![("id", s.get(atom!("_id")))]));
            } else {
                cx.intents.set(&id, "attack", obj(vec![("id", s.get(atom!("_id"))), ("x", s.get(atom!("x"))), ("y", s.get(atom!("y")))]));
            }
        }
    }
    Ok(())
}
