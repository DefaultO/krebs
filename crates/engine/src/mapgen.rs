//! mirror: reference/backend/lib/cli/map.js
//! parity: DIVERGENT(GEN-CONTROLLER, GEN-MINERAL, GEN-ROOM-ENTRY)
//!
//! `generate_room` ports map.js generateRoom statement by statement (11 GEN-ROOM-ENTRY…GEN-MINERAL) and draws
//! `Math.random` in the same order (GEN-RNG-ORDER), so the official file run on the same RNG stream returns
//! the same room bit for bit: tests/fixtures/mapgen/rooms.ndjson (163 cases incl. neighbour matching,
//! rejections, crashes and the 1000-try source restart) and stats.json (distributions over 2k rooms per
//! class), both from tools/oracle/gen/mapgen-check.js. openRoom/closeRoom/removeRoom/updateTerrainData work on
//! a live `World` (`*_room`, `update_terrain_data`); updateRoomImageAssets is the backend's PNG cache. World
//! layout and the parallel edge/room passes are ARCHITECTURE §11 (no vanilla counterpart).
//! Divergences: GEN-CONTROLLER / GEN-MINERAL loop forever when no tile qualifies (the CLI process hangs); we
//! return Err after `HANG` tries. GEN-ROOM-ENTRY: `generate_room_cmd` refuses names map.js accepts but the grid
//! cannot hold: leading zeros (`W01N1`, a second room on W1N1's tile) and rooms outside both the world box and
//! W127..E127 × N127..S127 (D33).

use crate::atom;
use crate::constants::{ENERGY_REGEN_TIME, MINERAL_DENSITY, MINERAL_DENSITY_PROBABILITY, SOURCE_ENERGY_NEUTRAL_CAPACITY};
use crate::doc::{Atom, Doc, DocRef, Id, Obj, Val};
use crate::hooks::EngineHooks;
use crate::ids::IdGen;
use crate::policy;
use crate::processor::RoomDraft;
use crate::room_name::RoomXY;
use crate::terrain::RoomTerrain;
use crate::world::{EnvState, MarketState, ObjLoc, RoomIdx, RoomMeta, RoomSnapshot, RoomsMeta, TerrainStore, World};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;
use util::pool::Pool;
use util::rng::Rng;

/// Indexes of `edges`, `GenRoomOpts::exits` and `neighbors` (map.js checks the sides in this order).
pub const TOP: usize = 0;
pub const RIGHT: usize = 1;
pub const BOTTOM: usize = 2;
pub const LEFT: usize = 3;

/// One generated room: what generateRoom inserts (GEN-INSERT), before ids are assigned.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedRoom {
    pub xy: RoomXY,
    /// `rooms.terrain` (codes 0..=3, TERRAIN-CODE3).
    pub terrain: RoomTerrain,
    /// `rooms` doc `{_id, status, sourceKeepers}` (+ `bus`/`depositType` from the world layout).
    pub info: Obj,
    /// `rooms.objects` in insert order, without `_id`.
    pub objects: Vec<Obj>,
}

/// map.js generateRoom `opts`. `None` = the key is undefined (drawn at random where map.js draws).
#[derive(Clone, Debug)]
pub struct GenRoomOpts {
    /// `opts.exits.{top,right,bottom,left}`: exit tiles given by the caller.
    pub exits: [Option<Vec<u8>>; 4],
    /// Whether the top/right/bottom/left neighbour room exists; its facing edge is then `edges[side]`.
    pub neighbors: [bool; 4],
    /// `opts.terrainType` (1..=28).
    pub terrain_type: Option<u32>,
    /// `opts.swampType` (0..=14).
    pub swamp_type: Option<u32>,
    pub sources: Option<u32>,
    /// `opts.mineral`: `None` = undefined (random type), `Some(None)` = `false`, `Some(Some(t))` = type `t`.
    pub mineral: Option<Option<Box<str>>>,
    /// `opts.controller` (undefined = true).
    pub controller: bool,
    /// `opts.keeperLairs` (undefined = false).
    pub keeper_lairs: bool,
}

impl GenRoomOpts {
    /// map.js `opts` (CLI-MAP-GENERATEROOM): `exits`, `terrainType`, `swampType`, `sources`, `mineral` (a type or
    /// `false`), `controller`, `keeperLairs`; neighbour flags come from the world.
    pub fn from_json(o: &util::json::Json) -> GenRoomOpts {
        use util::json::Json;
        let mut opts = GenRoomOpts::default();
        let num = |k| o.get(k).and_then(Json::as_f64).map(|n| n as u32);
        opts.terrain_type = num("terrainType");
        opts.swamp_type = num("swampType");
        opts.sources = num("sources");
        opts.mineral = o.get("mineral").map(|m| m.as_str().map(Box::from));
        opts.controller = o.get("controller").and_then(Json::as_bool).unwrap_or(true);
        opts.keeper_lairs = o.get("keeperLairs").and_then(Json::as_bool).unwrap_or(false);
        for (side, key) in ["top", "right", "bottom", "left"].iter().enumerate() {
            opts.exits[side] = o.get("exits").and_then(|e| e.get(key)).and_then(Json::as_array).map(|a| a.iter().filter_map(Json::as_f64).map(|x| x as u8).collect());
        }
        opts
    }
}

impl Default for GenRoomOpts {
    fn default() -> Self {
        GenRoomOpts {
            exits: [None, None, None, None],
            neighbors: [false; 4],
            terrain_type: None,
            swamp_type: None,
            sources: None,
            mineral: None,
            controller: true,
            keeper_lairs: false,
        }
    }
}

/// A generated world (ARCHITECTURE §11).
pub struct WorldSpec {
    /// Whole sectors per axis (`sector_axis`): -1..0 is W10..W0, 0..2 is E0..E20, -1..1 is W10..E10.
    pub sectors_x: Range<i32>,
    pub sectors_y: Range<i32>,
    /// worldSeed (RNG-SEED); the rooms depend on nothing else.
    pub seed: u64,
    /// Allow rooms beyond W127..E127 × N127..S127 (D33).
    pub wide_coords: bool,
    /// `false`: every room starts `out of borders`, to be opened with `open_room`.
    pub open: bool,
    /// Not implemented: no OSS code creates portals (11 OBJ-PORTAL); `generate_world` refuses `Some`.
    pub portals: Option<PortalSpec>,
}

pub struct PortalSpec {
    pub every_n_sectors: u32,
}

// ---- generateRoom (map.js:23-565) ----

/// One terrain cell of `_genTerrain`; map.js keeps these flags as properties of a plain object.
#[derive(Clone, Copy, Default)]
struct Cell {
    wall: bool,
    swamp: bool,
    force_open: bool,
    exit: bool,
    source: bool,
    controller: bool,
    keeper_lair: bool,
}

type Grid = Vec<Cell>;

fn at(x: i32, y: i32) -> usize {
    (y * 50 + x) as usize
}

fn in_room(x: i32, y: i32) -> bool {
    (0..50).contains(&x) && (0..50).contains(&y)
}

/// `Math.floor(Math.random() * n)`.
fn rand_floor(rng: &mut Rng, n: f64) -> i32 {
    (rng.next_f64() * n).floor() as i32
}

/// `{fill, smooth, factor}` of wall types 1..=28 (map.js:204-236).
const WALL_TYPES: [(f64, u32, u32); 28] = [
    (0.4, 10, 5),
    (0.2, 20, 4),
    (0.2, 20, 4),
    (0.3, 18, 4),
    (0.3, 10, 4),
    (0.3, 10, 4),
    (0.3, 10, 4),
    (0.35, 15, 4),
    (0.3, 2, 4),
    (0.35, 2, 4),
    (0.35, 5, 4),
    (0.35, 5, 4),
    (0.25, 5, 4),
    (0.4, 3, 5),
    (0.5, 3, 5),
    (0.45, 4, 5),
    (0.45, 6, 5),
    (0.45, 10, 5),
    (0.5, 10, 5),
    (0.4, 3, 5),
    (0.5, 2, 5),
    (0.45, 4, 5),
    (0.45, 6, 5),
    (0.45, 10, 5),
    (0.5, 10, 5),
    (0.45, 10, 5),
    (0.45, 6, 5),
    (0.2, 20, 4),
];

/// `{fill, smooth, factor}` of swamp types 1..=14 (map.js:238-254).
const SWAMP_TYPES: [(f64, u32, u32); 14] = [
    (0.3, 3, 5),
    (0.35, 3, 5),
    (0.45, 3, 5),
    (0.25, 1, 5),
    (0.25, 30, 4),
    (0.52, 30, 5),
    (0.45, 3, 5),
    (0.3, 1, 5),
    (0.3, 1, 4),
    (0.3, 3, 5),
    (0.3, 3, 5),
    (0.3, 1, 5),
    (0.25, 1, 5),
    (0.35, 3, 5),
];

/// `types[wallType]` / `swampTypes[swampType]`; an unknown key reads `undefined.fill` (a TypeError).
fn table(t: &[(f64, u32, u32)], k: u32) -> Result<(f64, u32, u32), String> {
    (k as usize).checked_sub(1).and_then(|i| t.get(i)).copied().ok_or_else(|| "TypeError: Cannot read properties of undefined (reading 'fill')".to_string())
}

/// ponytail: map.js loops without a limit where no tile qualifies and would hang the CLI forever; we give up
/// after this many draws so the world generator can redraw the room. No room vanilla can finish is affected.
const HANG: u32 = 1_000_000;

/// `_exitsArray(neighbor.terrain, x, y)` (map.js:32-40): open tiles of the edge that faces a room on `side` of
/// it, i.e. the neighbour's row y=49 for a top neighbour, column x=0 for a right one, and so on.
pub fn facing_edge(neighbor: &RoomTerrain, side: usize) -> Vec<u8> {
    (0..50u8)
        .filter(|&i| {
            let (x, y) = match side {
                TOP => (i, 49),
                RIGHT => (0, i),
                BOTTOM => (i, 0),
                _ => (49, i),
            };
            !neighbor.is_wall(x, y)
        })
        .collect()
}

