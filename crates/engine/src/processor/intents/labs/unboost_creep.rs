//! mirror: reference/engine/src/processor/intents/labs/unboost-creep.js
//! parity: FAITHFUL

use super::run_reaction::cooling_down;
use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::intents::_create_energy::create_energy;
use crate::processor::intents::creeps::_recalc_body::recalc_body;
use crate::processor::RoomScope;

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    let game_time = scope.game_time as f64;

    if cooling_down(object, scope) {
        return Ok(());
    }

    let Some(target) = by_key(scope, &get(intent, atom!("id"))?) else { return Ok(()) };
    if !target.eq_str(atom!("type"), "creep") || !loose_eq(&target.get(atom!("user")), &object.get(atom!("user"))) {
        return Ok(());
    }
    if !check_structure_against_controller(object, scope)? {
        return Ok(());
    }
    if (target.num(atom!("x")) - object.num(atom!("x"))).abs() > 1.0 || (target.num(atom!("y")) - object.num(atom!("y"))).abs() > 1.0 {
        return Ok(());
    }
    // `_.mapValues(_.groupBy(_.filter(target.body, p => !!p.boost), 'boost'), v => v.length)`
    let boosted_parts = O::new();
    for p in values(&target.get(atom!("body"))) {
        let boost = peek(&p, atom!("boost"));
        if truthy(&boost) {
            let k = key(&boost);
            boosted_parts.set(k, n(boosted_parts.num(k).max(0.0) + 1.0));
        }
    }
    if boosted_parts.is_empty() {
        return Ok(());
    }

    for p in values(&target.get(atom!("body"))) {
        put(&p, atom!("boost"), V::Null)?;
    }
    recalc_body(&target)?;
    update(&mut scope.bulk, &target, vec![(atom!("body"), target.get(atom!("body"))), (atom!("storeCapacity"), target.get(atom!("storeCapacity")))])?;

    let mut cooldown = 0.0;
    for r in C::RESOURCES_ALL {
        let count = boosted_parts.get(crate::doc::Atom::intern(r.as_str()));
        if !truthy(&count) {
            continue;
        }
        let count = to_number(&count);
        let room = to_js_string(&target.get(atom!("room")));

        let energy_return = count * C::LAB_UNBOOST_ENERGY as f64;
        if energy_return > 0.0 {
            create_energy(target.num(atom!("x")), target.num(atom!("y")), &room, energy_return, C::RESOURCE_ENERGY, scope)?;
        }

        let mineral_return = count * C::LAB_UNBOOST_MINERAL as f64;
        if mineral_return > 0.0 {
            create_energy(target.num(atom!("x")), target.num(atom!("y")), &room, mineral_return, r.as_str(), scope)?;
        }

        cooldown += count * calc_total_reactions_time(*r) * C::LAB_UNBOOST_MINERAL as f64 / C::LAB_REACTION_AMOUNT as f64;
    }

    if cooldown > 0.0 {
        update(&mut scope.bulk, object, vec![(atom!("cooldownTime"), n(cooldown + game_time))])?;
    }
    Ok(())
}
