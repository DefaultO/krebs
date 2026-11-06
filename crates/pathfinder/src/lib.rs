//! Port of the official Screeps native PathFinder (reference/driver/native/src/pf.cc, pf.h) and the
//! option handling of its JS glue (reference/driver/lib/path-finder.js).
//!
//! Results are bit-identical to the official addon: same path, ops, cost and incomplete flag, and the
//! room callback runs for the same rooms in the same order. That is why the code below mirrors the
//! C++ control flow statement by statement, including its u32 wrapping arithmetic and evaluation
//! order of `look()` calls (room allocation order depends on it). Differential tests against the
//! real addon live in tests/differential.rs.
//!
//! Coordinates: the C++ packs rooms into u8 (W127..E127). Here rooms are i32 (E0 = 0, W0 = -1,
//! S0 = 0, N0 = -1) and must satisfy |coord| < 2^24. Internally tiles live on a u32 plane that is the
//! official one translated by a multiple of 50, so every `% 50`, comparison and distance is
//! unchanged. The only behaviour that differs is the official u8/u32 wrap-around past W127/E127,
//! which no real world reaches (its edge rooms are walled).

use std::cell::Cell;

/// pf.h:14 `k_max_rooms`.
pub const MAX_ROOMS: usize = 64;

/// Room coordinates on the world map: E0 is x = 0, W0 is x = -1, S0 is y = 0, N0 is y = -1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoomXY {
    pub x: i32,
    pub y: i32,
}

/// A tile: room plus local coordinates in 0..50.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pos {
    pub room: RoomXY,
    pub x: u8,
    pub y: u8,
}

/// A search goal. The JS glue turns a bare RoomPosition into range 0 and `{pos, range}` into
/// `max(0, range | 0)` (path-finder.js:99-112).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Goal {
    pub pos: Pos,
    pub range: u32,
}

/// What the room callback says about a room (path-finder.js:120-128, pf.cc:37-59).
#[derive(Clone, Copy, Debug)]
pub enum RoomCost<'a> {
    /// Callback returned nothing usable (undefined, a CostMatrix-less object, ...): terrain only.
    Terrain,
    /// A CostMatrix's `_bits` in the official `x * 50 + y` layout. 0 = use terrain, 255 =
    /// impassable. Any length other than 2500 means terrain only (pf.cc:55). The addon takes the
    /// raw bytes of any ArrayBufferView (Nan::TypedArrayContents), so a JS bridge should pass the
    /// bytes of e.g. a Uint16Array(1250) too.
    ///
    /// Read live, like the C++ which keeps a pointer to the typed array (pf.cc:53-56, 112-113): a
    /// later room callback that writes to a matrix it returned earlier (a reused scratch matrix,
    /// say) changes the costs the rest of the search sees. Hence `Cell`; a plain `&mut [u8]`
    /// converts with `Cell::from_mut(m).as_slice_of_cells()`.
    Matrix(&'a [Cell<u8>]),
    /// Callback returned `false`: the room is never entered.
    Blocked,
    /// Callback threw: the search stops and returns `SearchError::Aborted`.
    Abort,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SearchOpts {
    /// 1..=254 after the JS clamping. 255 makes those tiles impassable, like a CostMatrix 255.
    pub plain_cost: u8,
    pub swamp_cost: u8,
    pub max_rooms: u8,
    pub max_ops: u32,
    pub max_cost: u32,
    pub flee: bool,
    pub heuristic_weight: f64,
}

impl Default for SearchOpts {
    fn default() -> Self {
        SearchOpts::from_js(0, 0, None, 0, 0, 0, false)
    }
}

impl SearchOpts {
    /// Option normalization from path-finder.js:90-96. Integer arguments are the JS `option | 0`
    /// (ToInt32) values, so 0 means "unset". `heuristic_weight` is None when the JS option is falsy
    /// (unset, 0, NaN, "", ...) and Some(ToNumber(option)) otherwise, because `||` tests the raw
    /// value: "0" gives 1 and a non-numeric truthy value like "abc" gives NaN, which the addon
    /// turns into a zero heuristic (plain Dijkstra).
    /// Note the official quirk this keeps: `maxCost: 0xffffffff` is -1 after `| 0`, which clamps to 1.
    pub fn from_js(
        plain_cost: i32,
        swamp_cost: i32,
        heuristic_weight: Option<f64>,
        max_ops: i32,
        max_cost: i32,
        max_rooms: i32,
        flee: bool,
    ) -> SearchOpts {
        let or = |v: i32, default: i64| if v == 0 { default } else { v as i64 };
        // Math.min(9, Math.max(1, w)) keeps NaN, and so does clamp (f64::max/min would drop it).
        let w = heuristic_weight.map_or(1.2, |w| w.clamp(1.0, 9.0));
        SearchOpts {
            plain_cost: or(plain_cost, 1).clamp(1, 254) as u8,
            swamp_cost: or(swamp_cost, 5).clamp(1, 254) as u8,
            heuristic_weight: w,
            max_ops: or(max_ops, 2000).max(1) as u32,
            max_cost: or(max_cost, 0xffff_ffff).max(1) as u32,
            max_rooms: or(max_rooms, 16).clamp(1, 64) as u8,
            flee,
        }
    }
}

/// Mirrors the JS result object. `path` runs from the tile after `origin` to the end.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchResult {
    pub path: Vec<Pos>,
    pub ops: u32,
    pub cost: u32,
    pub incomplete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchError {
    /// pf.cc:33 "Could not load terrain data": the terrain provider had no data for this room.
    NoTerrain(RoomXY),
    /// The room callback returned `RoomCost::Abort`.
    Aborted,
    /// A position had x/y >= 50 or a room coordinate outside +-2^24.
    InvalidPosition,
}

type Cost = u32;
const OBSTACLE: Cost = u32::MAX;
const ROOM_LIMIT: i32 = 1 << 24;
// Rooms are stored as room + 2^25, so tiles stay far from u32 overflow in both directions.
const ROOM_OFFSET: i64 = 1 << 25;
const NODES: usize = 2500 * MAX_ROOMS;
// Every room a search touches is 8-adjacent to a chain of at most MAX_ROOMS allocated rooms that
// starts at the origin room, so it lies within MAX_ROOMS rooms of it. A dense window indexed
// relative to the origin room replaces the C++ 65536-entry reverse_room_table.
const WINDOW: usize = 2 * MAX_ROOMS + 1;
const BLOCKED: u8 = u8::MAX;

type Matrix<'a> = Option<&'a [Cell<u8>; 2500]>;

// pf.cc:106-122 for every tile of a room, `x * 50 + y`: a non-zero matrix value wins, otherwise the
// terrain cost (`terrain` already went through look_table, 255 = wall).
fn merge(out: &mut [u8; 2500], matrix: Matrix, terrain: &[u8; 2500]) {
    match matrix {
        None => *out = *terrain,
        Some(m) => {
            for ((o, m), &t) in out.iter_mut().zip(m).zip(terrain) {
                *o = if m.get() != 0 { m.get() } else { t };
            }
        }
    }
}

// The cost byte tile_cost() maps to OBSTACLE. Every other byte is its own cost, so jumps compare bytes.
const WALL: u8 = 255;

#[inline(always)]
fn tile_cost(c: u8) -> Cost {
    match c {
        255 => OBSTACLE,
        c => c as Cost,
    }
}

/// A tile on the internal world plane: room_plane * 50 + local.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WPos {
    x: u32,
    y: u32,
}