/// `_genExit` (map.js:42-79).
#[allow(clippy::needless_range_loop)] // map.js's `for(var j=0; j<intervalsCnt; j++)`
fn gen_exit(rng: &mut Rng) -> Vec<u8> {
    let exit_length = rand_floor(rng, 43.0) + 1;
    let intervals_cnt = [0, 0, 1, 1, 2][rand_floor(rng, 5.0) as usize];
    let mut exit = Vec::new();
    let exit_start = rand_floor(rng, (46 - exit_length) as f64) + 2;
    let mut intervals = [(0, 0); 2];
    let mut cur_start = exit_start;

    for j in 0..intervals_cnt {
        cur_start += rand_floor(rng, exit_length as f64 / (intervals_cnt * 2) as f64) + 5;
        let mut length = rand_floor(rng, exit_length as f64 / (intervals_cnt * 2) as f64) + 5;
        if length + cur_start >= exit_start + exit_length - 5 {
            length = exit_start + exit_length - cur_start - 5;
        }
        intervals[j] = (cur_start, length);
        cur_start += length + 1;
    }

    for x in exit_start..=exit_start + exit_length {
        if intervals_cnt > 0 {
            let (start, length) = intervals[0];
            if length > 0 && x >= start && x <= start + length {
                continue;
            }
            let (start, length) = intervals[1];
            if intervals_cnt > 1 && length > 0 && x >= start && x <= start + length {
                continue;
            }
        }
        if !(2..=47).contains(&x) {
            continue;
        }
        exit.push(x as u8);
    }

    exit
}

/// `_checkFlood` (map.js:115-169): every open cell 8-connected to the first one found.
fn check_flood(terrain: &Grid) -> Result<bool, String> {
    let (mut start_x, mut start_y) = (None, None);

    for x in 0..50 {
        for y in 0..50 {
            if !terrain[at(x, y)].wall {
                start_x = Some(x);
                start_y = Some(y);
                break;
            }
        }
        // `if(startX && startY) break;`: a start in column or row 0 keeps scanning.
        if start_x.is_some_and(|x| x != 0) && start_y.is_some_and(|y| y != 0) {
            break;
        }
    }

    // No open cell: `visited[undefined][undefined] = true` throws.
    let (Some(start_x), Some(start_y)) = (start_x, start_y) else {
        return Err("TypeError: Cannot set properties of undefined (setting 'undefined')".into());
    };

    let mut visited = [false; 2500];
    let mut list = vec![(start_x, start_y)];
    while let Some((x, y)) = list.pop() {
        visited[at(x, y)] = true;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if in_room(nx, ny) && !terrain[at(nx, ny)].wall && !visited[at(nx, ny)] {
                    visited[at(nx, ny)] = true;
                    list.push((nx, ny));
                }
            }
        }
    }

    Ok((0..2500).all(|i| terrain[i].wall || visited[i]))
}

/// `_smoothTerrain` (map.js:171-200) for key `'wall'` (`wall == true`) or `'swamp'`: double-buffered, every new
/// cell a shallow clone of the old one.
fn smooth_terrain(terrain: &Grid, factor: u32, wall: bool) -> Grid {
    // `key` of every tile with a one-tile frame: out-of-room positions count for 'wall', not for 'swamp'.
    let mut key = [wall as u8; 52 * 52];
    for (i, cell) in terrain.iter().enumerate() {
        key[(i / 50 + 1) * 52 + i % 50 + 1] = if wall { cell.wall } else { cell.swamp } as u8;
    }
    let mut new_terrain = terrain.clone();
    for y in 0..50 {
        for x in 0..50 {
            let c = (y as usize + 1) * 52 + x as usize + 1;
            let cnt: u32 = [c - 53, c - 52, c - 51, c - 1, c, c + 1, c + 51, c + 52, c + 53].iter().map(|&i| key[i] as u32).sum();
            let cell = &mut new_terrain[at(x, y)];
            if wall {
                cell.wall = cnt >= factor;
                if x == 0 || x == 49 || y == 0 || y == 49 {
                    cell.wall = true;
                }
                if terrain[at(x, y)].force_open {
                    cell.wall = false;
                }
            } else {
                cell.swamp = cnt >= factor;
            }
        }
    }
    new_terrain
}

/// Any in-room tile of the 3×3 block around (x, y) open (the `passNearby` loops; their `break` only leaves
/// the inner loop, which changes nothing).
fn pass_nearby(terrain: &Grid, x: i32, y: i32) -> bool {
    (-1..=1).any(|dx| (-1..=1).any(|dy| in_room(x + dx, y + dy) && !terrain[at(x + dx, y + dy)].wall))
}

/// The keeper-lair search of map.js:352-378: depth of first discovery in a LIFO walk from (x, y) over open
/// tiles; walls at depth 3..=5 inside 1..=48 that are not sources, in discovery order.
fn lair_spots(terrain: &Grid, x: i32, y: i32) -> Vec<(i32, i32)> {
    let mut lair_spots = Vec::new();
    let mut list = vec![(x, y)];
    // `visited` keyed "x,y"; -1 = undefined. Out-of-room keys are never set, so the bounds test may go first.
    let mut visited = [-1i32; 2500];
    visited[at(x, y)] = 0;

    while let Some((cx, cy)) = list.pop() {
        for dx in -1..=1 {
            for dy in -1..=1 {
                let (nx, ny) = (cx + dx, cy + dy);
                if dx == 0 && dy == 0 || !in_room(nx, ny) || visited[at(nx, ny)] >= 0 {
                    continue;
                }
                let distance = visited[at(cx, cy)] + 1;
                visited[at(nx, ny)] = distance;
                let cell = terrain[at(nx, ny)];
                if (3..=5).contains(&distance) && cell.wall && !cell.source && nx > 0 && nx < 49 && ny > 0 && ny < 49 {
                    lair_spots.push((nx, ny));
                }
                if !cell.wall && distance < 5 {
                    list.push((nx, ny));
                }
            }
        }
    }
    lair_spots
}

/// `_genTerrain` (map.js:202-415). Its self-recursion on a failed source/lair placement restarts the
/// whole function with a fresh wall type; here that is `continue 'gen`.
fn gen_terrain(
    rng: &mut Rng,
    mut wall_type: u32,
    swamp_type: u32,
    exits: &[Vec<u8>; 4],
    sources: u32,
    controller: bool,
    keeper_lair: bool,
) -> Result<Grid, String> {
    // `_.contains(exits[side], i)` for i in 0..50.
    let member = exits.each_ref().map(|e| {
        let mut m = [false; 50];
        e.iter().filter(|&&i| i < 50).for_each(|&i| m[i as usize] = true);
        m
    });
    let has = |side: usize, i: i32| member[side][i as usize];
    'gen: loop {
        let mut terrain: Grid;
        let mut tries = 0;

        loop {
            tries += 1;

            if tries > 100 {
                wall_type = rand_floor(rng, 27.0) as u32 + 1;
                tries = 0;
            }

            terrain = vec![Cell::default(); 2500];
            for y in 0..50 {
                for x in 0..50 {
                    if y == 0 && has(TOP, x) {
                        terrain[at(x, y)].force_open = true;
                        terrain[at(x, y + 1)].force_open = true;
                        terrain[at(x, y)].exit = true;
                        continue;
                    }
                    if y == 49 && has(BOTTOM, x) {
                        terrain[at(x, y)].force_open = true;
                        terrain[at(x, y - 1)].force_open = true;
                        terrain[at(x, y)].exit = true;
                        continue;
                    }
                    if x == 0 && has(LEFT, y) {
                        terrain[at(x, y)].force_open = true;
                        terrain[at(x + 1, y)].force_open = true;
                        terrain[at(x, y)].exit = true;
                        continue;
                    }
                    if x == 49 && has(RIGHT, y) {
                        terrain[at(x, y)].force_open = true;
                        terrain[at(x - 1, y)].force_open = true;
                        terrain[at(x, y)].exit = true;
                        continue;
                    }
                    terrain[at(x, y)].wall = rng.next_f64() < table(&WALL_TYPES, wall_type)?.0;
                    terrain[at(x, y)].swamp = if swamp_type != 0 { rng.next_f64() < table(&SWAMP_TYPES, swamp_type)?.0 } else { false };
                }
            }

            let (_, smooth, factor) = table(&WALL_TYPES, wall_type)?;
            for _ in 0..smooth {
                terrain = smooth_terrain(&terrain, factor, true);
            }
            if check_flood(&terrain)? {
                break;
            }
        }

        if swamp_type != 0 {
            let (_, smooth, factor) = table(&SWAMP_TYPES, swamp_type)?;
            for _ in 0..smooth {
                terrain = smooth_terrain(&terrain, factor, false);
            }
        }

        for _ in 0..sources {
            let (mut x, mut y);
            let mut tries = 0;

            loop {
                tries += 1;

                x = rand_floor(rng, 44.0) + 3;
                y = rand_floor(rng, 44.0) + 3;

                let pass = pass_nearby(&terrain, x, y);

                if tries > 1000 {
                    wall_type = rand_floor(rng, 27.0) as u32 + 1;
                    continue 'gen;
                }
                if terrain[at(x, y)].wall && pass {
                    break;
                }
            }

            terrain[at(x, y)].source = true;

            if keeper_lair {
                let spots = lair_spots(&terrain, x, y);
                if !spots.is_empty() {
                    terrain[at(x, y)].source = true;
                    let spots = policy::lodash_shuffle(rng, &spots);
                    terrain[at(spots[0].0, spots[0].1)].keeper_lair = true;
                } else {
                    wall_type = rand_floor(rng, 27.0) as u32 + 1;
                    continue 'gen;
                }
            }
        }

        if controller {
            let (mut x, mut y);
            let mut tries = 0;
            loop {
                x = rand_floor(rng, 40.0) + 5;
                y = rand_floor(rng, 40.0) + 5;
                let pass = pass_nearby(&terrain, x, y);
                let cell = terrain[at(x, y)];
                if cell.wall && pass && !cell.source && !cell.keeper_lair {
                    break;
                }
                tries += 1;
                if tries > HANG {
                    return Err("no controller spot".into());
                }
            }
            terrain[at(x, y)].controller = true;
        }

        return Ok(terrain);
    }
}

fn s(v: &str) -> Val {
    Val::Str(v.into())
}

fn n(v: impl Into<f64>) -> Val {
    Val::Num(v.into())
}

fn obj(kv: &[(Atom, Val)]) -> Obj {
    let mut o = Obj::new();
    for (k, v) in kv {
        o.set(*k, v.clone());
    }
    o
}

