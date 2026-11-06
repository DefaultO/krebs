//! World natives: terrain, nativeMod (search/loadTerrain/version over crates/pathfinder), isAccessible,
//! roomStatus, gridData, worldSize (ARCHITECTURE.md §6.7). Owner: WP08.
//!
//! Every name lookup is exact, like vanilla's object-key reads (`staticTerrainData[room]`,
//! `_.contains(accessibleRooms, room)`, `roomStatusData.novice[room]`): `w1n1` is not `W1N1` here
//! even though the engine's name parser accepts both.

use crate::natives::{set_fn, with_run};
use engine::world::{RoomIdx, RoomStatus, UserIdx, World};
use pathfinder::{Goal, PathFinder, Pos, RoomCost, RoomXY, SearchError, SearchOpts};
use std::cell::{Cell, RefCell};

/// What the runtime reads from the world during a run (natives, intent routing, heap limit). `World`
/// implements it; tests implement it over fixtures because the world services are a separate package.
pub trait WorldAccess: Sync {
    /// `RoomsMeta::by_name` (engine name parser; callers check the exact name).
    fn room_by_name(&self, name: &str) -> Option<RoomIdx>;
    /// Engine coordinates (W0 = -1, E0 = 0, N0 = -1, S0 = 0).
    fn room_by_xy(&self, x: i32, y: i32) -> Option<RoomIdx>;
    fn room_name(&self, r: RoomIdx) -> &str;
    /// Row-major terrain codes (bit 1 wall, bit 2 swamp).
    fn terrain(&self, r: RoomIdx) -> &[u8; 2500];
    /// mapgrid.js plain-tile counts on the t/r/b/l edges.
    fn exit_counts(&self, r: RoomIdx) -> [u8; 4];
    fn accessible(&self, r: RoomIdx) -> bool;
    /// Non-normal rooms only: (`'closed' | 'novice' | 'respawn'`, timestamp).
    fn status(&self, r: RoomIdx) -> Option<(&'static str, Option<i64>)>;
    fn world_size(&self) -> u32;
    /// Vanilla `staticTerrainDataSize / 2^20`, added to the heap limit (VM-HEAP-LIMIT).
    fn terrain_heap_mb(&self) -> f64;
    /// `users._id`.
    fn user_id(&self, u: UserIdx) -> &str;
    /// INT-STORE routing (§4.3): the object's room if it is the user's or lies in a visible room.
    fn object_room(&self, id: &str, user: UserIdx, visible: &[RoomIdx]) -> Option<RoomIdx>;
    /// `engine::intents::parse_store_output` of a `__store` result.
    fn parse_intents(&self, store: &util::json::Json) -> engine::intents::UserIntents;
    /// The current market version (natives_market.rs builds its JSON once per version).
    fn market(&self) -> &std::sync::Arc<engine::world::MarketState>;
}

impl WorldAccess for World {
    fn room_by_name(&self, name: &str) -> Option<RoomIdx> {
        self.meta.by_name(name)
    }
    fn room_by_xy(&self, x: i32, y: i32) -> Option<RoomIdx> {
        self.meta.idx(engine::room_name::RoomXY::new(x, y))
    }
    fn room_name(&self, r: RoomIdx) -> &str {
        &self.meta.rooms[r as usize].name
    }
    fn terrain(&self, r: RoomIdx) -> &[u8; 2500] {
        self.terrain.raw(r)
    }
    fn exit_counts(&self, r: RoomIdx) -> [u8; 4] {
        self.terrain.exit_counts(r)
    }
    fn accessible(&self, r: RoomIdx) -> bool {
        self.access.accessible.get(r as usize / 64).is_some_and(|w| w >> (r % 64) & 1 == 1)
    }
    fn status(&self, r: RoomIdx) -> Option<(&'static str, Option<i64>)> {
        self.access.status.get(&r).and_then(|(s, t)| {
            let s = match s {
                RoomStatus::Normal => return None,
                RoomStatus::Closed => "closed",
                RoomStatus::Novice => "novice",
                RoomStatus::Respawn => "respawn",
            };
            Some((s, *t))
        })
    }
    fn world_size(&self) -> u32 {
        self.meta.world_size
    }
    fn terrain_heap_mb(&self) -> f64 {
        self.terrain.heap_mb()
    }
    fn user_id(&self, u: UserIdx) -> &str {
        self.users[u as usize].id.as_str()
    }
    fn object_room(&self, id: &str, user: UserIdx, visible: &[RoomIdx]) -> Option<RoomIdx> {
        let loc = self.index.objects.get(&engine::doc::Id::new(id)?)?;
        (loc.owner == Some(user) || visible.binary_search(&loc.room).is_ok()).then_some(loc.room)
    }
    fn parse_intents(&self, store: &util::json::Json) -> engine::intents::UserIntents {
        engine::intents::parse_store_output(store, &self.meta)
    }
    fn market(&self) -> &std::sync::Arc<engine::world::MarketState> {
        &self.market
    }
}

/// Adds the world natives to the `_init` natives object.
pub(crate) fn install(scope: &mut v8::PinScope, natives: v8::Local<v8::Object>) {
    set_fn(scope, natives, "terrain", terrain);
    set_fn(scope, natives, "worldSize", world_size);
    set_fn(scope, natives, "isAccessible", is_accessible);
    set_fn(scope, natives, "roomStatus", room_status);
    set_fn(scope, natives, "gridData", grid_data);
    // Addon object as `ivm.NativeModule.create(context)` exposes it (driver/native/src/main.cc:53-57).
    let native_mod = v8::Object::new(scope);
    set_fn(scope, native_mod, "search", search);
    set_fn(scope, native_mod, "loadTerrain", load_terrain);
    let version = v8::Number::new(scope, 11.0);
    set(scope, native_mod, "version", version.into());
    set(scope, natives, "nativeMod", native_mod.into());
}

pub(crate) fn set(scope: &mut v8::PinScope, o: v8::Local<v8::Object>, key: &str, v: v8::Local<v8::Value>) {
    let k = v8::String::new(scope, key).unwrap();
    o.set(scope, k.into(), v);
}

/// A room index for an exact room name (see the module doc).
fn room_exact(scope: &mut v8::PinScope, v: v8::Local<v8::Value>) -> Option<RoomIdx> {
    let name = v.to_string(scope)?.to_rust_string_lossy(scope);
    with_run(|ctx| {
        let w = ctx.world();
        w.room_by_name(&name).filter(|&r| w.room_name(r) == name)
    })
    .flatten()
}

/// `terrain(roomName)` → fresh `Uint8Array(2500)` | undefined (J1: each VM caches its own copy). Not
/// counted per copy: the VM was charged the whole terrain at creation (host.rs `create_vm`).
fn terrain(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(r) = room_exact(scope, args.get(0)) else { return };
    let Some(bytes) = with_run(|ctx| ctx.world().terrain(r).to_vec()) else { return };
    let store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
    let buf = v8::ArrayBuffer::with_backing_store(scope, &store);
    if let Some(a) = v8::Uint8Array::new(scope, buf, 0, 2500) {
        rv.set(a.into());
    }
}

fn world_size(scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    if let Some(n) = with_run(|ctx| ctx.world().world_size()) {
        rv.set(v8::Number::new(scope, n as f64).into());
    }
}

fn is_accessible(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let ok = room_exact(scope, args.get(0)).is_some_and(|r| with_run(|ctx| ctx.world().accessible(r)) == Some(true));
    rv.set_bool(ok);
}

/// `roomStatus(roomName)` → `{status, timestamp}` for closed/novice/respawn rooms, else undefined.
fn room_status(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(r) = room_exact(scope, args.get(0)) else { return };
    let Some((status, ts)) = with_run(|ctx| ctx.world().status(r)).flatten() else { return };
    let o = v8::Object::new(scope);
    let s = v8::String::new(scope, status).unwrap();
    set(scope, o, "status", s.into());
    let t: v8::Local<v8::Value> = match ts {
        Some(t) => v8::Number::new(scope, t as f64).into(),
        None => v8::null(scope).into(),
    };
    set(scope, o, "timestamp", t);
    rv.set(o.into());
}

/// `gridData(x, y)` → mapgrid.js `gridData["x,y"]` packed: the t/r/b/l edge counts (0–50) in bits 0, 6, 12
/// and 18, for accessible rooms only; undefined otherwise. runtime-driver.js `gridData` makes the
/// `{t?, r?, b?, l?}` object (only the non-zero counts), which costs less in JS than through the API here.
fn grid_data(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let coord = |v: Option<f64>| v.filter(|v| v.fract() == 0.0 && v.abs() < 2e9).map(|v| v as i32);
    let (Some(x), Some(y)) = (coord(args.get(0).number_value(scope)), coord(args.get(1).number_value(scope))) else {
        return;
    };
    let Some(exits) = with_run(|ctx| {
        let w = ctx.world();
        w.room_by_xy(x, y).filter(|&r| w.accessible(r)).map(|r| w.exit_counts(r))
    })
    .flatten() else {
        return;
    };
    rv.set_int32(exits.iter().rev().fold(0, |p, &n| p << 6 | n as i32));
}

/// The addon loads all terrain up front; ours reads the world directly.
fn load_terrain(_scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue<v8::Value>) {}

thread_local! {
    /// pf.cc keeps two path finders per thread and allocates more for deeper recursion.
    static PATH_FINDERS: RefCell<Vec<PathFinder>> = const { RefCell::new(Vec::new()) };
}

/// Map room coordinate of the addon (`parseRoomName`: W0 = 127, E0 = 128) to engine coordinates.
const MAP_OFFSET: i64 = 128;

/// pf.h `world_position_t(v8::Local<v8::Value>)`: `{xx, yy}` through ToObject/ToUint32.
fn world_pos(scope: &mut v8::PinScope, v: v8::Local<v8::Value>) -> Option<Pos> {
    let o = v.to_object(scope)?;
    let get = |k: &str| {
        let key = v8::String::new(scope, k).unwrap();
        o.get(scope, key.into())?.uint32_value(scope)
    };
    let (xx, yy) = (get("xx")? as i64, get("yy")? as i64);
    let room = RoomXY { x: (xx / 50 - MAP_OFFSET) as i32, y: (yy / 50 - MAP_OFFSET) as i32 };
    Some(Pos { room, x: (xx % 50) as u8, y: (yy % 50) as u8 })
}

/// `nativeMod.search(origin, goals, roomCallback, plainCost, swampCost, maxRooms, maxOps, maxCost, flee,
/// heuristicWeight)`: pf.cc `NAN_METHOD(search)` + `path_finder_t::search` on this thread's
/// `PathFinder`. Returns `{path, ops, cost, incomplete}` with `path` as `[xx, yy]` pairs from the end
/// back to the origin, exactly what driver/lib/path-finder.js reverses. The addon's `undefined` (origin
/// already at a goal) and `-1` (origin room blocked) shortcuts are returned as the objects the glue turns
/// them into, which leaves the glue's result unchanged.
fn search(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    // Conversion order as the addon: main.cc reads info[3..9], then pf.cc builds the goals and only
    // then the origin (observable through valueOf/getters and which bad argument throws first).
    // A throwing conversion leaves its exception pending (the addon's FromJust would abort).
    let u32_arg = |scope: &mut v8::PinScope, i: i32| args.get(i).uint32_value(scope);
    let Some(plain) = u32_arg(scope, 3) else { return };
    let Some(swamp) = u32_arg(scope, 4) else { return };
    let Some(rooms) = u32_arg(scope, 5) else { return };
    let Some(max_ops) = u32_arg(scope, 6) else { return };
    let Some(max_cost) = u32_arg(scope, 7) else { return };
    let flee = args.get(8).boolean_value(scope);
    let Some(heuristic_weight) = args.get(9).number_value(scope) else { return };
    let opts = SearchOpts {
        // `cost_t` is u32 in the addon; the glue has already clamped both costs to 1..=254.
        plain_cost: plain.min(255) as u8,
        swamp_cost: swamp.min(255) as u8,
        max_rooms: rooms as u8, // `uint8_t max_rooms = Nan::To<uint32_t>(info[5])`
        max_ops,
        max_cost,
        flee,
        heuristic_weight,
    };
    let mut goals = Vec::new();
    if let Ok(list) = v8::Local::<v8::Array>::try_from(args.get(1)) {
        for i in 0..list.length() {
            let Some(g) = list.get_index(scope, i).and_then(|g| g.to_object(scope)) else { return };
            let range_key = v8::String::new(scope, "range").unwrap();
            let Some(range) = g.get(scope, range_key.into()).and_then(|r| r.uint32_value(scope)) else { return };
            let pos_key = v8::String::new(scope, "pos").unwrap();
            let Some(pos) = g.get(scope, pos_key.into()).and_then(|p| world_pos(scope, p)) else { return };
            goals.push(Goal { pos, range });
        }
    }
    let callback = v8::Local::<v8::Function>::try_from(args.get(2)).ok();
    let Some(origin) = world_pos(scope, args.get(0)) else { return };
    let Some(world) = with_run(|ctx| ctx.world_static()) else { return };

    // Backing stores of returned matrices stay alive (even if their buffer is detached) until the
    // search ends; the crate reads them live, as the addon reads its TypedArrayContents pointers.
    let mut keep: Vec<v8::SharedRef<v8::BackingStore>> = Vec::new();
    let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
    let mut pf = PATH_FINDERS.with(|p| p.borrow_mut().pop()).unwrap_or_default();
    let terrain = |r: RoomXY| world.room_by_xy(r.x, r.y).map(|i| world.terrain(i));
    // CHITIN_PF_RECORD (pf_record.rs): what the callback returned for each room, in call order.
    let (recording, mut calls) = (crate::pf_record::enabled(), Vec::new());
    let result = pf.search(origin, &goals, &opts, terrain, |r| {
        let cost = room_cost(scope, callback, undefined, &mut keep, r);
        if recording {
            calls.push((r, crate::pf_record::code(&cost)));
        }
        cost
    });
    PATH_FINDERS.with(|p| p.borrow_mut().push(pf));
    if recording {
        crate::pf_record::record(origin, &goals, &opts, callback.is_some(), &calls, terrain, &result);
    }
    drop(keep);
    let result = match result {
        Ok(r) => r,
        Err(SearchError::Aborted) => return,
        Err(SearchError::NoTerrain(_)) => return throw_error(scope, "Could not load terrain data"),
        Err(SearchError::InvalidPosition) => return throw_error(scope, "Invalid room position"),
    };
    let path = v8::Array::new(scope, result.path.len() as i32);
    for (i, p) in result.path.iter().rev().enumerate() {
        let pair = v8::Array::new(scope, 2);
        let xx = v8::Number::new(scope, ((p.room.x as i64 + MAP_OFFSET) * 50 + p.x as i64) as f64);
        let yy = v8::Number::new(scope, ((p.room.y as i64 + MAP_OFFSET) * 50 + p.y as i64) as f64);
        pair.set_index(scope, 0, xx.into());
        pair.set_index(scope, 1, yy.into());
        path.set_index(scope, i as u32, pair.into());
    }
    let o = v8::Object::new(scope);
    set(scope, o, "path", path.into());
    let ops = v8::Number::new(scope, result.ops as f64);
    set(scope, o, "ops", ops.into());
    let cost = v8::Number::new(scope, result.cost as f64);
    set(scope, o, "cost", cost.into());
    let incomplete = v8::Boolean::new(scope, result.incomplete);
    set(scope, o, "incomplete", incomplete.into());
    rv.set(o.into());
}

/// The room callback's answer for one room (pf.cc:37-59).
fn room_cost<'a>(
    scope: &mut v8::PinScope,
    callback: Option<v8::Local<v8::Function>>,
    undefined: v8::Local<v8::Value>,
    keep: &mut Vec<v8::SharedRef<v8::BackingStore>>,
    r: RoomXY,
) -> RoomCost<'a> {
    let Some(cb) = callback else { return RoomCost::Terrain };
    let xx = v8::Number::new(scope, (r.x as i64 + MAP_OFFSET) as f64);
    let yy = v8::Number::new(scope, (r.y as i64 + MAP_OFFSET) as f64);
    // A throw (or termination) stays pending and propagates when the native returns (pf.cc ReThrow).
    let Some(ret) = cb.call(scope, undefined, &[xx.into(), yy.into()]) else { return RoomCost::Abort };
    if ret.is_boolean() && ret.is_false() {
        return RoomCost::Blocked;
    }
    let Ok(view) = v8::Local::<v8::ArrayBufferView>::try_from(ret) else { return RoomCost::Terrain };
    if view.byte_length() != 2500 {
        return RoomCost::Terrain;
    }
    if let Some(store) = view.get_backing_store() {
        keep.push(store);
    }
    // SAFETY: 2500 bytes of a view whose backing store `keep` holds until the search returns; Cell
    // because later callbacks may write to it, which the search must see.
    RoomCost::Matrix(unsafe { std::slice::from_raw_parts(view.data() as *const Cell<u8>, 2500) })
}

