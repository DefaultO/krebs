//! mirror: reference/engine/src/processor/intents/movement.js
//! parity: FAITHFUL
//!
//! The module globals (`matrix`, `objects`, `affectedCnt`) live in `RoomScope::movement`, one per room tick
//! (§5.4); `roomObjects`/`roomTerrain` are read from the scope. Tile keys stay vanilla's `"x,y"` strings, so
//! insertion order (MOVE-TIEBREAK, second-loop order) and the `parseInt` round trip are kept exactly.
//! Verified against the official processor by the fixtures in crates/engine/tests/fixtures/creeps (generate.js).

use crate::atom;
use crate::constants::{CARRY, CARRY_CAPACITY, MOVE, ROAD_WEAROUT, ROAD_WEAROUT_POWER_CREEP, TERRAIN_MASK_SWAMP, TERRAIN_MASK_WALL};
use crate::live::{parse_int, strict_eq, tile_coord, to_js_string, to_number, truthy, Crash, O, V};
use crate::processor::{boost_effect, bulk_remove, eq_str, ge, get_index, get_or_undef, gt, js_add, js_min, lodash_any, lodash_match, lodash_sum, lodash_values, loose_eq, lt, obj, part_is, prop, RoomScope};
use crate::utils;
use util::hash::FxHashMap;
use std::sync::Arc;

/// A `matrix` key, vanilla's `"x,y"`: the tile's number for every text that names a tile, the text otherwise.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Tile(u16),
    Text(Box<str>),
}

impl Key {
    /// The key of `x + "," + y`.
    fn of(x: &V, y: &V) -> Key {
        match (tile_coord(x), tile_coord(y)) {
            (Some(x), Some(y)) => Key::Tile(x * 50 + y),
            _ => Key::text(format!("{},{}", to_js_string(x), to_js_string(y))),
        }
    }

    fn text(s: String) -> Key {
        // What an integer 0–49 prints as.
        let coord = |t: &str| (matches!(t.len(), 1 | 2) && t.bytes().all(|b| b.is_ascii_digit()) && (t.len() == 1 || !t.starts_with('0'))).then(|| t.parse::<u16>().ok()).flatten().filter(|c| *c <= 49);
        match s.split_once(',').and_then(|(x, y)| Some((coord(x)?, coord(y)?))) {
            Some((x, y)) => Key::Tile(x * 50 + y),
            None => Key::Text(s.into()),
        }
    }
}

/// One `matrix` key: the movers registered for the tile (before `check`), then the winner (after).
struct Tile {
    key: Key,
    movers: Vec<O>,
    winner: Option<O>,
    /// `delete matrix[key]` happened.
    deleted: bool,
}

/// movement.js module state (§5.4).
#[derive(Default)]
pub struct Movement {
    tiles: Vec<Tile>,
    index: FxHashMap<Key, usize>,
    /// `check` replaced `matrix` with `newMatrix`.
    checked: bool,
    /// `objects`: mover id → its destination (`Some`) or `null` (`None`); absent = `undefined`.
    objects: FxHashMap<Arc<str>, Option<(f64, f64)>>,
    affected_cnt: FxHashMap<Key, f64>,
}

impl Movement {
    fn tile(&self, key: &Key) -> Option<&Tile> {
        self.index.get(key).map(|&i| &self.tiles[i]).filter(|t| !t.deleted)
    }

    /// `objects[id]` is truthy.
    fn has_move(&self, id: &V) -> Option<(f64, f64)> {
        self.objects.get(&*to_js_string(id)).copied().flatten()
    }
}

/// movement.js:11 canMove.
fn can_move(object: &O) -> Result<bool, Crash> {
    if eq_str(&object.get(atom!("type")), "powerCreep") || truthy(&object.get(atom!("_pulled"))) {
        return Ok(true);
    }
    if truthy(&object.get(atom!("_oldFatigue"))) {
        return Ok(false);
    }
    lodash_any(&object.get(atom!("body")), |part| Ok(gt(&prop(part, atom!("hits"))?, &V::Num(0.0)) && part_is(part, MOVE)))
}