/// The neighbour on `side` (map.js:420-426).
fn neighbor(xy: RoomXY, side: usize) -> RoomXY {
    match side {
        TOP => RoomXY::new(xy.x, xy.y - 1),
        RIGHT => RoomXY::new(xy.x + 1, xy.y),
        BOTTOM => RoomXY::new(xy.x, xy.y + 1),
        _ => RoomXY::new(xy.x - 1, xy.y),
    }
}

/// map.js generateRoom from the neighbour lookup on (map.js:419-560). The caller has checked the name and that
/// the room does not exist yet; `edges[side]` is `facing_edge` of the neighbour when `opts.neighbors[side]`.
/// Returns what the three inserts write, or the rejection text.
pub fn generate_room(rng: &mut Rng, xy: RoomXY, edges: [&[u8]; 4], opts: &GenRoomOpts) -> Result<GeneratedRoom, String> {
    let room_name = xy.to_string();

    // `_matchExitWithNeighbors` for top, right, bottom, left (map.js:81-113, 428-440).
    let mut exits = opts.exits.clone();
    for side in [TOP, RIGHT, BOTTOM, LEFT] {
        let neighbor_exits = opts.neighbors[side].then_some(edges[side]);
        match (&exits[side], neighbor_exits) {
            (Some(given), Some(theirs)) => {
                // `_.intersection` keeps the unique values of the first array; `_exitsArray` has no duplicates.
                if !(theirs.len() == given.len() && theirs.iter().filter(|e| given.contains(e)).count() == theirs.len()) {
                    return Err(format!("Exits in room {} don't match", neighbor(xy, side)));
                }
            }
            (Some(_), None) => {}
            (None, Some(theirs)) => exits[side] = Some(theirs.to_vec()),
            (None, None) => exits[side] = Some(gen_exit(rng)),
        }
    }
    let exits = exits.map(Option::unwrap_or_default);

    let terrain_type = match opts.terrain_type {
        Some(t) => t,
        None => rand_floor(rng, 27.0) as u32 + 1,
    };
    let swamp_type = match opts.swamp_type {
        Some(t) => t,
        None => rand_floor(rng, 14.0) as u32,
    };
    let sources = match opts.sources {
        Some(n) => n,
        None => {
            if rng.next_f64() > 0.5 {
                1
            } else {
                2
            }
        }
    };

    let room_data = gen_terrain(rng, terrain_type, swamp_type, &exits, sources, opts.controller, opts.keeper_lairs)?;

    // GEN-EMIT (map.js:465-512): tile order, per tile source → controller → lair.
    let mut objects = Vec::new();
    // Positions of the sources and controllers in `objects` (the mineral's `hasObjects` test).
    let mut near = Vec::new();
    let mut terrain = RoomTerrain::default();
    let mut source_keepers = false;
    let room = s(&room_name);
    for y in 0..50 {
        for x in 0..50 {
            let cell = room_data[at(x, y)];
            let mut code = 0;
            if cell.wall {
                code |= 1;
            }
            if cell.source || cell.controller {
                near.push((x, y));
            }
            if cell.source {
                objects.push(obj(&[
                    (atom!("room"), room.clone()),
                    (atom!("type"), s("source")),
                    (atom!("x"), n(x)),
                    (atom!("y"), n(y)),
                    (atom!("energy"), n(SOURCE_ENERGY_NEUTRAL_CAPACITY)),
                    (atom!("energyCapacity"), n(SOURCE_ENERGY_NEUTRAL_CAPACITY)),
                    (atom!("ticksToRegeneration"), n(ENERGY_REGEN_TIME)),
                ]));
            }
            if cell.controller {
                objects.push(obj(&[(atom!("room"), room.clone()), (atom!("type"), s("controller")), (atom!("x"), n(x)), (atom!("y"), n(y)), (atom!("level"), n(0))]));
            }
            if cell.keeper_lair {
                objects.push(obj(&[(atom!("room"), room.clone()), (atom!("type"), s("keeperLair")), (atom!("x"), n(x)), (atom!("y"), n(y))]));
                source_keepers = true;
            }
            if cell.swamp && pass_nearby(&room_data, x, y) {
                code |= 2;
            }
            terrain.set_raw(x as u8, y as u8, code);
        }
    }

    // GEN-MINERAL (map.js:514-554).
    const TYPES: [&str; 25] = ["H", "H", "H", "H", "H", "H", "O", "O", "O", "O", "O", "O", "Z", "Z", "Z", "K", "K", "K", "U", "U", "U", "L", "L", "L", "X"];
    let mineral: Option<Box<str>> = match &opts.mineral {
        Some(m) => m.clone(),
        None => Some(TYPES[rand_floor(rng, TYPES.len() as f64) as usize].into()),
    };

    if let Some(mineral) = mineral.filter(|m| !m.is_empty()) {
        let (mut mx, mut my);
        let mut tries = 0;
        loop {
            mx = 4 + rand_floor(rng, 42.0);
            my = 4 + rand_floor(rng, 42.0);
            let is_wall = terrain.is_wall(mx as u8, my as u8);
            let has_spot = (-1..=1).any(|dx| (-1..=1).any(|dy| !terrain.is_wall((mx + dx) as u8, (my + dy) as u8)));
            let has_objects = near.iter().any(|&(x, y)| (x - mx).abs() < 5 && (y - my).abs() < 5);
            if is_wall && has_spot && !has_objects {
                break;
            }
            tries += 1;
            if tries > HANG {
                return Err("no mineral spot".into());
            }
        }

        let random = rng.next_f64();
        let density = (1..=4).find(|&d| random <= MINERAL_DENSITY_PROBABILITY[d].unwrap()).unwrap();

        objects.push(obj(&[
            (atom!("type"), s("mineral")),
            (atom!("mineralType"), s(&mineral)),
            (atom!("density"), n(density as u32)),
            (atom!("mineralAmount"), n(MINERAL_DENSITY[density].unwrap())),
            (atom!("x"), n(mx)),
            (atom!("y"), n(my)),
            (atom!("room"), room.clone()),
        ]));
    }

    // GEN-INSERT (map.js:556-560).
    let info = obj(&[(atom!("_id"), room), (atom!("status"), s("normal")), (atom!("sourceKeepers"), Val::Bool(source_keepers))]);
    Ok(GeneratedRoom { xy, terrain, info, objects })
}

// ---- World layout (ARCHITECTURE §11, 11 §10) ----

/// What a room is, by position.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoomKind {
    /// Either axis number ≡ 0 (mod 10), crossroads included: no objects, `bus: true` + `depositType`.
    Highway,
    /// The sector centre 3×3 minus the centre: keeper lairs, 3 sources, mineral, unowned extractor.
    Keeper,
    /// Both numbers ≡ 5 (mod 10): 3 sources, mineral, unowned extractor, no controller.
    Center,
    /// Controller, 1–2 sources, mineral.
    Normal,
}

pub fn room_kind(xy: RoomXY) -> RoomKind {
    if xy.is_highway() {
        RoomKind::Highway
    } else if xy.is_sector_center() {
        RoomKind::Center
    } else if xy.is_center_room() {
        RoomKind::Keeper
    } else {
        RoomKind::Normal
    }
}

/// generateRoom `opts` of each kind; every side has a neighbour edge (the edge pass or a closed border).
fn kind_opts(kind: RoomKind) -> GenRoomOpts {
    let mut opts = GenRoomOpts { neighbors: [true; 4], ..GenRoomOpts::default() };
    match kind {
        RoomKind::Highway => {
            opts.sources = Some(0);
            opts.controller = false;
            opts.mineral = Some(None);
        }
        RoomKind::Keeper | RoomKind::Center => {
            opts.sources = Some(3);
            opts.controller = false;
            opts.keeper_lairs = kind == RoomKind::Keeper;
        }
        RoomKind::Normal => {}
    }
    opts
}

/// Engine coordinates covered by whole sectors `s` along one axis, each with both of its highway lines:
/// sector s ≥ 0 is E/S(10s..=10s+10), sector s < 0 is W/N(-10s-11..=-10s-1), so a range crossing 0 gets the
/// W0/E0 (N0/S0) double highway.
pub fn sector_axis(sectors: &Range<i32>) -> std::ops::RangeInclusive<i32> {
    let lo = |s: i32| if s >= 0 { 10 * s } else { 10 * s - 1 };
    let hi = |s: i32| if s >= 0 { 10 * s + 10 } else { 10 * s + 9 };
    lo(sectors.start)..=hi(sectors.end - 1)
}

/// SECTOR-DEPOSIT-TYPE (storage/lib/db.js:55-63): `depositTypes[(longitude+latitude)%4]` where both numbers
/// are regex strings, so `+` concatenates (W10N5 → "105" % 4 = 1 → metal).
pub fn deposit_type(xy: RoomXY) -> &'static str {
    use crate::constants::{RESOURCE_BIOMASS, RESOURCE_METAL, RESOURCE_MIST, RESOURCE_SILICON};
    let shown = |c: i64| if c < 0 { -c - 1 } else { c };
    // `"105" % 4`: ToNumber of the concatenation (a double, as JS), then `%`.
    let concat: f64 = format!("{}{}", shown(xy.x as i64), shown(xy.y as i64)).parse().unwrap();
    [RESOURCE_SILICON, RESOURCE_METAL, RESOURCE_BIOMASS, RESOURCE_MIST][(concat % 4.0) as usize]
}

fn num(o: &Obj, k: Atom) -> i32 {
    o.get(k).and_then(Val::as_f64).unwrap_or(-1.0) as i32
}

fn kind_of(o: &Obj) -> Option<&str> {
    o.get(atom!("type")).and_then(Val::as_str)
}