impl WPos {
    // pf.h:83: (0, 0) doubles as "no position". It is a border tile, which jumps never return.
    const NULL: WPos = WPos { x: 0, y: 0 };

    fn is_null(self) -> bool {
        self == WPos::NULL
    }

    fn add(self, dx: i32, dy: i32) -> WPos {
        WPos { x: self.x.wrapping_add_signed(dx), y: self.y.wrapping_add_signed(dy) }
    }

    fn range_to(self, o: WPos) -> Cost {
        self.x.abs_diff(o.x).max(self.y.abs_diff(o.y))
    }

    fn from_pos(p: Pos) -> Result<WPos, SearchError> {
        let ok = |r: i32| (-ROOM_LIMIT..ROOM_LIMIT).contains(&r);
        if p.x >= 50 || p.y >= 50 || !ok(p.room.x) || !ok(p.room.y) {
            return Err(SearchError::InvalidPosition);
        }
        let plane = |r: i32, l: u8| ((r as i64 + ROOM_OFFSET) * 50 + l as i64) as u32;
        Ok(WPos { x: plane(p.room.x, p.x), y: plane(p.room.y, p.y) })
    }

    fn to_pos(self) -> Pos {
        let room = |v: u32| ((v / 50) as i64 - ROOM_OFFSET) as i32;
        Pos { room: RoomXY { x: room(self.x), y: room(self.y) }, x: (self.x % 50) as u8, y: (self.y % 50) as u8 }
    }
}

// pf.cc:9-17
fn is_border_pos(v: u32) -> bool {
    v.wrapping_add(1) % 50 < 2
}

fn is_near_border_pos(v: u32) -> bool {
    v.wrapping_add(2) % 50 < 4
}

/// pf.h:174 open_closed_t: generation markers so clearing is O(1).
struct OpenClosed {
    list: Vec<u32>,
    marker: u32,
}

impl OpenClosed {
    fn clear(&mut self) {
        if u32::MAX - 2 <= self.marker {
            self.list.fill(0);
            self.marker = 1;
        } else {
            self.marker += 2;
        }
    }
    fn is_open(&self, i: usize) -> bool {
        self.list[i] == self.marker
    }
    fn is_closed(&self, i: usize) -> bool {
        self.list[i] == self.marker + 1
    }
    fn open(&mut self, i: usize) {
        self.list[i] = self.marker;
    }
    fn close(&mut self, i: usize) {
        self.list[i] = self.marker + 1;
    }
}

/// pf.h:251 heap_t: 1-based binary heap whose tie-breaking (`<=` up, `>=` down) decides which of
/// equal-f nodes pops first, so it is copied exactly. The C++ finds a node for `update` by a linear
/// scan; a node is in the heap at most once (only while open), so the `slot` map finds the same
/// entry in O(1).
struct Heap {
    heap: Vec<(Cost, u32)>, // (priority, node); heap[0] unused
    slot: Vec<u32>,         // node -> index in `heap`, valid while the node is open
}

impl Heap {
    fn clear(&mut self) {
        self.heap.truncate(1);
    }
    fn is_empty(&self) -> bool {
        self.heap.len() == 1
    }
    fn priority(&self, node: u32) -> Cost {
        self.heap[self.slot[node as usize] as usize].0
    }
    // The C++ moves the last entry to the root and swaps it down. Here it is held aside while the children it
    // would swap with move up (same comparisons, ties included), and written once where it stops.
    fn pop(&mut self) -> (u32, Cost) {
        let (prio, node) = self.heap[1];
        let last = self.heap.pop().unwrap();
        let size = self.heap.len() - 1;
        if size == 0 {
            return (node, prio);
        }
        let mut uu = 1;
        loop {
            let (l, r) = (uu << 1, (uu << 1) + 1);
            let vv;
            if r <= size {
                // Without branches on which child wins: it is a coin flip.
                let (lp, rp) = (self.heap[l].0, self.heap[r].0);
                let to_l = last.0 >= lp;
                let to_r = (if to_l { lp } else { last.0 }) >= rp;
                if !to_l && !to_r {
                    break;
                }
                vv = l + to_r as usize;
            } else if l <= size && last.0 >= self.heap[l].0 {
                vv = l;
            } else {
                break;
            }
            self.heap[uu] = self.heap[vv];
            self.slot[self.heap[uu].1 as usize] = uu as u32;
            uu = vv;
        }
        self.heap[uu] = last;
        self.slot[last.1 as usize] = uu as u32;
        (node, prio)
    }
    // ponytail: the C++ throws "Max heap" at 2500*64/8 entries (crashing node); a Vec just grows.
    fn insert(&mut self, node: u32, prio: Cost) {
        self.heap.push((prio, node));
        self.bubble_up(self.heap.len() - 1);
    }
    fn update(&mut self, node: u32, prio: Cost) {
        let i = self.slot[node as usize] as usize;
        self.heap[i].0 = prio;
        self.bubble_up(i);
    }
    // Swaps up while `<=` its parent, as the C++ does, with the entry written once where it stops.
    fn bubble_up(&mut self, mut i: usize) {
        let e = self.heap[i];
        while i != 1 && e.0 <= self.heap[i >> 1].0 {
            self.heap[i] = self.heap[i >> 1];
            self.slot[self.heap[i].1 as usize] = i as u32;
            i >>= 1;
        }
        self.heap[i] = e;
        self.slot[e.1 as usize] = i as u32;
    }
}

