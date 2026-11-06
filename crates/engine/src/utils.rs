//! mirror: reference/engine/src/utils.js
//! parity: FAITHFUL
//!
//! Processor half of utils.js: checkConstructionSite, checkControllerAvailability,
//! checkStructureAgainstController, calcResources, calcBodyEffectiveness, dist, calcRoomsDistance,
//! calcTerminalEnergyCost, calcNeededGcl, calcTotalReactionsTime, capacityForResource, calcReward,
//! getReactionVariants, getDirection, getOffsetsByDirection, calcCreepCost, isAtEdge, comparatorDistance,
//! sendAttackingNotification (ARCHITECTURE.md §5.3), plus checkTerrain, roomNameToXY and getRoomNameFromXY,
//! which the processor files call. Values follow JS semantics through `engine::live`.
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).
//! Math.exp / Math.pow go through `processor::js_exp` / `js_pow`, bit-exact ports of V8's (Rust's libm differs).

use crate::atom;
use crate::constants::{self, BodyPart, ResourceType, StructureType};
use crate::live::{strict_eq, to_js_string, to_number, truthy, Crash, RoomObjects, O, V};
use crate::processor::{boosts_entry, eq_str, get_or_undef, js_add, js_max, js_round, lodash_match, lodash_match_str, lodash_sum, lodash_values, loose_eq, lt, prop, RoomScope};
use crate::terrain::RoomTerrain;
use std::str::FromStr;
use util::rng::Rng;

/// `offsetsByDirection` (utils.js:3); index 0 is a hole.
const OFFSETS_BY_DIRECTION: [Option<(f64, f64)>; 9] =
    [None, Some((0.0, -1.0)), Some((1.0, -1.0)), Some((1.0, 0.0)), Some((1.0, 1.0)), Some((0.0, 1.0)), Some((-1.0, 1.0)), Some((-1.0, 0.0)), Some((-1.0, -1.0))];

/// utils.js:65 getDirection; `None` is `undefined` (dx = dy = 0).
pub fn get_direction(dx: f64, dy: f64) -> Option<u32> {
    let (adx, ady) = (dx.abs(), dy.abs());
    if adx > ady * 2.0 {
        Some(if dx > 0.0 { constants::RIGHT } else { constants::LEFT })
    } else if ady > adx * 2.0 {
        Some(if dy > 0.0 { constants::BOTTOM } else { constants::TOP })
    } else if dx > 0.0 && dy > 0.0 {
        Some(constants::BOTTOM_RIGHT)
    } else if dx > 0.0 && dy < 0.0 {
        Some(constants::TOP_RIGHT)
    } else if dx < 0.0 && dy > 0.0 {
        Some(constants::BOTTOM_LEFT)
    } else if dx < 0.0 && dy < 0.0 {
        Some(constants::TOP_LEFT)
    } else {
        None
    }
}

/// utils.js:101 getOffsetsByDirection: `offsetsByDirection[direction]` (a property read, so `"3"` works too).
/// The miss is only logged in vanilla.
pub fn get_offsets_by_direction(direction: &V) -> Option<(f64, f64)> {
    let key = to_js_string(direction);
    crate::doc::array_index(&key).and_then(|i| OFFSETS_BY_DIRECTION.get(i as usize).copied().flatten())
}

/// utils.js:114 calcCreepCost.
pub fn calc_creep_cost(body: &V) -> Result<f64, Crash> {
    let mut result = 0.0;
    for i in for_each_items(body, "body.forEach")? {
        let part_type = if matches!(i, V::Obj(_) | V::Arr(_)) { get_or_undef(&i, atom!("type")) } else { i };
        let cost = BodyPart::from_str(&to_js_string(&part_type)).map(BodyPart::cost).unwrap_or(0);
        if cost == 0 {
            return Err(Crash(format!("Invalid body part {}", to_js_string(&part_type)).into()));
        }
        result += cost as f64;
    }
    Ok(result)
}

/// utils.js:333 checkTerrain over the room's terrain string: `Number(terrain.charAt(y*50 + x)) & mask`.
pub fn check_terrain(terrain: &RoomTerrain, x: f64, y: f64, mask: u8) -> bool {
    // charAt(ToIntegerOrInfinity(pos)): NaN reads position 0, out of range reads "" (0).
    let pos = crate::processor::to_integer(y * 50.0 + x);
    if !(0.0..2500.0).contains(&pos) {
        return false;
    }
    let i = pos as usize;
    terrain.raw((i % 50) as u8, (i / 50) as u8) & mask > 0
}