/// `utils::calc_body_effectiveness(body, MOVE, "fatigue", 1.0, false)`, read in place when no part is boosted.
pub(crate) fn move_power(body: &V) -> Result<f64, Crash> {
    if let V::Arr(a) = body {
        let plain = a.with_items(|parts| {
            let mut power = 0.0;
            for part in parts {
                let V::Obj(p) = part else { return None };
                if !p.eq_str(atom!("type"), MOVE) || !truthy(&p.get(atom!("hits"))) && !truthy(&p.get(atom!("_oldHits"))) {
                    continue;
                }
                if truthy(&p.get(atom!("boost"))) {
                    return None;
                }
                power += 1.0;
            }
            Some(power)
        });
        if let Some(power) = plain {
            return Ok(power);
        }
    }
    utils::calc_body_effectiveness(body, MOVE, "fatigue", 1.0, false)
}

/// movement.js:16 checkObstacleAtXY.
fn check_obstacle_at_xy(x: f64, y: f64, object: &O, room_is_in_safe_mode: &V, scope: &RoomScope) -> bool {
    obstacle_at_xy(scope.room_objects.near(&V::Num(x), &V::Num(y)), x, y, object, room_is_in_safe_mode, scope)
}

fn obstacle_at_xy<'a>(objects: impl Iterator<Item = &'a O>, x: f64, y: f64, object: &O, room_is_in_safe_mode: &V, scope: &RoomScope) -> bool {
    let (vx, vy) = (V::Num(x), V::Num(y));
    let user = object.get(atom!("user"));
    let mut has_road = false;
    for i in objects {
        if !loose_eq(&i.get(atom!("x")), &vx) || !loose_eq(&i.get(atom!("y")), &vy) {
            continue;
        }
        let ty = i.get(atom!("type"));
        let is_creep = eq_str(&ty, "creep") || eq_str(&ty, "powerCreep");
        let obstacle = is_creep
            && scope.movement.has_move(&i.get(atom!("_id"))).is_none()
            && (!truthy(room_is_in_safe_mode) || !loose_eq(room_is_in_safe_mode, &user) || loose_eq(room_is_in_safe_mode, &user) && loose_eq(&user, &i.get(atom!("user"))))
            || !is_creep && crate::processor::is_obstacle_type(&ty)
            || eq_str(&ty, "rampart") && !truthy(&i.get(atom!("isPublic"))) && !loose_eq(&i.get(atom!("user")), &user)
            || eq_str(&ty, "constructionSite") && loose_eq(&i.get(atom!("user")), &user) && crate::processor::is_obstacle_type(&i.get(atom!("structureType")));
        if obstacle {
            return true;
        }
        if eq_str(&ty, "road") {
            has_road = true;
        }
    }
    utils::check_terrain(scope.room_terrain, x, y, TERRAIN_MASK_WALL) && !has_road
}

/// movement.js:41 calcResourcesWeight.
fn calc_resources_weight(creep: &O) -> Result<f64, Crash> {
    let mut total_carry = lodash_sum(&lodash_values(&creep.get(atom!("store"))));
    let mut weight = 0.0;
    let body = creep.get(atom!("body"));
    let len = to_number(&prop(&body, atom!("length"))?);
    let mut i = len - 1.0;
    while i >= 0.0 {
        if !truthy(&V::Num(total_carry)) {
            break;
        }
        let part = get_index(&body, i);
        i -= 1.0;
        if matches!(part, V::Undef | V::Null) {
            return Err(crate::processor::read_crash(&part, "type"));
        }
        if !part_is(&part, CARRY) || !truthy(&part_hits(&part)) {
            continue;
        }
        let mut boost = 1.0;
        let b = get_or_undef(&part, atom!("boost"));
        if truthy(&b) {
            let capacity = boost_effect(CARRY, &b, "capacity")?;
            boost = if truthy(&capacity) { to_number(&capacity) } else { 1.0 };
        }
        total_carry -= js_min(total_carry, CARRY_CAPACITY as f64 * boost);
        weight += 1.0;
    }
    Ok(weight)
}