/// Reusable search state (about 2 MB, like the C++ path_finder_t). Keep one per worker thread; a
/// room callback that wants to search recursively needs a second instance.
pub struct PathFinder {
    parents: Vec<u32>,
    open_closed: OpenClosed,
    heap: Heap,
    window: Vec<u8>,     // 0 = not looked up yet, 1..=64 = room index + 1, BLOCKED
    touched: Vec<usize>, // window slots to reset before the next search
    goals: Vec<(WPos, Cost)>,
    terrain_costs: Vec<[u8; 2500]>, // per room, x * 50 + y: plain/swamp cost, 255 = wall
    costs: Vec<[u8; 2500]>,         // per room: terrain_costs merged with the room's matrix
}

impl Default for PathFinder {
    fn default() -> Self {
        PathFinder::new()
    }
}

impl PathFinder {
    pub fn new() -> PathFinder {
        PathFinder {
            parents: vec![0; NODES],
            open_closed: OpenClosed { list: vec![0; NODES], marker: 1 },
            heap: Heap { heap: vec![(0, 0)], slot: vec![0; NODES] },
            window: vec![0; WINDOW * WINDOW],
            touched: Vec::new(),
            goals: Vec::new(),
            terrain_costs: vec![[0; 2500]; MAX_ROOMS],
            costs: vec![[0; 2500]; MAX_ROOMS],
        }
    }

    /// pf.cc:456 `search` plus the result shaping of path-finder.js:134-141.
    ///
    /// `terrain` returns a room's 2500 tiles in `y * 50 + x` order (as in room terrain strings):
    /// bit 1 = wall, bit 2 = swamp. It is asked once per room, before `room_callback`.
    /// `room_callback` is asked once per room the search wants to enter, in official order; pass
    /// `|_| RoomCost::Terrain` when the player gave no callback.
    ///
    /// Empty `goals` returns an empty complete result: the engine's `PathFinder.search` returns
    /// `{path: [], ops: 0}` before reaching the native code (engine/src/game/path-finder.js:60).
    pub fn search<'a>(
        &mut self,
        origin: Pos,
        goals: &[Goal],
        opts: &SearchOpts,
        terrain: impl FnMut(RoomXY) -> Option<&'a [u8; 2500]>,
        room_callback: impl FnMut(RoomXY) -> RoomCost<'a>,
    ) -> Result<SearchResult, SearchError> {
        // Before validating anything: the engine returns early even for a bad origin.
        if goals.is_empty() {
            return Ok(SearchResult::default());
        }
        self.goals.clear();
        for g in goals {
            self.goals.push((WPos::from_pos(g.pos)?, g.range));
        }
        let origin = WPos::from_pos(origin)?;

        // pf.cc:469-477 clean up from the previous search
        for &slot in &self.touched {
            self.window[slot] = 0;
        }
        self.touched.clear();
        self.open_closed.clear();
        self.heap.clear();

        let mut s = Search {
            parents: &mut self.parents,
            open_closed: &mut self.open_closed,
            heap: &mut self.heap,
            window: &mut self.window,
            touched: &mut self.touched,
            goals: &self.goals,
            terrain_costs: &mut self.terrain_costs,
            costs: &mut self.costs,
            rooms: [Room { base_x: 0, base_y: 0, matrix: None }; MAX_ROOMS],
            rooms_len: 0,
            max_rooms: (opts.max_rooms as usize).min(MAX_ROOMS),
            look_table: [opts.plain_cost, 255, opts.swamp_cost, 255],
            weight: opts.heuristic_weight,
            flee: opts.flee,
            win_x: (origin.x / 50).wrapping_sub(MAX_ROOMS as u32),
            win_y: (origin.y / 50).wrapping_sub(MAX_ROOMS as u32),
            hot: (0, 0, 0),
            terrain,
            callback: room_callback,
            error: None,
        };
        s.run(origin, opts.max_ops, opts.max_cost)
    }
}

/// One room's tile costs, for jumps. A jump starts on a non-border tile and stops before stepping
/// next to a border (pf.cc:197, 228, 259), so every tile it looks at is in its start room: it can
/// read that room's costs directly instead of going through `look()` (same values, no side effects).
struct JumpRoom<'c> {
    costs: &'c [u8; 2500],
    base_x: u32,
    base_y: u32,
}

impl JumpRoom<'_> {
    fn index(&self, p: WPos) -> usize {
        let (x, y) = (p.x.wrapping_sub(self.base_x), p.y.wrapping_sub(self.base_y));
        debug_assert!(x < 50 && y < 50, "jump left its room");
        (x * 50 + y) as usize
    }

    fn at(&self, i: usize, offset: isize) -> u8 {
        self.costs[i.wrapping_add_signed(offset)]
    }

    // Steps along one axis from `p` (direction `d`) before is_near_border_pos holds.
    fn border_steps(c: u32, d: i32, base: u32) -> u32 {
        let local = c.wrapping_sub(base);
        if d == 0 {
            u32::MAX
        } else if is_near_border_pos(local) {
            0
        } else if d > 0 {
            48 - local
        } else {
            local - 1
        }
    }
}

// Costs of a room the search can't enter: look() returns OBSTACLE for all of its tiles.
static ALL_OBSTACLE: [u8; 2500] = [255; 2500];

#[derive(Clone, Copy)]
struct Room<'a> {
    base_x: u32,
    base_y: u32,
    matrix: Matrix<'a>,
}

