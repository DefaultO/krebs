//! mirror: reference/common/index.js
//! parity: FAITHFUL
//!
//! The game-data half of `@screeps/common`: getDiff (FMT-ROOM-DIFF), roomNameToXY, calcWorldSize (BUG-WORLDSIZE
//! replicated, a COMPAT `R` row) and the terrain codecs (TERRAIN-CODEC, TERRAIN-CHECK). findPort, getGametime and
//! qSequence are process plumbing with no counterpart here; getRoomNameFromXY is `room_name::RoomXY`'s Display.
//! Fixtures: `node tools/oracle/gen/diff-fixture.js` → tests/fixtures/world/common.ndjson.

use crate::doc::{array_index, Atom, Obj, Val};
use std::borrow::Cow;
use std::collections::HashMap;
use util::json::Json;

// ---- getDiff (index.js:69-136) ----

/// `getDiff(oldData, newData)`: the room-socket object diff. Keys are `""+obj._id`; a removed object maps to
/// `null`, a new one to the whole document, a changed one to `{key: new value}` with removed keys as `null`
/// and object/array values diffed one level deep, where a removed nested key holds `undefined` and so vanishes
/// from the JSON (the "dropped nested deletion" of FMT-ROOM-DIFF). Docs are compared by pointer first: a
/// shared `Arc<Doc>` has an empty diff without a key walk.
///
/// Not modelled: ids naming `Object.prototype` members (`"constructor"`, …), which vanilla's plain-object index
/// would find there. chitin ids are generated hex or imported ObjectIds.
pub fn get_diff<'a>(old_data: impl IntoIterator<Item = &'a Obj>, new_data: impl IntoIterator<Item = &'a Obj>) -> Json {
    let old_data: Vec<&Obj> = old_data.into_iter().collect();
    let new_data: Vec<&Obj> = new_data.into_iter().collect();
    // getIndex: `_.forEach(data, obj => index[obj._id] = obj)`, the last duplicate wins.
    let index = |data: &[&'a Obj]| -> HashMap<Cow<'a, str>, &'a Obj> { data.iter().map(|&o| (id_key(o), o)).collect() };
    let (old_index, new_index) = (index(&old_data), index(&new_data));
    let mut result = JsMap::default();

    for &obj in &old_data {
        let id = id_key(obj);
        let Some(&new_obj) = new_index.get(&id) else {
            result.set(&id, Some(Json::Null));
            continue;
        };
        // `var objDiff = result[obj._id] = {}`: every key below is fresh (the second loop only adds keys `obj`
        // lacks), and a delete only ever removes the key just set, so a Vec is the whole JS object.
        let mut obj_diff: Vec<(Atom, Option<Json>)> = Vec::new();
        if !std::ptr::eq(obj, new_obj) {
            for (key, value) in obj.iter() {
                match new_obj.get(key) {
                    None => obj_diff.push((key, Some(Json::Null))),
                    Some(nv) if type_of(value) != type_of(nv) || truthy(value) && !truthy(nv) => obj_diff.push((key, Some(nv.to_json()))),
                    Some(nv) if is_object(value) => {
                        let mut sub: Vec<(String, Option<Json>)> = Vec::new();
                        for sk in for_in(value) {
                            let nsv = prop(nv, &sk);
                            if !opt_equal(prop(value, &sk).as_deref(), nsv.as_deref()) {
                                sub.push((sk.into_owned(), nsv.map(|v| v.to_json())));
                            }
                        }
                        for sk in for_in(nv) {
                            if prop(value, &sk).is_none() {
                                let nsv = prop(nv, &sk).map(|v| v.to_json());
                                sub.push((sk.into_owned(), nsv));
                            }
                        }
                        // `_.size` counts keys holding undefined, so `{store: {energy: undefined}}` stays as `{}`.
                        if size(sub.iter().map(|(k, v)| (k.as_str(), v))) != 0 {
                            obj_diff.push((key, Some(js_object(sub))));
                        }
                    }
                    Some(nv) if !val_equal(value, nv) => obj_diff.push((key, Some(nv.to_json()))),
                    Some(_) => {}
                }
            }
            for (key, value) in new_obj.iter() {
                if obj.get(key).is_none() {
                    obj_diff.push((key, Some(value.to_json())));
                }
            }
        }
        if size(obj_diff.iter().map(|(k, v)| (k.as_str(), v))) == 0 {
            result.delete(&id);
        } else {
            result.set(&id, Some(js_object(obj_diff.into_iter().map(|(k, v)| (k.as_str().to_string(), v)).collect())));
        }
    }

    for &obj in &new_data {
        let id = id_key(obj);
        if !old_index.contains_key(&id) {
            result.set(&id, Some(Val::Obj(obj.clone()).to_json()));
        }
    }
    result.into_json()
}

