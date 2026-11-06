//! mirror: reference/engine/src/processor/intents/invader-core/stronghold/creeps.js
//! parity: FAITHFUL

use super::{defence, stronghold::Context};
use crate::atom;
use crate::constants as C;
use crate::live::{to_number, truthy, Crash, O, V};
use crate::processor::common::fake_runtime::{find_closest_by_path, flee, walk_to, Opts};
use crate::processor::global::obj;
use crate::processor::{lt, RoomScope};
use crate::utils::dist;

/// `bodies[name]` (makeBody): `(body, boosts)`, boosts `undefined` where none.
pub fn body(name: &str) -> Option<(Vec<&'static str>, Vec<Option<&'static str>>)> {
    let segments: &[(&str, usize, Option<&str>)] = match name {
        "fortifier" => &[(C::WORK, 15, Some("XLH2O")), (C::CARRY, 15, None), (C::MOVE, 15, None)],
        "weakDefender" => &[(C::ATTACK, 15, None), (C::MOVE, 15, None)],
        "fullDefender" => &[(C::ATTACK, 25, None), (C::MOVE, 25, None)],
        "boostedDefender" => &[(C::ATTACK, 25, Some("UH2O")), (C::MOVE, 25, None)],
        "boostedRanger" => &[(C::RANGED_ATTACK, 25, Some("KHO2")), (C::MOVE, 25, None)],
        "fullBoostedMelee" => &[(C::ATTACK, 44, Some("XUH2O")), (C::MOVE, 6, Some("XZHO2"))],
        "fullBoostedRanger" => &[(C::RANGED_ATTACK, 44, Some("XKHO2")), (C::MOVE, 6, Some("XZHO2"))],
        _ => return None,
    };
    let (mut body, mut boosts) = (Vec::new(), Vec::new());
    for &(part, count, boost) in segments {
        for _ in 0..count {
            body.push(part);
            boosts.push(boost);
        }
    }
    Some((body, boosts))
}

/// `behaviors[name]` exists.
pub fn has_behavior(name: &str) -> bool {
    matches!(name, "coordinated" | "simple-melee" | "fortifier")
}

pub fn run_behavior(name: &str, creep: &O, cx: &Context, scope: &mut RoomScope) -> Result<(), Crash> {
    match name {
        "coordinated" => coordinated(cx, scope),
        "simple-melee" => simple_melee(creep, cx, scope),
        "fortifier" => fortifier(creep, cx, scope),
        _ => Ok(()),
    }
}

fn opts(range: Option<f64>, safe: &Option<std::rc::Rc<Vec<u8>>>) -> Opts {
    Opts { range, replace_matrix: safe.clone(), ..Opts::default() }
}

/// coordinatedDefender: every assigned spot's creep walks there; with the core spawning, a defender next to it
/// steps away.
fn coordinated(cx: &Context, scope: &mut RoomScope) -> Result<(), Crash> {
    let safe = defence::create_safe_matrix(cx, scope);
    let mut creeps = cx.defenders.clone();
    for (&spot, id) in &cx.spots {
        let creep = scope.room_objects.get(id);
        if let Some(c) = &creep {
            creeps.retain(|d| !d.same(c));
        }
        let Some(creep) = creep else { continue };
        if 50.0 * to_number(&creep.get(atom!("x"))) + to_number(&creep.get(atom!("y"))) == spot as f64 {
            continue;
        }
        let target = O::new();
        target.set(atom!("x"), V::Num((spot / 50) as f64));
        target.set(atom!("y"), V::Num((spot % 50) as f64));
        target.set(atom!("room"), creep.get(atom!("room")));
        walk_to(&creep, &target, opts(Some(0.0), &safe), &cx.intents, scope)?;
    }
    if truthy(&cx.core.get(atom!("spawning"))) {
        let core = V::Obj(cx.core.clone());
        for creep in &creeps {
            if dist(&V::Obj(creep.clone()), &core) == 1.0 {
                // fakeRuntime.flee ignores its options (and so the safe matrix)
                let direction = flee(creep, std::slice::from_ref(&cx.core), 2.0, scope)?;
                if direction != 0.0 {
                    cx.intents.set(&creep.get(atom!("_id")), "move", obj(vec![("direction", V::Num(direction))]));
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

fn simple_melee(creep: &O, cx: &Context, scope: &mut RoomScope) -> Result<(), Crash> {
    if cx.hostiles.is_empty() {
        return Ok(());
    }
    let safe = defence::create_safe_matrix(cx, scope);
    let Some(target) = find_closest_by_path(creep, &cx.hostiles, opts(None, &safe), scope)? else { return Ok(()) };
    if dist(&V::Obj(creep.clone()), &V::Obj(target.clone())) <= 1.0 {
        cx.intents.set(&creep.get(atom!("_id")), "attack", obj(vec![("id", target.get(atom!("_id"))), ("x", target.get(atom!("x"))), ("y", target.get(atom!("y")))]));
    } else {
        walk_to(creep, &target, opts(None, &safe), &cx.intents, scope)?;
    }
    Ok(())
}

fn fortifier(creep: &O, cx: &Context, scope: &mut RoomScope) -> Result<(), Crash> {
    let energy = crate::processor::prop(&creep.get(atom!("store")), atom!("energy"))?;
    if !truthy(&energy) || cx.ramparts.is_empty() {
        return Ok(());
    }
    let repair: Vec<O> = cx.ramparts.iter().filter(|r| truthy(&r.get(atom!("hitsTarget"))) && lt(&r.get(atom!("hits")), &r.get(atom!("hitsTarget")))).cloned().collect();
    let Some(target) = repair.first().cloned() else { return Ok(()) };
    let cv = V::Obj(creep.clone());
    let id = creep.get(atom!("_id"));
    let repair_intent = |t: &O| obj(vec![("id", t.get(atom!("_id"))), ("x", t.get(atom!("x"))), ("y", t.get(atom!("y")))]);
    if dist(&cv, &V::Obj(target.clone())) <= 3.0 {
        cx.intents.set(&id, "repair", repair_intent(&target));
        return Ok(());
    }
    let safe = defence::create_safe_matrix(cx, scope);
    walk_to(creep, &target, opts(Some(3.0), &safe), &cx.intents, scope)?;
    if let Some(t) = repair.iter().find(|r| dist(&cv, &V::Obj((*r).clone())) <= 3.0) {
        cx.intents.set(&id, "repair", repair_intent(t));
    }
    Ok(())
}