struct Search<'s, 'a, T, C> {
    parents: &'s mut [u32],
    open_closed: &'s mut OpenClosed,
    heap: &'s mut Heap,
    window: &'s mut [u8],
    touched: &'s mut Vec<usize>,
    goals: &'s [(WPos, Cost)],
    terrain_costs: &'s mut [[u8; 2500]],
    costs: &'s mut [[u8; 2500]],
    rooms: [Room<'a>; MAX_ROOMS],
    rooms_len: usize,
    max_rooms: usize,
    look_table: [u8; 4],
    weight: f64,
    flee: bool,
    win_x: u32,
    win_y: u32,
    // The last room room_index() found: (base_x, base_y, room). A room's entry never changes once set.
    hot: (u32, u32, usize),
    terrain: T,
    callback: C,
    // The C++ unwinds with an exception. Here the first error is recorded, later room lookups fail
    // without calling out again, and the main loop returns it at the next check.
    error: Option<SearchError>,
}

impl<'s, 'a, T, C> Search<'s, 'a, T, C>
where
    T: FnMut(RoomXY) -> Option<&'a [u8; 2500]>,
    C: FnMut(RoomXY) -> RoomCost<'a>,
{
    fn run(&mut self, origin: WPos, max_ops: u32, max_cost: u32) -> Result<SearchResult, SearchError> {
        let mut ops_remaining = max_ops;
        let mut min_node_h_cost = Cost::MAX;
        let mut min_node_g_cost = Cost::MAX;

        // pf.cc:506 searching to the same node: JS `undefined` -> empty, complete
        if self.heuristic(origin) == 0 {
            return Ok(SearchResult::default());
        }
        // pf.cc:515 initial room inaccessible: JS -1 -> empty, incomplete
        if self.room_index(origin) == 0 {
            return match self.error {
                Some(e) => Err(e),
                None => Ok(SearchResult { incomplete: true, ..SearchResult::default() }),
            };
        }
        let mut min_node = self.index_from_pos(origin);
        self.astar(min_node, origin, 0);
        if let Some(e) = self.error {
            return Err(e);
        }

        // pf.cc:526
        while !self.heap.is_empty() && ops_remaining > 0 {
            let (index, f_cost) = self.heap.pop();
            self.open_closed.close(index as usize);

            let pos = self.pos_from_index(index);
            let h_cost = self.heuristic(pos);
            let g_cost = f_cost.wrapping_sub(self.weighted(h_cost));

            if h_cost == 0 {
                min_node = index;
                min_node_h_cost = 0;
                min_node_g_cost = g_cost;
                break;
            } else if h_cost < min_node_h_cost {
                min_node = index;
                min_node_h_cost = h_cost;
                min_node_g_cost = g_cost;
            }
            if g_cost.wrapping_add(h_cost) > max_cost {
                break;
            }

            self.jps(index, pos, g_cost);
            ops_remaining -= 1;
            // ponytail: pf.cc:558 also stops here when the isolate is being terminated (CPU limit);
            // there is no such hook yet, add a cancel flag checked here if the runtime needs one.
            if let Some(e) = self.error {
                return Err(e);
            }
        }

        // pf.cc:569 reconstruct the path, filling in the tiles skipped by jumps
        let mut path = Vec::new();
        let mut index = min_node;
        let mut pos = self.pos_from_index(index);
        while pos != origin {
            path.push(pos.to_pos());
            index = self.parents[index as usize];
            let next = self.pos_from_index(index);
            if next.range_to(pos) > 1 {
                let dx = (next.x as i64 - pos.x as i64).signum() as i32;
                let dy = (next.y as i64 - pos.y as i64).signum() as i32;
                loop {
                    pos = pos.add(dx, dy);
                    path.push(pos.to_pos());
                    if pos.range_to(next) <= 1 {
                        break;
                    }
                }
            }
            pos = next;
        }
        path.reverse();
        Ok(SearchResult {
            path,
            ops: max_ops - ops_remaining,
            cost: min_node_g_cost,
            incomplete: min_node_h_cost != 0,
        })
    }

    // `cost_t(h * heuristic_weight)` in pf.cc:88 and :535. Out of u32 range that is UB in C++; the
    // x64 builds convert through a 64-bit signed integer (cvttsd2si), so values wrap mod 2^32 and
    // NaN gives 0. Reachable from JS: flee with `range: 1e9, heuristicWeight: 9`, or a weight "abc".
    fn weighted(&self, h: Cost) -> Cost {
        (h as f64 * self.weight) as i64 as Cost
    }

    /// pf.cc:22 room_index_from_pos: 1-based room index, 0 if the room can't be entered; and the
    /// tile's index in the room, `x * 50 + y`.
    #[inline(always)]
    fn tile(&mut self, p: WPos) -> (usize, usize) {
        let (x, y) = (p.x.wrapping_sub(self.hot.0), p.y.wrapping_sub(self.hot.1));
        if x < 50 && y < 50 {
            return (self.hot.2, (x * 50 + y) as usize);
        }
        (self.room_index(p), (p.x % 50 * 50 + p.y % 50) as usize)
    }

    fn room_index(&mut self, p: WPos) -> usize {
        let (rx, ry) = (p.x / 50, p.y / 50);
        let (dx, dy) = (rx.wrapping_sub(self.win_x) as usize, ry.wrapping_sub(self.win_y) as usize);
        assert!(dx < WINDOW && dy < WINDOW, "room outside the search window");
        let slot = dx * WINDOW + dy;
        let room = match self.window[slot] {
            0 => self.alloc_room(slot, rx, ry),
            BLOCKED => 0,
            v => v as usize,
        };
        if room != 0 {
            self.hot = (rx * 50, ry * 50, room);
        }
        room
    }

    #[cold]
    fn alloc_room(&mut self, slot: usize, rx: u32, ry: u32) -> usize {
        if self.rooms_len >= self.max_rooms || self.error.is_some() {
            return 0;
        }
        let xy = RoomXY { x: (rx as i64 - ROOM_OFFSET) as i32, y: (ry as i64 - ROOM_OFFSET) as i32 };
        let Some(terrain) = (self.terrain)(xy) else {
            self.error = Some(SearchError::NoTerrain(xy));
            return 0;
        };
        let cost = (self.callback)(xy);
        // The callback is player code: it may have written to a matrix it returned for an earlier
        // room, which the C++ reads live.
        for k in 0..self.rooms_len {
            if let Some(m) = self.rooms[k].matrix {
                merge(&mut self.costs[k], Some(m), &self.terrain_costs[k]);
            }
        }
        let matrix = match cost {
            RoomCost::Terrain => None,
            RoomCost::Matrix(m) => m.try_into().ok(),
            RoomCost::Blocked => {
                self.window[slot] = BLOCKED;
                self.touched.push(slot);
                return 0;
            }
            RoomCost::Abort => {
                self.error = Some(SearchError::Aborted);
                return 0;
            }
        };
        // pf.cc:122 terrain half of look(), transposed to the matrix layout.
        let costs = &mut self.terrain_costs[self.rooms_len];
        for x in 0..50 {
            for y in 0..50 {
                costs[x * 50 + y] = self.look_table[(terrain[y * 50 + x] & 3) as usize];
            }
        }
        merge(&mut self.costs[self.rooms_len], matrix, costs);
        self.rooms[self.rooms_len] = Room { base_x: rx * 50, base_y: ry * 50, matrix };
        self.rooms_len += 1;
        self.window[slot] = self.rooms_len as u8;
        self.touched.push(slot);
        self.rooms_len
    }

    // pf.cc:67 index_from_pos. Callers only pass tiles whose look() succeeded, so the room exists;
    // the C++ throws (and takes node down) if not.
    fn index_from_pos(&mut self, p: WPos) -> u32 {
        let (room, i) = self.tile(p);
        assert!(room != 0, "index_from_pos on an unavailable room");
        ((room - 1) * 2500 + i) as u32
    }

    fn pos_from_index(&self, index: u32) -> WPos {
        let room = &self.rooms[(index / 2500) as usize];
        let coord = index % 2500;
        WPos { x: coord / 50 + room.base_x, y: coord % 50 + room.base_y }
    }

    // pf.cc:83
    fn push_node(&mut self, parent_index: u32, node: WPos, g_cost: Cost) {
        let index = self.index_from_pos(node);
        self.push_index(parent_index, index, node, g_cost);
    }

    #[inline]
    fn push_index(&mut self, parent_index: u32, index: u32, node: WPos, g_cost: Cost) {
        if self.open_closed.is_closed(index as usize) {
            return;
        }
        let h_cost = self.weighted(self.heuristic(node));
        let f_cost = h_cost.wrapping_add(g_cost);

        if self.open_closed.is_open(index as usize) {
            if self.heap.priority(index) > f_cost {
                self.heap.update(index, f_cost);
                self.parents[index as usize] = parent_index;
            }
        } else {
            self.heap.insert(index, f_cost);
            self.open_closed.open(index as usize);
            self.parents[index as usize] = parent_index;
        }
    }

    // pf.cc:106
    #[inline]
    fn look(&mut self, p: WPos) -> Cost {
        let (room, i) = self.tile(p);
        if room == 0 {
            return OBSTACLE;
        }
        tile_cost(self.costs[room - 1][i])
    }

    // pf.cc:126
    fn heuristic(&self, p: WPos) -> Cost {
        if self.flee {
            let mut ret = 0;
            for &(goal, range) in self.goals {
                let dist = p.range_to(goal);
                if dist < range {
                    ret = ret.max(range - dist);
                }
            }
            ret
        } else {
            let mut ret = Cost::MAX;
            for &(goal, range) in self.goals {
                let dist = p.range_to(goal);
                if dist > range {
                    ret = ret.min(dist - range);
                } else {
                    ret = 0;
                }
            }
            ret
        }
    }

    // pf.cc:151 plain A* step, only used for the origin. Direction order is TOP, TOP_RIGHT, ...
    fn astar(&mut self, index: u32, pos: WPos, g_cost: Cost) {
        const DIRS: [(i32, i32); 8] = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];
        for (dx, dy) in DIRS {
            let n = pos.add(dx, dy);
            // Moves along a room edge or diagonally through an exit are impossible
            if pos.x % 50 == 0 {
                if (n.x % 50 == 49 && pos.y != n.y) || pos.x == n.x {
                    continue;
                }
            } else if pos.x % 50 == 49 {
                if (n.x % 50 == 0 && pos.y != n.y) || pos.x == n.x {
                    continue;
                }
            } else if pos.y % 50 == 0 {
                if (n.y % 50 == 49 && pos.x != n.x) || pos.y == n.y {
                    continue;
                }
            } else if pos.y % 50 == 49 {
                if (n.y % 50 == 0 && pos.x != n.x) || pos.y == n.y {
                    continue;
                }
            }
            let n_cost = self.look(n);
            if n_cost == OBSTACLE {
                continue;
            }
            self.push_node(index, n, g_cost.wrapping_add(n_cost));
        }
    }