/// A JS object built by assignments: setting an existing key keeps its position. `None` is a property holding
/// `undefined`.
#[derive(Default)]
struct JsMap {
    slots: Vec<Option<(String, Option<Json>)>>,
    pos: HashMap<String, usize>,
}

impl JsMap {
    fn set(&mut self, k: &str, v: Option<Json>) {
        match self.pos.get(k) {
            Some(&i) => self.slots[i].as_mut().expect("live slot").1 = v,
            None => {
                self.pos.insert(k.to_string(), self.slots.len());
                self.slots.push(Some((k.to_string(), v)));
            }
        }
    }

    fn delete(&mut self, k: &str) {
        if let Some(i) = self.pos.remove(k) {
            self.slots[i] = None;
        }
    }

    fn into_json(self) -> Json {
        js_object(self.slots.into_iter().flatten().collect())
    }
}

/// lodash 3 `_.size(object)`: a `length` property holding a valid array length (`isLength`) is taken as the size,
/// so a diff like `{length: 0}` counts as empty and is deleted; otherwise the key count, undefined values included.
fn size<'k>(members: impl Iterator<Item = (&'k str, &'k Option<Json>)> + Clone) -> usize {
    match members.clone().find(|(k, _)| *k == "length") {
        Some((_, Some(Json::Num(n)))) if *n > -1.0 && n.fract() == 0.0 && *n <= 9007199254740991.0 => *n as usize,
        _ => members.count(),
    }
}

/// JSON.stringify of a JS object: canonical array-index keys first (ascending), then insertion order;
/// properties holding `undefined` are skipped.
fn js_object(mut members: Vec<(String, Option<Json>)>) -> Json {
    members.sort_by_key(|(k, _)| array_index(k).map_or((1, 0), |i| (0, i))); // stable
    Json::Obj(members.into_iter().filter_map(|(k, v)| Some((k, v?))).collect())
}

/// `""+obj._id` as a property key.
fn id_key(o: &Obj) -> Cow<'_, str> {
    match o.get(crate::atom!("_id")) {
        Some(Val::Str(s)) => Cow::Borrowed(s),
        Some(v) => Cow::Owned(crate::live::to_js_string(&crate::live::from_val(v)).to_string()),
        None => Cow::Borrowed("undefined"),
    }
}

/// `for (k in v)` over a JSON value: array indices or own keys, in JS order.
fn for_in(v: &Val) -> Vec<Cow<'static, str>> {
    match v {
        Val::Arr(a) => (0..a.len()).map(|i| Cow::Owned(i.to_string())).collect(),
        Val::Obj(o) => o.iter().map(|(k, _)| Cow::Borrowed(k.as_str())).collect(),
        _ => Vec::new(),
    }
}

/// `v[k]` on an object or array (`length` included); `None` = undefined.
fn prop<'v>(v: &'v Val, k: &str) -> Option<Cow<'v, Val>> {
    match v {
        Val::Arr(a) if k == "length" => Some(Cow::Owned(Val::Num(a.len() as f64))),
        Val::Arr(a) => a.get(array_index(k)? as usize).map(Cow::Borrowed),
        Val::Obj(o) => o.iter().find(|(a, _)| a.as_str() == k).map(|(_, x)| Cow::Borrowed(x)),
        _ => None,
    }
}

fn type_of(v: &Val) -> &'static str {
    match v {
        Val::Null | Val::Arr(_) | Val::Obj(_) => "object",
        Val::Bool(_) => "boolean",
        Val::Num(_) => "number",
        Val::Str(_) => "string",
    }
}

fn truthy(v: &Val) -> bool {
    match v {
        Val::Null => false,
        Val::Bool(b) => *b,
        Val::Num(n) => *n != 0.0 && !n.is_nan(),
        Val::Str(s) => !s.is_empty(),
        Val::Arr(_) | Val::Obj(_) => true,
    }
}

/// lodash `_.isObject`.
fn is_object(v: &Val) -> bool {
    matches!(v, Val::Arr(_) | Val::Obj(_))
}

/// lodash 3 `_.isEqual` over JSON values (no cycles, no NaN): key order is irrelevant, 0 equals -0.
fn val_equal(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Arr(x), Val::Arr(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| val_equal(p, q)),
        (Val::Obj(x), Val::Obj(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| val_equal(v, w))),
        _ => a == b,
    }
}

fn opt_equal(a: Option<&Val>, b: Option<&Val>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => val_equal(a, b),
        (a, b) => a.is_none() && b.is_none(),
    }
}

// ---- roomNameToXY / calcWorldSize (index.js:142-193) ----