pub(crate) fn throw_error(scope: &mut v8::PinScope, msg: &str) {
    let m = v8::String::new(scope, msg).unwrap();
    let e = v8::Exception::error(scope, m);
    scope.throw_exception(e);
}

#[cfg(test)]
mod tests {
    //! The bridge replays crates/pathfinder's fixtures (recorded from the official addon by
    //! tools/pf-gen-fixture.js) through `nativeMod.search` from JS, with room callbacks that return
    //! `_bits`-style typed arrays, `false`, a wrong-length array or throw, exactly as the addon sees them.
    use super::*;
    use crate::cpu::{Clock, RunMeter};
    use crate::natives::{RunCtx, RunGuard};
    use std::collections::HashMap;

    struct Rooms {
        names: Vec<String>,
        xy: HashMap<(i32, i32), RoomIdx>,
        terrain: Vec<[u8; 2500]>,
    }

    impl WorldAccess for Rooms {
        fn room_by_name(&self, name: &str) -> Option<RoomIdx> {
            self.names.iter().position(|n| n == name).map(|i| i as RoomIdx)
        }
        fn room_by_xy(&self, x: i32, y: i32) -> Option<RoomIdx> {
            self.xy.get(&(x, y)).copied()
        }
        fn room_name(&self, r: RoomIdx) -> &str {
            &self.names[r as usize]
        }
        fn terrain(&self, r: RoomIdx) -> &[u8; 2500] {
            &self.terrain[r as usize]
        }
        fn exit_counts(&self, _r: RoomIdx) -> [u8; 4] {
            [0; 4]
        }
        fn accessible(&self, _r: RoomIdx) -> bool {
            true
        }
        fn status(&self, _r: RoomIdx) -> Option<(&'static str, Option<i64>)> {
            None
        }
        fn world_size(&self) -> u32 {
            12
        }
        fn terrain_heap_mb(&self) -> f64 {
            0.0
        }
        fn user_id(&self, _u: UserIdx) -> &str {
            "1"
        }
        fn object_room(&self, _id: &str, _user: UserIdx, _visible: &[RoomIdx]) -> Option<RoomIdx> {
            None
        }
        fn parse_intents(&self, _store: &util::json::Json) -> engine::intents::UserIntents {
            unreachable!()
        }
        fn market(&self) -> &std::sync::Arc<engine::world::MarketState> {
            unreachable!()
        }
    }

