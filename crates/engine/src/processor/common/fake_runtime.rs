//! mirror: reference/engine/src/processor/common/fake-runtime.js
//! parity: FAITHFUL
//!
//! The NPC AI's RoomPosition/CostMatrix/findPath/moveTo over the official PathFinder port (crates/pathfinder)
//! through the driver glue's option handling (driver/lib/path-finder.js:86-141). A RoomPosition is a plain
//! `{x, y, roomName}` object; paths are `Vec` of them. `lookFor`, `sUnpackLocal` and `CostMatrix` are only used
//! internally here. One `PathFinder` per thread.

use crate::atom;
use crate::constants as C;
use crate::live::{to_js_string, to_number, truthy, Crash, O, V};
use crate::processor::global::obj;
use crate::processor::{get_or_undef, loose_eq, RoomScope};
use crate::room_name::RoomXY;
use crate::utils::{dist, get_direction};
use pathfinder::{Goal, PathFinder, Pos, RoomCost, SearchOpts};
use std::cell::{Cell, RefCell};

thread_local! {
    static PF: RefCell<PathFinder> = RefCell::new(PathFinder::new());
}

/// The options the NPC AI passes (literal objects in vanilla, so plain fields here).
#[derive(Clone, Default)]
pub struct Opts {
    pub ignore_roads: bool,
    pub ignore_creeps: bool,
    pub ignore_destructible: bool,
    pub flee: bool,
    /// findAttack's `costCallbackIgnoreRamparts`: these objects' tiles get cost 0 in a clone of the matrix.
    pub zero_tiles: Option<Vec<O>>,
    /// A costCallback that returns its own CostMatrix (the strongholds' safeMatrixCallback): it replaces the
    /// default matrix.
    pub replace_matrix: Option<std::rc::Rc<Vec<u8>>>,
    pub range: Option<f64>,
    pub reuse_path: Option<f64>,
}

/// The goal argument of findPath: a RoomPosition (range 0, and the target-append rule) or `{range, pos}` goals.
pub enum Target {
    Pos(V),
    Goals(Vec<(f64, V)>),
}

/// `new RoomPosition(x, y, roomName)`.
pub fn position(x: &V, y: &V, room: &V) -> Result<V, Crash> {
    let (x, y) = (to_number(x), to_number(y));
    if x.is_nan() || y.is_nan() || !matches!(room, V::Str(_)) {
        return Err(Crash("invalid arguments in RoomPosition constructor".into()));
    }
    Ok(obj(vec![("x", V::Num(x)), ("y", V::Num(y)), ("roomName", room.clone())]))
}

/// The position of a room object.
pub fn pos_of(o: &O) -> Result<V, Crash> {
    position(&o.get(atom!("x")), &o.get(atom!("y")), &o.get(atom!("room")))
}

fn pf_pos(p: &V) -> Result<Pos, Crash> {
    let bad = || Crash("invalid position for PathFinder".into());
    let room = RoomXY::parse_engine(&to_js_string(&get_or_undef(p, atom!("roomName")))).ok_or_else(bad)?;
    let c = |k| {
        let v = to_number(&get_or_undef(p, k));
        if (0.0..50.0).contains(&v) {
            Ok(v as u8)
        } else {
            Err(bad())
        }
    };
    Ok(Pos { room: pathfinder::RoomXY { x: room.x, y: room.y }, x: c(atom!("x"))?, y: c(atom!("y"))? })
}

fn same_room(a: &V, b: &V) -> bool {
    loose_eq(&get_or_undef(a, atom!("roomName")), &get_or_undef(b, atom!("roomName")))
}

/// RoomPosition.prototype.getRangeTo.
fn range_to(a: &V, b: &V) -> f64 {
    if same_room(a, b) {
        dist(b, a)
    } else {
        f64::INFINITY
    }
}