/// utils.js:128 checkConstructionSite with the room's terrain string as `objects`.
pub fn check_construction_site_terrain(terrain: &RoomTerrain, structure_type: &str, x: f64, y: f64) -> bool {
    for (bx, by) in border_tiles(structure_type, x, y).into_iter().flatten() {
        if !check_terrain(terrain, bx, by, constants::TERRAIN_MASK_WALL) {
            return false;
        }
    }
    if structure_type == "extractor" {
        return true;
    }
    !(structure_type != "road" && check_terrain(terrain, x, y, constants::TERRAIN_MASK_WALL))
}

/// utils.js:128 checkConstructionSite with the room objects as `objects` (`x`/`y` are matched with `===`).
pub fn check_construction_site_objects(objects: &RoomObjects, structure_type: &str, x: &V, y: &V) -> bool {
    let all: Vec<V> = objects.keys().iter().filter_map(|k| objects.get(k)).map(V::Obj).collect();
    let at = |o: &V, ty: &str| lodash_match(o, atom!("x"), x) && lodash_match(o, atom!("y"), y) && lodash_match_str(o, atom!("type"), ty);
    if all.iter().any(|o| at(o, structure_type)) || all.iter().any(|o| at(o, "constructionSite")) {
        return false;
    }
    if structure_type == "extractor" {
        return all.iter().any(|o| at(o, "mineral")) && !all.iter().any(|o| at(o, "extractor"));
    }
    if structure_type != "rampart"
        && structure_type != "road"
        && all.iter().any(|i| {
            let ty = get_or_undef(i, atom!("type"));
            loose_eq(&get_or_undef(i, atom!("x")), x) && loose_eq(&get_or_undef(i, atom!("y")), y) && !eq_str(&ty, "rampart") && !eq_str(&ty, "road") && construction_cost(&ty).is_some()
        })
    {
        return false;
    }
    let zero = V::Num(0.0);
    let edge = V::Num(49.0);
    !(crate::processor::le(x, &zero) || crate::processor::le(y, &zero) || crate::processor::ge(x, &edge) || crate::processor::ge(y, &edge))
}

/// The exit-adjacent tiles that must be walls (utils.js:130-136); later `if`s overwrite earlier ones.
fn border_tiles(structure_type: &str, x: f64, y: f64) -> Option<[(f64, f64); 3]> {
    let mut tiles = None;
    if structure_type != "road" && structure_type != "container" && (x == 1.0 || x == 48.0 || y == 1.0 || y == 48.0) {
        if x == 1.0 {
            tiles = Some([(0.0, y - 1.0), (0.0, y), (0.0, y + 1.0)]);
        }
        if x == 48.0 {
            tiles = Some([(49.0, y - 1.0), (49.0, y), (49.0, y + 1.0)]);
        }
        if y == 1.0 {
            tiles = Some([(x - 1.0, 0.0), (x, 0.0), (x + 1.0, 0.0)]);
        }
        if y == 48.0 {
            tiles = Some([(x - 1.0, 49.0), (x, 49.0), (x + 1.0, 49.0)]);
        }
    }
    tiles
}

/// utils.js:338 checkControllerAvailability.
pub fn check_controller_availability(ty: &str, room_objects: &RoomObjects, room_controller: &V, offset: f64) -> Result<bool, Crash> {
    let mut rcl = V::Num(0.0);
    if let V::Obj(c) = room_controller {
        if truthy(&c.get(atom!("level"))) && (truthy(&c.get(atom!("user"))) || truthy(&c.get(atom!("owner")))) {
            rcl = c.get(atom!("level"));
        }
    }
    if let V::Num(_) = room_controller {
        rcl = room_controller.clone();
    }
    let offset = if offset.is_nan() { 0.0 } else { offset };
    let count = room_objects
        .keys()
        .iter()
        .filter_map(|k| room_objects.get(k))
        .filter(|i| {
            let t = i.get(atom!("type"));
            eq_str(&t, ty) || eq_str(&t, "constructionSite") && eq_str(&i.get(atom!("structureType")), ty)
        })
        .count();
    let available = controller_structures(&V::str(ty), &rcl)? + offset;
    Ok((count as f64) < available)
}