    /// Engine coordinates of a room name (W0 = -1, N0 = -1).
    fn xy(name: &str) -> (i32, i32) {
        let split = name[1..].find(['N', 'S']).unwrap() + 1;
        let (x, y): (i32, i32) = (name[1..split].parse().unwrap(), name[split + 1..].parse().unwrap());
        (if name.starts_with('W') { -x - 1 } else { x }, if name[split..].starts_with('N') { -y - 1 } else { y })
    }

    const HARNESS: &str = r#"
        function hex(s) { var a = new Uint8Array(s.length / 2); for (var i = 0; i < a.length; i++) a[i] = parseInt(s.substr(i * 2, 2), 16); return a; }
        function roomName(xx, yy) { return (xx <= 127 ? 'W' + (127 - xx) : 'E' + (xx - 128)) + (yy <= 127 ? 'N' + (127 - yy) : 'S' + (yy - 128)); }
        function runCase(s) {
            var c = JSON.parse(s);
            var p = P[c.profile], calls = [], scratch = new Uint8Array(2500), wrong = new Uint8Array(100);
            var cb = p.cb ? function (xx, yy) {
                var n = roomName(xx, yy), code = p.rooms[n];
                calls.push(n);
                if (code === undefined) return undefined;
                if (code === 'F') return false;
                if (code === 'T') throw new Error('callback threw');
                if (code === 'X') return wrong;
                if (p.shared) { scratch.set(M[code]); return scratch; }
                return M[code];
            } : undefined;
            var r, n = c.n;
            try { r = N.nativeMod.search(c.o, c.g, cb, n[0], n[1], n[2], n[3], n[4], n[5], Number(n[6])); }
            catch (e) { return JSON.stringify({err: e.message, calls: calls}); }
            return JSON.stringify({ops: r.ops, cost: r.cost, incomplete: r.incomplete, path: r.path, calls: calls});
        }"#;