fn part_hits(part: &V) -> V {
    get_or_undef(part, atom!("hits"))
}

/// `_.filter(object.body, (i) => i.type != C.MOVE && i.type != C.CARRY).length`.
fn non_move_carry_parts(object: &O) -> Result<f64, Crash> {
    let mut n = 0.0;
    lodash_any(&object.get(atom!("body")), |i| {
        if matches!(i, V::Undef | V::Null) {
            return Err(crate::processor::read_crash(i, "type"));
        }
        if !part_is(i, MOVE) && !part_is(i, CARRY) {
            n += 1.0;
        }
        Ok(false)
    })?;
    Ok(n)
}

/// movement.js:61 init.
pub fn init(scope: &mut RoomScope) {
    scope.movement = Movement::default();
}

/// movement.js:69 addPulling.
pub fn add_pulling(scope: &mut RoomScope, object: &O, target: &O) -> Result<(), Crash> {
    let object_id = object.get(atom!("_id"));
    // checkRecursiveTarget, iteratively; the recursion can only overflow on a cycle that avoids `object`.
    let mut t = target.clone();
    let mut depth = 0;
    let recursive = loop {
        if loose_eq(&t.get(atom!("_id")), &object_id) {
            break true;
        }
        let pull = t.get(atom!("_pull"));
        let next = if truthy(&pull) { scope.room_objects.get(&to_js_string(&pull)) } else { None };
        match next {
            Some(n) => t = n,
            None => break false,
        }
        depth += 1;
        if depth > scope.room_objects.len() + 1 {
            return Err(Crash("Maximum call stack size exceeded".into()));
        }
    };
    if !recursive {
        object.set(atom!("_pull"), target.get(atom!("_id")));
        target.set(atom!("_pulled"), object_id);
    }
    Ok(())
}

/// movement.js:77 removePulling.
pub fn remove_pulling(scope: &mut RoomScope, object: &O) {
    let pull = object.get(atom!("_pull"));
    if truthy(&pull) {
        if let Some(t) = scope.room_objects.get(&to_js_string(&pull)) {
            t.delete(atom!("_pulled"));
        }
    }
    object.delete(atom!("_pull"));
}

/// movement.js:84 add.
pub fn add(scope: &mut RoomScope, object: &O, dx: f64, dy: f64) {
    let mut new_x = js_add(&object.get(atom!("x")), &V::Num(dx));
    let mut new_y = js_add(&object.get(atom!("y")), &V::Num(dy));
    if ge(&new_x, &V::Num(50.0)) {
        new_x = V::Num(49.0);
    }
    if ge(&new_y, &V::Num(50.0)) {
        new_y = V::Num(49.0);
    }
    if lt(&new_x, &V::Num(0.0)) {
        new_x = V::Num(0.0);
    }
    if lt(&new_y, &V::Num(0.0)) {
        new_y = V::Num(0.0);
    }
    let key = Key::of(&new_x, &new_y);
    let m = &mut scope.movement;
    match m.index.get(&key) {
        Some(&i) => m.tiles[i].movers.push(object.clone()),
        None => {
            m.index.insert(key.clone(), m.tiles.len());
            m.tiles.push(Tile { key: key.clone(), movers: vec![object.clone()], winner: None, deleted: false });
        }
    }
    // affectedCnt[key] = affectedCnt[key]+1 || 1
    *m.affected_cnt.entry(key).or_insert(0.0) += 1.0;
}

/// movement.js:100 isTileBusy.
pub fn is_tile_busy(scope: &RoomScope, x: &V, y: &V) -> bool {
    scope.movement.tile(&Key::of(x, y)).is_some()
}

