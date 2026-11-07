//! Player intents after the system isolate's `__store`: parsing and per-room routing
//! (ARCHITECTURE.md §4.3, FROZEN). The split follows driver/lib/index.js:180-243 saveUserIntents.
//! Fixtures: tools/oracle/gen/system-fixture.js → tests/fixtures/system/store.ndjson.

use crate::doc::{Atom, Obj, Val};
use crate::room_name::RoomXY;
use crate::world::{RoomIdx, RoomsMeta, UserIdx};
use util::json::Json;

pub type IntentName = Atom;

pub struct UserIntents {
    /// Per room: (object id | "room", intent name → sanitized value).
    pub rooms: Vec<(RoomIdx, Vec<(Box<str>, Obj)>)>,
    /// Name → Val::Arr.
    pub global: Option<Obj>,
    pub notify: Vec<Val>,
    /// CPU-BUCKET-UPDATE count over the raw list.
    pub count: u32,
}

/// `json` is the whole `__store` output (§4.2). Its `intents` keys in order: `notify` (an array, else nothing:
/// vanilla's `forEach` throws), `global`, and room names; a name that is not an existing room is dropped
/// (BUG-CRASH-UNDEF-ROOM, F) while its intents stay in `count`.
pub fn parse_store_output(json: &Json, meta: &RoomsMeta) -> UserIntents {
    let mut out = UserIntents { rooms: Vec::new(), global: None, notify: Vec::new(), count: 0 };
    out.count = json.get("count").and_then(Json::as_f64).unwrap_or(0.0) as u32;
    for (key, value) in json.get("intents").and_then(Json::as_object).into_iter().flatten() {
        match key.as_str() {
            "notify" => out.notify = value.as_array().into_iter().flatten().map(Val::from_trusted_json).collect(),
            "global" => out.global = value.as_object().map(|_| obj(value)),
            room => {
                let Some(idx) = room_idx(meta, room) else { continue };
                let objects = value.as_object().into_iter().flatten();
                out.rooms.push((idx, objects.map(|(id, v)| (id.as_str().into(), obj(v))).collect()));
            }
        }
    }
    out
}

/// Keys are intent and field names (fixed tables), so interning them is fine.
fn obj(v: &Json) -> Obj {
    match Val::from_trusted_json(v) {
        Val::Obj(o) => o,
        _ => Obj::new(),
    }
}

/// The room called exactly `name`: vanilla saves intents under the string and the processor finds no such room
/// for any other spelling ("w1n1", "W01N1").
// ponytail: RoomsMeta::by_name (WP01) is a stub; this reads the grid row-major (cell = dy * w + dx) and checks the
// name, so it stays correct either way. Switch to by_name + the name check once it lands.
fn room_idx(meta: &RoomsMeta, name: &str) -> Option<RoomIdx> {
    let RoomXY { x, y } = RoomXY::parse_engine(name)?;
    let (dx, dy) = (i64::from(x) - i64::from(meta.x0), i64::from(y) - i64::from(meta.y0));
    if dx < 0 || dy < 0 || dx >= i64::from(meta.w) || dy >= i64::from(meta.h) {
        return None;
    }
    let idx = meta.grid.get((dy * i64::from(meta.w) + dx) as usize)?.checked_sub(1)?;
    let room = meta.rooms.get(idx as usize)?;
    (!room.removed && *room.name == *name).then_some(idx)
}

/// Arrival order; the processor applies ORD-POLICY.
#[derive(Clone)]
pub struct RoomIntents {
    pub users: Vec<UserRoomIntents>,
}

#[derive(Clone)]
pub struct UserRoomIntents {
    pub user: UserIdx,
    pub objects: Vec<(Box<str>, Obj)>,
    pub manual: Vec<(Box<str>, Obj)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::RoomMeta;

    fn meta(names: &[(&str, bool)]) -> RoomsMeta {
        let rooms: Vec<RoomMeta> = names
            .iter()
            .map(|&(n, removed)| RoomMeta { name: n.into(), xy: RoomXY::parse_engine(n).unwrap(), removed })
            .collect();
        let x0 = rooms.iter().map(|r| r.xy.x).min().unwrap();
        let y0 = rooms.iter().map(|r| r.xy.y).min().unwrap();
        let w = (rooms.iter().map(|r| r.xy.x).max().unwrap() - x0 + 1) as u32;
        let h = (rooms.iter().map(|r| r.xy.y).max().unwrap() - y0 + 1) as u32;
        let mut grid = vec![0; (w * h) as usize];
        for (i, r) in rooms.iter().enumerate() {
            grid[((r.xy.y - y0) as u32 * w + (r.xy.x - x0) as u32) as usize] = i as u32 + 1;
        }
        RoomsMeta { x0, y0, w, h, grid, rooms, world_size: 0 }
    }

    fn to_json(u: &UserIntents) -> String {
        let pair = |(id, o): &(Box<str>, Obj)| Json::Arr(vec![Json::Str(id.to_string()), Val::Obj(o.clone()).to_json()]);
        let room = |(r, objs): &(RoomIdx, Vec<(Box<str>, Obj)>)| {
            Json::Arr(vec![Json::Num(*r as f64), Json::Arr(objs.iter().map(pair).collect())])
        };
        Json::Obj(vec![
            ("rooms".into(), Json::Arr(u.rooms.iter().map(room).collect())),
            ("global".into(), u.global.clone().map_or(Json::Null, |g| Val::Obj(g).to_json())),
            ("notify".into(), Json::Arr(u.notify.iter().map(Val::to_json).collect())),
            ("count".into(), Json::Num(u.count as f64)),
        ])
        .stringify()
    }

    /// Our `__store` output (equal to the official storeIntents + intentsCount) → UserIntents, expected values
    /// computed by the generator from the official result.
    #[test]
    fn fixtures() {
        let text = include_str!("../tests/fixtures/system/store.ndjson");
        let mut lines = text.lines();
        let world = Json::parse(lines.next().unwrap()).unwrap();
        let names: Vec<(&str, bool)> = world
            .get("rooms")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r.at(0).unwrap().as_str().unwrap(), r.at(1).unwrap().as_bool().unwrap()))
            .collect();
        let meta = meta(&names);
        let mut n = 0;
        for (line_no, line) in lines.enumerate() {
            let case = Json::parse(line).unwrap();
            let out = Json::parse(case.get("out").unwrap().as_str().unwrap()).unwrap();
            let got = to_json(&parse_store_output(&out, &meta));
            assert_eq!(got, case.get("expect").unwrap().stringify(), "line {}", line_no + 2);
            n += 1;
        }
        assert!(n > 200, "{n} cases");
    }

    #[test]
    fn room_lookup_is_exact() {
        let m = meta(&[("W1N1", false), ("E0S0", false), ("W2N1", true)]);
        assert_eq!(room_idx(&m, "W1N1"), Some(0));
        assert_eq!(room_idx(&m, "E0S0"), Some(1));
        for bad in ["w1n1", "W01N1", "W1N1x", "W2N1", "W5N5", "undefined", "", "E0S0 "] {
            assert_eq!(room_idx(&m, bad), None, "{bad}");
        }
    }
}