    // How many steps a jump from `pos` along (dx, dy) takes before one of the checks pf.cc:197, 228,
    // 259 make ahead of every step stops it: next to a border, or heuristic 0. Non-flee heuristic 0
    // means some goal in range, a run of steps per goal; flee jumps check theirs per step instead.
    fn jump_steps(&self, r: &JumpRoom, pos: WPos, dx: i32, dy: i32) -> u32 {
        let border = JumpRoom::border_steps(pos.x, dx, r.base_x).min(JumpRoom::border_steps(pos.y, dy, r.base_y));
        if self.flee {
            return border;
        }
        let mut steps = border as i64;
        for &(goal, range) in self.goals {
            // |c + k * d - g| <= range for k >= 0: an interval of k per axis, all k if d == 0.
            let (mut lo, mut hi) = (0, i64::MAX);
            for (c, d, g) in [(pos.x, dx, goal.x), (pos.y, dy, goal.y)] {
                let off = g as i64 - c as i64;
                if d == 0 {
                    if off.abs() > range as i64 {
                        hi = -1;
                    }
                } else {
                    let mid = d as i64 * off;
                    lo = lo.max(mid - range as i64);
                    hi = hi.min(mid + range as i64);
                }
            }
            if lo <= hi {
                steps = steps.min(lo);
            }
        }
        steps as u32
    }