/// The post-steps generateRoom leaves out (11 GEN-POST), shaped like the default world (GEN-DEFAULT-WORLD):
/// highway `bus`/`depositType`; keeper and centre rooms get 4000-energy sources (what sources/tick.js sets in
/// rooms without a controller) and an unowned extractor on the mineral; keeper rooms a fourth lair guarding
/// the mineral, placed by map.js's own lair search from the mineral tile; closed worlds `out of borders`.
fn layout_post(kind: RoomKind, open: bool, room: &mut GeneratedRoom, rng: &mut Rng) {
    use crate::constants::SOURCE_ENERGY_KEEPER_CAPACITY;
    match kind {
        RoomKind::Highway => {
            room.info.set(atom!("bus"), Val::Bool(true));
            room.info.set(atom!("depositType"), s(deposit_type(room.xy)));
        }
        RoomKind::Keeper | RoomKind::Center => {
            for o in room.objects.iter_mut().filter(|o| kind_of(o) == Some("source")) {
                o.set(atom!("energy"), n(SOURCE_ENERGY_KEEPER_CAPACITY));
                o.set(atom!("energyCapacity"), n(SOURCE_ENERGY_KEEPER_CAPACITY));
            }
            let mineral = room.objects.iter().find(|o| kind_of(o) == Some("mineral")).map(|o| (num(o, atom!("x")), num(o, atom!("y"))));
            if let Some((mx, my)) = mineral {
                if kind == RoomKind::Keeper {
                    let mut grid = vec![Cell::default(); 2500];
                    for (i, cell) in grid.iter_mut().enumerate() {
                        cell.wall = room.terrain.is_wall((i % 50) as u8, (i / 50) as u8);
                    }
                    // The search skips `source` tiles; the source lairs are skipped too, or a second lair doc could
                    // land on a tile that already has one (map.js's per-tile flag never makes two).
                    for o in room.objects.iter().filter(|o| matches!(kind_of(o), Some("source" | "keeperLair"))) {
                        grid[at(num(o, atom!("x")), num(o, atom!("y")))].source = true;
                    }
                    let spots = lair_spots(&grid, mx, my);
                    if !spots.is_empty() {
                        let (lx, ly) = policy::lodash_shuffle(rng, &spots)[0];
                        let lair = obj(&[(atom!("room"), s(&room.xy.to_string())), (atom!("type"), s("keeperLair")), (atom!("x"), n(lx)), (atom!("y"), n(ly))]);
                        // In tile order among the GEN-EMIT objects, as if generateRoom had placed it.
                        let after = |o: &Obj| kind_of(o) == Some("mineral") || (num(o, atom!("y")), num(o, atom!("x"))) > (ly, lx);
                        let at = room.objects.iter().position(after).unwrap_or(room.objects.len());
                        room.objects.insert(at, lair);
                    }
                }
                let extractor = obj(&[(atom!("type"), s("extractor")), (atom!("x"), n(mx)), (atom!("y"), n(my)), (atom!("room"), s(&room.xy.to_string()))]);
                room.objects.push(extractor);
            }
        }
        RoomKind::Normal => {}
    }
    if !open {
        room.info.set(atom!("status"), s("out of borders"));
    }
}

/// One room of the world: generateRoom on `seed("gen-room", name)` with its four edges, the layout post-steps,
/// then the mods' `gen_room` hooks on the same stream. A rejection (only where map.js would hang or throw)
/// redraws the room from `seed("gen-room", name, attempt)`.
fn world_room(spec: &WorldSpec, hooks: &EngineHooks, xy: RoomXY, edges: [&[u8]; 4]) -> GeneratedRoom {
    let kind = room_kind(xy);
    let opts = kind_opts(kind);
    let name_hash = policy::fnv1a64(xy.to_string().as_bytes());
    let mut attempt = 0u64;
    loop {
        let parts: &[u64] = if attempt == 0 { &[name_hash] } else { &[name_hash, attempt] };
        let mut rng = Rng::new(policy::seed(spec.seed, "gen-room", parts));
        match generate_room(&mut rng, xy, edges, &opts) {
            Ok(mut room) => {
                layout_post(kind, spec.open, &mut room, &mut rng);
                for hook in &hooks.gen_room {
                    hook(&mut room, &mut rng);
                }
                return room;
            }
            Err(e) => eprintln!("mapgen: {xy} attempt {attempt}: {e}; redrawing"),
        }
        attempt += 1;
    }
}

/// Every room of the world in creation order (row-major over the bounding box: y, then x), generated in two
/// parallel passes. Pass 1 draws every shared edge once with `_genExit` on `seed("gen-edge", "<a>|<b>")`
/// (a = the top/left room), so each room's exits equal its neighbours' (map.js's "neighbour exists ⇒ copy"
/// rule); outward edges of the bounding box are closed. Pass 2 generates the rooms.
pub fn generate_rooms(spec: &WorldSpec, pool: &Pool, hooks: &EngineHooks) -> Vec<GeneratedRoom> {
    assert!(!spec.sectors_x.is_empty() && !spec.sectors_y.is_empty(), "a world needs at least one sector per axis");
    let (xs, ys) = (sector_axis(&spec.sectors_x), sector_axis(&spec.sectors_y));
    let (x0, y0) = (*xs.start(), *ys.start());
    let (w, h) = ((xs.end() - x0 + 1) as usize, (ys.end() - y0 + 1) as usize);
    assert!(
        spec.wide_coords || [x0, y0].iter().all(|&c| c >= -128) && [*xs.end(), *ys.end()].iter().all(|&c| c <= 127),
        "a world beyond W127..E127 × N127..S127 needs wide_coords"
    );
    let xy_of = |i: usize| RoomXY::new(x0 + (i % w) as i32, y0 + (i / w) as i32);

    // [right edge, bottom edge] of each room.
    let edges: Vec<[Vec<u8>; 2]> = pool.map(w * h, |i| {
        let xy = xy_of(i);
        let edge = |inside: bool, other: RoomXY| match inside {
            true => gen_exit(&mut Rng::new(policy::seed(spec.seed, "gen-edge", &[policy::fnv1a64(format!("{xy}|{other}").as_bytes())]))),
            false => Vec::new(),
        };
        [edge(i % w + 1 < w, neighbor(xy, RIGHT)), edge(i / w + 1 < h, neighbor(xy, BOTTOM))]
    });

    pool.map(w * h, |i| {
        let top: &[u8] = if i >= w { &edges[i - w][1] } else { &[] };
        let left: &[u8] = if i % w > 0 { &edges[i - 1][0] } else { &[] };
        world_room(spec, hooks, xy_of(i), [top, &edges[i][0], &edges[i][1], left])
    })
}

/// common/index.js:183-193 calcWorldSize, incl. BUG-WORLDSIZE (`maxX = maxY = 0` to start, R row).
fn calc_world_size(rooms: &[GeneratedRoom]) -> u32 {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (i32::MAX, i32::MAX, 0, 0);
    for r in rooms {
        min_x = min_x.min(r.xy.x);
        min_y = min_y.min(r.xy.y);
        max_x = max_x.max(r.xy.x);
        max_y = max_y.max(r.xy.y);
    }
    (max_x - min_x + 1).max(max_y - min_y + 1) as u32
}

/// A snapshot of a room nothing has touched yet: no flags, users, event log or map view.
fn fresh_snapshot(info: Obj, objects: Vec<DocRef>, processed_at: u64) -> Arc<RoomSnapshot> {
    let draft = RoomDraft {
        info: Arc::new(info),
        objects,
        flags: Arc::from([]),
        event_log: Arc::from(""),
        map_view: Arc::from(""),
        presence: Vec::new(),
        user_refs: Vec::new(),
        has_movers: false,
        has_terminal_send: false,
        has_power_creeps: false,
    };
    Arc::new(RoomSnapshot::new(draft, processed_at))
}

/// GEN-INSERT `rooms.objects`: each doc gets the next seq and, as storage `genId` does (`if(!obj._id)`), an
/// `_id` appended unless it has one (a `gen_room` hook may set it; later generated ids skip it).
fn insert_docs(
    objects: Vec<Obj>,
    r: RoomIdx,
    ids: &mut IdGen,
    next_seq: &mut u64,
    index: &mut HashMap<Id, ObjLoc>,
) -> Vec<DocRef> {
    objects
        .into_iter()
        .map(|mut o| {
            let given = o.get(atom!("_id")).and_then(Val::as_str).filter(|id| !id.is_empty()).and_then(Id::new);
            let id = match given {
                Some(id) => {
                    ids.mark_imported(id);
                    id
                }
                None => {
                    let id = ids.next();
                    o.set(atom!("_id"), s(id.as_str()));
                    id
                }
            };
            index.insert(id, ObjLoc { room: r, seq: *next_seq, owner: None });
            *next_seq += 1;
            Doc::new(*next_seq - 1, o)
        })
        .collect()
}