    /// The player allocator (isolated-vm's LimitedAllocator) counts this VM's array buffers, JS-made and
    /// host-made, but not the host's external strings; refuses buffers past the memory limit with V8's
    /// RangeError; and every count comes back when the isolate is disposed.
    #[test]
    fn array_buffers_are_counted_and_limited_per_vm() {
        use crate::natives::{array_buffer_allocator, uint8_array, VmFlags};
        use std::sync::atomic::Ordering::Relaxed;
        use std::sync::Arc;
        crate::init_v8();
        let rooms = Rooms { names: Vec::new(), xy: HashMap::new(), terrain: Vec::new() };
        let flags = VmFlags::new(8 << 20);
        let params = v8::CreateParams::default()
            .set_max_young_generation_size_in_bytes(1 << 20)
            .set_max_old_generation_size_in_bytes(8 << 20)
            .array_buffer_allocator(array_buffer_allocator(&flags));
        let mut iso = v8::Isolate::new(params);
        let limit = iso.get_heap_statistics().heap_size_limit();
        flags.created(&mut iso, limit);
        let mut ctx = RunCtx::new(&rooms, 0, Vec::new(), 0, 1000, RunMeter::start(crate::Meter::Thread, 0.0), Clock::ThreadCpu);
        ctx.flags = Arc::as_ptr(&flags);
        let guard = RunGuard::enter(ctx);
        let blob: Arc<[u8]> = Arc::from(vec![b'x'; 1 << 20]);
        {
            v8::scope!(let scope, &mut iso);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            let global = context.global(scope);
            let s = crate::marshal::external_latin1(scope, &blob);
            set(scope, global, "blob", s.into());
            let t = uint8_array(scope, vec![1; 2500]).unwrap();
            set(scope, global, "terrain", t.into());
            let code = v8::String::new(scope, "var a = new Uint8Array(1 << 20); try { new ArrayBuffer(64 << 20); 'ok' } catch (e) { e.name + ': ' + e.message }").unwrap();
            let out = v8::Script::compile(scope, code, None).unwrap().run(scope).unwrap().to_rust_string_lossy(scope);
            assert_eq!(out, "RangeError: Array buffer allocation failed");
        }
        assert_eq!(flags.extra.load(Relaxed), (1 << 20) + 2500);
        drop(guard);
        drop(iso);
        assert_eq!((flags.extra.load(Relaxed), Arc::strong_count(&flags), Arc::strong_count(&blob)), (0, 1, 1));
    }