    // pf.cc:193 JPS dragons. Tile i + 50 is one step in x, i + 1 one in y.
    fn jump_x(&self, r: &JumpRoom, cost: u8, pos: WPos, dx: i32) -> WPos {
        let steps = self.jump_steps(r, pos, dx, 0);
        let sx = dx as isize * 50;
        let mut i = r.index(pos);
        let mut prev_cost_u = r.at(i, -1);
        let mut prev_cost_d = r.at(i, 1);
        let mut k = 0;
        loop {
            if k == steps || (self.flee && self.heuristic(pos.add(k as i32 * dx, 0)) == 0) {
                break;
            }
            let cost_u = r.at(i, sx - 1);
            let cost_d = r.at(i, sx + 1);
            if (cost_u != WALL && prev_cost_u != cost) || (cost_d != WALL && prev_cost_d != cost) {
                break;
            }
            prev_cost_u = cost_u;
            prev_cost_d = cost_d;
            i = i.wrapping_add_signed(sx);
            k += 1;

            let jump_cost = r.at(i, 0);
            if jump_cost == WALL {
                return WPos::NULL;
            } else if jump_cost != cost {
                break;
            }
        }
        pos.add(k as i32 * dx, 0)
    }

    // pf.cc:224
    fn jump_y(&self, r: &JumpRoom, cost: u8, pos: WPos, dy: i32) -> WPos {
        let steps = self.jump_steps(r, pos, 0, dy);
        let sy = dy as isize;
        let mut i = r.index(pos);
        let mut prev_cost_l = r.at(i, -50);
        let mut prev_cost_r = r.at(i, 50);
        let mut k = 0;
        loop {
            if k == steps || (self.flee && self.heuristic(pos.add(0, k as i32 * dy)) == 0) {
                break;
            }
            let cost_l = r.at(i, sy - 50);
            let cost_r = r.at(i, sy + 50);
            if (cost_l != WALL && prev_cost_l != cost) || (cost_r != WALL && prev_cost_r != cost) {
                break;
            }
            prev_cost_l = cost_l;
            prev_cost_r = cost_r;
            i = i.wrapping_add_signed(sy);
            k += 1;

            let jump_cost = r.at(i, 0);
            if jump_cost == WALL {
                return WPos::NULL;
            } else if jump_cost != cost {
                break;
            }
        }
        pos.add(0, k as i32 * dy)
    }

    // pf.cc:255
    fn jump_xy(&self, r: &JumpRoom, cost: u8, mut pos: WPos, dx: i32, dy: i32) -> WPos {
        let steps = self.jump_steps(r, pos, dx, dy);
        let (sx, sy) = (dx as isize * 50, dy as isize);
        let mut i = r.index(pos);
        let mut prev_cost_x = r.at(i, -sx);
        let mut prev_cost_y = r.at(i, -sy);
        let mut k = 0;
        loop {
            if k == steps || (self.flee && self.heuristic(pos) == 0) {
                break;
            }
            if (r.at(i, sy - sx) != WALL && prev_cost_x != cost) || (r.at(i, sx - sy) != WALL && prev_cost_y != cost) {
                break;
            }
            prev_cost_x = r.at(i, sy);
            prev_cost_y = r.at(i, sx);
            if (prev_cost_y != WALL && !self.jump_x(r, cost, pos.add(dx, 0), dx).is_null())
                || (prev_cost_x != WALL && !self.jump_y(r, cost, pos.add(0, dy), dy).is_null())
            {
                break;
            }
            pos = pos.add(dx, dy);
            i = i.wrapping_add_signed(sx + sy);
            k += 1;

            let jump_cost = r.at(i, 0);
            if jump_cost == WALL {
                pos = WPos::NULL;
                break;
            } else if jump_cost != cost {
                break;
            }
        }
        pos
    }

    // pf.cc:292
    // Returns the jump's end with what jump_neighbor needs of it: index_from_pos(end) and look(end).
    fn jump(&mut self, cost: Cost, pos: WPos, dx: i32, dy: i32) -> Option<(WPos, u32, Cost)> {
        let cost = if cost == OBSTACLE { WALL } else { cost as u8 };
        // The start tile was just looked at, so this never allocates a room.
        let room = self.tile(pos).0;
        let r = if room == 0 {
            JumpRoom { costs: &ALL_OBSTACLE, base_x: pos.x / 50 * 50, base_y: pos.y / 50 * 50 }
        } else {
            let base = &self.rooms[room - 1];
            JumpRoom { costs: &self.costs[room - 1], base_x: base.base_x, base_y: base.base_y }
        };
        let end = if dx != 0 {
            if dy != 0 {
                self.jump_xy(&r, cost, pos, dx, dy)
            } else {
                self.jump_x(&r, cost, pos, dx)
            }
        } else {
            self.jump_y(&r, cost, pos, dy)
        };
        if end.is_null() {
            return None;
        }
        let i = r.index(end);
        assert!(room != 0, "index_from_pos on an unavailable room");
        Some((end, ((room - 1) * 2500 + i) as u32, tile_cost(r.costs[i])))
    }