/// `roomNameToXY(name)`: upper-cased, then `^(\w)(\d+)(\w)(\d+)$` (backtracking: when everything after the first
/// character is digits, the middle `\w` is the second-to-last digit, so `W105` is `[-2, 5]`). Any letter other
/// than `W` means east, other than `N` south. `None` = `[undefined, undefined]`.
pub fn room_name_to_xy(name: &str) -> Option<(f64, f64)> {
    let name = name.to_uppercase();
    let b = name.as_bytes();
    if b.len() < 4 || !b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') {
        return None;
    }
    let rest = &b[1..];
    let mid = rest.iter().position(|c| !c.is_ascii_digit()).unwrap_or(rest.len() - 2);
    if mid == 0 || mid + 1 >= rest.len() || !rest[mid + 1..].iter().all(u8::is_ascii_digit) {
        return None;
    }
    let num = |d: &[u8]| std::str::from_utf8(d).unwrap().parse::<f64>().unwrap();
    let (x, y) = (num(&rest[..mid]), num(&rest[mid + 1..]));
    Some((if b[0] == b'W' { -x - 1.0 } else { x }, if rest[mid] == b'N' { -y - 1.0 } else { y }))
}

/// `calcWorldSize(rooms)` over the `rooms` `_id`s. `maxX`/`maxY` start at 0 (BUG-WORLDSIZE: a phantom column or
/// row for a one-quadrant world); unparsable names change nothing; no rooms gives `-Infinity`; `Math.max` returns
/// NaN when a side is NaN (a 309-digit coordinate parses to Infinity, and Infinity − Infinity is NaN).
pub fn calc_world_size<'a>(names: impl IntoIterator<Item = &'a str>) -> f64 {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, 0.0f64, 0.0f64);
    for (x, y) in names.into_iter().filter_map(room_name_to_xy) {
        if x < min_x {
            min_x = x;
        }
        if y < min_y {
            min_y = y;
        }
        if x > max_x {
            max_x = x;
        }
        if y > max_y {
            max_y = y;
        }
    }
    let (w, h) = (max_x - min_x + 1.0, max_y - min_y + 1.0);
    if w.is_nan() || h.is_nan() {
        f64::NAN // f64::max would return the other side
    } else {
        w.max(h)
    }
}

// ---- terrain codecs (index.js:25-63) ----

/// `encodeTerrain(terrain)`: per tile, bit 1 if any item `{x, y}` (strict `===`) has `type === 'wall'`, bit 2 for
/// `'swamp'`. One pass instead of vanilla's `_.filter` per tile; same result.
pub fn encode_terrain(terrain: &[Json]) -> String {
    let mut codes = [0u8; 2500];
    for item in terrain {
        let (Some(Json::Num(x)), Some(Json::Num(y))) = (item.get("x"), item.get("y")) else { continue };
        let bit = match item.get("type") {
            Some(Json::Str(t)) if t == "wall" => 1,
            Some(Json::Str(t)) if t == "swamp" => 2,
            _ => continue,
        };
        if (0.0..50.0).contains(x) && (0.0..50.0).contains(y) && x.fract() == 0.0 && y.fract() == 0.0 {
            codes[*y as usize * 50 + *x as usize] |= bit;
        }
    }
    codes.iter().map(|c| (b'0' + c) as char).collect()
}

/// `decodeTerrain(str, room)`: `{room, x, y, type}` per wall bit, then per swamp bit, y-major. `charAt` reads
/// UTF-16 units; a non-digit is `NaN`/0 under `&`.
pub fn decode_terrain(s: &str, room: &str) -> Json {
    let units: Vec<u16> = s.encode_utf16().take(2500).collect();
    let mut out = Vec::new();
    for i in 0..2500usize {
        let code = units.get(i).filter(|u| (0x30..=0x39).contains(*u)).map_or(0, |u| u - 0x30);
        for (bit, kind) in [(1, "wall"), (2, "swamp")] {
            if code & bit != 0 {
                let tile = |n: usize| Json::Num(n as f64);
                out.push(Json::Obj(vec![
                    ("room".into(), Json::Str(room.into())),
                    ("x".into(), tile(i % 50)),
                    ("y".into(), tile(i / 50)),
                    ("type".into(), Json::Str(kind.into())),
                ]));
            }
        }
    }
    Json::Arr(out)
}