/// fake-runtime.js:110 defaultCostMatrix for the creep's own room (other rooms return false).
fn cost_matrix(opts: &Opts, creep: &O, scope: &RoomScope) -> Vec<u8> {
    let mut costs = vec![0u8; 2500];
    let excluded = |t: &str| {
        (opts.ignore_destructible
            && matches!(t, "constructedWall" | "rampart" | "spawn" | "extension" | "link" | "storage" | "observer" | "tower" | "powerBank" | "powerSpawn" | "lab" | "terminal"))
            || (opts.ignore_creeps && t == "creep")
    };
    let obstacle = |t: &str| (C::OBSTACLE_OBJECT_TYPES.contains(&t) || t == C::STRUCTURE_PORTAL) && !excluded(t);
    let set = |costs: &mut Vec<u8>, o: &O, v: u8| {
        let xx = to_number(&o.get(atom!("x"))) as i32; // `xx|0` (no larger values reach the room heap)
        let yy = to_number(&o.get(atom!("y"))) as i32;
        let i = xx * 50 + yy;
        if (0..2500).contains(&i) {
            costs[i as usize] = v;
        }
    };
    let user = creep.get(atom!("user"));
    for key in scope.room_objects.keys() {
        let Some(o) = scope.room_objects.get(&key) else { continue };
        let ty = to_js_string(&o.get(atom!("type")));
        let rampart = !opts.ignore_destructible && &*ty == "rampart" && !truthy(&o.get(atom!("isPublic"))) && !loose_eq(&o.get(atom!("user")), &user);
        let site = !opts.ignore_destructible
            && &*ty == "constructionSite"
            && loose_eq(&o.get(atom!("user")), &user)
            && C::OBSTACLE_OBJECT_TYPES.contains(&&*to_js_string(&o.get(atom!("structureType"))));
        if obstacle(&ty) || rampart || site {
            set(&mut costs, &o, 255);
        }
        // `costs.get(x, y) < Infinity` always holds, so a road always ends at 1
        if !opts.ignore_roads && &*ty == "road" {
            set(&mut costs, &o, 1);
        }
    }
    if let Some(tiles) = &opts.zero_tiles {
        for o in tiles {
            set(&mut costs, o, 0);
        }
    }
    costs
}

/// fake-runtime.js:144 findPath: the path (RoomPositions, origin excluded).
pub fn find_path(source: &O, target: &Target, opts: &Opts, scope: &RoomScope) -> Result<Vec<V>, Crash> {
    let goals: Vec<Goal> = match target {
        Target::Pos(p) => vec![Goal { pos: pf_pos(p)?, range: 0 }],
        Target::Goals(gs) => gs.iter().map(|(r, p)| Ok(Goal { pos: pf_pos(p)?, range: (*r as i32).max(0) as u32 })).collect::<Result<_, Crash>>()?,
    };
    let from = pos_of(source)?;
    let origin = pf_pos(&from)?;
    let (plain, swamp) = if opts.ignore_roads { (0, 0) } else { (2, 10) };
    let search_opts = SearchOpts::from_js(plain, swamp, None, 0, 0, 1, opts.flee);
    let room = to_js_string(&source.get(atom!("room")));
    let own = RoomXY::parse_engine(&room).map(|r| pathfinder::RoomXY { x: r.x, y: r.y });
    let mut matrix = match &opts.replace_matrix {
        Some(m) => m.to_vec(),
        None => cost_matrix(opts, source, scope),
    };
    let cells = Cell::from_mut(&mut matrix[..]).as_slice_of_cells();
    let (meta, terrain) = (scope.ctx.meta, scope.ctx.terrain);
    let result = PF.with(|pf| {
        pf.borrow_mut().search(
            origin,
            &goals,
            &search_opts,
            |r| meta.idx(RoomXY { x: r.x, y: r.y }).map(|i| terrain.raw(i)),
            |r| if Some(r) == own { RoomCost::Matrix(cells) } else { RoomCost::Blocked },
        )
    });
    let result = result.map_err(|_| Crash("Could not load terrain data".into()))?;
    let mut path: Vec<V> = result
        .path
        .iter()
        .map(|p| {
            let name = if Some(p.room) == own { room.to_string() } else { RoomXY { x: p.room.x, y: p.room.y }.to_string() };
            obj(vec![("x", V::Num(p.x as f64)), ("y", V::Num(p.y as f64)), ("roomName", V::str(&name))])
        })
        .collect();
    if let Target::Pos(t) = target {
        let no_range = opts.range.is_none_or(|r| r == 0.0 || r.is_nan());
        let near = match path.last() {
            Some(last) => range_to(last, t) == 1.0,
            None => range_to(&from, t) == 1.0,
        };
        if no_range && near {
            path.push(t.clone());
        }
    }
    Ok(path)
}

/// fake-runtime.js:189 flee: the direction away from `hostiles`, 0 when there is no path.
pub fn flee(creep: &O, hostiles: &[O], range: f64, scope: &RoomScope) -> Result<f64, Crash> {
    let danger = hostiles.iter().map(|c| Ok((range, pos_of(c)?))).collect::<Result<Vec<_>, Crash>>()?;
    let path = find_path(creep, &Target::Goals(danger), &Opts { flee: true, ..Opts::default() }, scope)?;
    let Some(p) = path.first() else { return Ok(0.0) };
    let c = |o: &V, k| to_number(&get_or_undef(o, k));
    let cv = V::Obj(creep.clone());
    Ok(dir(c(p, atom!("x")) - c(&cv, atom!("x")), c(p, atom!("y")) - c(&cv, atom!("y"))))
}