    // pf.cc:304
    fn jps(&mut self, index: u32, pos: WPos, g_cost: Cost) {
        let parent = self.pos_from_index(self.parents[index as usize]);
        let sign = |a: u32, b: u32| if a > b { 1 } else if a < b { -1 } else { 0 };
        let dx = sign(pos.x, parent.x);
        let dy = sign(pos.y, parent.y);

        // Jumping to/from a border tile: only the moves through or away from the exit are possible
        let mut neighbors = [WPos::NULL; 3];
        let mut neighbor_count = 0;
        if pos.x % 50 == 0 {
            if dx == -1 {
                neighbors[0] = pos.add(-1, 0);
                neighbor_count = 1;
            } else if dx == 1 {
                neighbors = [pos.add(1, -1), pos.add(1, 0), pos.add(1, 1)];
                neighbor_count = 3;
            }
        } else if pos.x % 50 == 49 {
            if dx == 1 {
                neighbors[0] = pos.add(1, 0);
                neighbor_count = 1;
            } else if dx == -1 {
                neighbors = [pos.add(-1, -1), pos.add(-1, 0), pos.add(-1, 1)];
                neighbor_count = 3;
            }
        } else if pos.y % 50 == 0 {
            if dy == -1 {
                neighbors[0] = pos.add(0, -1);
                neighbor_count = 1;
            } else if dy == 1 {
                neighbors = [pos.add(-1, 1), pos.add(0, 1), pos.add(1, 1)];
                neighbor_count = 3;
            }
        } else if pos.y % 50 == 49 {
            if dy == 1 {
                neighbors[0] = pos.add(0, 1);
                neighbor_count = 1;
            } else if dy == -1 {
                neighbors = [pos.add(-1, -1), pos.add(0, -1), pos.add(1, -1)];
                neighbor_count = 3;
            }
        }
        if neighbor_count != 0 {
            for &n in &neighbors[..neighbor_count] {
                let n_cost = self.look(n);
                if n_cost == OBSTACLE {
                    continue;
                }
                self.push_node(index, n, g_cost.wrapping_add(n_cost));
            }
            return;
        }

        // Regular JPS iteration. Off the border (it can get here on one, moving along it), pos and its
        // 8 neighbours are in its allocated room: no look() can reach a room callback, so their costs
        // are read up front.
        let i = index as usize % 2500;
        let around = if (1..49).contains(&(i / 50)) && (1..49).contains(&(i % 50)) {
            let room = &self.costs[index as usize / 2500];
            let mut a = [[0; 3]; 3];
            for (x, col) in a.iter_mut().enumerate() {
                for (y, c) in col.iter_mut().enumerate() {
                    *c = tile_cost(room[i + x * 50 + y - 51]);
                }
            }
            Some(a)
        } else {
            None
        };

        // Next to a border, jumps parallel to it are not taken.
        let border_dx = match pos.x % 50 {
            1 => -1,
            48 => 1,
            _ => 0,
        };
        let border_dy = match pos.y % 50 {
            1 => -1,
            48 => 1,
            _ => 0,
        };

        let cost = self.near(&around, pos, 0, 0);
        if dx != 0 {
            let n_cost = self.near(&around, pos, dx, 0);
            if n_cost != OBSTACLE {
                if border_dy == 0 {
                    self.jump_neighbor(pos, index, pos.add(dx, 0), g_cost, cost, n_cost);
                } else {
                    self.push_node(index, pos.add(dx, 0), g_cost.wrapping_add(n_cost));
                }
            }
        }
        if dy != 0 {
            let n_cost = self.near(&around, pos, 0, dy);
            if n_cost != OBSTACLE {
                if border_dx == 0 {
                    self.jump_neighbor(pos, index, pos.add(0, dy), g_cost, cost, n_cost);
                } else {
                    self.push_node(index, pos.add(0, dy), g_cost.wrapping_add(n_cost));
                }
            }
        }

        // Forced neighbor rules
        if dx != 0 {
            if dy != 0 {
                let n_cost = self.near(&around, pos, dx, dy);
                if n_cost != OBSTACLE {
                    self.jump_neighbor(pos, index, pos.add(dx, dy), g_cost, cost, n_cost);
                }
                if self.near(&around, pos, -dx, 0) != cost {
                    self.forced(&around, pos, index, -dx, dy, g_cost, cost);
                }
                if self.near(&around, pos, 0, -dy) != cost {
                    self.forced(&around, pos, index, dx, -dy, g_cost, cost);
                }
            } else {
                if border_dy == 1 || self.near(&around, pos, 0, 1) != cost {
                    self.forced(&around, pos, index, dx, 1, g_cost, cost);
                }
                if border_dy == -1 || self.near(&around, pos, 0, -1) != cost {
                    self.forced(&around, pos, index, dx, -1, g_cost, cost);
                }
            }
        } else {
            if border_dx == 1 || self.near(&around, pos, 1, 0) != cost {
                self.forced(&around, pos, index, 1, dy, g_cost, cost);
            }
            if border_dx == -1 || self.near(&around, pos, -1, 0) != cost {
                self.forced(&around, pos, index, -1, dy, g_cost, cost);
            }
        }
    }

    // look(pos + (dx, dy)), from the costs jps read up front when it could.
    fn near(&mut self, around: &Option<[[Cost; 3]; 3]>, pos: WPos, dx: i32, dy: i32) -> Cost {
        match around {
            Some(a) => a[(dx + 1) as usize][(dy + 1) as usize],
            None => self.look(pos.add(dx, dy)),
        }
    }

    // `jump_neighbor(pos, index, n, g_cost, cost, look(n))`, which the C++ spells out each time.
    #[allow(clippy::too_many_arguments)]
    fn forced(&mut self, around: &Option<[[Cost; 3]; 3]>, pos: WPos, index: u32, dx: i32, dy: i32, g_cost: Cost, cost: Cost) {
        let n_cost = self.near(around, pos, dx, dy);
        self.jump_neighbor(pos, index, pos.add(dx, dy), g_cost, cost, n_cost);
    }