/// `C.CONTROLLER_STRUCTURES[type][level]` as a number (`undefined` → NaN); the row lookup throws when
/// `type` has no row.
fn controller_structures(ty: &V, level: &V) -> Result<f64, Crash> {
    let row = StructureType::from_str(&to_js_string(ty)).ok().filter(|t| constants::CONTROLLER_STRUCTURES.iter().any(|(r, _)| r == t));
    let Some(t) = row else {
        return Err(crate::processor::read_crash(&V::Undef, &to_js_string(level)));
    };
    Ok(crate::doc::array_index(&to_js_string(level)).and_then(|l| constants::controller_structures(t, l as usize)).map_or(f64::NAN, |n| n as f64))
}

/// utils.js:456 checkStructureAgainstController.
pub fn check_structure_against_controller(object: &O, room_objects: &RoomObjects, room_controller: Option<&O>) -> Result<bool, Crash> {
    // owner-less objects are always active
    if !truthy(&object.get(atom!("user"))) {
        return Ok(true);
    }
    let Some(rc) = room_controller else { return Ok(false) };
    if lt(&rc.get(atom!("level")), &V::Num(1.0)) || !strict_eq(&rc.get(atom!("user")), &object.get(atom!("user"))) {
        return Ok(false);
    }
    let ty = object.get(atom!("type"));
    let mut allowed_remaining = controller_structures(&ty, &rc.get(atom!("level")))?;
    if allowed_remaining == 0.0 {
        return Ok(false);
    }
    // if only one object ever allowed, this is it
    if controller_structures(&ty, &V::Num(8.0))? == 1.0 {
        return Ok(allowed_remaining != 0.0);
    }
    let mut found_self = false;
    let d = |o: &O| js_max((to_number(&o.get(atom!("x"))) - to_number(&rc.get(atom!("x")))).abs(), (to_number(&o.get(atom!("y"))) - to_number(&rc.get(atom!("y")))).abs());
    let object_dist = d(object);
    for compare in room_objects.iter() {
        if strict_eq(&compare.get(atom!("type")), &ty) && strict_eq(&compare.get(atom!("user")), &object.get(atom!("user"))) {
            let compare_dist = d(&compare);
            if compare_dist < object_dist {
                allowed_remaining -= 1.0;
                if allowed_remaining == 0.0 {
                    return Ok(false);
                }
            } else if !found_self && compare_dist == object_dist {
                // Objects of equal distance that are discovered before we scan over the selected object are
                // considered closer
                if object.same(&compare) {
                    found_self = true;
                } else {
                    allowed_remaining -= 1.0;
                    if allowed_remaining == 0.0 {
                        return Ok(false);
                    }
                }
            }
        }
    }
    Ok(true)
}

/// utils.js:547 isAtEdge.
pub fn is_at_edge(object: &V) -> bool {
    let o = position_of(object);
    let (x, y) = (get_or_undef(&o, atom!("x")), get_or_undef(&o, atom!("y")));
    let at = |v: &V, n: f64| loose_eq(v, &V::Num(n));
    at(&x, 0.0) || at(&x, 49.0) || at(&y, 0.0) || at(&y, 49.0)
}

/// `if(o.pos) o = o.pos`.
fn position_of(o: &V) -> V {
    let pos = get_or_undef(o, atom!("pos"));
    if truthy(&pos) {
        pos
    } else {
        o.clone()
    }
}

/// utils.js:615 calcResources.
pub fn calc_resources(object: &V) -> Result<f64, Crash> {
    let store = prop(object, atom!("store"))?;
    if truthy(&store) {
        return Ok(lodash_sum(&lodash_values(&store)));
    }
    let mut amounts = Vec::new();
    for r in constants::RESOURCES_ALL {
        let v = get_or_undef(object, crate::doc::Atom::intern(r.as_str()));
        amounts.push(match v {
            V::Obj(_) | V::Arr(_) => get_or_undef(&v, atom!("amount")),
            V::Null => return Err(crate::processor::read_crash(&V::Null, "amount")),
            v if truthy(&v) => v,
            _ => V::Num(0.0),
        });
    }
    Ok(lodash_sum(&amounts))
}