/// `var [x,y] = i.split(/,/); x = parseInt(x); y = parseInt(y);`
fn parse_key(key: &Key) -> (f64, f64) {
    let key = match key {
        Key::Tile(t) => return ((t / 50) as f64, (t % 50) as f64),
        Key::Text(key) => key,
    };
    let mut parts = key.split(',');
    let x = parts.next().map_or(f64::NAN, |p| parse_int(&V::str(p)));
    let y = parts.next().map_or(f64::NAN, |p| parse_int(&V::str(p)));
    (x, y)
}

/// movement.js:104 check.
pub fn check(scope: &mut RoomScope, room_is_in_safe_mode: &V) -> Result<(), Crash> {
    // First loop: one winner per destination; `matrix` keeps the registration lists until it is replaced.
    for t in 0..scope.movement.tiles.len() {
        let (x, y) = parse_key(&scope.movement.tiles[t].key);
        let movers = scope.movement.tiles[t].movers.clone();
        let winner = if movers.len() > 1 {
            let mut rates = Vec::with_capacity(movers.len());
            for object in &movers {
                let pc = eq_str(&object.get(atom!("type")), "powerCreep");
                let moves = if pc { 0.0 } else { move_power(&object.get(atom!("body")))? };
                let mut weight = if pc { 0.0 } else { non_move_carry_parts(object)? };
                weight += if pc { 0.0 } else { calc_resources_weight(object)? };
                if !truthy(&V::Num(weight)) {
                    weight = 1.0;
                }
                let key = Key::of(&object.get(atom!("x")), &object.get(atom!("y")));
                let m = &scope.movement;
                let mut rate1 = m.affected_cnt.get(&key).copied().unwrap_or(0.0);
                if let Some(&i) = m.index.get(&key) {
                    let (vx, vy) = (V::Num(x), V::Num(y));
                    if m.tiles[i].movers.iter().any(|o| lodash_match(&V::Obj(o.clone()), atom!("x"), &vx) && lodash_match(&V::Obj(o.clone()), atom!("y"), &vy)) {
                        rate1 = 100.0;
                    }
                }
                let rate2 = if truthy(&object.get(atom!("_pulled"))) { 1.0 } else { 0.0 };
                let rate3 = if truthy(&object.get(atom!("_pull"))) { 1.0 } else { 0.0 };
                rates.push((object.clone(), [rate1, rate2, rate3, moves / weight]));
            }
            // b.rate1 - a.rate1 || … (finite rates: a consistent order, so any stable sort agrees with V8's).
            rates.sort_by(|a, b| {
                let d = (0..4).map(|k| b.1[k] - a.1[k]).find(|d| *d != 0.0 && !d.is_nan()).unwrap_or(0.0);
                d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            rates.swap_remove(0).0
        } else {
            movers[0].clone()
        };
        scope.movement.objects.insert(to_js_string(&winner.get(atom!("_id"))), Some((x, y)));
        scope.movement.tiles[t].winner = Some(winner);
    }
    scope.movement.checked = true;

    // Second loop over `newMatrix`: keys deleted before their visit are skipped (for…in).
    for t in 0..scope.movement.tiles.len() {
        if scope.movement.tiles[t].deleted {
            continue;
        }
        let key = scope.movement.tiles[t].key.clone();
        let (x, y) = parse_key(&key);
        let object = scope.movement.tiles[t].winner.clone().expect("every tile has a winner after the first loop");
        let pulled = object.get(atom!("_pulled"));
        if truthy(&pulled) {
            if let Some(puller) = scope.room_objects.get(&to_js_string(&pulled)) {
                let puller_tile = Key::of(&puller.get(atom!("x")), &puller.get(atom!("y")));
                if !loose_eq(&puller.get(atom!("_pull")), &object.get(atom!("_id"))) || key != puller_tile {
                    puller.delete(atom!("_pull"));
                    object.delete(atom!("_pulled"));
                }
            }
        }
        if !can_move(&object)? || check_obstacle_at_xy(x, y, &object, room_is_in_safe_mode, scope) {
            remove_from_matrix(scope, key);
        }
    }
    Ok(())
}

/// movement.js:154 removeFromMatrix (its recursion as a loop).
fn remove_from_matrix(scope: &mut RoomScope, mut key: Key) {
    loop {
        let m = &mut scope.movement;
        let Some(&i) = m.index.get(&key) else { return };
        let object = m.tiles[i].winner.clone().expect("a live tile has a winner");
        m.objects.insert(to_js_string(&object.get(atom!("_id"))), None);
        m.tiles[i].deleted = true;
        let next = Key::of(&object.get(atom!("x")), &object.get(atom!("y")));
        if m.tile(&next).is_none() {
            return;
        }
        key = next;
    }
}

/// movement.js:189 execute, at the start of the mover's own tick.
pub fn execute(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    let Some((mx, my)) = scope.movement.has_move(&object.get(atom!("_id"))) else { return Ok(()) };
    if !can_move(object)? {
        return Ok(());
    }
    let (vx, vy) = (V::Num(mx), V::Num(my));
    let cell_objects: Vec<O> = scope.room_objects.near(&vx, &vy).filter(|i| loose_eq(&i.get(atom!("x")), &vx) && loose_eq(&i.get(atom!("y")), &vy)).cloned().collect();
    let is_type = |i: &O, t: &str| i.is_str(atom!("type"), t);

    let mut fatigue_rate = 2.0;
    if cell_objects.iter().any(|i| is_type(i, "swamp")) || utils::check_terrain(scope.room_terrain, mx, my, TERRAIN_MASK_SWAMP) {
        fatigue_rate = 10.0;
    }
    let is_pc = eq_str(&object.get(atom!("type")), "powerCreep");
    if let Some(road) = cell_objects.iter().find(|i| is_type(i, "road")) {
        fatigue_rate = 1.0;
        let wear = if is_pc { ROAD_WEAROUT_POWER_CREEP as f64 } else { ROAD_WEAROUT as f64 * to_number(&prop(&object.get(atom!("body")), atom!("length"))?) };
        let next = V::Num(to_number(&road.get(atom!("nextDecayTime"))) - wear);
        road.set(atom!("nextDecayTime"), next.clone());
        scope.bulk.update(crate::bulk::Target::Obj(road), &obj! { "nextDecayTime" => next })?;
    }

    let user = object.get(atom!("user"));
    let walk_over_sites = match &scope.room_controller {
        None => true,
        Some(c) => strict_eq(&c.get(atom!("user")), &user) || !gt(&c.get(atom!("safeMode")), &V::Num(scope.game_time as f64)),
    };
    if walk_over_sites {
        let site = cell_objects.iter().find(|i| eq_str(&i.get(atom!("type")), "constructionSite") && !loose_eq(&i.get(atom!("user")), &user));
        if let Some(site) = site {
            bulk_remove(&mut scope.bulk, &site.get(atom!("_id")));
            if gt(&site.get(atom!("progress")), &V::Num(1.0)) {
                let amount = (to_number(&site.get(atom!("progress"))) / 2.0).floor();
                super::_create_energy::create_energy(to_number(&site.get(atom!("x"))), to_number(&site.get(atom!("y"))), &to_js_string(&site.get(atom!("room"))), amount, "energy", scope)?;
            }
        }
    }

    let mut fatigue = V::Undef;
    if eq_str(&object.get(atom!("type")), "creep") {
        let f = non_move_carry_parts(object)? + calc_resources_weight(object)?;
        fatigue = V::Num(f * fatigue_rate);
    }

    let target = obj! { "x" => vx.clone(), "y" => vy.clone() };
    if utils::is_at_edge(&target) && !utils::is_at_edge(&V::Obj(object.clone())) {
        fatigue = V::Num(0.0);
        object.set(atom!("_fatigue"), V::Num(0.0));
        scope.bulk.update(crate::bulk::Target::Obj(object), &obj! { "x" => vx, "y" => vy, "fatigue" => fatigue })?;
    } else {
        scope.bulk.update(crate::bulk::Target::Obj(object), &obj! { "x" => vx, "y" => vy })?;
        if eq_str(&object.get(atom!("type")), "creep") {
            super::creeps::_add_fatigue::add_fatigue(object, to_number(&fatigue), scope)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Profile;
    use crate::processor::intents::_calc_spawns::oracle::with_scope;

    /// The two forms of a key: every text that names a tile is that tile, whatever value printed it.
    #[test]
    fn keys_are_the_text_of_the_tile() {
        let arr = |x: f64| V::Arr(crate::live::A::from_vec(vec![V::Num(x)]));
        assert!(Key::of(&V::Num(5.0), &V::Num(7.0)) == Key::Tile(257));
        assert!(Key::of(&V::str("5"), &V::Num(7.0)) == Key::Tile(257) && Key::of(&arr(5.0), &V::str("7")) == Key::Tile(257));
        assert!(Key::of(&V::Num(-0.0), &V::Num(49.0)) == Key::Tile(49) && Key::of(&V::str("0"), &V::str("49")) == Key::Tile(49));
        assert!(Key::of(&V::str("05"), &V::Num(7.0)) == Key::Text("05,7".into()) && Key::of(&V::Num(50.0), &V::Num(7.0)) == Key::Text("50,7".into()));
        assert!(Key::of(&V::Num(1.5), &V::Undef) == Key::Text("1.5,undefined".into()) && Key::of(&V::str("1,2"), &V::Num(3.0)) == Key::Text("1,2,3".into()));
        for key in ["5,7", "0,0", "49,49", "1.5,3", "50,1", "1,2,3", "x,y"] {
            let t = Key::text(key.to_string());
            let parts: Vec<&str> = key.split(',').collect();
            let want = (parse_int(&V::str(parts[0])), parts.get(1).map_or(f64::NAN, |p| parse_int(&V::str(p))));
            let got = parse_key(&t);
            assert!(got.0.to_bits() == want.0.to_bits() && got.1.to_bits() == want.1.to_bits(), "{key}");
        }
    }

    /// The tile index answers as the full scan does, also for x/y that are strings, arrays, -0 or fractions.
    #[test]
    fn by_tile_matches_the_full_scan() {
        let objects = r#"[{"_id":"c1","type":"creep","user":"u1","x":0,"y":0},
            {"_id":"w1","type":"constructedWall","x":"5","y":5},
            {"_id":"w2","type":"constructedWall","x":[6],"y":6},
            {"_id":"w3","type":"constructedWall","x":7,"y":-0},
            {"_id":"w4","type":"constructedWall","x":8.5,"y":8},
            {"_id":"w5","type":"constructedWall","x":9,"y":9},
            {"_id":"r1","type":"rampart","user":"u2","x":10,"y":10}]"#;
        with_scope(Profile::Private, objects, "[]", |scope| {
            let mover = scope.room_objects.get("c1").unwrap();
            let safe = V::Bool(false);
            let blocked: Vec<(f64, f64)> = (0..50)
                .flat_map(|x| (0..50).map(move |y| (x as f64, y as f64)))
                .filter(|&(x, y)| {
                    let fast = check_obstacle_at_xy(x, y, &mover, &safe, scope);
                    assert_eq!(fast, obstacle_at_xy(scope.room_objects.iter(), x, y, &mover, &safe, scope), "{x},{y}");
                    fast
                })
                .collect();
            // c1 itself: a creep without a registered move
            assert_eq!(blocked, [(0.0, 0.0), (5.0, 5.0), (6.0, 6.0), (7.0, 0.0), (9.0, 9.0), (10.0, 10.0)]);
            assert!(check_obstacle_at_xy(f64::NAN, 5.0, &mover, &safe, scope) == obstacle_at_xy(scope.room_objects.iter(), f64::NAN, 5.0, &mover, &safe, scope));
        });
    }
}