/// `checkTerrain(terrain, x, y, mask)`: `(parseInt(terrain.charAt(y*50 + x)) & mask) > 0`. Out-of-range
/// positions read `""` (NaN, not set); x outside 0..49 wraps into the neighbouring row (TERRAIN-CHECK).
pub fn check_terrain(terrain: &str, x: f64, y: f64, mask: i32) -> bool {
    let pos = y * 50.0 + x;
    let pos = if pos.is_nan() { 0.0 } else { pos.trunc() }; // charAt: ToIntegerOrInfinity
    if pos < 0.0 {
        return false;
    }
    let unit = terrain.encode_utf16().nth(pos as usize);
    unit.filter(|u| (0x30..=0x39).contains(u)).is_some_and(|u| (u - 0x30) as i32 & mask > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(s: &str) -> Obj {
        match Val::from_json(&Json::parse(s).unwrap()) {
            Val::Obj(o) => o,
            _ => panic!("not an object: {s}"),
        }
    }

    /// Every case of tests/fixtures/world/common.ndjson, produced by the official @screeps/common.
    #[test]
    fn fixtures() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/world/common.ndjson")).unwrap();
        let mut counts = HashMap::new();
        for line in text.lines() {
            let c = Json::parse(line).unwrap();
            let kind = c.get("kind").and_then(Json::as_str).unwrap();
            *counts.entry(kind.to_string()).or_insert(0) += 1;
            let arr = |k: &str| c.get(k).and_then(Json::as_array).unwrap().clone();
            match kind {
                "diff" => {
                    let docs = |k: &str| arr(k).iter().map(|d| match Val::from_json(d) { Val::Obj(o) => o, _ => panic!() }).collect::<Vec<_>>();
                    let (old, new) = (docs("old"), docs("new"));
                    let got = get_diff(&old, &new).stringify();
                    assert_eq!(got, c.get("out").unwrap().as_str().unwrap(), "{line}");
                }
                "xy" => {
                    let name = c.get("name").and_then(Json::as_str).unwrap();
                    let want = c.get("out").unwrap();
                    let got = room_name_to_xy(name).map_or(Json::Null, |(x, y)| Json::Arr(vec![Json::Num(x), Json::Num(y)]));
                    assert_eq!(got.stringify(), want.stringify(), "{name}");
                }
                "worldSize" => {
                    let names = arr("names");
                    let got = calc_world_size(names.iter().map(|n| n.as_str().unwrap()));
                    let want = c.get("out").unwrap();
                    // NaN / ±Infinity travel as strings.
                    let want = want.as_f64().unwrap_or_else(|| util::num::string_to_number(want.as_str().unwrap()));
                    assert!(got == want || got.is_nan() && want.is_nan(), "{got} {line}");
                }
                "encode" => assert_eq!(encode_terrain(&arr("terrain")), c.get("out").unwrap().as_str().unwrap(), "{line}"),
                "decode" => {
                    let s = c.get("str").and_then(Json::as_str).unwrap();
                    assert_eq!(decode_terrain(s, "W1N1").stringify(), c.get("out").unwrap().stringify(), "{s}");
                }
                "check" => {
                    let s = c.get("str").and_then(Json::as_str).unwrap();
                    for q in arr("queries") {
                        let q = q.as_array().unwrap();
                        let n = |i: usize| q[i].as_f64().unwrap();
                        assert_eq!(check_terrain(s, n(0), n(1), n(2) as i32), q[3].as_bool().unwrap(), "{s} {q:?}");
                    }
                }
                _ => {}
            }
        }
        for k in ["diff", "xy", "worldSize", "encode", "decode", "check"] {
            assert!(counts.get(k).copied().unwrap_or(0) > 0, "no {k} cases");
        }
    }

    #[test]
    fn diff_shapes() {
        let a = obj(r#"{"_id":"a","x":1,"store":{"energy":50},"body":[1,2],"n":null}"#);
        let b = obj(r#"{"_id":"a","x":2,"store":{},"body":[1,3],"n":null,"fresh":0}"#);
        let gone = obj(r#"{"_id":"g"}"#);
        let new = obj(r#"{"_id":"7","y":1}"#);
        let d = get_diff([&a, &gone], [&b, &new]).stringify();
        assert_eq!(d, r#"{"7":{"_id":"7","y":1},"a":{"x":2,"store":{},"body":{"1":3},"fresh":0},"g":null}"#);
        assert_eq!(get_diff([&a], [&a]).stringify(), "{}");
    }

    #[test]
    fn world_size_and_names() {
        assert_eq!(room_name_to_xy("w105"), Some((-2.0, 5.0)));
        assert_eq!(room_name_to_xy("E0S0"), Some((0.0, 0.0)));
        assert_eq!(room_name_to_xy("W1N"), None);
        assert_eq!(calc_world_size(["W5N5", "W1N1"]), 7.0);
        assert_eq!(calc_world_size(["W5N5", "E4S4"]), 11.0);
        assert_eq!(calc_world_size([]), f64::NEG_INFINITY);
        let huge = format!("E{}S1", "9".repeat(400));
        assert!(calc_world_size([huge.as_str()]).is_nan(), "Math.max(NaN, 1)");
        // charAt(NaN) reads index 0; negative positions read "".
        assert!(check_terrain("1", f64::NAN, 0.0, 1) && !check_terrain("1", -1.0, 0.0, 1) && check_terrain("1", -0.5, 0.0, 1));
    }
}