/// utils.js:623 calcBodyEffectiveness.
pub fn calc_body_effectiveness(body: &V, body_part_type: &str, method_name: &str, base_power: f64, without_old_hits: bool) -> Result<f64, Crash> {
    let mut power = 0.0;
    for i in for_each_items(body, "body.forEach")? {
        let alive = truthy(&prop(&i, atom!("hits"))?) || !without_old_hits && truthy(&get_or_undef(&i, atom!("_oldHits")));
        if !alive || !eq_str(&get_or_undef(&i, atom!("type")), body_part_type) {
            continue;
        }
        let mut i_power = base_power;
        let boost = get_or_undef(&i, atom!("boost"));
        if truthy(&boost) {
            if let Some(m) = boosts_entry(body_part_type, &boost)?.and_then(|e| e.iter().find(|(m, _)| *m == method_name).map(|(_, m)| *m)) {
                i_power *= m;
            }
        }
        power += i_power;
    }
    Ok(power)
}

/// `callee(…)` for an array method called as written in the source (`"body.forEach"`): holes (Undef) are
/// skipped; a non-array throws V8's text, which names the callee expression.
pub(crate) fn for_each_items(v: &V, callee: &str) -> Result<Vec<V>, Crash> {
    match v {
        V::Arr(a) => Ok(a.to_vec().into_iter().filter(|x| !matches!(x, V::Undef)).collect()),
        V::Undef | V::Null => Err(crate::processor::read_crash(v, callee.rsplit('.').next().unwrap_or(callee))),
        _ => Err(Crash(format!("{callee} is not a function").into())),
    }
}

/// utils.js:638 dist.
pub fn dist(a: &V, b: &V) -> f64 {
    let (a, b) = (position_of(a), position_of(b));
    let c = |o: &V, k| to_number(&get_or_undef(o, k));
    js_max((c(&a, atom!("x")) - c(&b, atom!("x"))).abs(), (c(&a, atom!("y")) - c(&b, atom!("y"))).abs())
}

/// `parseInt(s, 10)`.
fn parse_int10(units: &[u16]) -> f64 {
    let s = String::from_utf16_lossy(units);
    let t = s.trim_start_matches(util::num::is_js_whitespace);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let d = t.bytes().take_while(u8::is_ascii_digit).count();
    if d == 0 {
        return f64::NAN;
    }
    let n: f64 = t[..d].parse().unwrap_or(f64::NAN);
    if neg {
        -n
    } else {
        n
    }
}

/// utils.js:374 roomNameToXY.
pub fn room_name_to_xy(name: &str) -> (f64, f64) {
    let u: Vec<u16> = name.encode_utf16().collect();
    let sub = |from: usize| if from < u.len() { &u[from..] } else { &[][..] };
    let mut xx = parse_int10(sub(1));
    let vertical_pos = if xx >= 100.0 {
        4
    } else if xx >= 10.0 {
        3
    } else {
        2
    };
    let mut yy = parse_int10(sub(vertical_pos + 1));
    let horizontal_dir = u.first().copied();
    let vertical_dir = u.get(vertical_pos).copied();
    if horizontal_dir == Some(b'W' as u16) || horizontal_dir == Some(b'w' as u16) {
        xx = -xx - 1.0;
    }
    if vertical_dir == Some(b'N' as u16) || vertical_dir == Some(b'n' as u16) {
        yy = -yy - 1.0;
    }
    (xx, yy)
}

/// utils.js:358 getRoomNameFromXY.
pub fn get_room_name_from_xy(x: f64, y: f64) -> String {
    let n = util::num::to_js_string;
    let x = if x < 0.0 { format!("W{}", n(-x - 1.0)) } else { format!("E{}", n(x)) };
    let y = if y < 0.0 { format!("N{}", n(-y - 1.0)) } else { format!("S{}", n(y)) };
    x + &y
}

/// utils.js:644 calcRoomsDistance; `world_size` is `driver.getWorldSize()`.
pub fn calc_rooms_distance(room1: &str, room2: &str, continuous: bool, world_size: f64) -> f64 {
    let (x1, y1) = room_name_to_xy(room1);
    let (x2, y2) = room_name_to_xy(room2);
    let mut dx = (x2 - x1).abs();
    let mut dy = (y2 - y1).abs();
    if continuous {
        dx = crate::processor::js_min(world_size - dx, dx);
        dy = crate::processor::js_min(world_size - dy, dy);
    }
    js_max(dx, dy)
}

/// utils.js:657 calcTerminalEnergyCost.
pub fn calc_terminal_energy_cost(amount: f64, range: f64) -> f64 {
    (amount * (1.0 - crate::processor::js_exp(-range / 30.0))).ceil()
}

/// utils.js:661 calcNeededGcl.
pub fn calc_needed_gcl(gcl_level: f64) -> f64 {
    constants::GCL_MULTIPLY as f64 * crate::processor::js_pow(gcl_level - 1.0, constants::GCL_POW)
}