    // pf.cc:439
    fn jump_neighbor(&mut self, pos: WPos, index: u32, neighbor: WPos, mut g_cost: Cost, cost: Cost, n_cost: Cost) {
        if n_cost != cost || is_border_pos(neighbor.x) || is_border_pos(neighbor.y) {
            if n_cost == OBSTACLE {
                return;
            }
            g_cost = g_cost.wrapping_add(n_cost);
        } else {
            let dx = neighbor.x.wrapping_sub(pos.x) as i32;
            let dy = neighbor.y.wrapping_sub(pos.y) as i32;
            let Some((end, end_index, end_cost)) = self.jump(n_cost, neighbor, dx, dy) else { return };
            let skipped = n_cost.wrapping_mul(pos.range_to(end).wrapping_sub(1));
            g_cost = g_cost.wrapping_add(skipped.wrapping_add(end_cost));
            return self.push_index(index, end_index, end, g_cost);
        }
        self.push_node(index, neighbor, g_cost);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_room_straight_line() {
        let terrain = [0u8; 2500];
        let room = RoomXY { x: 0, y: 0 };
        let at = |x, y| Pos { room, x, y };
        let mut pf = PathFinder::new();
        let r = pf
            .search(at(10, 10), &[Goal { pos: at(20, 10), range: 0 }], &SearchOpts::default(), |_| Some(&terrain), |_| {
                RoomCost::Terrain
            })
            .unwrap();
        assert_eq!(r.path.len(), 10);
        assert_eq!(*r.path.last().unwrap(), at(20, 10));
        assert_eq!(r.cost, 10);
        assert!(!r.incomplete);
    }

    // The fixture world is 10 rooms wide; this walks the search window to its edges (64 rooms out).
    #[test]
    fn sixty_four_rooms_in_a_line() {
        // Walled top and bottom rows: rooms connect only east-west.
        let mut terrain = [0u8; 2500];
        terrain[..50].fill(1);
        terrain[49 * 50..].fill(1);
        let opts = SearchOpts { max_rooms: 64, max_ops: 1_000_000, ..SearchOpts::default() };
        let mut pf = PathFinder::new();
        for dir in [1, -1] {
            let origin = Pos { room: RoomXY { x: 0, y: 0 }, x: 25, y: 25 };
            let goal = Goal { pos: Pos { room: RoomXY { x: 100 * dir, y: 0 }, x: 25, y: 25 }, range: 0 };
            let mut entered = 0;
            let r = pf
                .search(origin, &[goal], &opts, |_| Some(&terrain), |room| {
                    if room.x * dir < 0 {
                        return RoomCost::Blocked;
                    }
                    entered += 1;
                    RoomCost::Terrain
                })
                .unwrap();
            assert_eq!(entered, 64);
            assert!(r.incomplete);
            let end = r.path.last().unwrap();
            assert_eq!((end.room.x, end.room.y), (63 * dir, 0), "{end:?}");
        }
    }

    #[test]
    fn js_option_defaults() {
        let d = SearchOpts::default();
        assert_eq!((d.plain_cost, d.swamp_cost, d.max_ops, d.max_cost, d.max_rooms), (1, 5, 2000, u32::MAX, 16));
        assert_eq!(d.heuristic_weight, 1.2);
        assert_eq!(SearchOpts::from_js(0, 0, None, 0, -1, 0, false).max_cost, 1);
    }

    // E0S0..E2S0 in a row, open between each other, walled around the outside.
    fn three_rooms() -> [[u8; 2500]; 3] {
        let mut rooms = [[0u8; 2500]; 3];
        for (i, t) in rooms.iter_mut().enumerate() {
            for k in 0..50 {
                t[k] = 1;
                t[49 * 50 + k] = 1;
                if i == 0 {
                    t[k * 50] = 1;
                }
                if i == 2 {
                    t[k * 50 + 49] = 1;
                }
            }
        }
        rooms
    }

    // Expected numbers recorded from the official addon (version 11) through path-finder.js.
    #[test]
    fn heuristic_weight_from_raw_js_values() {
        let rooms = three_rooms();
        let at = |r, x, y| Pos { room: RoomXY { x: r, y: 0 }, x, y };
        let mut pf = PathFinder::new();
        // (JS heuristicWeight, from_js argument, ops)
        for (js, w, ops) in [
            ("undefined", None, 30),
            ("'0'", Some(0.0), 138),
            ("'2'", Some(2.0), 66),
            ("'abc'", Some(f64::NAN), 1405),
            ("[3]", Some(3.0), 94),
            ("-5", Some(-5.0), 138),
            ("Infinity", Some(f64::INFINITY), 96),
        ] {
            let opts = SearchOpts::from_js(0, 0, w, 0, 0, 3, false);
            let r = pf
                .search(at(0, 10, 25), &[Goal { pos: at(2, 40, 25), range: 0 }], &opts, |r| rooms.get(r.x as usize), |_| {
                    RoomCost::Terrain
                })
                .unwrap();
            assert_eq!((r.ops, r.cost, r.path.len(), r.incomplete), (ops, 130, 130, false), "heuristicWeight {js}");
        }
    }

    // A callback that rewrites a matrix it returned earlier changes what the search sees, because
    // the addon keeps a pointer. Numbers recorded from the official addon.
    #[test]
    fn matrices_are_read_live() {
        let rooms = three_rooms();
        let at = |x, y| Pos { room: RoomXY { x: 0, y: 0 }, x, y };
        let wall = |m: &[Cell<u8>], room: i32| {
            for (i, c) in m.iter().enumerate() {
                let (x, y) = (i / 50, i % 50);
                c.set(if room == 0 && x == 30 && (1..49).contains(&y) && y != 2 { 255 } else { 0 });
            }
        };
        let mut pf = PathFinder::new();
        let goal = [Goal { pos: at(5, 25), range: 0 }];
        let opts = SearchOpts::default();

        let fresh: Vec<Vec<Cell<u8>>> = (0..3).map(|_| (0..2500).map(|_| Cell::new(0)).collect()).collect();
        let r = pf
            .search(at(49, 25), &goal, &opts, |r| rooms.get(r.x as usize), |r| {
                wall(&fresh[r.x as usize], r.x);
                RoomCost::Matrix(&fresh[r.x as usize])
            })
            .unwrap();
        assert_eq!((r.ops, r.cost, r.path.len()), (63, 48, 48));

        // One scratch matrix for every room: entering E1S0 clears E0S0's wall.
        let scratch: Vec<Cell<u8>> = (0..2500).map(|_| Cell::new(0)).collect();
        let mut calls = Vec::new();
        let r = pf
            .search(at(49, 25), &goal, &opts, |r| rooms.get(r.x as usize), |r| {
                calls.push(r.x);
                wall(&scratch, r.x);
                RoomCost::Matrix(&scratch)
            })
            .unwrap();
        assert_eq!(calls, [0, 1]);
        assert_eq!((r.ops, r.cost, r.path.len()), (60, 44, 44));
    }

    // engine/src/game/path-finder.js:60 returns before the glue validates any position.
    #[test]
    fn empty_goals_skip_validation() {
        let bad = Pos { room: RoomXY { x: 0, y: 0 }, x: 50, y: 0 };
        let ok = Pos { x: 0, ..bad };
        let mut pf = PathFinder::new();
        let r = pf.search(bad, &[], &SearchOpts::default(), |_| None, |_| RoomCost::Terrain);
        assert_eq!(r, Ok(SearchResult::default()));
        let r = pf.search(bad, &[Goal { pos: ok, range: 0 }], &SearchOpts::default(), |_| None, |_| RoomCost::Terrain);
        assert_eq!(r, Err(SearchError::InvalidPosition));
    }
}
