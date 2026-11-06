//! mirror: reference/engine/src/processor/intents/creeps/invaders/healer.js
//! parity: FAITHFUL

use super::{flee, pretick::Context};
use crate::atom;
use crate::constants as C;
use crate::live::{to_number, Crash, O, V};
use crate::processor::common::fake_runtime::{find_closest_by_path, has_active_bodyparts, move_to, Opts};
use crate::processor::global::obj;
use crate::processor::{lt, RoomScope};
use crate::utils::{dist, get_direction};

pub fn run(creep: &O, cx: &mut Context, scope: &mut RoomScope) -> Result<(), Crash> {
    let cv = V::Obj(creep.clone());
    let id = creep.get(atom!("_id"));
    let mut heal_targets: Vec<O> = cx.invaders.iter().filter(|c| dist(&V::Obj((*c).clone()), &cv) <= 3.0).cloned().collect();
    if !heal_targets.is_empty() {
        // stable sort by damage, most damaged first (a NaN difference compares equal)
        let damage = |c: &O| to_number(&c.get(atom!("hitsMax"))) - to_number(&c.get(atom!("hits")));
        heal_targets.sort_by(|a, b| (damage(b) - damage(a)).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal));
        let t = &heal_targets[0];
        if dist(&cv, &V::Obj(t.clone())) <= 1.0 {
            cx.intents.set(&id, "heal", obj(vec![("id", t.get(atom!("_id"))), ("x", t.get(atom!("x"))), ("y", t.get(atom!("y")))]));
        } else {
            cx.intents.set(&id, "rangedHeal", obj(vec![("id", t.get(atom!("_id")))]));
        }
    }

    if lt(&creep.get(atom!("hits")), &V::Num(to_number(&creep.get(atom!("hitsMax"))) / 2.0)) {
        if !flee::run(creep, 4.0, cx, scope)? {
            let others: Vec<O> = cx.invaders.iter().filter(|c| !c.same(creep) && has_active_bodyparts(c, C::HEAL)).cloned().collect();
            if let Some(t) = find_closest_by_path(creep, &others, Opts::default(), scope)? {
                let direction = move_to(creep, &t, Opts { range: Some(1.0), ..Opts::default() }, scope)?;
                if direction != 0.0 {
                    cx.intents.set(&id, "move", obj(vec![("direction", V::Num(direction))]));
                }
            }
        }
        return Ok(());
    }

    let damaged: Vec<O> = cx.invaders.iter().filter(|c| lt(&c.get(atom!("hits")), &c.get(atom!("hitsMax")))).cloned().collect();
    let mut target = find_closest_by_path(creep, &damaged, Opts::default(), scope)?;
    if target.is_none() {
        if flee::run(creep, 4.0, cx, scope)? {
            return Ok(());
        }
        let others: Vec<O> = cx.invaders.iter().filter(|c| !c.same(creep) && !has_active_bodyparts(c, C::HEAL)).cloned().collect();
        target = find_closest_by_path(creep, &others, Opts::default(), scope)?;
    }
    let Some(target) = target else {
        cx.intents.set(&id, "suicide", obj(vec![]));
        return Ok(());
    };
    let direction = if dist(&cv, &V::Obj(target.clone())) <= 1.0 {
        let c = |o: &O, k| to_number(&o.get(k));
        get_direction(c(&target, atom!("x")) - c(creep, atom!("x")), c(&target, atom!("y")) - c(creep, atom!("y"))).map_or(0.0, f64::from)
    } else {
        move_to(creep, &target, Opts { range: Some(1.0), ..Opts::default() }, scope)?
    };
    if direction != 0.0 {
        cx.intents.set(&id, "move", obj(vec![("direction", V::Num(direction))]));
    }
    Ok(())
}