/// utils.js:665 calcTotalReactionsTime.
pub fn calc_total_reactions_time(mineral: &str) -> f64 {
    // reagents[product] = [b, a] for REACTIONS[a][b] = product; the last pair in key order wins.
    let mut reagents: Vec<(ResourceType, (ResourceType, ResourceType))> = Vec::new();
    for &(a, row) in constants::REACTIONS {
        for &(b, product) in row {
            match reagents.iter_mut().find(|(p, _)| *p == product) {
                Some(slot) => slot.1 = (b, a),
                None => reagents.push((product, (b, a))),
            }
        }
    }
    fn step(m: ResourceType, reagents: &[(ResourceType, (ResourceType, ResourceType))]) -> f64 {
        match constants::reaction_time(m) {
            Some(t) if t > 0 => {
                let (a, b) = reagents.iter().find(|(p, _)| *p == m).map(|(_, r)| *r).expect("every timed compound has reagents");
                t as f64 + step(a, reagents) + step(b, reagents)
            }
            _ => 0.0,
        }
    }
    ResourceType::from_str(mineral).map_or(0.0, |m| step(m, &reagents))
}

/// utils.js:671 capacityForResource.
pub fn capacity_for_resource(object: &V, resource_type: &str) -> V {
    let scr = get_or_undef(object, atom!("storeCapacityResource"));
    if truthy(&scr) {
        let v = get_or_undef(&scr, crate::doc::Atom::intern(resource_type));
        if truthy(&v) {
            return v;
        }
    }
    let capacity = crate::processor::or0(get_or_undef(object, atom!("storeCapacity")));
    let used = lodash_sum(&lodash_values(&scr));
    V::Num(js_max(0.0, to_number(&capacity) - used))
}

/// utils.js:677 calcReward (BUG-CALC-REWARD replicated: the last slot divides by `densities[len-1]`).
pub fn calc_reward(resource_densities: &O, target_density: f64, items_limit: Option<f64>, rng: &mut Rng) -> V {
    let mut resources = Vec::new();
    let mut densities = Vec::new();
    for k in resource_densities.keys() {
        resources.push(k);
        densities.push(to_number(&resource_densities.get(k)));
    }
    let range: Vec<usize> = (0..resources.len()).collect();
    let mut order = crate::policy::lodash_shuffle(rng, &range);
    if let Some(limit) = items_limit.filter(|l| truthy(&V::Num(*l))) {
        order.truncate(crate::processor::slice_to(&order.iter().map(|&i| i as f64).collect::<Vec<_>>(), limit).len());
    }
    let out = O::new();
    if order.is_empty() {
        return V::Obj(out);
    }
    let mut result = vec![0.0; order.len()];
    let mut current_density = 0.0;
    for i in 0..order.len() - 1 {
        result[i] = js_max(0.0, js_round(rng.next_f64() * (target_density - current_density) / densities[order[i]]));
        current_density += result[i] * densities[order[i]];
    }
    let last = order.len() - 1;
    let d = densities.get(last).copied().unwrap_or(f64::NAN);
    result[last] = js_max(0.0, js_round((target_density - current_density) / d));
    for (i, &o) in order.iter().enumerate() {
        out.set(resources[o], V::Num(result[i]));
    }
    V::Obj(out)
}

/// utils.js:700 getReactionVariants.
pub fn get_reaction_variants(compound: &str) -> Vec<(ResourceType, ResourceType)> {
    let mut result = Vec::new();
    for &(r1, row) in constants::REACTIONS {
        for &(r2, product) in row {
            if product.as_str() == compound {
                result.push((r1, r2));
            }
        }
    }
    result
}

/// utils.js:394 comparatorDistance: `da - db`.
pub fn comparator_distance(target: &V) -> impl Fn(&V, &V) -> f64 {
    let target = position_of(target);
    move |a, b| {
        let d = |o: &V| {
            let o = position_of(o);
            let c = |v: &V, k| to_number(&get_or_undef(v, k));
            js_max((c(&o, atom!("x")) - c(&target, atom!("x"))).abs(), (c(&o, atom!("y")) - c(&target, atom!("y"))).abs())
        };
        d(a) - d(b)
    }
}

