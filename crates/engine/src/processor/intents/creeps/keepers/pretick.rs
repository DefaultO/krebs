//! mirror: reference/engine/src/processor/intents/creeps/keepers/pretick.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{to_js_string, truthy, Crash, O, V};
use crate::processor::common::fake_runtime::{move_to, Intents, Opts};
use crate::processor::global::obj;
use crate::processor::{eq_str, get_or_undef, loose_eq, update, RoomScope};
use crate::utils::dist;

/// Returns `{objectId: intents}` (PROC-NPC-PRETICK).
pub fn pretick(creep: &O, scope: &mut RoomScope) -> Result<V, Crash> {
    let cv = V::Obj(creep.clone());
    let intents = Intents::new();
    let (mut resources, mut melee, mut ranged) = (Vec::new(), Vec::new(), Vec::new());
    for key in scope.room_objects.keys() {
        let Some(o) = scope.room_objects.get(&key) else { continue };
        let ty = o.get(atom!("type"));
        if eq_str(&ty, "source") || eq_str(&ty, "mineral") {
            resources.push(o.clone());
        }
        let user = o.get(atom!("user"));
        if (eq_str(&ty, "creep") || eq_str(&ty, "powerCreep")) && !loose_eq(&user, &V::Num(2.0)) && !loose_eq(&user, &V::Num(3.0)) {
            let d = dist(&cv, &V::Obj(o.clone()));
            if d <= 1.0 {
                melee.push(o.clone());
            }
            if d <= 3.0 {
                ranged.push(o);
            }
        }
    }

    let mut source = None;
    let memory_source = creep.get(atom!("memory_sourceId"));
    if truthy(&memory_source) {
        source = scope.room_objects.get(&to_js_string(&memory_source));
    }
    if source.is_none() {
        source = resources.iter().find(|o| dist(&cv, &V::Obj((*o).clone())) <= 5.0).cloned();
        if let Some(s) = &source {
            update(scope, creep, &obj(vec![("memory_sourceId", V::Str(to_js_string(&s.get(atom!("_id")))))]))?;
        }
    }
    if let Some(s) = &source {
        if dist(&V::Obj(s.clone()), &cv) > 1.0 {
            let direction = move_to(creep, s, Opts { range: Some(1.0), reuse_path: Some(50.0), ..Opts::default() }, scope)?;
            if direction > 0.0 {
                intents.set(&creep.get(atom!("_id")), "move", obj(vec![("direction", V::Num(direction))]));
            } else {
                update(scope, creep, &obj(vec![("memory_move", V::Null)]))?;
            }
        }
    }

    // _.min of an empty list is Infinity (truthy): vanilla files an attack with an undefined target then
    let target = lodash_min_by(&melee, atom!("hits"));
    if truthy(&target) {
        let g = |k| get_or_undef(&target, k);
        intents.set(&creep.get(atom!("_id")), "attack", obj(vec![("id", g(atom!("_id"))), ("x", g(atom!("x"))), ("y", g(atom!("y")))]));
    }

    if !ranged.is_empty() {
        // damageByRange[dist]: dist >= 4 cannot be in range 3
        let damage_by_range = [10.0, 10.0, 4.0, 1.0];
        // _.sum(..., fn): lodash arraySum adds from the last element, `+x || 0`
        let mass: f64 = ranged.iter().rev().map(|c| damage_by_range.get(dist(&cv, &V::Obj(c.clone())) as usize).copied().unwrap_or(0.0)).fold(0.0, |a, b| a + b);
        if mass > 13.0 {
            intents.set(&creep.get(atom!("_id")), "rangedMassAttack", obj(vec![]));
        } else {
            let target = lodash_min_by(&ranged, atom!("hits"));
            intents.set(&creep.get(atom!("_id")), "rangedAttack", obj(vec![("id", get_or_undef(&target, atom!("_id")))]));
        }
    }
    Ok(intents.list())
}

/// lodash 3.10.1 `_.min(list, key)`: see [`lodash_extremum`].
pub(crate) fn lodash_min_by(list: &[O], key: crate::doc::Atom) -> V {
    lodash_extremum(list, |o| crate::live::to_number(&o.get(key)), false)
}

/// lodash 3.10.1 `_.min`/`_.max` with an iteratee (createExtremum: arrayExtremum, then baseExtremum when the array
/// pass found nothing): the first element whose `+iteratee(o)` is strictly below (above) the running value, which
/// starts at Infinity (-Infinity); that start value itself when none qualifies, an empty list included.
pub(crate) fn lodash_extremum(list: &[O], f: impl Fn(&O) -> f64, max: bool) -> V {
    let ex = if max { f64::NEG_INFINITY } else { f64::INFINITY };
    let better = |c: f64, computed: f64| if max { c > computed } else { c < computed };
    let values: Vec<f64> = list.iter().map(&f).collect();
    let (mut computed, mut result) = (ex, V::Num(ex));
    for (o, &c) in list.iter().zip(&values) {
        if better(c, computed) {
            computed = c;
            result = V::Obj(o.clone());
        }
    }
    let is_ex = |r: &V| matches!(r, V::Num(x) if *x == ex);
    if !(!list.is_empty() && is_ex(&result)) {
        return result;
    }
    let (mut computed, mut result) = (ex, V::Num(ex));
    for (o, &c) in list.iter().zip(&values) {
        if better(c, computed) || (c == ex && is_ex(&result)) {
            computed = c;
            result = V::Obj(o.clone());
        }
    }
    result
}