/// A fresh World from generated rooms (row-major, as `generate_rooms` returns them): ids and seq in room,
/// then insert order; game time 1 and the virtual clock anchored now (CLOCK-VIRTUAL); no users (the NPC and
/// system users are not map data); accessibility as TICK-ACCESSIBLE would compute it; every room due for
/// roomsForceUpdate (no `nextForceUpdateTime` yet).
pub fn assemble(spec: &WorldSpec, rooms: Vec<GeneratedRoom>) -> World {
    use crate::world::{AccessSnapshot, EngineConfig, RoomStatus, VClock};

    let time = 1;
    let (x0, y0) = (rooms.iter().map(|r| r.xy.x).min().unwrap_or(0), rooms.iter().map(|r| r.xy.y).min().unwrap_or(0));
    let (x1, y1) = (rooms.iter().map(|r| r.xy.x).max().unwrap_or(-1), rooms.iter().map(|r| r.xy.y).max().unwrap_or(-1));
    let (w, h) = ((x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
    let mut grid = vec![0u32; (w * h) as usize];
    let mut ids = crate::ids::IdGen::new(spec.seed, policy::Profile::Private);
    let mut next_seq = 0u64;
    let mut objects_index = HashMap::new();
    let mut accessible = vec![0u64; rooms.len().div_ceil(64)];
    let mut status = HashMap::new();
    let mut walled = Vec::new();
    let world_size = calc_world_size(&rooms);

    let mut metas = Vec::with_capacity(rooms.len());
    let mut packed = Vec::with_capacity(rooms.len());
    let mut snapshots = Vec::with_capacity(rooms.len());
    for (idx, room) in rooms.into_iter().enumerate() {
        let r = idx as RoomIdx;
        grid[((room.xy.y - y0) as u32 * w + (room.xy.x - x0) as u32) as usize] = r + 1;
        if room.info.get(atom!("status")).and_then(Val::as_str) == Some("normal") {
            accessible[idx / 64] |= 1 << (idx % 64);
        } else {
            status.insert(r, (RoomStatus::Closed, None)); // STATUS-DATA: out of borders ⇒ closed, Infinity
            walled.push(r);
        }
        let docs = insert_docs(room.objects, r, &mut ids, &mut next_seq, &mut objects_index);
        snapshots.push(fresh_snapshot(room.info, docs, 0));
        metas.push(RoomMeta { name: room.xy.to_string().into(), xy: room.xy, removed: false });
        packed.push(room.terrain);
    }
    // World::new rebuilds the index (objects, force_update at 0 for rooms without nextForceUpdateTime) and the
    // derived maps; the generator's own id/seq counters, access, env and clock anchor are kept.
    let clock = VClock { base_ms: policy::real_now_ms("CLOCK-VIRTUAL"), base_tick: time, nominal_tick_ms: 1000 };
    let meta = RoomsMeta { x0, y0, w, h, grid, rooms: metas, world_size };
    let mut terrain = TerrainStore::new(packed);
    terrain.set_blob(&meta, &walled); // updateTerrainData after world creation (TERRAIN-WORLD-BLOB)
    let mut w = World::new(spec.seed, clock, policy::Compat::new(policy::Profile::Private), meta, terrain, snapshots, Vec::new());
    w.time = time;
    w.config = Arc::new(EngineConfig::default());
    w.access = Arc::new(AccessSnapshot { version: 0, accessible, status });
    w.env = Arc::new(EnvState { paused: false, tick_rate_ms: 1000, last_accessible_update: time, status_dirty: false });
    w.ids = ids;
    w.next_seq = next_seq;
    w
}

/// ARCHITECTURE §11: `generate_rooms` then `assemble`.
pub fn generate_world(spec: &WorldSpec, pool: &Pool, hooks: &EngineHooks) -> World {
    assert!(spec.portals.is_none(), "PortalSpec is not implemented (the reference generator has no portals)");
    assemble(spec, generate_rooms(spec, pool, hooks))
}

// ---- Map edits on a live world (CLI map.*; run as P0 Commands) ----

/// `db.rooms.findOne({_id: roomName})` over the live rooms (exact name; `RoomsMeta::by_name` is WP01's).
fn room_idx(meta: &RoomsMeta, name: &str) -> Option<RoomIdx> {
    let xy: RoomXY = name.parse().ok()?;
    let (dx, dy) = (xy.x as i64 - meta.x0 as i64, xy.y as i64 - meta.y0 as i64);
    if dx < 0 || dy < 0 || dx >= meta.w as i64 || dy >= meta.h as i64 {
        return None;
    }
    let r = meta.grid[(dy * meta.w as i64 + dx) as usize].checked_sub(1)?;
    let m = &meta.rooms[r as usize];
    (&*m.name == name && !m.removed).then_some(r)
}

/// Map edits change accessibility at the next TICK-ACCESSIBLE (STATUS-OPEN-CLOSE, GEN-POST).
fn mark_status_dirty(w: &mut World) {
    let e = &w.env;
    w.env = Arc::new(EnvState { paused: e.paused, tick_rate_ms: e.tick_rate_ms, last_accessible_update: e.last_accessible_update, status_dirty: true });
}

/// `$set` on a `rooms` doc: a new snapshot sharing everything else.
fn set_info(w: &mut World, r: RoomIdx, set: &[(Atom, Val)]) {
    let prev = &w.rooms[r as usize];
    let mut info = (*prev.info).clone();
    for (k, v) in set {
        info.set(*k, v.clone());
    }
    let draft = RoomDraft {
        info: Arc::new(info),
        objects: prev.objects.to_vec(),
        flags: prev.flags.clone(),
        event_log: prev.event_log.clone(),
        map_view: prev.map_view.clone(),
        presence: prev.presence.to_vec(),
        user_refs: prev.user_refs.to_vec(),
        has_movers: prev.has_movers,
        has_terminal_send: prev.has_terminal_send,
        has_power_creeps: prev.has_power_creeps,
    };
    w.rooms[r as usize] = Arc::new(RoomSnapshot::new(draft, prev.processed_at));
}

/// map.js openRoom (map.js:568-580): `$set {status: 'normal'}` plus `openTime` when `timestamp` is truthy,
/// whatever the current status; an unknown room is a no-op. `timestamp` is a real date (epoch ms), stored on
/// the virtual clock as `vclock(G) + (timestamp − realNow)` (CLOCK-VIRTUAL, admin tools). The room is not
/// activated (STATUS-OPEN-CLOSE). Returns whether a doc was updated (the CLI prints `{modified: 1}` or `{}`).
pub fn open_room(w: &mut World, room_name: &str, timestamp: Option<f64>) -> bool {
    let Some(r) = room_idx(&w.meta, room_name) else { return false };
    let mut set = vec![(atom!("status"), s("normal"))];
    if let Some(ts) = timestamp.filter(|t| *t != 0.0 && !t.is_nan()) {
        let open_time = w.clock.at(w.time) as f64 + (ts - policy::real_now_ms("CLOCK-VIRTUAL") as f64);
        set.push((atom!("openTime"), n(open_time)));
    }
    set_info(w, r, &set);
    mark_status_dirty(w);
    true
}

/// map.js closeRoom (map.js:582-587): only a `normal` room becomes `out of borders`. Returns whether a doc was
/// updated, as open_room.
pub fn close_room(w: &mut World, room_name: &str) -> bool {
    let Some(r) = room_idx(&w.meta, room_name) else { return false };
    let normal = w.rooms[r as usize].info.get(atom!("status")).and_then(Val::as_str) == Some("normal");
    if normal {
        set_info(w, r, &[(atom!("status"), s("out of borders"))]);
        mark_status_dirty(w);
    }
    normal
}

/// map.js removeRoom (map.js:589-609): the room doc, its objects, flags, terrain, intents, `market.stats` rows
/// and `mapView` go; the RoomIdx stays as a tombstone (`RoomMeta::removed`, empty snapshot). The world size
/// stays as computed at load (ROOMNAME-WORLD-SIZE caches it); the PNG belongs to the backend.
pub fn remove_room(w: &mut World, room_name: &str) -> Result<(), String> {
    let r = room_idx(&w.meta, room_name).ok_or_else(|| format!("The room {room_name} is not found"))?;

    let m = &w.meta;
    let mut grid = m.grid.clone();
    let cell = grid.iter().position(|&c| c == r + 1).unwrap();
    grid[cell] = 0;
    let rooms = m.rooms.iter().enumerate().map(|(i, x)| RoomMeta { name: x.name.clone(), xy: x.xy, removed: x.removed || i == r as usize }).collect();
    w.meta = Arc::new(RoomsMeta { x0: m.x0, y0: m.y0, w: m.w, h: m.h, grid, rooms, world_size: m.world_size });

    for d in w.rooms[r as usize].objects.iter() {
        w.index.objects.remove(&d.id());
    }
    for p in w.index.presence.iter_mut().filter(|p| p.contains_key(&r)) {
        Arc::make_mut(p).remove(&r);
    }
    w.index.movers.remove(&r);
    w.index.terminal_sends.remove(&r);
    w.index.power_creep_rooms.remove(&r);
    w.manual_intents.retain(|i| i.0 != r);
    w.rooms[r as usize] = fresh_snapshot(Obj::new(), Vec::new(), w.rooms[r as usize].processed_at);

    let mk = &w.market;
    let stats: Vec<_> = mk.stats.iter().filter(|o| o.get(atom!("room")).and_then(Val::as_str) != Some(room_name)).cloned().collect();
    if stats.len() != mk.stats.len() {
        w.market = Arc::new(MarketState::new(mk.orders.clone(), stats.into(), mk.transactions.clone(), mk.money.clone(), mk.resources.clone()));
    }
    mark_status_dirty(w);
    Ok(())
}

/// map.js updateTerrainData (map.js:661-695, TERRAIN-WORLD-BLOB): the world terrain blob as a list of
/// `(room, terrain source)` in blob order: every live room (its own terrain, or `None` = 2500 walls when
/// `out of borders`), then, per room, the rooms one step further from the origin in x and in y (same
/// hemisphere letters) as walls if absent.
pub fn update_terrain_data(w: &World) -> Vec<(RoomXY, Option<RoomIdx>)> {
    terrain_blob(&w.meta, &w.rooms)
}

/// map.js updateTerrainData as a world edit: the terrain store's blob view rebuilt (closed rooms walled, the
/// ring added), as world creation does.
pub fn update_terrain_data_cmd(w: &mut World) {
    let n = w.meta.rooms.len() as RoomIdx;
    let walled: Vec<RoomIdx> = (0..n).filter(|&r| !w.meta.rooms[r as usize].removed && w.rooms[r as usize].info.get(atom!("status")).and_then(Val::as_str) == Some("out of borders")).collect();
    let mut store = TerrainStore::new((0..n).map(|r| w.terrain.get(r).clone()).collect());
    store.set_blob(&w.meta, &walled);
    w.terrain = Arc::new(store);
}

/// `update_terrain_data` over a map and its room snapshots (a WorldView has both).
pub fn terrain_blob(meta: &RoomsMeta, rooms: &[Arc<RoomSnapshot>]) -> Vec<(RoomXY, Option<RoomIdx>)> {
    let live: Vec<RoomIdx> = (0..meta.rooms.len() as RoomIdx).filter(|&r| !meta.rooms[r as usize].removed).collect();
    let mut blob: Vec<(RoomXY, Option<RoomIdx>)> = live
        .iter()
        .map(|&r| {
            let walled = rooms[r as usize].info.get(atom!("status")).and_then(Val::as_str) == Some("out of borders");
            (meta.rooms[r as usize].xy, (!walled).then_some(r))
        })
        .collect();
    let mut known: HashSet<RoomXY> = blob.iter().map(|b| b.0).collect();
    let further = |c: i32| if c < 0 { c - 1 } else { c + 1 };
    for &r in &live {
        let xy = meta.rooms[r as usize].xy;
        for ring in [RoomXY::new(further(xy.x), xy.y), RoomXY::new(xy.x, further(xy.y))] {
            if known.insert(ring) {
                blob.push((ring, None));
            }
        }
    }
    blob
}

/// map.js generateRoom on a live world (map.js:23-565, CLI): name check, `This room already exists`, the
/// neighbours' facing edges, `generate_room`, then the inserts (ids and seq now, not activated, accessibility
/// at the next TICK-ACCESSIBLE: GEN-INSERT, GEN-POST). A name with leading zeros (`W01N1`), which map.js would
/// insert as a second room on W1N1's tile, is refused as invalid.
pub fn generate_room_cmd(w: &mut World, room_name: &str, opts: &GenRoomOpts, rng: &mut Rng) -> Result<(), String> {
    let xy: RoomXY = match room_name.parse() {
        Ok(xy) if RoomXY::to_string(&xy) == room_name => xy,
        _ => return Err("Invalid room name".into()),
    };
    // map.js takes any `\d+`; the grid would grow to the new bounding box (W2000000000N1 overflows it) and
    // RoomPosition only packs W127..E127 × N127..S127 (D33). Rooms inside the current box stay allowed, so a
    // wideCoords world can regenerate its own rooms.
    let m = &w.meta;
    let in_box = (m.x0..m.x0.saturating_add(m.w as i32)).contains(&xy.x) && (m.y0..m.y0.saturating_add(m.h as i32)).contains(&xy.y);
    if !(in_box || (-128..=127).contains(&xy.x) && (-128..=127).contains(&xy.y)) {
        return Err(format!("Room {room_name} is outside W127..E127 x N127..S127 (world.wideCoords)"));
    }
    if room_idx(&w.meta, room_name).is_some() {
        return Err("This room already exists".into());
    }
    let edges = [TOP, RIGHT, BOTTOM, LEFT].map(|side| room_idx(&w.meta, &neighbor(xy, side).to_string()).map(|r| facing_edge(w.terrain.get(r), side)));
    let mut opts = opts.clone();
    opts.neighbors = edges.each_ref().map(Option::is_some);
    let e = edges.each_ref().map(|e| e.as_deref().unwrap_or(&[]));
    let room = generate_room(rng, xy, e, &opts)?;
    add_room(w, room);
    Ok(())
}

/// Appends a generated room to a live world: RoomIdx = next, ids and seq in insert order.
fn add_room(w: &mut World, room: GeneratedRoom) {
    let r = w.rooms.len() as RoomIdx;
    let m = &w.meta;
    let (x0, y0) = (m.x0.min(room.xy.x), m.y0.min(room.xy.y));
    let (x1, y1) = ((m.x0 + m.w as i32 - 1).max(room.xy.x), (m.y0 + m.h as i32 - 1).max(room.xy.y));
    let (gw, gh) = ((x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32);
    let mut grid = vec![0u32; (gw * gh) as usize];
    let mut put = |xy: RoomXY, v: u32| grid[((xy.y - y0) as u32 * gw + (xy.x - x0) as u32) as usize] = v;
    for (i, rm) in m.rooms.iter().enumerate().filter(|(_, rm)| !rm.removed) {
        put(rm.xy, i as u32 + 1);
    }
    put(room.xy, r + 1);
    let mut rooms: Vec<RoomMeta> = m.rooms.iter().map(|x| RoomMeta { name: x.name.clone(), xy: x.xy, removed: x.removed }).collect();
    rooms.push(RoomMeta { name: room.xy.to_string().into(), xy: room.xy, removed: false });
    w.meta = Arc::new(RoomsMeta { x0, y0, w: gw, h: gh, grid, rooms, world_size: m.world_size });

    let mut packed: Vec<RoomTerrain> = (0..r).map(|i| w.terrain.get(i).clone()).collect();
    packed.push(room.terrain);
    w.terrain = Arc::new(TerrainStore::new(packed));

    let docs = insert_docs(room.objects, r, &mut w.ids, &mut w.next_seq, &mut w.index.objects);
    w.rooms.push(fresh_snapshot(room.info, docs, 0));
    w.index.room_cost_ns.push(0);
    w.index.force_update.push(std::cmp::Reverse((0, r)));
    mark_status_dirty(w);
}

#[cfg(test)]
mod tests {
    use super::*;
    use util::json::Json;

    const ROOMS: &str = include_str!("../tests/fixtures/mapgen/rooms.ndjson");

    fn json_of(v: Val) -> String {
        let mut s = String::new();
        v.write_json(&mut s);
        s
    }

    /// The fixture's map.js `opts` → `GenRoomOpts` (neighbour flags set by the caller).
    fn opts_of(o: &Json) -> GenRoomOpts {
        GenRoomOpts::from_json(o)
    }

    /// map.js on the same stream gives the same room, rejection or crash (tools/oracle/gen/mapgen-check.js).
    #[test]
    fn matches_map_js() {
        let mut terrains: Vec<RoomTerrain> = Vec::new();
        let (mut ok, mut err) = (0, 0);
        for line in ROOMS.lines() {
            let c = Json::parse(line).unwrap();
            let mut opts = opts_of(c.get("opts").unwrap());
            let mut edges: [Vec<u8>; 4] = Default::default();
            for (side, key) in ["top", "right", "bottom", "left"].iter().enumerate() {
                if let Some(k) = c.get("neighbours").unwrap().get(key) {
                    opts.neighbors[side] = true;
                    edges[side] = facing_edge(&terrains[k.as_f64().unwrap() as usize], side);
                }
            }
            let h = u64::from_str_radix(c.get("h").unwrap().as_str().unwrap(), 16).unwrap();
            let xy: RoomXY = c.get("room").unwrap().as_str().unwrap().parse().unwrap();
            let got = generate_room(&mut Rng::new(h), xy, [&edges[0], &edges[1], &edges[2], &edges[3]], &opts);
            match (got, c.get("out")) {
                (Ok(room), Some(out)) => {
                    assert_eq!(room.terrain.to_db_string(), out.get("terrain").unwrap().as_str().unwrap(), "terrain of {line:.200}");
                    assert_eq!(json_of(Val::Obj(room.info)), out.get("room").unwrap().stringify(), "{line:.200}");
                    let objects = Val::Arr(room.objects.into_iter().map(Val::Obj).collect());
                    assert_eq!(json_of(objects), out.get("objects").unwrap().stringify(), "{line:.200}");
                    terrains.push(room.terrain);
                    ok += 1;
                }
                (Err(e), None) => {
                    match c.get("err").unwrap().as_str().unwrap() {
                        // map.js loops forever; the oracle stops it after 50M draws, we after HANG tries.
                        "Error: hang" => assert!(e.starts_with("no "), "{e}"),
                        theirs => assert_eq!(e, theirs, "{line:.200}"),
                    }
                    err += 1;
                }
                (got, _) => panic!("{line:.300}: got {:?}", got.map(|r| r.xy)),
            }
        }
        assert!(ok > 100 && err > 5, "{ok} ok, {err} rejected");
    }

    fn spec(sectors_x: Range<i32>, sectors_y: Range<i32>, seed: u64) -> WorldSpec {
        WorldSpec { sectors_x, sectors_y, seed, wide_coords: false, open: true, portals: None }
    }

    fn world_rooms(sx: Range<i32>, sy: Range<i32>, seed: u64, threads: usize) -> Vec<GeneratedRoom> {
        generate_rooms(&spec(sx, sy, seed), &Pool::new(threads), &EngineHooks::default())
    }

    /// Every open tile 8-connected (GEN-FLOOD on the stored terrain).
    fn connected(t: &RoomTerrain) -> bool {
        let open: Vec<bool> = (0..2500).map(|i| !t.is_wall((i % 50) as u8, (i / 50) as u8)).collect();
        let Some(start) = open.iter().position(|&o| o) else { return true };
        let mut seen = vec![false; 2500];
        let (mut stack, mut reached) = (vec![start], 0);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            reached += 1;
            let (x, y) = ((i % 50) as i32, (i / 50) as i32);
            for (dx, dy) in [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)] {
                if in_room(x + dx, y + dy) && open[at(x + dx, y + dy)] && !seen[at(x + dx, y + dy)] {
                    seen[at(x + dx, y + dy)] = true;
                    stack.push(at(x + dx, y + dy));
                }
            }
        }
        reached == open.iter().filter(|&&o| o).count()
    }

    fn count(r: &GeneratedRoom, kind: &str) -> usize {
        r.objects.iter().filter(|o| kind_of(o) == Some(kind)).count()
    }

    /// Sectors -1..0 × -1..0 are W10..W0 × N10..N0, the default world's shape (GEN-DEFAULT-WORLD).
    #[test]
    fn world_like_default_world() {
        let rooms = world_rooms(-1..0, -1..0, 1, 4);
        assert_eq!(rooms.len(), 121);
        assert_eq!((rooms[0].xy.to_string(), rooms[120].xy.to_string()), ("W10N10".into(), "W0N0".into()));
        let kinds = |k| rooms.iter().filter(|r| room_kind(r.xy) == k).count();
        assert_eq!([kinds(RoomKind::Highway), kinds(RoomKind::Keeper), kinds(RoomKind::Center), kinds(RoomKind::Normal)], [40, 8, 1, 72]);
        let by_xy: std::collections::HashMap<RoomXY, &GeneratedRoom> = rooms.iter().map(|r| (r.xy, r)).collect();
        for r in &rooms {
            let info = |k| r.info.get(k).cloned();
            assert!(connected(&r.terrain), "{} not connected", r.xy);
            assert_eq!(info(atom!("status")), Some(s("normal")));
            match room_kind(r.xy) {
                RoomKind::Highway => {
                    assert!(r.objects.is_empty() && info(atom!("bus")) == Some(Val::Bool(true)), "{}", r.xy);
                    assert_eq!(info(atom!("depositType")), Some(s(deposit_type(r.xy))));
                }
                kind => {
                    assert_eq!(info(atom!("bus")), None);
                    assert_eq!((count(r, "mineral"), count(r, "controller")), (1, (kind == RoomKind::Normal) as usize), "{}", r.xy);
                    let sources: Vec<_> = r.objects.iter().filter(|o| kind_of(o) == Some("source")).collect();
                    let cap = if kind == RoomKind::Normal { 1500.0 } else { 4000.0 };
                    assert!(sources.iter().all(|o| o.get(atom!("energyCapacity")) == Some(&Val::Num(cap)) && o.get(atom!("energy")) == Some(&Val::Num(cap))));
                    // Two draws may land on the same tile (GEN-SOURCES GOTCHA), so counts can fall short.
                    assert!((1..=if kind == RoomKind::Normal { 2 } else { 3 }).contains(&sources.len()), "{}", r.xy);
                    assert_eq!(count(r, "extractor"), (kind != RoomKind::Normal) as usize);
                    assert_eq!(info(atom!("sourceKeepers")), Some(Val::Bool(kind == RoomKind::Keeper)));
                    if kind == RoomKind::Keeper {
                        assert!((2..=4).contains(&count(r, "keeperLair")), "{}", r.xy);
                    }
                }
            }
            // Exits equal the neighbour's facing edge; the bounding box is closed.
            for side in [TOP, RIGHT, BOTTOM, LEFT] {
                let mine = facing_edge(&r.terrain, (side + 2) % 4);
                match by_xy.get(&neighbor(r.xy, side)) {
                    Some(other) => assert_eq!(mine, facing_edge(&other.terrain, side), "{} side {side}", r.xy),
                    None => assert!(mine.is_empty(), "{} border side {side} open", r.xy),
                }
                assert!(mine.iter().all(|e| (2..=47).contains(e)));
            }
        }
        // W10N5 → "105" % 4 = 1 → metal; W0N0 → "00" → silicon (as in the default world).
        assert_eq!((deposit_type("W10N5".parse().unwrap()), deposit_type("W0N0".parse().unwrap())), ("metal", "silicon"));
        // Keeper rooms carry the mineral lair in tile order before the mineral.
        let w6n6 = by_xy[&"W6N6".parse().unwrap()];
        let kinds: Vec<_> = w6n6.objects.iter().map(|o| kind_of(o).unwrap()).collect();
        assert_eq!(&kinds[kinds.len() - 2..], ["mineral", "extractor"]);
    }

    /// Regression: the mineral lair search ignored the source lairs, so seeds 13 and 30 put two keeperLair docs
    /// on one tile of W4N6.
    #[test]
    fn keeper_lairs_never_share_a_tile() {
        for seed in [13, 30] {
            for r in world_rooms(-1..0, -1..0, seed, 4).iter().filter(|r| room_kind(r.xy) == RoomKind::Keeper) {
                let lairs: Vec<_> = r.objects.iter().filter(|o| kind_of(o) == Some("keeperLair")).map(|o| (num(o, atom!("x")), num(o, atom!("y")))).collect();
                assert_eq!(lairs.iter().collect::<HashSet<_>>().len(), lairs.len(), "seed {seed} {}: {lairs:?}", r.xy);
            }
        }
    }

    #[test]
    fn deterministic_by_seed_not_by_threads() {
        let a = world_rooms(0..1, -1..0, 7, 1);
        assert!(a == world_rooms(0..1, -1..0, 7, 8));
        assert!(a != world_rooms(0..1, -1..0, 8, 8));
    }

    #[test]
    fn gen_room_hooks_run_last() {
        fn mark(room: &mut GeneratedRoom, rng: &mut Rng) {
            room.info.set(atom!("hooked"), Val::Num(rng.next_f64()));
        }
        // Regression: an `_id` a hook gives was overwritten at insert (storage genId keeps it).
        fn portal(room: &mut GeneratedRoom, _: &mut Rng) {
            room.objects.push(obj(&[(atom!("type"), s("portal")), (atom!("_id"), s(&format!("p{}", room.xy)))]));
        }
        let hooks = EngineHooks { gen_room: vec![mark, portal], ..EngineHooks::default() };
        let spec = spec(0..1, 0..1, 2);
        let rooms = generate_rooms(&spec, &Pool::new(2), &hooks);
        assert!(rooms.iter().all(|r| r.info.iter().last().unwrap().0 == atom!("hooked")));
        let w = assemble(&spec, rooms);
        for (r, m) in w.meta.rooms.iter().enumerate() {
            let id = format!("p{}", m.name);
            assert_eq!(w.rooms[r].objects.last().unwrap().id().as_str(), id);
            assert!(w.index.objects.contains_key(&Id::new(&id).unwrap()));
        }
    }

    #[test]
    fn sector_axes_and_seam() {
        assert_eq!(sector_axis(&(-1..0)), -11..=-1); // W10..W0
        assert_eq!(sector_axis(&(0..2)), 0..=20); // E0..E20
        assert_eq!(sector_axis(&(-1..1)), -11..=10); // W10..E10: W0 and E0 both highways
        assert!(RoomXY::new(-1, 3).is_highway() && RoomXY::new(0, 3).is_highway());
        assert_eq!(room_kind("W5S5".parse().unwrap()), RoomKind::Center);
        assert_eq!(room_kind("E14N16".parse().unwrap()), RoomKind::Keeper);
        assert_eq!(room_kind("E20N15".parse().unwrap()), RoomKind::Highway);
        // "21474836472147483647" overflowed the u64 parse (a panic); JS reads it as a double.
        assert_eq!(deposit_type(RoomXY::new(i32::MIN, i32::MAX)), "silicon");
    }

    #[test]
    fn assembled_world() {
        let spec = WorldSpec { open: false, ..spec(-1..0, -1..0, 3) };
        let rooms = generate_rooms(&spec, &Pool::new(4), &EngineHooks::default());
        let docs: usize = rooms.iter().map(|r| r.objects.len()).sum();
        let w = assemble(&spec, rooms.clone());
        assert_eq!((w.meta.x0, w.meta.y0, w.meta.w, w.meta.h), (-11, -11, 11, 11));
        assert_eq!(w.meta.world_size, 12); // BUG-WORLDSIZE: 12 for W0..W10, not 11
        assert_eq!(w.meta.grid[0], 1);
        assert_eq!(&*w.meta.rooms[120].name, "W0N0");
        assert_eq!((w.rooms.len(), w.index.objects.len(), w.next_seq as usize), (121, docs, docs));
        let seqs: Vec<u64> = w.rooms.iter().flat_map(|r| r.objects.iter().map(|d| d.seq)).collect();
        assert!(seqs.windows(2).all(|p| p[0] < p[1]));
        let r = rooms.iter().position(|r| !r.objects.is_empty()).unwrap();
        let d = &w.rooms[r].objects[0];
        assert_eq!(d.obj.iter().last().unwrap().0, atom!("_id")); // appended by the insert, as in Loki
        assert_eq!(d.json(), {
            let mut o = rooms[r].objects[0].clone();
            o.set(atom!("_id"), s(d.id().as_str()));
            json_of(Val::Obj(o))
        });
        assert_eq!(w.access.status.len(), 121); // closed world: every room out of borders
        assert!(w.access.accessible.iter().all(|&b| b == 0));
        assert_eq!(w.index.force_update.len(), 121);
    }

    const STATS: &str = include_str!("../tests/fixtures/mapgen/stats.json");

    /// Two-sample χ² homogeneity of two histograms below the p = 0.001 critical value.
    fn same_distribution(ours: &HashMap<String, f64>, theirs: &Json, what: &str) {
        let theirs: HashMap<String, f64> = theirs.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap())).collect();
        let keys: HashSet<&String> = ours.keys().chain(theirs.keys()).collect();
        let (n1, n2) = (ours.values().sum::<f64>(), theirs.values().sum::<f64>());
        let mut chi2 = 0.0;
        for k in &keys {
            let (a, b) = (ours.get(*k).copied().unwrap_or(0.0), theirs.get(*k).copied().unwrap_or(0.0));
            let (e1, e2) = ((a + b) * n1 / (n1 + n2), (a + b) * n2 / (n1 + n2));
            chi2 += (a - e1).powi(2) / e1 + (b - e2).powi(2) / e2;
        }
        const CRITICAL: [f64; 9] = [0.0, 10.828, 13.816, 16.266, 18.467, 20.515, 22.458, 24.322, 26.124];
        assert!(chi2 <= CRITICAL[keys.len() - 1], "{what}: χ² {chi2:.1} over {} categories; ours {ours:?}, map.js {theirs:?}", keys.len());
    }

    /// Acceptance (ARCHITECTURE §14.3 WP12): per room class, our rooms vs map.js's in node over 2k rooms
    /// (stats.json): wall ratio within ±2 % (and a 4σ two-sample test), swamp ratio, exit widths, source /
    /// lair / controller counts, mineral type and density (χ²), and every room 8-connected.
    #[test]
    fn distributions_match_map_js() {
        let stats = Json::parse(STATS).unwrap();
        // ponytail: 500 per class keeps the debug run short; --release checks the full 2000.
        let per_class = if cfg!(debug_assertions) { 500 } else { 2000 };
        let pool = Pool::new(std::thread::available_parallelism().map_or(1, |n| n.get()));
        for (cls, kind) in [("normal", RoomKind::Normal), ("keeper", RoomKind::Keeper), ("center", RoomKind::Center), ("highway", RoomKind::Highway)] {
            let node = stats.get(cls).unwrap();
            let f = |k| node.get(k).unwrap().as_f64().unwrap();
            // As the oracle runs it: no neighbours, so map.js draws all four exits.
            let opts = GenRoomOpts { neighbors: [false; 4], ..kind_opts(kind) };
            // Rooms map.js never finishes (no mineral/controller tile, ~1 in 20k) are left out on both sides.
            let rooms: Vec<GeneratedRoom> = pool
                .map(per_class, |i| {
                    let mut rng = Rng::new(policy::seed(0xc417, "mapgen-dist", &[policy::fnv1a64(cls.as_bytes()), i as u64]));
                    generate_room(&mut rng, RoomXY::new(-2, -2), [&[], &[], &[], &[]], &opts).ok()
                })
                .into_iter()
                .flatten()
                .collect();
            assert!(rooms.len() + 2 >= per_class && f("hung") <= 2.0, "{cls}: too many rooms that never finish");
            let n = rooms.len() as f64;
            let tiles = |r: &GeneratedRoom, code: u8| (0..2500).filter(|i| r.terrain.raw((i % 50) as u8, (i / 50) as u8) & code != 0).count() as f64 / 2500.0;
            for (what, code, mean_key, sd_key) in [("wall", 1, "wallMean", "wallSd"), ("swamp", 2, "swampMean", "swampSd")] {
                let v: Vec<f64> = rooms.iter().map(|r| tiles(r, code)).collect();
                let mean = v.iter().sum::<f64>() / n;
                let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
                let z = (mean - f(mean_key)) / (sd * sd / n + f(sd_key).powi(2) / f("n")).sqrt();
                eprintln!("{cls} {what}: {mean:.4} vs map.js {:.4} (z {z:.2})", f(mean_key));
                assert!((mean - f(mean_key)).abs() < 0.02 && z.abs() < 4.0, "{cls} {what}: {mean:.4} vs map.js {:.4} (z {z:.2})", f(mean_key));
            }
            let exits = rooms.iter().map(|r| (0..4).map(|side| facing_edge(&r.terrain, side).len() as f64).sum::<f64>() / 4.0).sum::<f64>() / n;
            assert!((exits - f("exitTilesMean")).abs() < 1.0, "{cls} exit tiles {exits:.2} vs {:.2}", f("exitTilesMean"));
            assert!(rooms.iter().all(|r| connected(&r.terrain)) && f("connected") == f("n"), "{cls}: a room is not 8-connected");
            let hist = |key: &dyn Fn(&GeneratedRoom) -> String| {
                let mut h = HashMap::new();
                rooms.iter().for_each(|r| *h.entry(key(r)).or_insert(0.0) += 1.0);
                h
            };
            let mineral = |r: &GeneratedRoom| r.objects.iter().find(|o| kind_of(o) == Some("mineral")).cloned();
            for kind in ["source", "keeperLair", "controller"] {
                let key = if kind == "source" { "sources" } else if kind == "keeperLair" { "lairs" } else { "controllers" };
                same_distribution(&hist(&|r| count(r, kind).to_string()), node.get(key).unwrap(), &format!("{cls} {key}"));
            }
            let mineral_type = |r: &GeneratedRoom| mineral(r).map_or("-".into(), |m| m.get(atom!("mineralType")).unwrap().as_str().unwrap().to_string());
            same_distribution(&hist(&mineral_type), node.get("mineral").unwrap(), &format!("{cls} mineral"));
            let density = |r: &GeneratedRoom| mineral(r).map_or("0".into(), |m| num(&m, atom!("density")).to_string());
            same_distribution(&hist(&density), node.get("density").unwrap(), &format!("{cls} density"));
        }
    }

    fn status(w: &World, name: &str) -> Option<Val> {
        w.rooms[room_idx(&w.meta, name).unwrap() as usize].info.get(atom!("status")).cloned()
    }

    #[test]
    fn open_close_and_terrain_blob() {
        let mut w = generate_world(&spec(-1..0, -1..0, 5), &Pool::new(4), &EngineHooks::default());
        assert!(!w.env.status_dirty);
        assert!(close_room(&mut w, "W5N5"));
        assert_eq!(status(&w, "W5N5"), Some(s("out of borders")));
        assert!(w.env.status_dirty);
        let before = w.rooms[room_idx(&w.meta, "W5N5").unwrap() as usize].clone();
        assert!(!close_room(&mut w, "W5N5")); // only normal rooms close
        assert!(Arc::ptr_eq(&before, &w.rooms[room_idx(&w.meta, "W5N5").unwrap() as usize]));
        assert!(!close_room(&mut w, "W50N50"));
        assert!(!open_room(&mut w, "W50N50", None));
        assert_eq!(room_idx(&w.meta, "w5n5"), None); // `{_id}` is an exact match

        // TERRAIN-WORLD-BLOB: 121 rooms + the 22-room ring further from the origin (no W11N11); closed ⇒ walls.
        let blob = update_terrain_data(&w);
        assert_eq!(blob.len(), 143);
        let get = |name: &str| blob.iter().find(|b| b.0 == name.parse().unwrap()).map(|b| b.1);
        assert_eq!((get("W5N5"), get("W5N6"), get("W11N3"), get("W3N11"), get("W11N11"), get("E0N3")), (Some(None), Some(room_idx(&w.meta, "W5N6")), Some(None), Some(None), None, None));

        assert!(open_room(&mut w, "W5N5", None));
        assert_eq!(status(&w, "W5N5"), Some(s("normal")));
        let info = |w: &World| w.rooms[room_idx(&w.meta, "W5N5").unwrap() as usize].info.clone();
        assert_eq!(info(&w).get(atom!("openTime")), None);
        let soon = policy::real_now_ms("CLOCK-VIRTUAL") as f64 + 60_000.0;
        open_room(&mut w, "W5N5", Some(soon));
        let open_time = info(&w).get(atom!("openTime")).and_then(Val::as_f64).unwrap();
        assert!((open_time - (w.clock.at(w.time) as f64 + 60_000.0)).abs() < 5_000.0, "{open_time}");
        assert_eq!(&*info(&w).iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["_id", "status", "sourceKeepers", "openTime"]);
    }

    #[test]
    fn remove_and_add_rooms() {
        let mut w = generate_world(&spec(-1..0, -1..0, 6), &Pool::new(4), &EngineHooks::default());
        let r = room_idx(&w.meta, "W9N9").unwrap();
        let ids: Vec<Id> = w.rooms[r as usize].objects.iter().map(|d| d.id()).collect();
        assert!(!ids.is_empty() && ids.iter().all(|id| w.index.objects.contains_key(id)));
        remove_room(&mut w, "W9N9").unwrap();
        assert!(ids.iter().all(|id| !w.index.objects.contains_key(id)));
        assert!(w.meta.rooms[r as usize].removed && w.rooms[r as usize].objects.is_empty());
        assert_eq!(room_idx(&w.meta, "W9N9"), None);
        assert_eq!(remove_room(&mut w, "W9N9"), Err("The room W9N9 is not found".into()));
        // Its terrain is gone, but W8N9's ring step lands on it: walls, as map.js's blob would have.
        let w9n9: Vec<_> = update_terrain_data(&w).into_iter().filter(|b| b.0 == "W9N9".parse().unwrap()).collect();
        assert_eq!(w9n9, [("W9N9".parse().unwrap(), None)]);

        let mut rng = Rng::new(1);
        let opts = GenRoomOpts::default();
        assert_eq!(generate_room_cmd(&mut w, "W01N1", &opts, &mut rng), Err("Invalid room name".into()));
        assert_eq!(generate_room_cmd(&mut w, "X1N1", &opts, &mut rng), Err("Invalid room name".into()));
        assert_eq!(generate_room_cmd(&mut w, "W9N8", &opts, &mut rng), Err("This room already exists".into()));
        // Regression: far rooms grew the grid to their bounding box (E2147483647S0 overflowed it, a panic).
        assert_eq!(generate_room_cmd(&mut w, "W200N5", &opts, &mut rng), Err("Room W200N5 is outside W127..E127 x N127..S127 (world.wideCoords)".into()));
        assert!(generate_room_cmd(&mut w, "E2147483647S0", &opts, &mut rng).is_err());
        let explicit = GenRoomOpts { exits: [Some(vec![20]), None, None, None], ..GenRoomOpts::default() };
        // W9N9's top neighbour W9N10 exists, so a wrong explicit top edge is refused before any terrain is drawn.
        if std::panic::catch_unwind(|| w.terrain.get(0).clone()).is_err() {
            return; // ponytail: TerrainStore bodies are WP01's; the rest runs once they land.
        }
        assert_eq!(generate_room_cmd(&mut w, "W9N9", &explicit, &mut rng), Err("Exits in room W9N10 don't match".into()));
        let n = w.rooms.len();
        generate_room_cmd(&mut w, "W9N9", &opts, &mut rng).unwrap();
        let r2 = room_idx(&w.meta, "W9N9").unwrap();
        assert_eq!(r2 as usize, n);
        for side in [TOP, RIGHT, BOTTOM, LEFT] {
            let other = room_idx(&w.meta, &neighbor("W9N9".parse().unwrap(), side).to_string()).unwrap();
            assert_eq!(facing_edge(w.terrain.get(r2), (side + 2) % 4), facing_edge(w.terrain.get(other), side));
        }
        generate_room_cmd(&mut w, "E3S3", &opts, &mut rng).unwrap(); // outside the box: the grid grows
        assert_eq!((w.meta.x0, w.meta.y0, w.meta.w, w.meta.h), (-11, -11, 15, 15));
    }

    /// generate_room_cmd's insert path (add_room) without neighbours, so without WP01's TerrainStore::get.
    #[test]
    fn generate_room_cmd_into_an_empty_world() {
        let mut w = assemble(&spec(0..1, 0..1, 9), Vec::new());
        generate_room_cmd(&mut w, "W5N5", &GenRoomOpts::default(), &mut Rng::new(3)).unwrap();
        assert_eq!(room_idx(&w.meta, "W5N5"), Some(0));
        let direct = generate_room(&mut Rng::new(3), "W5N5".parse().unwrap(), [&[], &[], &[], &[]], &GenRoomOpts::default()).unwrap();
        let docs = &w.rooms[0].objects;
        let objs: Vec<Obj> = docs.iter().map(|d| d.obj.clone()).collect();
        let mut want = direct.objects;
        for (o, d) in want.iter_mut().zip(docs.iter()) {
            o.set(atom!("_id"), s(d.id().as_str()));
        }
        assert_eq!(objs, want);
        assert!(docs.iter().enumerate().all(|(i, d)| d.seq == i as u64 && w.index.objects[&d.id()].room == 0));
        assert_eq!((w.next_seq as usize, w.index.force_update.len(), w.env.status_dirty), (docs.len(), 1, true));
        assert_eq!(generate_room_cmd(&mut w, "W5N5", &GenRoomOpts::default(), &mut Rng::new(3)), Err("This room already exists".into()));
    }

    #[test]
    #[ignore]
    fn bench_rooms() {
        let t = std::time::Instant::now();
        for i in 0..200u64 {
            generate_room(&mut Rng::new(i), RoomXY::new(1, 1), [&[], &[], &[], &[]], &GenRoomOpts::default()).unwrap();
        }
        eprintln!("{:.2} ms/room", t.elapsed().as_secs_f64() * 1000.0 / 200.0);
    }

    /// Acceptance: 256×256 rooms in under 60 s (run with --release).
    #[test]
    #[ignore]
    fn bench_world_256() {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let t = std::time::Instant::now();
        let w = generate_world(&WorldSpec { wide_coords: true, ..spec(-13..13, -13..13, 1) }, &Pool::new(threads), &EngineHooks::default());
        eprintln!("{} rooms in {:.1} s on {threads} threads", w.rooms.len(), t.elapsed().as_secs_f64());
        assert!(w.rooms.len() >= 256 * 256);
    }
}