/// utils.js:438 sendAttackingNotification.
pub fn send_attacking_notification(target: &O, room_controller: Option<&O>, scope: &mut RoomScope) {
    let ty = target.get(atom!("type"));
    let label = if eq_str(&ty, "creep") {
        to_js_string(&js_add(&V::str("creep "), &target.get(atom!("name"))))
    } else if eq_str(&ty, "spawn") {
        to_js_string(&js_add(&V::str("spawn "), &target.get(atom!("name"))))
    } else {
        format!("{} #{}", to_js_string(&ty), to_js_string(&target.get(atom!("_id")))).into()
    };
    let user = match (target.get(atom!("user")), room_controller) {
        (u, _) if truthy(&u) => u,
        (_, Some(c)) => c.get(atom!("user")),
        _ => V::Null,
    };
    if truthy(&user) {
        let room = to_js_string(&target.get(atom!("room")));
        scope.send_notification(&user, format!("Your {label} in room {room} is under attack!"));
    }
}

/// `C.CONSTRUCTION_COST[type]` when truthy.
pub fn construction_cost(ty: &V) -> Option<u32> {
    let V::Str(s) = ty else { return None };
    StructureType::from_str(s).ok().and_then(constants::construction_cost).filter(|&c| c > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_and_rooms() {
        assert_eq!(get_direction(1.0, -1.0), Some(constants::TOP_RIGHT));
        assert_eq!(get_direction(5.0, 1.0), Some(constants::RIGHT));
        assert_eq!(get_direction(0.0, 0.0), None);
        assert_eq!(get_offsets_by_direction(&V::Num(3.0)), Some((1.0, 0.0)));
        assert_eq!(get_offsets_by_direction(&V::str("8")), Some((-1.0, -1.0)));
        assert_eq!(get_offsets_by_direction(&V::Num(0.0)), None);
        assert_eq!(get_offsets_by_direction(&V::Num(1.5)), None);
        assert_eq!(room_name_to_xy("W1N1"), (-2.0, -2.0));
        assert_eq!(room_name_to_xy("E10S105"), (10.0, 105.0));
        assert_eq!(get_room_name_from_xy(-2.0, -2.0), "W1N1");
        assert_eq!(get_room_name_from_xy(0.0, 3.0), "E0S3");
        assert_eq!(calc_rooms_distance("W1N1", "E1S1", false, 4.0), 3.0);
        assert_eq!(calc_rooms_distance("W1N1", "E1S1", true, 4.0), 1.0);
    }

    #[test]
    fn body_and_resources() {
        let v = |s: &str| crate::live::from_val(&crate::doc::Val::from_json(&util::json::Json::parse(s).unwrap()));
        let body = v(r#"[{"type":"work","hits":100,"boost":"UO"},{"type":"work","hits":0},{"type":"move","hits":100},{"type":"work","hits":50,"boost":"XGH2O"}]"#);
        assert_eq!(calc_body_effectiveness(&body, "work", "harvest", 2.0, false).unwrap(), 8.0);
        assert_eq!(calc_body_effectiveness(&body, "work", "upgradeController", 1.0, false).unwrap(), 3.0);
        assert!(calc_body_effectiveness(&V::Undef, "work", "harvest", 2.0, false).is_err());
        assert_eq!(calc_resources(&v(r#"{"store":{"energy":5,"H":null,"O":"x","U":2}}"#)).unwrap(), 7.0);
        assert_eq!(calc_resources(&v(r#"{"store":{},"energy":5}"#)).unwrap(), 0.0, "an empty store is still truthy");
        assert_eq!(calc_resources(&v(r#"{"energy":5,"power":{"amount":3}}"#)).unwrap(), 8.0);
        assert!(calc_resources(&v(r#"{"energy":null}"#)).is_err(), "typeof null == 'object'");
        assert_eq!(to_number(&capacity_for_resource(&v(r#"{"storeCapacityResource":{"energy":300}}"#), "energy")), 300.0);
        assert_eq!(to_number(&capacity_for_resource(&v(r#"{"storeCapacityResource":{"energy":300}}"#), "H")), 0.0);
        assert_eq!(to_number(&capacity_for_resource(&v(r#"{"storeCapacity":3000,"storeCapacityResource":{"energy":2000}}"#), "H")), 1000.0);
        assert_eq!(calc_creep_cost(&v(r#"["work",{"type":"move"}]"#)).unwrap(), 150.0);
        assert_eq!(calc_creep_cost(&v(r#"["wings"]"#)), Err(Crash("Invalid body part wings".into())));
        assert_eq!(calc_total_reactions_time("OH"), 20.0);
        assert_eq!(calc_total_reactions_time("G"), 5.0 + 5.0 + 5.0);
        assert!(get_reaction_variants("OH").contains(&(ResourceType::O, ResourceType::H)));
        assert!(is_at_edge(&v(r#"{"x":49,"y":5}"#)) && !is_at_edge(&v(r#"{"x":48,"y":5}"#)));
    }

    #[test]
    fn terrain_and_sites() {
        let mut t = RoomTerrain::default();
        for y in 0..50 {
            t.set_raw(0, y, 1);
        }
        t.set_raw(10, 10, 1);
        assert!(check_terrain(&t, 10.0, 10.0, constants::TERRAIN_MASK_WALL));
        assert!(!check_terrain(&t, 11.0, 10.0, constants::TERRAIN_MASK_WALL));
        assert!(check_terrain(&t, f64::NAN, 0.0, constants::TERRAIN_MASK_WALL), "charAt(NaN) reads index 0");
        assert!(!check_terrain(&t, 10.0, 60.0, constants::TERRAIN_MASK_WALL));
        assert!(check_construction_site_terrain(&t, "spawn", 1.0, 20.0), "x=1 next to a wall border");
        assert!(!check_construction_site_terrain(&t, "spawn", 48.0, 20.0));
        assert!(check_construction_site_terrain(&t, "road", 48.0, 20.0));
        assert!(!check_construction_site_terrain(&t, "spawn", 10.0, 10.0));
        assert!(check_construction_site_terrain(&t, "extractor", 10.0, 10.0));
    }
}

#[cfg(test)]
mod oracle {
    //! crates/engine/tests/fixtures/creeps/utils-cases.ndjson: utils.js called directly by generate.js.
    use super::*;
    use crate::doc::Val;
    use crate::live::from_val;
    use util::json::Json;

    const CASES: &str = include_str!("../tests/fixtures/creeps/utils-cases.ndjson");

    fn v(j: &Json) -> V {
        from_val(&Val::from_json(j))
    }

    /// The generator's result encoding: `{$u:1}` undefined, `{$n:"NaN"|"Infinity"|…}`, arrays recursively.
    fn expected(j: &Json) -> V {
        match j {
            Json::Obj(m) if m.iter().any(|(k, _)| k == "$u") => V::Undef,
            Json::Obj(m) if m.iter().any(|(k, _)| k == "$n") => V::Num(util::num::string_to_number(m[0].1.as_str().unwrap())),
            Json::Arr(a) => V::Arr(crate::live::A::from_vec(a.iter().map(expected).collect())),
            other => v(other),
        }
    }

    fn room_objects(j: &Json) -> RoomObjects {
        let mut ro = RoomObjects::new();
        for (k, o) in j.as_object().unwrap() {
            let V::Obj(o) = v(o) else { panic!() };
            ro.insert(k.as_str().into(), o);
        }
        ro
    }

    fn num(n: f64) -> V {
        V::Num(n)
    }

    fn arg_undef(j: &Json) -> V {
        if j.is_null() {
            V::Undef
        } else {
            v(j)
        }
    }

    fn run(case: &Json) -> Result<V, Crash> {
        let a = |i: usize| case.get("args").unwrap().at(i).unwrap();
        let s = |i: usize| a(i).as_str().unwrap().to_string();
        let f = |i: usize| a(i).as_f64().unwrap_or(f64::NAN);
        Ok(match case.get("fn").unwrap().as_str().unwrap() {
            "Math.pow" => num(crate::processor::js_pow(to_number(&expected(a(0))), to_number(&expected(a(1))))),
            "Math.exp" => num(crate::processor::js_exp(to_number(&expected(a(0))))),
            "getDirection" => get_direction(f(0), f(1)).map_or(V::Undef, |d| num(d as f64)),
            "getOffsetsByDirection" => get_offsets_by_direction(&v(a(0))).map_or(V::Undef, |(x, y)| V::Arr(crate::live::A::from_vec(vec![num(x), num(y)]))),
            "calcCreepCost" => num(calc_creep_cost(&v(a(0)))?),
            "checkConstructionSite/terrain" => V::Bool(check_construction_site_terrain(&RoomTerrain::from_db_str(&s(0)).unwrap(), &s(1), f(2), f(3))),
            "checkConstructionSite/objects" => V::Bool(check_construction_site_objects(&room_objects(a(0)), &s(1), &v(a(2)), &v(a(3)))),
            "checkControllerAvailability" => V::Bool(check_controller_availability(&s(0), &room_objects(a(1)), &arg_undef(a(2)), f(3))?),
            "checkStructureAgainstController" => {
                let ro = room_objects(a(1));
                let object = ro.get(&s(0)).unwrap();
                let rc = match arg_undef(a(2)) {
                    V::Obj(o) => Some(o),
                    _ => None,
                };
                V::Bool(check_structure_against_controller(&object, &ro, rc.as_ref())?)
            }
            "calcResources" => num(calc_resources(&v(a(0)))?),
            "capacityForResource" => capacity_for_resource(&v(a(0)), &s(1)),
            "calcBodyEffectiveness" => num(calc_body_effectiveness(&v(a(0)), &s(1), &s(2), f(3), a(4).as_bool().unwrap())?),
            "dist" => num(dist(&v(a(0)), &v(a(1)))),
            "comparatorDistance" => num(comparator_distance(&v(a(0)))(&v(a(1)), &v(a(2)))),
            "isAtEdge" => V::Bool(is_at_edge(&v(a(0)))),
            "calcRoomsDistance" => num(calc_rooms_distance(&s(0), &s(1), a(2).as_bool().unwrap(), case.get("worldSize").unwrap().as_f64().unwrap())),
            "roomNameToXY" => {
                let (x, y) = room_name_to_xy(&s(0));
                V::Arr(crate::live::A::from_vec(vec![num(x), num(y)]))
            }
            "getRoomNameFromXY" => V::str(&get_room_name_from_xy(f(0), f(1))),
            "calcTerminalEnergyCost" => num(calc_terminal_energy_cost(f(0), f(1))),
            "calcNeededGcl" => num(calc_needed_gcl(f(0))),
            "calcTotalReactionsTime" => num(calc_total_reactions_time(&s(0))),
            "getReactionVariants" => V::Arr(crate::live::A::from_vec(
                get_reaction_variants(&s(0)).into_iter().map(|(x, y)| V::Arr(crate::live::A::from_vec(vec![V::str(x.as_str()), V::str(y.as_str())]))).collect(),
            )),
            "calcReward" => {
                let V::Obj(d) = v(a(0)) else { panic!() };
                let seed = u64::from_str_radix(case.get("seed").unwrap().as_str().unwrap(), 16).unwrap();
                let limit = a(2).as_f64();
                calc_reward(&d, f(1), limit, &mut Rng::new(seed))
            }
            "sendAttackingNotification" => {
                let V::Obj(target) = v(a(0)) else { panic!() };
                let rc = match arg_undef(a(1)) {
                    V::Obj(o) => Some(o),
                    _ => None,
                };
                crate::processor::tests::with_scope(|scope| {
                    send_attacking_notification(&target, rc.as_ref(), scope);
                    let users = scope.ctx.users;
                    V::Arr(crate::live::A::from_vec(
                        scope.out.notifications.iter().map(|(u, m)| V::Arr(crate::live::A::from_vec(vec![V::str(users[*u as usize].id.as_str()), V::str(m)]))).collect(),
                    ))
                })
            }
            other => panic!("unknown fn {other}"),
        })
    }

    #[test]
    fn utils_cases() {
        let mut failures = Vec::new();
        let mut n = 0;
        for line in CASES.lines().filter(|l| !l.trim().is_empty()) {
            let case = Json::parse(line).unwrap();
            n += 1;
            let got = run(&case);
            let bitwise = case.get("fn").unwrap().as_str().unwrap().starts_with("Math.");
            let ok = match (case.get("throws"), &got) {
                (Some(Json::Str(m)), Err(Crash(c))) => m == c,
                (None, Ok(V::Num(g))) if bitwise => match expected(case.get("result").unwrap()) {
                    V::Num(e) => g.to_bits() == e.to_bits() || g.is_nan() && e.is_nan(),
                    _ => false,
                },
                (None, Ok(g)) => crate::live::is_equal(g, &expected(case.get("result").unwrap())) && crate::live::stringify(g) == crate::live::stringify(&expected(case.get("result").unwrap())),
                _ => false,
            };
            if !ok {
                failures.push(format!("{} -> ours {:?}", &line[..line.len().min(300)], got));
            }
        }
        assert!(n > 1500, "{n} cases");
        assert!(failures.is_empty(), "{} of {n} differ:\n{}", failures.len(), failures.iter().take(10).cloned().collect::<Vec<_>>().join("\n"));
    }
}