fn dir(dx: f64, dy: f64) -> f64 {
    get_direction(dx, dy).map_or(0.0, f64::from)
}

/// `String.fromCharCode(32 + (x << 6 | y))`.
fn pack_local(x: f64, y: f64) -> u16 {
    let (x, y) = (x as i32 as u32 & 0x3f, y as i32 as u32 & 0x3f);
    (32 + ((x << 6) | y)) as u16
}

/// RoomPosition.sUnpackLocal of one UTF-16 unit.
pub(crate) fn unpack_local(unit: u16, room: &V) -> Result<V, Crash> {
    if unit < 32 {
        return Err(Crash(format!("Invalid uint value {unit}").into()));
    }
    let u = (unit - 32) as u32;
    position(&V::Num(((u >> 6) & 0x3f) as f64), &V::Num((u & 0x3f) as f64), room)
}

fn units(v: &V) -> Vec<u16> {
    to_js_string(v).encode_utf16().collect()
}

/// fake-runtime.js:286 nextDirectionByPath.
fn next_direction_by_path(creep: &O, path: &V) -> Result<f64, Crash> {
    let path = match path {
        V::Str(_) => units(path),
        V::Undef | V::Null => return Err(Crash("Cannot read properties of null (reading 'indexOf')".into())),
        other => units(other),
    };
    let (cx, cy) = (to_number(&creep.get(atom!("x"))), to_number(&creep.get(atom!("y"))));
    let here = pack_local(cx, cy);
    let index = path.iter().position(|&u| u == here).map_or(-1, |i| i as i64);
    if index == path.len() as i64 - 1 {
        return Ok(0.0);
    }
    let room = creep.get(atom!("room"));
    let next = if index < 0 {
        let first = unpack_local(path[0], &room)?;
        (dist(&V::Obj(creep.clone()), &first) <= 1.0).then_some(first)
    } else {
        Some(unpack_local(path[index as usize + 1], &room)?)
    };
    let Some(next) = next else { return Ok(0.0) };
    let c = |k| to_number(&get_or_undef(&next, k));
    Ok(dir(c(atom!("x")) - cx, c(atom!("y")) - cy))
}

/// fake-runtime.js:205 moveTo: the direction to move (0 for none); keeps the path in `memory_move`.
pub fn move_to(creep: &O, target: &O, opts: Opts, scope: &mut RoomScope) -> Result<f64, Crash> {
    let reuse = opts.reuse_path.unwrap_or(5.0);
    let range = opts.range.unwrap_or(0.0);
    let (cv, tv) = (V::Obj(creep.clone()), V::Obj(target.clone()));
    if dist(&cv, &tv) <= range {
        return Ok(0.0);
    }
    let game_time = scope.game_time as f64;
    let target_pos = pos_of(target)?;
    let dest = String::from_utf16_lossy(&[pack_local(to_number(&target.get(atom!("x"))), to_number(&target.get(atom!("y"))))]);
    let mm = creep.get(atom!("memory_move"));
    let field = |k| get_or_undef(&mm, k);
    let stale = !truthy(&mm)
        || !truthy(&field(atom!("dest")))
        || !truthy(&field(atom!("time")))
        || !loose_eq(&field(atom!("dest")), &V::str(&dest))
        || game_time > to_number(&crate::processor::js_add(&field(atom!("time")), &V::Num(reuse)));
    if stale {
        let path = find_path(creep, &Target::Goals(vec![(range, target_pos)]), &Opts { range: Some(range), reuse_path: Some(reuse), ..opts }, scope)?;
        let packed: Vec<u16> = path.iter().map(|p| pack_local(to_number(&get_or_undef(p, atom!("x"))), to_number(&get_or_undef(p, atom!("y"))))).collect();
        let memory_move = obj(vec![("dest", V::str(&dest)), ("path", V::Str(String::from_utf16_lossy(&packed).into())), ("time", V::Num(game_time))]);
        crate::processor::update(scope, creep, &obj(vec![("memory_move", memory_move)]))?;
    }
    let direction = next_direction_by_path(creep, &get_or_undef(&creep.get(atom!("memory_move")), atom!("path")))?;
    if direction != 0.0 {
        crate::processor::update(scope, creep, &obj(vec![("memory_move", obj(vec![("lastMove", V::Num(game_time))]))]))?;
    }
    Ok(direction)
}

/// The NPC `intents` helper: `set(id, name, data)` into `list[id][name]`.
pub struct Intents(pub O);

