//! mirror: reference/engine/src/processor/intents/invader-core/create-creep.js
//! parity: FAITHFUL

use crate::atom;
use crate::constants as C;
use crate::live::{to_js_string, truthy, Crash, O, V, A};
use crate::processor::global::obj;
use crate::processor::{eq_str, get_or_undef, prop, update, RoomScope};
use crate::utils::calc_body_effectiveness;

/// `C.BOOSTS[part][boost]` is truthy.
fn boostable(part: &V, boost: &V) -> bool {
    let (Ok(p), Ok(r)) = (to_js_string(part).parse::<C::BodyPart>(), to_js_string(boost).parse::<C::ResourceType>()) else { return false };
    C::BOOSTS.iter().any(|(bp, list)| *bp == p && list.iter().any(|(res, _)| *res == r))
}

/// `C.INVADER_CORE_CREEP_SPAWN_TIME[level]` (undefined → 0).
fn spawn_time(level: &V) -> f64 {
    let key = to_js_string(level);
    key.parse::<usize>().ok().filter(|i| i.to_string() == *key).and_then(|i| C::INVADER_CORE_CREEP_SPAWN_TIME.get(i)).map_or(0.0, |&t| t as f64)
}

pub fn run(object: &O, intent: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    if truthy(&object.get(atom!("spawning"))) || !eq_str(&object.get(atom!("type")), "invaderCore") {
        return Ok(());
    }
    let level = object.get(atom!("level"));
    let t = spawn_time(&level);
    if !truthy(&level) || t == 0.0 {
        return Ok(());
    }
    // intent.body = intent.body.slice(0, MAX_CREEP_SIZE)
    let parts = match prop(intent, atom!("body"))? {
        V::Arr(a) => a.to_vec().into_iter().take(C::MAX_CREEP_SIZE as usize).collect::<Vec<_>>(),
        _ => return Err(Crash("intent.body.slice is not a function".into())),
    };
    crate::processor::put(intent, atom!("body"), V::Arr(A::from_vec(parts.clone())))?;
    let boosts = get_or_undef(intent, atom!("boosts"));
    let boosts_len = crate::live::to_number(&get_or_undef(&boosts, atom!("length")));
    let mut body = Vec::new();
    for (i, ty) in parts.iter().enumerate() {
        if !matches!(ty, V::Str(s) if C::BODYPARTS_ALL.iter().any(|p| p.as_str() == &**s)) {
            continue;
        }
        let boost = get_or_undef(&boosts, crate::doc::Atom::intern(&i.to_string()));
        if truthy(&boosts) && boosts_len >= i as f64 && boostable(ty, &boost) {
            body.push(obj(vec![("type", ty.clone()), ("hits", V::Num(100.0)), ("boost", boost)]));
        } else {
            body.push(obj(vec![("type", ty.clone()), ("hits", V::Num(100.0))]));
        }
    }
    let n = body.len() as f64;
    let body = V::Arr(A::from_vec(body));
    let store_capacity = calc_body_effectiveness(&body, C::CARRY, "capacity", C::CARRY_CAPACITY as f64, true)?;
    let name = get_or_undef(intent, atom!("name"));
    let creep = obj(vec![
        ("strongholdId", object.get(atom!("strongholdId"))),
        ("type", V::str("creep")),
        ("name", name.clone()),
        ("x", object.get(atom!("x"))),
        ("y", object.get(atom!("y"))),
        ("body", body),
        ("store", obj(vec![("energy", V::Num(0.0))])),
        ("storeCapacity", V::Num(store_capacity)),
        ("room", object.get(atom!("room"))),
        ("user", object.get(atom!("user"))),
        ("hits", V::Num(n * 100.0)),
        ("hitsMax", V::Num(n * 100.0)),
        ("spawning", V::Bool(true)),
        ("fatigue", V::Num(0.0)),
        ("notifyWhenAttacked", V::Bool(false)),
        ("ageTime", object.get(atom!("decayTime"))),
    ]);
    scope.bulk.insert(&creep, None);
    let spawning = obj(vec![("name", name), ("needTime", V::Num(t * n)), ("spawnTime", V::Num(scope.game_time as f64 + t * n))]);
    update(scope, object, &obj(vec![("spawning", spawning)]))
}