    #[test]
    fn search_bridge_matches_official_native_pathfinder() {
        crate::init_v8();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../pathfinder/tests/fixtures");
        let mut rooms = Rooms { names: Vec::new(), xy: HashMap::new(), terrain: Vec::new() };
        let mut setup = String::from("var M = [], P = [];\n");
        let mut has_callback = Vec::new();
        for line in std::fs::read_to_string(dir.join("world.txt")).unwrap().lines().filter(|l| !l.starts_with('#')) {
            let t: Vec<&str> = line.split(' ').filter(|s| !s.is_empty()).collect();
            match t[0] {
                "room" => {
                    rooms.xy.insert(xy(t[1]), rooms.names.len() as RoomIdx);
                    rooms.names.push(t[1].into());
                    rooms.terrain.push(std::array::from_fn(|i| t[2].as_bytes()[i] - b'0'));
                }
                "matrix" => setup.push_str(&format!("M[{}] = hex('{}');\n", t[1], t[2])),
                _ => {
                    let entries: Vec<String> = t[3..]
                        .iter()
                        .map(|e| {
                            let (r, c) = e.split_once('=').unwrap();
                            let c = c.strip_prefix('M').map_or(format!("'{c}'"), str::to_string);
                            format!("{r}: {c}")
                        })
                        .collect();
                    has_callback.push(t[2] != "none");
                    setup.push_str(&format!("P[{}] = {{cb: {}, shared: {}, rooms: {{{}}}}};\n", t[1], t[2] != "none", t[2] == "shared", entries.join(", ")));
                }
            }
        }

        let mut iso = v8::Isolate::new(Default::default());
        v8::scope!(let scope, &mut iso);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let natives = crate::natives::player_natives(scope, "shard0", "private", false);
        let global = context.global(scope);
        set(scope, global, "N", natives.into());
        let code = v8::String::new(scope, &format!("{setup}{HARNESS}")).unwrap();
        v8::Script::compile(scope, code, None).unwrap().run(scope).unwrap();
        let run_case: v8::Local<v8::Function> = {
            let k = v8::String::new(scope, "runCase").unwrap();
            context.global(scope).get(scope, k.into()).unwrap().try_into().unwrap()
        };
        let _guard = RunGuard::enter(RunCtx::new(&rooms, 0, Vec::new(), 0, 1000, RunMeter::start(crate::Meter::Thread, 0.0), Clock::ThreadCpu));

        let world = |r: &str, x: &str, y: &str| {
            let (rx, ry) = xy(r);
            let w = |c: i32, l: &str| (c as i64 + 128) * 50 + l.parse::<i64>().unwrap();
            (w(rx, x), w(ry, y))
        };
        let (mut cases, mut failures) = (0, Vec::new());
        for (line, text) in std::fs::read_to_string(dir.join("cases-00.txt")).unwrap().lines().enumerate() {
            let t: Vec<&str> = text.split(' ').collect();
            let origin = world(t[1], t[2], t[3]);
            let n_goals: usize = t[5].parse().unwrap();
            let goals: Vec<String> = (0..n_goals)
                .map(|i| {
                    let g = &t[6 + i * 4..10 + i * 4];
                    let (xx, yy) = world(g[1], g[2], g[3]);
                    format!(r#"{{"range":{},"pos":{{"xx":{xx},"yy":{yy}}}}}"#, g[0])
                })
                .collect();
            let n = 6 + n_goals * 4 + 8 + 1; // past "J" and its seven raw options, at the first "N" value
            let native = &t[n..n + 7];
            let profile = t[n + 8];
            let n_calls: usize = t[n + 10].parse().unwrap();
            let calls = &t[n + 11..n + 11 + n_calls];
            let expect = &t[n + 12 + n_calls..];
            let case = format!(
                r#"{{"o":{{"xx":{},"yy":{}}},"g":[{}],"n":[{},{},{},{},{},{},"{}"],"profile":{profile}}}"#,
                origin.0,
                origin.1,
                goals.join(","),
                native[0],
                native[1],
                native[2],
                native[3],
                native[4],
                native[5] == "1",
                native[6]
            );
            let arg = v8::String::new(scope, &case).unwrap();
            let recv = v8::undefined(scope).into();
            let out = run_case.call(scope, recv, &[arg.into()]).unwrap().to_rust_string_lossy(scope);
            let got = util::json::Json::parse(&out).unwrap();
            let got_calls: Vec<&str> = got.get("calls").unwrap().as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
            let summary = match got.get("err").and_then(util::json::Json::as_str) {
                Some("Could not load terrain data") => "err terrain".to_string(),
                Some("callback threw") => "err throw".to_string(),
                Some(e) => format!("err {e}"),
                None => {
                    // The addon's path runs from the end back to the origin.
                    let mut prev = origin;
                    let mut dirs = String::new();
                    for p in got.get("path").unwrap().as_array().unwrap().iter().rev() {
                        let p = (p.at(0).unwrap().as_f64().unwrap() as i64, p.at(1).unwrap().as_f64().unwrap() as i64);
                        dirs.push(match (p.0 - prev.0, p.1 - prev.1) {
                            (0, -1) => '1',
                            (1, -1) => '2',
                            (1, 0) => '3',
                            (1, 1) => '4',
                            (0, 1) => '5',
                            (-1, 1) => '6',
                            (-1, 0) => '7',
                            (-1, -1) => '8',
                            _ => '?',
                        });
                        prev = p;
                    }
                    let num = |k: &str| got.get(k).unwrap().as_f64().unwrap() as u64;
                    let inc = got.get("incomplete").unwrap().as_bool().unwrap() as u8;
                    format!("ok {} {} {inc} {}", num("ops"), num("cost"), if dirs.is_empty() { "-".into() } else { dirs })
                }
            };
            let has_cb = has_callback[profile.parse::<usize>().unwrap()];
            if summary != expect.join(" ") || (has_cb && got_calls != calls) {
                failures.push(format!("line {}: got {summary} calls {got_calls:?}\n  want {} calls {calls:?}", line + 1, expect.join(" ")));
            }
            cases += 1;
        }
        for f in failures.iter().take(5) {
            eprintln!("{f}");
        }
        assert!(cases > 8000, "{cases} cases");
        assert!(failures.is_empty(), "{} of {cases} cases differ", failures.len());
    }
}