impl Intents {
    pub fn new() -> Intents {
        Intents(O::new())
    }
    pub fn set(&self, id: &V, name: &str, data: V) {
        let key = crate::doc::Atom::intern(&to_js_string(id));
        let entry = match self.0.get(key) {
            V::Obj(o) => o,
            _ => {
                let o = O::new();
                self.0.set(key, V::Obj(o.clone()));
                o
            }
        };
        entry.set(crate::doc::Atom::intern(name), data);
    }
    pub fn list(&self) -> V {
        V::Obj(self.0.clone())
    }
}

impl Default for Intents {
    fn default() -> Self {
        Intents::new()
    }
}

/// fake-runtime.js:234 walkTo: moveTo, and a resting own creep ahead is asked to swap places.
pub fn walk_to(creep: &O, target: &O, opts: Opts, intents: &Intents, scope: &mut RoomScope) -> Result<f64, Crash> {
    let direction = move_to(creep, target, opts, scope)?;
    if direction == 0.0 {
        return Ok(direction);
    }
    let (ox, oy) = crate::utils::get_offsets_by_direction(&V::Num(direction)).unwrap_or((f64::NAN, f64::NAN));
    let (x, y) = (to_number(&creep.get(atom!("x"))) + ox, to_number(&creep.get(atom!("y"))) + oy);
    let user = creep.get(atom!("user"));
    let game_time = scope.game_time as f64;
    // _.find(roomObjects, {type: 'creep', user, x, y})
    let ahead = scope.room_objects.iter().find(|o| {
        crate::processor::eq_str(&o.get(atom!("type")), "creep")
            && crate::live::is_equal(&o.get(atom!("user")), &user)
            && crate::live::is_equal(&o.get(atom!("x")), &V::Num(x))
            && crate::live::is_equal(&o.get(atom!("y")), &V::Num(y))
    }).cloned();
    if let Some(ahead) = ahead {
        let mm = ahead.get(atom!("memory_move"));
        let last = get_or_undef(&mm, atom!("lastMove"));
        if !truthy(&mm) || (truthy(&last) && to_number(&crate::processor::js_add(&last, &V::Num(1.0))) < game_time) {
            let d = dir(to_number(&creep.get(atom!("x"))) - to_number(&ahead.get(atom!("x"))), to_number(&creep.get(atom!("y"))) - to_number(&ahead.get(atom!("y"))));
            intents.set(&ahead.get(atom!("_id")), "move", obj(vec![("direction", if d == 0.0 { V::Undef } else { V::Num(d) })]));
            let reset = obj(vec![("dest", V::Null), ("time", V::Null), ("path", V::Null), ("lastMove", V::Num(game_time))]);
            crate::processor::update(scope, &ahead, &obj(vec![("memory_move", reset)]))?;
        }
    }
    intents.set(&creep.get(atom!("_id")), "move", obj(vec![("direction", V::Num(direction))]));
    Ok(direction)
}

/// fake-runtime.js:249 findClosestByPath.
pub fn find_closest_by_path(from: &O, objects: &[O], opts: Opts, scope: &RoomScope) -> Result<Option<O>, Crash> {
    if objects.is_empty() {
        return Ok(None);
    }
    let fv = V::Obj(from.clone());
    if let Some(here) = objects.iter().find(|o| dist(&fv, &V::Obj((*o).clone())) == 0.0) {
        return Ok(Some(here.clone()));
    }
    let goals = objects.iter().map(|o| Ok((1.0, pos_of(o)?))).collect::<Result<Vec<_>, Crash>>()?;
    let path = find_path(from, &Target::Goals(goals), &Opts { range: Some(opts.range.unwrap_or(0.0)), ..opts }, scope)?;
    let last = path.last().cloned().unwrap_or(fv);
    Ok(objects.iter().filter(|o| dist(&last, &V::Obj((*o).clone())) <= 1.0).next_back().cloned())
}

/// fake-runtime.js:309 hasActiveBodyparts.
pub fn has_active_bodyparts(creep: &O, part: &str) -> bool {
    match creep.get(atom!("body")) {
        V::Arr(body) => body.to_vec().iter().any(|p| crate::processor::gt(&get_or_undef(p, atom!("hits")), &V::Num(0.0)) && crate::processor::eq_str(&get_or_undef(p, atom!("type")), part)),
        V::Obj(body) => body.keys().into_iter().any(|k| {
            let p = body.get(k);
            crate::processor::gt(&get_or_undef(&p, atom!("hits")), &V::Num(0.0)) && crate::processor::eq_str(&get_or_undef(&p, atom!("type")), part)
        }),
        _ => false,
    }
}
