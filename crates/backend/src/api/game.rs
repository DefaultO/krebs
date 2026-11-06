//! mirror: reference/backend/lib/game/api/game.js
//! parity: DIVERGENT(GAME-MAP-STATS): users are projected to `{_id, username, badge}` (map-stats, room users);
//! writes run as Commands at the next tick boundary (spec 10 §7); `world-size` is not cached forever;
//! `add-object-intent`/`add-global-intent` wait for the Rust `system.js` sanitizer (WP02) and answer
//! 'not implemented'; `change-flag` does not exist in vanilla (Express 404); set-notify-when-attacked without
//! `_id` fails (the original's `update({_id: undefined})` becomes `update({})` and flags every object); flags of a
//! user across rooms are ordered by room, not by flag-doc creation (unobservable: names are unique per user).
//! Everything else is replayed against the official router in tests/fixtures/api (server.rs oracle).
#![allow(unused_variables, clippy::manual_range_contains)] // JS range checks: NaN must pass them


use crate::http::{obj, Request, Response};
use crate::{activate, field, install_room, new_doc, obj_set, room_idx, room_idx_meta, s, str_field, truthy, user_brief, write, ApiCtx};
use engine::constants::{self as C, StructureType};
use engine::doc::{Doc, DocRef, Obj, Val};
use engine::terrain::RoomTerrain;
use engine::world::{Presence, RoomIdx, RoomSnapshot, RoomsMeta, UserIdx, UserState, World, WorldView};
use std::collections::BTreeMap;
use std::sync::Arc;
use util::json::Json;

macro_rules! auth {
    ($ctx:expr) => {
        match $ctx.user {
            Some(u) => u,
            None => return Response::unauthorized(),
        }
    };
}

fn reply<T>(r: Result<T, String>, f: impl FnOnce(T) -> Json) -> Response {
    match r {
        Ok(v) => Response::ok(f(v)),
        Err(e) => Response::error(&e),
    }
}

/// The world as the checks read it: the published view in handlers, the live world inside Commands.
struct Wv<'a> {
    meta: &'a RoomsMeta,
    rooms: &'a [Arc<RoomSnapshot>],
    presence: &'a [Arc<BTreeMap<RoomIdx, Presence>>],
    users: &'a [Arc<UserState>],
    time: u64,
    vnow: i64,
}

impl<'a> Wv<'a> {
    fn of_view(v: &'a WorldView) -> Wv<'a> {
        Wv { meta: &v.meta, rooms: &v.rooms, presence: &v.presence, users: &v.users, time: v.time, vnow: v.vnow }
    }
    fn of_world(w: &'a World) -> Wv<'a> {
        Wv { meta: &w.meta, rooms: &w.rooms, presence: &w.index.presence, users: &w.users, time: w.time, vnow: crate::vnow(w) }
    }
    fn uid(&self, u: UserIdx) -> &'a str {
        self.users[u as usize].id.as_str()
    }
    /// `rooms.objects.find({user})` in collection (seq) order.
    fn user_objects(&self, u: UserIdx) -> Vec<&'a DocRef> {
        let id = self.uid(u);
        let mut out: Vec<&DocRef> = Vec::new();
        for &r in self.presence.get(u as usize).map(|p| p.keys().copied().collect::<Vec<_>>()).unwrap_or_default().iter() {
            out.extend(self.rooms[r as usize].objects.iter().filter(|d| str_field(&d.obj, "user") == Some(id)));
        }
        out.sort_by_key(|d| d.seq);
        out
    }
    /// The user's flags docs (`rooms.flags.find({user})`) as (room, doc), RoomIdx order.
    fn user_flags(&self, u: UserIdx) -> Vec<(RoomIdx, &'a Arc<Obj>)> {
        let id = self.uid(u);
        self.rooms.iter().enumerate().flat_map(|(r, s)| s.flags.iter().filter(move |f| str_field(f, "user") == Some(id)).map(move |f| (r as RoomIdx, f))).collect()
    }
}

fn num(o: &Obj, k: &str) -> Option<f64> {
    field(o, k).and_then(Val::as_f64)
}

fn at(o: &Obj, x: f64, y: f64) -> bool {
    num(o, "x") == Some(x) && num(o, "y") == Some(y)
}

/// `parseInt(v)` (NaN when it does not start with digits).
fn parse_int(v: Option<&Json>) -> f64 {
    let s = crate::socket::user::js_to_string(v);
    let t = s.trim_start_matches(util::num::is_js_whitespace);
    let (neg, rest) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    match digits.parse::<f64>() {
        Ok(x) if neg => -x,
        Ok(x) => x,
        Err(_) => f64::NAN,
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `request.body.name && (!_.isString(name) || name.length > max)`.
fn bad_name(name: Option<&Json>, max: usize) -> bool {
    match name {
        Some(Json::Str(n)) => utf16_len(n) > max,
        Some(v) => truthy(Some(&Val::from_json(v))),
        None => false,
    }
}

/// `checkGame(request)` (GAME-CHECK-GAME).
fn check_game(w: &Wv, room: Option<&Json>) -> Result<RoomIdx, String> {
    let name = room.and_then(Json::as_str).unwrap_or("");
    let r = room_idx_meta(w.meta, name).ok_or("invalid room")?;
    if !name.starts_with(['W', 'E']) {
        return Err("not supported".into());
    }
    let info = &w.rooms[r as usize].info;
    let open_time = field(info, "openTime");
    if str_field(info, "status") == Some("out of borders") || (truthy(open_time) && open_time.and_then(Val::as_f64).is_some_and(|t| t > w.vnow as f64)) {
        return Err("out of borders".into());
    }
    Ok(r)
}

/// The terrain code at `y*50+x` as `parseInt(terrain.charAt(...))` sees it (NaN index → charAt(0)).
fn terrain_code(t: &RoomTerrain, x: f64, y: f64) -> u8 {
    let i = y * 50.0 + x;
    let i = if i.is_nan() { 0.0 } else { i };
    if !(0.0..2500.0).contains(&i) || i.fract() != 0.0 {
        return 0;
    }
    t.raw((i as usize % 50) as u8, (i as usize / 50) as u8)
}

const SPOT_BLOCKERS: [&str; 9] = ["wall", "constructedWall", "spawn", "extension", "link", "storage", "tower", "observer", "powerSpawn"];

/// `checkConstructionSpot(room, structureType, x, y)` (GAME-CHECK-SPOT).
fn check_spot(objs: &[DocRef], t: &RoomTerrain, ty: &str, x: f64, y: f64) -> Result<(), String> {
    let bad = || Err("invalid location".to_string());
    if x <= 0.0 || y <= 0.0 || x >= 49.0 || y >= 49.0 {
        return bad();
    }
    let here = |d: &&DocRef| at(&d.obj, x, y);
    if ty == "extractor" {
        return if objs.iter().filter(here).any(|d| d.kind() == "mineral") { Ok(()) } else { bad() };
    }
    if objs.iter().filter(here).any(|d| d.kind() == ty || d.kind() == "constructionSite") {
        return bad();
    }
    if ty != "rampart" && objs.iter().filter(here).any(|d| SPOT_BLOCKERS.contains(&d.kind())) {
        return bad();
    }
    if ty != "road" && terrain_code(t, x, y) & C::TERRAIN_MASK_WALL != 0 {
        return bad();
    }
    let near = |v: Option<f64>, c: f64| v.is_some_and(|v| v > c - 2.0 && v < c + 2.0);
    if objs.iter().any(|d| d.kind() == "exit" && near(num(&d.obj, "x"), x) && near(num(&d.obj, "y"), y)) {
        return bad();
    }
    if ty != "road" && ty != "container" && (x == 1.0 || x == 48.0 || y == 1.0 || y == 48.0) {
        let mut border = vec![];
        if x == 1.0 {
            border = vec![(0.0, y - 1.0), (0.0, y), (0.0, y + 1.0)];
        }
        if x == 48.0 {
            border = vec![(49.0, y - 1.0), (49.0, y), (49.0, y + 1.0)];
        }
        if y == 1.0 {
            border = vec![(x - 1.0, 0.0), (x, 0.0), (x + 1.0, 0.0)];
        }
        if y == 48.0 {
            border = vec![(x - 1.0, 49.0), (x, 49.0), (x + 1.0, 49.0)];
        }
        if border.iter().any(|&(bx, by)| terrain_code(t, bx, by) & C::TERRAIN_MASK_WALL == 0) {
            return bad();
        }
    }
    Ok(())
}

fn controller(objs: &[DocRef]) -> Option<&DocRef> {
    objs.iter().find(|d| d.kind() == "controller")
}

/// `checkController(room, 'spawn', …)`.
fn check_controller_spawn(objs: &[DocRef], uid: &str) -> Result<(), String> {
    let c = controller(objs).ok_or("invalid room")?;
    let busy = truthy(field(&c.obj, "user")) || truthy(field(&c.obj, "reservation")) || (truthy(field(&c.obj, "bindUser")) && str_field(&c.obj, "bindUser") != Some(uid));
    if busy { Err("room busy".into()) } else { Ok(()) }
}

/// `checkController(room, 'construct', type, user)` + `checkControllerAvailability`.
fn check_controller_construct(objs: &[DocRef], uid: &str, ty: &str) -> Result<(), String> {
    let c = controller(objs);
    if let Some(c) = c {
        let o = &c.obj;
        let reserved_by_other = field(o, "reservation").filter(|r| truthy(Some(r))).is_some_and(|r| r.as_obj().and_then(|r| str_field(r, "user")) != Some(uid));
        if (truthy(field(o, "user")) && str_field(o, "user") != Some(uid)) || reserved_by_other {
            return Err("not a controller owner".into());
        }
    }
    let rcl = c.filter(|c| truthy(field(&c.obj, "level")) && truthy(field(&c.obj, "user"))).and_then(|c| num(&c.obj, "level")).unwrap_or(0.0);
    let count = objs.iter().filter(|d| d.kind() == ty || (d.kind() == "constructionSite" && str_field(&d.obj, "structureType") == Some(ty))).count() as u32;
    let available = ty.parse::<StructureType>().ok().filter(|_| rcl.fract() == 0.0 && (0.0..=8.0).contains(&rcl)).and_then(|t| C::controller_structures(t, rcl as usize));
    match available {
        Some(n) if count < n => Ok(()),
        _ => Err("RCL not enough".into()),
    }
}

/// The room's terrain for checks made inside a Command (read on the connection thread: WP01's TerrainStore
/// accessors may still be stubs and a Command must not panic on the tick thread).
fn room_terrain_of(view: &WorldView, room: Option<&Json>) -> RoomTerrain {
    match room.and_then(Json::as_str).and_then(|n| room_idx(view, n)) {
        Some(r) => crate::terrain_of(view, r),
        None => RoomTerrain::default(),
    }
}

fn set(o: &mut Obj, k: &str, v: Val) {
    obj_set(o, k, v);
}

fn json_obj(o: &Obj) -> Json {
    Json::Obj(o.iter().map(|(k, v)| (k.as_str().to_string(), v.to_json())).collect())
}

/// Replaces the docs of room `r` that `edit` returns `Some` for (same seq) and drops those it maps to `None`.
fn edit_docs(objs: &[DocRef], mut edit: impl FnMut(&DocRef) -> Option<Option<Obj>>) -> Vec<DocRef> {
    let mut out = Vec::with_capacity(objs.len());
    for d in objs {
        match edit(d) {
            None => out.push(d.clone()),
            Some(None) => {}
            Some(Some(o)) => out.push(Doc::new(d.seq, o)),
        }
    }
    out
}

// ---- routes ----

/// POST /api/game/map-stats
pub fn map_stats(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    let b = req.body();
    let Some(Json::Arr(rooms)) = b.get("rooms") else { return Response::error("invalid params") };
    let stat_name = match b.get("statName") {
        Some(Json::Str(s)) => s.clone(),
        _ => return Response::error("TypeError: request.body.statName.match is not a function"),
    };
    // /^(.*?)(\d+)$/
    if !stat_name.ends_with(|c: char| c.is_ascii_digit()) {
        return Response::error("invalid params");
    }
    let view = ctx.published.get();
    let game_time = view.time as f64;
    let mut idx: Vec<RoomIdx> = rooms.iter().filter_map(|r| r.as_str()).filter_map(|n| room_idx(&view, n)).collect();
    idx.sort_unstable();
    idx.dedup();
    let mut stats: Vec<(String, Vec<(String, Json)>)> = Vec::new();
    for &r in &idx {
        let info = &view.rooms[r as usize].info;
        let mut m = vec![];
        for k in ["status", "novice", "respawnArea", "openTime"] {
            if let Some(v) = field(info, k) {
                m.push((k.to_string(), v.to_json()));
            }
        }
        stats.push((view.meta.rooms[r as usize].name.to_string(), m));
    }
    let mut users: Vec<String> = Vec::new();
    let put = |stats: &mut Vec<(String, Vec<(String, Json)>)>, room: &str, k: &str, v: Json| {
        if let Some((_, m)) = stats.iter_mut().find(|(n, _)| n == room) {
            match m.iter_mut().find(|(x, _)| x == k) {
                Some(slot) => slot.1 = v,
                None => m.push((k.to_string(), v)),
            }
        }
    };
    let mut docs: Vec<&DocRef> = idx.iter().flat_map(|&r| view.rooms[r as usize].objects.iter()).collect();
    docs.sort_by_key(|d| d.seq);
    for d in docs.iter().filter(|d| matches!(d.kind(), "controller" | "invaderCore")) {
        let o = &d.obj;
        let room = str_field(o, "room").unwrap_or("");
        let pick = || Json::Obj(["user", "level"].iter().filter_map(|k| field(o, k).map(|v| (k.to_string(), v.to_json()))).collect());
        if d.kind() == "controller" {
            if truthy(field(o, "user")) {
                put(&mut stats, room, "own", pick());
                users.push(crate::socket::user::js_to_string(field(o, "user").map(Val::to_json).as_ref()));
            }
            if let Some(res) = field(o, "reservation").filter(|v| truthy(Some(v))) {
                let ru = res.as_obj().and_then(|r| field(r, "user")).map(Val::to_json);
                // `{user: i.reservation.user, level: 0}`: an undefined user is dropped by JSON
                let own = ru.clone().map(|u| ("user".to_string(), u)).into_iter().chain([("level".to_string(), Json::Num(0.0))]).collect();
                put(&mut stats, room, "own", Json::Obj(own));
                users.push(crate::socket::user::js_to_string(ru.as_ref()));
            }
            if let Some(sign) = field(o, "sign").filter(|v| truthy(Some(v))) {
                put(&mut stats, room, "sign", sign.to_json());
                users.push(crate::socket::user::js_to_string(sign.as_obj().and_then(|x| field(x, "user")).map(Val::to_json).as_ref()));
            }
            if num(o, "safeMode").is_some_and(|t| t > game_time) {
                put(&mut stats, room, "safeMode", Json::Bool(true));
            }
        } else if num(o, "level").is_some_and(|l| l > 0.0) {
            put(&mut stats, room, "own", pick());
            users.push(crate::socket::user::js_to_string(field(o, "user").map(Val::to_json).as_ref()));
        }
    }
    for d in docs.iter().filter(|d| d.kind() == "mineral") {
        let o = &d.obj;
        let mut m = vec![];
        for (k, f) in [("type", "mineralType"), ("density", "density")] {
            if let Some(v) = field(o, f) {
                m.push((k.to_string(), v.to_json()));
            }
        }
        put(&mut stats, str_field(o, "room").unwrap_or(""), "minerals0", Json::Obj(m));
    }
    let mut found: Vec<UserIdx> = users.iter().filter_map(|id| crate::user_by_id(&view, id)).collect();
    found.sort_unstable();
    found.dedup();
    let mut users_out: Vec<(String, String)> = Vec::new();
    for u in found {
        crate::socket::rooms::js_push(&mut users_out, view.users[u as usize].id.as_str().to_string(), user_brief(&view, u).stringify());
    }
    let users_json = Json::parse(&crate::socket::rooms::write_members(&users_out)).unwrap_or(Json::Obj(vec![]));
    Response::ok(obj(vec![
        ("gameTime", Json::Num(game_time)),
        ("stats", Json::Obj(stats.into_iter().map(|(k, m)| (k, Json::Obj(m))).collect())),
        ("statsMax", Json::Obj(vec![])),
        ("users", users_json),
    ]))
}

/// GET /api/game/time
pub fn time(req: &Request, ctx: &ApiCtx) -> Response {
    Response::ok(obj(vec![("time", Json::Num(ctx.published.get().time as f64))]))
}

/// A `rooms.terrain` document as the API returns it.
fn terrain_doc(view: &WorldView, r: RoomIdx) -> Json {
    let name = view.meta.rooms[r as usize].name.to_string();
    obj(vec![("room", Json::Str(name)), ("terrain", Json::Str(crate::terrain_of(view, r).to_db_string())), ("type", Json::Str("terrain".into()))])
}

/// GET /api/game/room-terrain
pub fn room_terrain(req: &Request, ctx: &ApiCtx) -> Response {
    let view = ctx.published.get();
    let encoded = req.param("encoded").is_some_and(|e| !e.is_empty());
    let Some(name) = req.param("room") else {
        // `find({room: undefined})` reaches storage as `find({})`: every terrain doc, never decoded (unless one room)
        let all: Vec<RoomIdx> = (0..view.meta.rooms.len() as RoomIdx).filter(|&r| !view.meta.rooms[r as usize].removed).collect();
        if all.is_empty() {
            return Response::error("invalid room");
        }
        if encoded || all.len() != 1 {
            return Response::ok(obj(vec![("terrain", Json::Arr(all.into_iter().map(|r| terrain_doc(&view, r)).collect()))]));
        }
        return room_terrain_decoded(&view, all[0]);
    };
    let Some(r) = room_idx(&view, name) else { return Response::error("invalid room") };
    if encoded {
        return Response::ok(obj(vec![("terrain", Json::Arr(vec![terrain_doc(&view, r)]))]));
    }
    room_terrain_decoded(&view, r)
}

/// `{terrain: common.decodeTerrain(doc.terrain, room)}`
fn room_terrain_decoded(view: &WorldView, r: RoomIdx) -> Response {
    let name = &*view.meta.rooms[r as usize].name;
    let t = crate::terrain_of(view, r);
    let mut list = vec![];
    for y in 0..50u8 {
        for x in 0..50u8 {
            let code = t.raw(x, y);
            for (bit, ty) in [(1, "wall"), (2, "swamp")] {
                if code & bit != 0 {
                    list.push(obj(vec![("room", Json::Str(name.into())), ("x", Json::Num(x as f64)), ("y", Json::Num(y as f64)), ("type", Json::Str(ty.into()))]));
                }
            }
        }
    }
    Response::ok(obj(vec![("terrain", Json::Arr(list))]))
}

/// GET /api/game/room-status
pub fn room_status(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    let view = ctx.published.get();
    let mut m = vec![];
    // `findOne({_id: undefined})` reaches storage as `findOne({})`: the first room
    let r = match req.param("room") {
        Some(n) => room_idx(&view, n),
        None => (0..view.meta.rooms.len() as RoomIdx).find(|&r| !view.meta.rooms[r as usize].removed),
    };
    if let Some(r) = r {
        let info = &view.rooms[r as usize].info;
        for k in ["status", "novice", "respawnArea", "openTime"] {
            if let Some(v) = field(info, k) {
                m.push((k.to_string(), v.to_json()));
            }
        }
    }
    Response::ok(obj(vec![("room", Json::Obj(m))]))
}

/// POST /api/game/gen-unique-object-name
pub fn gen_unique_object_name(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    if req.body().get("type").and_then(Json::as_str) != Some("spawn") {
        return Response::error("invalid params");
    }
    let view = ctx.published.get();
    let w = Wv::of_view(&view);
    let objs = w.user_objects(u);
    let taken = |n: &str| {
        objs.iter().any(|d| {
            let o = &d.obj;
            str_field(o, "name") == Some(n) && (d.kind() == "spawn" || (d.kind() == "constructionSite" && str_field(o, "structureType") == Some("spawn")))
        })
    };
    let mut c = 1;
    while taken(&format!("Spawn{c}")) {
        c += 1;
    }
    Response::ok(obj(vec![("name", Json::Str(format!("Spawn{c}")))]))
}

/// POST /api/game/check-unique-object-name
pub fn check_unique_object_name(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let (Some("spawn"), Some(Json::Str(name))) = (b.get("type").and_then(Json::as_str), b.get("name")) else {
        return Response::error("invalid params");
    };
    let view = ctx.published.get();
    let exists = Wv::of_view(&view).user_objects(u).iter().any(|d| matches!(d.kind(), "spawn" | "constructionSite") && str_field(&d.obj, "name") == Some(name.as_str()));
    if exists { Response::error("name exists") } else { Response::ok(Json::Null) }
}

/// POST /api/game/place-spawn
pub fn place_spawn(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let (x, y) = (parse_int(b.get("x")), parse_int(b.get("y")));
    if x < 0.0 || x > 49.0 || y < 0.0 || y > 49.0 || bad_name(b.get("name"), 50) {
        return Response::error("invalid params");
    }
    let terrain = room_terrain_of(&ctx.published.get(), b.get("room"));
    let now_real = engine::policy::real_now_ms("CLK-API") as f64;
    let cmd = place_spawn_cmd(u, b.get("room").cloned(), b.get("name").cloned(), x, y, terrain, now_real);
    reply(write(ctx, cmd), |_| obj(vec![("newbie", Json::Bool(true))]))
}

/// The checks and effects of place-spawn, run at P0 (GAME-PLACE-SPAWN steps 2–9 and a–g).
fn place_spawn_cmd(u: UserIdx, room_name: Option<Json>, name: Option<Json>, x: f64, y: f64, terrain: RoomTerrain, now_real: f64) -> impl FnOnce(&mut World) -> Result<(), String> + Send + 'static {
    move |w| {
            let doc = w.users[u as usize].doc.clone();
            if truthy(field(&doc, "blocked")) {
                return Err("blocked".into());
            }
            if !truthy(field(&doc, "cpu")) {
                return Err("no cpu".into());
            }
            if truthy(field(&doc, "lastRespawnDate")) && num(&doc, "lastRespawnDate").is_some_and(|t| now_real - t < 180000.0) {
                return Err("too soon after last respawn".into());
            }
            let (r, uid, game_time) = {
                let v = Wv::of_world(w);
                let r = check_game(&v, room_name.as_ref())?;
                if !v.user_objects(u).is_empty() {
                    return Err("already playing".into());
                }
                let objs = &v.rooms[r as usize].objects;
                let uid = v.uid(u).to_string();
                check_controller_spawn(objs, &uid)?;
                check_spot(objs, &terrain, "spawn", x, y)?;
                (r, uid, v.time as f64)
            };
            let room = room_name.as_ref().and_then(Json::as_str).unwrap_or("").to_string();
            let objs = w.rooms[r as usize].objects.clone();
            let owned = |d: &DocRef| !matches!(field(&d.obj, "user"), None | Some(Val::Null));
            let structure_types: Vec<&str> = C::CONSTRUCTION_COST.iter().map(|(t, _)| t.as_str()).collect();
            // removeWhere creeps/sites with a user; ruins for owned structures, then remove them
            let mut ruins = Vec::new();
            for d in objs.iter().filter(|d| owned(d) && structure_types.contains(&d.kind())) {
                let o = &d.obj;
                let mut ruin = Obj::new();
                set(&mut ruin, "type", s("ruin"));
                for k in ["user", "room", "x", "y"] {
                    if let Some(v) = field(o, k) {
                        set(&mut ruin, k, v.clone());
                    }
                }
                let mut st = Obj::new();
                set(&mut st, "id", s(d.id().as_str()));
                set(&mut st, "type", s(d.kind()));
                set(&mut st, "hits", Val::Num(0.0));
                if let Some(h) = field(o, "hitsMax") {
                    set(&mut st, "hitsMax", h.clone());
                }
                set(&mut st, "user", s(&crate::socket::user::js_to_string(field(o, "user").map(Val::to_json).as_ref())));
                set(&mut ruin, "structure", Val::Obj(st));
                set(&mut ruin, "store", field(o, "store").filter(|v| truthy(Some(v))).cloned().unwrap_or(Val::Obj(Obj::new())));
                set(&mut ruin, "destroyTime", Val::Num(game_time));
                set(&mut ruin, "decayTime", Val::Num(game_time + 100000.0));
                ruins.push(ruin);
            }
            let mut kept = edit_docs(&objs, |d| {
                let kind = d.kind();
                if owned(d) && (matches!(kind, "creep" | "constructionSite") || structure_types.contains(&kind)) {
                    return Some(None);
                }
                if kind == "controller" && num(&d.obj, "level") == Some(0.0) {
                    let mut o = d.obj.clone();
                    set(&mut o, "user", s(&uid));
                    set(&mut o, "level", Val::Num(1.0));
                    set(&mut o, "progress", Val::Num(0.0));
                    set(&mut o, "downgradeTime", Val::Null);
                    set(&mut o, "safeMode", Val::Num(game_time + 20000.0));
                    return Some(Some(o));
                }
                if kind == "source" {
                    let mut o = d.obj.clone();
                    set(&mut o, "invaderHarvested", Val::Num(0.0));
                    return Some(Some(o));
                }
                None
            });
            for ruin in ruins {
                kept.push(new_doc(w, ruin));
            }
            let mut spawn = Obj::new();
            set(&mut spawn, "type", s("spawn"));
            set(&mut spawn, "room", s(&room));
            set(&mut spawn, "x", Val::Num(x));
            set(&mut spawn, "y", Val::Num(y));
            if let Some(n) = &name {
                set(&mut spawn, "name", Val::from_json(n));
            }
            set(&mut spawn, "user", s(&uid));
            let energy = |n: u32| {
                let mut e = Obj::new();
                set(&mut e, "energy", Val::Num(n as f64));
                Val::Obj(e)
            };
            set(&mut spawn, "store", energy(C::SPAWN_ENERGY_START));
            set(&mut spawn, "storeCapacityResource", energy(C::SPAWN_ENERGY_CAPACITY));
            set(&mut spawn, "hits", Val::Num(C::SPAWN_HITS as f64));
            set(&mut spawn, "hitsMax", Val::Num(C::SPAWN_HITS as f64));
            set(&mut spawn, "spawning", Val::Null);
            set(&mut spawn, "notifyWhenAttacked", Val::Bool(true));
            kept.push(new_doc(w, spawn));
            let mut info = (*w.rooms[r as usize].info).clone();
            set(&mut info, "invaderGoal", Val::Num(1000000.0));
            install_room(w, r, kept, None, Some(Arc::new(info)));
            activate(w, r);
            crate::edit_user(w, u, |st| {
                let mut d = (*st.doc).clone();
                set(&mut d, "active", Val::Num(10000.0));
                st.doc = Arc::new(d);
            });
            Ok(())
    }
}

/// POST /api/game/rooms
pub fn rooms(req: &Request, ctx: &ApiCtx) -> Response {
    let view = ctx.published.get();
    // `{room: {$in: rooms}}` on the binary `room` index: request order, each doc once
    let mut idx: Vec<RoomIdx> = Vec::new();
    if let Some(Json::Arr(a)) = req.body().get("rooms") {
        for r in a.iter().filter_map(Json::as_str).filter_map(|n| room_idx(&view, n)) {
            if !idx.contains(&r) {
                idx.push(r);
            }
        }
    }
    Response::ok(obj(vec![("rooms", Json::Arr(idx.into_iter().map(|r| terrain_doc(&view, r)).collect()))]))
}

// ---- flags (GAME-FLAGS-STORE, FMT-FLAGS) ----

type Flag = Vec<String>;

fn parse_flags(data: &str) -> Vec<Flag> {
    if data.is_empty() {
        return vec![];
    }
    data.split('|')
        .map(|j| {
            let mut item: Flag = j.split('~').map(str::to_string).collect();
            item[0] = item[0].replace("$VLINE$", "|").replace("$TILDE$", "~");
            item
        })
        .collect()
}

fn serialize_flags(flags: &[Flag]) -> String {
    flags
        .iter()
        .map(|i| {
            let mut i = i.clone();
            i[0] = i[0].replace('|', "$VLINE$").replace('~', "$TILDE$");
            i.join("~")
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// `getFlags(user)`: room name → flags, in doc order.
fn all_flags(w: &Wv, u: UserIdx) -> Vec<(RoomIdx, Vec<Flag>)> {
    w.user_flags(u).into_iter().map(|(r, f)| (r, parse_flags(str_field(f, "data").unwrap_or("")))).collect()
}

/// `getFlags(user, room)`.
fn room_flags(w: &Wv, u: UserIdx, r: RoomIdx) -> Vec<Flag> {
    let id = w.uid(u);
    w.rooms[r as usize].flags.iter().find(|f| str_field(f, "user") == Some(id)).map_or(vec![], |f| parse_flags(str_field(f, "data").unwrap_or("")))
}

/// `setFlags(user, room, flags)`: upserts the `rooms.flags` doc; `{modified:1}` or `{inserted:1}`.
fn set_flags(w: &mut World, u: UserIdx, r: RoomIdx, flags: &[Flag]) -> Json {
    let id = w.users[u as usize].id;
    let data = serialize_flags(flags);
    let prev = w.rooms[r as usize].flags.clone();
    let mut list: Vec<Arc<Obj>> = prev.to_vec();
    let result = match list.iter_mut().find(|f| str_field(f, "user") == Some(id.as_str())) {
        Some(f) => {
            let mut o = (**f).clone();
            set(&mut o, "data", s(&data));
            *f = Arc::new(o);
            "modified"
        }
        None => {
            let mut o = Obj::new();
            set(&mut o, "user", s(id.as_str()));
            set(&mut o, "room", s(&w.meta.rooms[r as usize].name));
            set(&mut o, "data", s(&data));
            let fid = w.ids.next();
            set(&mut o, "_id", s(fid.as_str()));
            list.push(Arc::new(o));
            "inserted"
        }
    };
    let objs = w.rooms[r as usize].objects.to_vec();
    install_room(w, r, objs, Some(list.into()), None);
    obj(vec![(result, Json::Num(1.0))])
}

fn color_ok(v: Option<&Json>) -> bool {
    matches!(v, Some(Json::Num(n)) if C::COLORS_ALL.iter().any(|c| *c as f64 == *n))
}

fn js_str(v: Option<&Json>) -> String {
    crate::socket::user::js_to_string(v)
}

/// POST /api/game/create-flag
pub fn create_flag(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let coord = |k| matches!(b.get(k), Some(Json::Num(n)) if (0.0..=49.0).contains(n));
    let name_ok = matches!(b.get("name"), Some(Json::Str(n)) if utf16_len(n) <= 60);
    if !coord("x") || !coord("y") || !name_ok || !color_ok(b.get("color")) || !color_ok(b.get("secondaryColor")) {
        return Response::error("invalid params");
    }
    let name = b.get("name").and_then(Json::as_str).unwrap_or("").to_string();
    let item: Flag = ["name", "color", "secondaryColor", "x", "y"].iter().map(|k| js_str(b.get(k))).collect();
    let room = b.get("room").cloned();
    reply(
        write(ctx, move |w| {
            let r = check_game(&Wv::of_world(w), room.as_ref())?;
            let existing = all_flags(&Wv::of_world(w), u);
            if let Some((fr, mut flags)) = existing.into_iter().find(|(_, f)| f.iter().any(|i| i[0] == name)) {
                flags.retain(|i| i[0] != name);
                set_flags(w, u, fr, &flags);
            }
            let flags = all_flags(&Wv::of_world(w), u);
            if flags.iter().map(|(_, f)| f.len()).sum::<usize>() >= C::FLAGS_LIMIT as usize {
                return Err("flags limit exceeded".into());
            }
            let mut room_flags = flags.into_iter().find(|(fr, _)| *fr == r).map(|(_, f)| f).unwrap_or_default();
            room_flags.push(item);
            Ok(set_flags(w, u, r, &room_flags))
        }),
        |j| j,
    )
}

/// POST /api/game/gen-unique-flag-name
pub fn gen_unique_flag_name(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let flags = all_flags(&Wv::of_view(&view), u);
    let mut c = 1;
    while flags.iter().any(|(_, f)| f.iter().any(|i| i[0] == format!("Flag{c}"))) {
        c += 1;
    }
    Response::ok(obj(vec![("name", Json::Str(format!("Flag{c}")))]))
}

/// POST /api/game/check-unique-flag-name
pub fn check_unique_flag_name(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let name = js_str(req.body().get("name"));
    let view = ctx.published.get();
    if all_flags(&Wv::of_view(&view), u).iter().any(|(_, f)| f.iter().any(|i| i[0] == name)) {
        return Response::error("name exists");
    }
    Response::ok(Json::Null)
}

/// getFlags(user, room) → edit → setFlags, for change-flag-color / remove-flag.
fn edit_room_flags(ctx: &ApiCtx, u: UserIdx, room: Option<Json>, f: impl FnOnce(&mut Vec<Flag>) + Send + 'static) -> Response {
    let Some(name) = room.filter(|r| truthy(Some(&Val::from_json(r)))) else { return Response::ok(Json::Null) };
    let name = js_str(Some(&name));
    reply(
        write(ctx, move |w| {
            // a room this world does not have: vanilla would upsert a doc nobody reads
            let Some(r) = room_idx_meta(&w.meta, &name) else { return Ok(obj(vec![("inserted", Json::Num(1.0))])) };
            let mut flags = room_flags(&Wv::of_world(w), u, r);
            f(&mut flags);
            Ok(set_flags(w, u, r, &flags))
        }),
        |j| j,
    )
}

/// POST /api/game/change-flag-color
pub fn change_flag_color(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    if !color_ok(b.get("color")) || !color_ok(b.get("secondaryColor")) {
        return Response::error("invalid params");
    }
    let (name, color, secondary) = (js_str(b.get("name")), js_str(b.get("color")), js_str(b.get("secondaryColor")));
    edit_room_flags(ctx, u, b.get("room").cloned(), move |flags| {
        if let Some(f) = flags.iter_mut().find(|i| i[0] == name) {
            f.resize(f.len().max(3), String::new()); // flag[1] = …; flag[2] = … extend short arrays
            f[1] = color;
            f[2] = secondary;
        }
    })
}

/// POST /api/game/remove-flag
pub fn remove_flag(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let name = js_str(b.get("name"));
    edit_room_flags(ctx, u, b.get("room").cloned(), move |flags| flags.retain(|i| i[0] != name))
}

/// POST /api/game/add-object-intent
// ponytail: needs `engine::system` sanitizeUserIntents (WP02); interface request filed.
pub fn add_object_intent(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    Response::error("not implemented")
}

/// POST /api/game/create-construction
pub fn create_construction(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let (x, y) = (parse_int(b.get("x")), parse_int(b.get("y")));
    let ty_key = js_str(b.get("structureType"));
    let cost = ty_key.parse::<StructureType>().ok().and_then(C::construction_cost).filter(|_| b.get("structureType").is_some());
    if x < 0.0 || x > 49.0 || y < 0.0 || y > 49.0 || cost.is_none() || bad_name(b.get("name"), 50) {
        return Response::error("invalid params");
    }
    if ty_key == "spawn" && !truthy(b.get("name").map(Val::from_json).as_ref()) {
        return Response::error("invalid params");
    }
    let terrain = room_terrain_of(&ctx.published.get(), b.get("room"));
    let site = Site { room: b.get("room").cloned(), name: b.get("name").cloned(), st_raw: b.get("structureType").cloned().unwrap_or(Json::Null), ty_key, cost: cost.unwrap_or(0) as f64, x, y };
    reply(write(ctx, construction_cmd(u, site, terrain)), |id| obj(vec![("_id", Json::Str(id.as_str().into()))]))
}

struct Site {
    room: Option<Json>,
    name: Option<Json>,
    st_raw: Json,
    ty_key: String,
    cost: f64,
    x: f64,
    y: f64,
}

/// The checks and effects of create-construction at P0 (GAME-CREATE-CONSTRUCTION steps 3–6).
fn construction_cmd(u: UserIdx, site: Site, terrain: RoomTerrain) -> impl FnOnce(&mut World) -> Result<engine::doc::Id, String> + Send + 'static {
    let Site { room, name, st_raw, ty_key, cost, x, y } = site;
    move |w| {
            let (r, uid, sites) = {
                let v = Wv::of_world(w);
                let r = check_game(&v, room.as_ref())?;
                let objs = &v.rooms[r as usize].objects;
                check_spot(objs, &terrain, &ty_key, x, y)?;
                let uid = v.uid(u).to_string();
                check_controller_construct(objs, &uid, &ty_key)?;
                (r, uid, v.user_objects(u).iter().filter(|d| d.kind() == "constructionSite").count())
            };
            let mut progress_total = cost;
            if ty_key == "road" {
                let code = terrain_code(&terrain, x, y);
                if code & C::TERRAIN_MASK_SWAMP != 0 {
                    progress_total *= C::CONSTRUCTION_COST_ROAD_SWAMP_RATIO as f64;
                }
                if code & C::TERRAIN_MASK_WALL != 0 {
                    progress_total *= C::CONSTRUCTION_COST_ROAD_WALL_RATIO as f64;
                }
            }
            if sites >= C::MAX_CONSTRUCTION_SITES as usize {
                return Err("too many".into());
            }
            let mut site = Obj::new();
            set(&mut site, "type", s("constructionSite"));
            set(&mut site, "room", Val::from_json(room.as_ref().unwrap_or(&Json::Null)));
            set(&mut site, "x", Val::Num(x));
            set(&mut site, "y", Val::Num(y));
            set(&mut site, "structureType", Val::from_json(&st_raw));
            if let Some(n) = &name {
                set(&mut site, "name", Val::from_json(n));
            }
            set(&mut site, "user", s(&uid));
            set(&mut site, "progress", Val::Num(0.0));
            set(&mut site, "progressTotal", Val::Num(progress_total));
            let d = new_doc(w, site);
            let id = d.id();
            let mut objs = w.rooms[r as usize].objects.to_vec();
            objs.push(d);
            install_room(w, r, objs, None, None);
            activate(w, r);
            Ok(id)
    }
}

/// GET /api/game/room-overview
pub fn room_overview(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    let view = ctx.published.get();
    let ctrl = match req.param("room") {
        Some(n) => room_idx(&view, n).and_then(|r| controller(&view.rooms[r as usize].objects).cloned()),
        // `{room: undefined}` is dropped by the RPC: the first controller of the world
        None => view.rooms.iter().flat_map(|s| s.objects.iter()).filter(|d| d.kind() == "controller").min_by_key(|d| d.seq).cloned(),
    };
    let owner = ctrl
        .and_then(|c| str_field(&c.obj, "user").and_then(|id| crate::user_by_id(&view, id)))
        .map(|u| {
            let d = &view.users[u as usize].doc;
            Json::Obj(["username", "badge"].iter().filter_map(|k| field(d, k).map(|v| (k.to_string(), v.to_json()))).collect())
        })
        .unwrap_or(Json::Null);
    Response::ok(obj(vec![("owner", owner), ("stats", Json::Obj(vec![])), ("statsMax", Json::Obj(vec![])), ("totals", Json::Obj(vec![]))]))
}

/// Finds a `rooms.objects` doc by `_id` (scan).
fn object_by_id(w: &Wv, id: &str) -> Option<(RoomIdx, DocRef)> {
    w.rooms.iter().enumerate().find_map(|(r, s)| s.objects.iter().find(|d| d.id().as_str() == id).map(|d| (r as RoomIdx, d.clone())))
}

/// POST /api/game/set-notify-when-attacked
pub fn set_notify_when_attacked(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let id = js_str(b.get("_id"));
    let enabled = truthy(b.get("enabled").map(Val::from_json).as_ref());
    reply(
        write(ctx, move |w| {
            let (r, d) = object_by_id(&Wv::of_world(w), &id).ok_or("TypeError: Cannot read properties of null (reading 'user')")?;
            let uid = w.users[u as usize].id;
            if truthy(field(&d.obj, "user")) {
                if str_field(&d.obj, "user") != Some(uid.as_str()) {
                    return Err("not owner".into());
                }
            } else {
                let objs = &w.rooms[r as usize].objects;
                let c = controller(objs).ok_or("TypeError: Cannot read properties of null (reading 'user')")?;
                if str_field(&c.obj, "user") != Some(uid.as_str()) {
                    return Err("not controller owner".into());
                }
            }
            let objs = edit_docs(&w.rooms[r as usize].objects.clone(), |x| {
                (x.id() == d.id()).then(|| {
                    let mut o = x.obj.clone();
                    set(&mut o, "notifyWhenAttacked", Val::Bool(enabled));
                    Some(o)
                })
            });
            install_room(w, r, objs, None, None);
            Ok(())
        }),
        |_| obj(vec![("modified", Json::Num(1.0))]),
    )
}

/// game.js create-invader bodies.
fn invader_body(kind: &str) -> Option<Vec<&'static str>> {
    let rep = |n: usize, p: &'static str| std::iter::repeat_n(p, n);
    let v: Vec<&str> = match kind {
        "bigHealer" => rep(24, "move").chain(rep(25, "heal")).chain(rep(1, "move")).collect(),
        "bigRanged" => rep(6, "tough").chain(rep(24, "move")).chain(rep(18, "ranged_attack")).chain(rep(1, "work")).chain(rep(1, "move")).collect(),
        "bigMelee" => rep(16, "tough").chain(rep(24, "move")).chain(rep(3, "ranged_attack")).chain(rep(4, "work")).chain(rep(2, "attack")).chain(rep(1, "move")).collect(),
        "smallHealer" => rep(4, "move").chain(rep(5, "heal")).chain(rep(1, "move")).collect(),
        "smallRanged" => rep(2, "tough").chain(rep(4, "move")).chain(rep(3, "ranged_attack")).chain(rep(1, "move")).collect(),
        "smallMelee" => rep(2, "tough").chain(rep(4, "move")).chain(["ranged_attack", "work", "attack", "move"]).collect(),
        _ => return None,
    };
    Some(v)
}

/// POST /api/game/create-invader
pub fn create_invader(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let room = js_str(b.get("room"));
    let kind = format!("{}{}", js_str(b.get("size")), js_str(b.get("type")));
    let (x, y, boosted) = (b.get("x").cloned(), b.get("y").cloned(), truthy(b.get("boosted").map(Val::from_json).as_ref()));
    let mut rnd = [0u8; 8];
    util::sys::os_random(&mut rnd);
    let suffix = (u64::from_le_bytes(rnd) % 1000).to_string(); // Math.floor(Math.random() * 1000)
    reply(
        write(ctx, move |w| {
            let uid = w.users[u as usize].id.as_str().to_string();
            let r = room_idx_meta(&w.meta, &room);
            let objs: Vec<DocRef> = r.map(|r| w.rooms[r as usize].objects.to_vec()).unwrap_or_default();
            let creeps: Vec<&DocRef> = objs.iter().filter(|d| d.kind() == "creep").collect();
            if creeps.iter().filter(|d| str_field(&d.obj, "user") == Some("2")).count() >= 5 {
                return Err("too many invaders exist".into());
            }
            if creeps.iter().any(|d| !matches!(str_field(&d.obj, "user"), Some("2")) && str_field(&d.obj, "user") != Some(uid.as_str())) {
                return Err("hostiles present".into());
            }
            let owned = controller(&objs).is_some_and(|c| {
                str_field(&c.obj, "user") == Some(uid.as_str()) || field(&c.obj, "reservation").and_then(Val::as_obj).and_then(|r| str_field(r, "user")) == Some(uid.as_str())
            });
            let (Some(r), true) = (r, owned) else { return Err("not owned".into()) };
            let body = invader_body(&kind).ok_or("TypeError: Cannot read properties of undefined (reading 'length')")?;
            let mut creep = Obj::new();
            set(&mut creep, "type", s("creep"));
            set(&mut creep, "user", s("2"));
            let parts = body
                .iter()
                .map(|t| {
                    let mut p = Obj::new();
                    set(&mut p, "type", s(t));
                    set(&mut p, "hits", Val::Num(100.0));
                    if boosted {
                        let boost = match *t {
                            "heal" => Some("LO"),
                            "ranged_attack" => Some("KO"),
                            "work" => Some("ZH"),
                            "attack" => Some("UH"),
                            _ => None,
                        };
                        if let Some(bst) = boost {
                            set(&mut p, "boost", s(bst));
                        }
                    }
                    Val::Obj(p)
                })
                .collect();
            set(&mut creep, "body", Val::Arr(parts));
            set(&mut creep, "hits", Val::Num(body.len() as f64 * 100.0));
            set(&mut creep, "hitsMax", Val::Num(body.len() as f64 * 100.0));
            set(&mut creep, "ticksToLive", Val::Num(1500.0));
            for (k, v) in [("x", &x), ("y", &y)] {
                if let Some(v) = v {
                    set(&mut creep, k, Val::from_json(v));
                }
            }
            set(&mut creep, "room", s(&room));
            set(&mut creep, "fatigue", Val::Num(0.0));
            set(&mut creep, "store", Val::Obj(Obj::new()));
            set(&mut creep, "storeCapacity", Val::Num(0.0));
            set(&mut creep, "name", s(&format!("invader_{room}_{suffix}")));
            set(&mut creep, "userSummoned", s(&uid));
            let d = new_doc(w, creep);
            let out = json_obj(&d.obj);
            let mut all = objs.clone();
            all.push(d);
            install_room(w, r, all, None, None); // no activateRoom (GAME-CREATE-INVADER GOTCHA)
            Ok(out)
        }),
        |d| d,
    )
}

/// POST /api/game/remove-invader
pub fn remove_invader(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let id = req.body().get("_id").map(|v| js_str(Some(v)));
    reply(
        write(ctx, move |w| {
            let uid = w.users[u as usize].id;
            let found = match &id {
                Some(id) => object_by_id(&Wv::of_world(w), id),
                // `findOne({_id: undefined})` → `findOne({})`: the first object of the world
                None => w.rooms.iter().enumerate().flat_map(|(r, s)| s.objects.iter().map(move |d| (r as RoomIdx, d.clone()))).min_by_key(|(_, d)| d.seq),
            };
            let (r, d) = found.filter(|(_, d)| str_field(&d.obj, "userSummoned") == Some(uid.as_str())).ok_or("invalid object")?;
            let objs = edit_docs(&w.rooms[r as usize].objects.clone(), |x| (x.id() == d.id()).then_some(None));
            install_room(w, r, objs, None, None);
            Ok(())
        }),
        |_| Json::Null,
    )
}

/// GET /api/game/world-size
pub fn world_size(req: &Request, ctx: &ApiCtx) -> Response {
    let n = Json::Num(ctx.published.get().meta.world_size as f64);
    Response::ok(obj(vec![("width", n.clone()), ("height", n)]))
}

/// POST /api/game/add-global-intent
// ponytail: needs `engine::system` sanitizeUserIntents (WP02); interface request filed.
pub fn add_global_intent(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    Response::error("not implemented")
}

/// GET /api/game/tick
pub fn tick(req: &Request, ctx: &ApiCtx) -> Response {
    Response::ok(obj(vec![("tick", ctx.published.min_tick_ms().map_or(Json::Null, |t| Json::Num(t as f64)))]))
}

/// POST /api/game/change-flag (no such route in vanilla: Express 404)
pub fn change_flag(req: &Request, ctx: &ApiCtx) -> Response {
    Response::not_found(&req.method, &req.path)
}

/// GET /api/game/room-decorations (STUB-ROOM-DECORATIONS)
pub fn room_decorations(req: &Request, ctx: &ApiCtx) -> Response {
    Response::ok(obj(vec![("decorations", Json::Arr(vec![]))]))
}

/// GET /api/game/find-object (STUB-FIND-OBJECT): `{object}` when the id exists.
pub fn find_object(req: &Request, ctx: &ApiCtx) -> Response {
    let view = ctx.published.get();
    match req.param("_id").and_then(|id| object_by_id(&Wv::of_view(&view), id)) {
        Some((_, d)) => Response::ok(obj(vec![("object", json_obj(&d.obj))])),
        None => Response::error("not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    fn err(j: &Json) -> Option<&str> {
        j.get("error").and_then(Json::as_str)
    }

    fn terrain_with_walls(cells: &[(u8, u8, u8)]) -> RoomTerrain {
        let mut t = RoomTerrain::default();
        for &(x, y, c) in cells {
            t.set_raw(x, y, c);
        }
        t
    }

    #[test]
    fn construction_spot_rules() {
        let w = fake::world();
        let objs = w.rooms[1].objects.to_vec(); // spawn at 25,25; creep 26,25; extension site 27,25
        let t = terrain_with_walls(&[(10, 10, 1), (11, 11, 2), (0, 4, 1), (0, 5, 1), (0, 6, 1), (0, 9, 1), (0, 10, 1)]);
        assert!(check_spot(&objs, &t, "spawn", 0.0, 5.0).is_err());
        assert!(check_spot(&objs, &t, "road", 25.0, 25.0).is_err(), "spawn blocks roads");
        assert!(check_spot(&objs, &t, "rampart", 25.0, 25.0).is_ok(), "ramparts go over spawns");
        assert!(check_spot(&objs, &t, "rampart", 27.0, 25.0).is_err(), "no site on a site");
        assert!(check_spot(&objs, &t, "road", 26.0, 25.0).is_ok(), "creeps do not block");
        assert!(check_spot(&objs, &t, "spawn", 10.0, 10.0).is_err(), "wall");
        assert!(check_spot(&objs, &t, "road", 10.0, 10.0).is_ok(), "roads on walls");
        assert!(check_spot(&objs, &t, "spawn", 11.0, 11.0).is_ok(), "swamp is fine");
        assert!(check_spot(&objs, &t, "spawn", 1.0, 5.0).is_ok(), "next to a walled border");
        assert!(check_spot(&objs, &t, "spawn", 1.0, 9.0).is_err(), "border tile (0,8) is open");
        assert!(check_spot(&objs, &t, "container", 1.0, 9.0).is_ok());
        assert!(check_spot(&objs, &t, "extractor", 30.0, 30.0).is_err());
        let w1 = w.rooms[0].objects.to_vec();
        assert!(check_spot(&w1, &t, "extractor", 40.0, 40.0).is_ok(), "on the mineral");
    }

    #[test]
    fn controller_checks() {
        let w = fake::world();
        let alice = "a11ce0000000001";
        assert_eq!(check_controller_spawn(&w.rooms[0].objects, alice), Ok(()));
        assert_eq!(check_controller_spawn(&w.rooms[1].objects, alice), Err("room busy".into()));
        assert_eq!(check_controller_spawn(&w.rooms[2].objects, alice), Err("room busy".into()));
        // RCL 2 with one extension site: 5 allowed
        assert_eq!(check_controller_construct(&w.rooms[1].objects, alice, "extension"), Ok(()));
        assert_eq!(check_controller_construct(&w.rooms[1].objects, alice, "tower"), Err("RCL not enough".into()));
        assert_eq!(check_controller_construct(&w.rooms[1].objects, "b0b000000000002", "road"), Err("not a controller owner".into()));
        assert_eq!(check_controller_construct(&w.rooms[2].objects, alice, "road"), Err("not a controller owner".into()));
        assert_eq!(check_controller_construct(&w.rooms[0].objects, alice, "road"), Ok(()));
        assert_eq!(check_controller_construct(&w.rooms[0].objects, alice, "spawn"), Err("RCL not enough".into()));
    }

    /// node: game.js getFlags/setFlags string handling (tests/fixtures/game/flags.json).
    #[test]
    fn flags_round_trip_matches_node() {
        let fx = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/game/flags.json")).unwrap();
        for c in Json::parse(&fx).unwrap().as_array().unwrap() {
            let data = c.get("data").unwrap().as_str().unwrap();
            let parsed: Vec<Vec<String>> = c.get("parsed").unwrap().as_array().unwrap().iter().map(|f| f.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()).collect();
            assert_eq!(parse_flags(data), parsed, "{data}");
            assert_eq!(serialize_flags(&parsed), c.get("reserialized").unwrap().as_str().unwrap());
        }
    }

    fn room(n: &str) -> Option<Json> {
        Some(Json::Str(n.into()))
    }

    #[test]
    fn place_spawn_flow() {
        // the handler reads terrain through WP01's TerrainStore (still a stub): it must answer, not crash
        let (ctx, _h) = fake::ctx(Some(fake::BOB));
        let (st, _) = crate::fake::run(place_spawn, &ctx, "POST", "/x", r#"{"room":"W1N1","x":"25","y":25,"name":"Spawn1"}"#);
        assert_eq!(st, 200);

        let mut w = fake::world();
        let t = RoomTerrain::default();
        let cmd = |name: &str, x: f64| place_spawn_cmd(fake::BOB, room("W1N1"), Some(Json::Str(name.into())), x, 25.0, t.clone(), 0.0);
        assert_eq!(place_spawn_cmd(fake::BOB, room("W2N1"), None, 5.0, 5.0, t.clone(), 0.0)(&mut w), Err("room busy".into()));
        assert_eq!(place_spawn_cmd(fake::BOB, room("W9N9"), None, 5.0, 5.0, t.clone(), 0.0)(&mut w), Err("invalid room".into()));
        assert_eq!(cmd("S", 0.0)(&mut w), Err("invalid location".into()));
        assert_eq!(cmd("Spawn1", 25.0)(&mut w), Ok(()));
        assert_eq!(cmd("Spawn2", 5.0)(&mut w), Err("already playing".into()));
        let objs = &w.rooms[0].objects;
        let ctrl = objs.iter().find(|d| d.kind() == "controller").unwrap();
        assert_eq!(str_field(&ctrl.obj, "user"), Some("b0b000000000002"));
        assert_eq!(num(&ctrl.obj, "safeMode"), Some(21000.0));
        let spawn = objs.iter().find(|d| d.kind() == "spawn").unwrap();
        let mut j = String::new();
        Val::Obj(spawn.obj.clone()).write_json(&mut j);
        assert!(j.starts_with(r#"{"type":"spawn","room":"W1N1","x":25,"y":25,"name":"Spawn1","user":"b0b000000000002","store":{"energy":300},"storeCapacityResource":{"energy":300},"hits":5000,"hitsMax":5000,"spawning":null,"notifyWhenAttacked":true,"_id":""#), "{j}");
        assert!(objs.iter().filter(|d| d.kind() == "source").all(|d| num(&d.obj, "invaderHarvested") == Some(0.0)));
        assert_eq!(field(&w.rooms[0].info, "invaderGoal"), Some(&Val::Num(1000000.0)));
        assert!(w.index.presence[fake::BOB as usize].contains_key(&0));
    }

    #[test]
    fn construction_flow() {
        let mut w = fake::world();
        let mut t = RoomTerrain::default();
        t.set_raw(30, 30, 3);
        let site = |ty: &str, x: f64, y: f64| Site { room: room("W2N1"), name: None, st_raw: Json::Str(ty.into()), ty_key: ty.into(), cost: C::construction_cost(ty.parse().unwrap()).unwrap() as f64, x, y };
        let id = construction_cmd(fake::ALICE, site("road", 30.0, 30.0), t.clone())(&mut w).unwrap();
        let d = w.rooms[1].objects.iter().find(|d| d.id() == id).unwrap().clone();
        assert_eq!(num(&d.obj, "progressTotal"), Some(300.0 * 5.0 * 150.0), "swamp and wall bits both multiply");
        let mut j = String::new();
        Val::Obj(d.obj.clone()).write_json(&mut j);
        assert!(j.starts_with(r#"{"type":"constructionSite","room":"W2N1","x":30,"y":30,"structureType":"road","user":"a11ce0000000001","progress":0,"progressTotal":225000,"_id":"#), "{j}");
        assert_eq!(construction_cmd(fake::BOB, site("road", 31.0, 31.0), t.clone())(&mut w), Err("not a controller owner".into()));
        assert_eq!(construction_cmd(fake::ALICE, site("tower", 31.0, 31.0), t.clone())(&mut w), Err("RCL not enough".into()));
        assert_eq!(construction_cmd(fake::ALICE, site("road", 30.0, 30.0), t.clone())(&mut w), Err("invalid location".into()));
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        assert_eq!(err(&fake::run(create_construction, &ctx, "POST", "/x", r#"{"room":"W2N1","x":5,"y":5,"structureType":"nope"}"#).1), Some("invalid params"));
        assert_eq!(err(&fake::run(create_construction, &ctx, "POST", "/x", r#"{"room":"W2N1","x":5,"y":5,"structureType":"spawn"}"#).1), Some("invalid params"));
    }

    #[test]
    fn place_spawn_rejections() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        assert_eq!(err(&fake::run(place_spawn, &ctx, "POST", "/x", r#"{"room":"W1N1","x":50,"y":5}"#).1), Some("invalid params"));
        assert_eq!(err(&fake::run(place_spawn, &ctx, "POST", "/x", r#"{"room":"W1N1","x":5,"y":5,"name":7}"#).1), Some("invalid params"));
    }

    #[test]
    fn flags_flow() {
        let (ctx, h) = fake::ctx(Some(fake::ALICE));
        assert_eq!(fake::run(gen_unique_flag_name, &ctx, "POST", "/x", "{}").1.stringify(), r#"{"ok":1,"name":"Flag2"}"#);
        assert_eq!(err(&fake::run(check_unique_flag_name, &ctx, "POST", "/x", r#"{"name":"Home"}"#).1), Some("name exists"));
        assert_eq!(err(&fake::run(create_flag, &ctx, "POST", "/x", r#"{"room":"W1N1","x":"1","y":1,"name":"A","color":1,"secondaryColor":1}"#).1), Some("invalid params"));
        assert_eq!(err(&fake::run(create_flag, &ctx, "POST", "/x", r#"{"room":"W1N1","x":1,"y":1,"name":"A","color":11,"secondaryColor":1}"#).1), Some("invalid params"));
        // moving "Home" from W2N1 to W1N1
        let (_, j) = fake::run(create_flag, &ctx, "POST", "/x", r#"{"room":"W1N1","x":3,"y":4.5,"name":"Home","color":2,"secondaryColor":3}"#);
        assert_eq!(j.stringify(), r#"{"ok":1,"inserted":1}"#);
        let (_, j) = fake::run(change_flag_color, &ctx, "POST", "/x", r#"{"room":"W2N1","name":"Flag1","color":7,"secondaryColor":8}"#);
        assert_eq!(j.stringify(), r#"{"ok":1,"modified":1}"#);
        assert_eq!(fake::run(remove_flag, &ctx, "POST", "/x", r#"{"room":"W2N1","name":"nope"}"#).1.stringify(), r#"{"ok":1,"modified":1}"#);
        assert_eq!(fake::run(change_flag_color, &ctx, "POST", "/x", r#"{"name":"Flag1","color":7,"secondaryColor":8}"#).1.stringify(), r#"{"ok":1}"#);
        drop(ctx);
        let w = h.join().unwrap();
        let data = |r: usize| w.rooms[r].flags.iter().find(|f| str_field(f, "user") == Some("a11ce0000000001")).and_then(|f| str_field(f, "data")).unwrap().to_string();
        assert_eq!(data(1), "Flag1~7~8~10~10");
        assert_eq!(data(0), "Home~2~3~3~4.5");
    }

    #[test]
    fn map_stats_shape() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        let (_, j) = fake::run(map_stats, &ctx, "POST", "/x", r#"{"rooms":["W1N2","W2N1","W1N1","W9N9"],"statName":"owner0"}"#);
        assert_eq!(j.get("gameTime").and_then(Json::as_f64), Some(1000.0));
        let stats = j.get("stats").unwrap();
        let rooms: Vec<&str> = stats.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(rooms, ["W1N1", "W2N1", "W1N2"]);
        assert_eq!(stats.get("W2N1").unwrap().stringify(), r#"{"status":"normal","own":{"user":"a11ce0000000001","level":2},"safeMode":true}"#);
        assert_eq!(stats.get("W1N1").unwrap().stringify(), r#"{"status":"normal","minerals0":{"type":"H","density":3}}"#);
        // the invader core (later seq) overwrites the reservation's `own`
        assert_eq!(stats.get("W1N2").unwrap().stringify(), r#"{"status":"normal","own":{"user":"2","level":2},"sign":{"user":"b0b000000000002","text":"hi","time":5,"datetime":1}}"#);
        let users: Vec<&str> = j.get("users").unwrap().as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(users, ["2", "a11ce0000000001", "b0b000000000002"]);
        assert!(j.get("users").unwrap().get("a11ce0000000001").unwrap().get("money").is_none());
        assert_eq!(err(&fake::run(map_stats, &ctx, "POST", "/x", r#"{"rooms":"W1N1","statName":"owner0"}"#).1), Some("invalid params"));
        assert_eq!(err(&fake::run(map_stats, &ctx, "POST", "/x", r#"{"rooms":[],"statName":"owner"}"#).1), Some("invalid params"));
    }

    /// `own = {user: i.reservation.user, level: 0}` with no reservation user: JSON drops the undefined user.
    #[test]
    fn map_stats_reservation_without_user() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        write(&ctx, |w| {
            let objs = edit_docs(&w.rooms[2].objects.clone(), |d| {
                (d.kind() == "controller").then(|| {
                    let mut o = d.obj.clone();
                    set(&mut o, "reservation", Val::from_json(&Json::parse(r#"{"endTime":500}"#).unwrap()));
                    o.remove(engine::atom!("sign"));
                    Some(o)
                })
            });
            let objs = objs.into_iter().filter(|d| d.kind() != "invaderCore").collect();
            install_room(w, 2, objs, None, None);
            Ok(())
        })
        .unwrap();
        let (_, j) = fake::run(map_stats, &ctx, "POST", "/x", r#"{"rooms":["W1N2"],"statName":"owner0"}"#);
        assert_eq!(j.get("stats").unwrap().get("W1N2").unwrap().stringify(), r#"{"status":"normal","own":{"level":0}}"#);
        assert_eq!(j.get("users").unwrap().stringify(), "{}");
    }

    #[test]
    fn object_names_and_misc() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        assert_eq!(fake::run(gen_unique_object_name, &ctx, "POST", "/x", r#"{"type":"spawn"}"#).1.stringify(), r#"{"ok":1,"name":"Spawn2"}"#);
        assert_eq!(err(&fake::run(gen_unique_object_name, &ctx, "POST", "/x", r#"{"type":"flag"}"#).1), Some("invalid params"));
        assert_eq!(err(&fake::run(check_unique_object_name, &ctx, "POST", "/x", r#"{"type":"spawn","name":"Spawn1"}"#).1), Some("name exists"));
        assert_eq!(err(&fake::run(check_unique_object_name, &ctx, "POST", "/x", r#"{"type":"spawn","name":""}"#).1), Some("name exists"), "any site name matches");
        assert_eq!(fake::run(time, &ctx, "GET", "/x", "").1.stringify(), r#"{"ok":1,"time":1000}"#);
        assert_eq!(fake::run(world_size, &ctx, "GET", "/x", "").1.stringify(), r#"{"ok":1,"width":12,"height":12}"#);
        assert_eq!(fake::run(tick, &ctx, "GET", "/x", "").1.stringify(), r#"{"ok":1,"tick":null}"#);
        assert_eq!(fake::run(room_status, &ctx, "GET", "/x?room=W1N1", "").1.stringify(), r#"{"ok":1,"room":{"status":"normal"}}"#);
        assert_eq!(fake::run(room_status, &ctx, "GET", "/x?room=W7N7", "").1.stringify(), r#"{"ok":1,"room":{}}"#);
        assert_eq!(fake::run(room_overview, &ctx, "GET", "/x?room=W2N1", "").1.get("owner").unwrap().get("username").and_then(Json::as_str), Some("alice"));
        assert_eq!(err(&fake::run(room_terrain, &ctx, "GET", "/x?room=W9N9", "").1), Some("invalid room"));
    }

    #[test]
    fn invaders() {
        let (ctx, h) = fake::ctx(Some(fake::ALICE));
        assert_eq!(err(&fake::run(create_invader, &ctx, "POST", "/x", r#"{"room":"W1N1","x":5,"y":5,"size":"small","type":"Melee"}"#).1), Some("not owned"));
        let (_, j) = fake::run(create_invader, &ctx, "POST", "/x", r#"{"room":"W2N1","x":5,"y":5,"size":"small","type":"Healer","boosted":true}"#);
        assert_eq!(j.get("user").and_then(Json::as_str), Some("2"));
        assert_eq!(j.get("hitsMax").and_then(Json::as_f64), Some(1000.0));
        assert_eq!(j.get("body").unwrap().at(4).unwrap().stringify(), r#"{"type":"heal","hits":100,"boost":"LO"}"#);
        let id = j.get("_id").and_then(Json::as_str).unwrap().to_string();
        let (ctx_b, _hb) = (crate::ApiCtx { published: ctx.published.clone(), commands: ctx.commands.clone(), user: Some(fake::BOB) }, ());
        assert_eq!(err(&fake::run(remove_invader, &ctx_b, "POST", "/x", &format!(r#"{{"_id":"{id}"}}"#)).1), Some("invalid object"));
        assert_eq!(fake::run(remove_invader, &ctx, "POST", "/x", &format!(r#"{{"_id":"{id}"}}"#)).1.stringify(), r#"{"ok":1}"#);
        assert_eq!(fake::run(set_notify_when_attacked, &ctx, "POST", "/x", r#"{"_id":"spawn2","enabled":0}"#).1.stringify(), r#"{"ok":1,"modified":1}"#);
        assert_eq!(err(&fake::run(set_notify_when_attacked, &ctx_b, "POST", "/x", r#"{"_id":"spawn2","enabled":0}"#).1), Some("not owner"));
        assert_eq!(err(&fake::run(set_notify_when_attacked, &ctx_b, "POST", "/x", r#"{"_id":"src1","enabled":0}"#).1), Some("not controller owner"));
        drop((ctx, ctx_b));
        let w = h.join().unwrap();
        assert!(!w.rooms[1].objects.iter().any(|d| d.kind() == "creep" && str_field(&d.obj, "user") == Some("2")));
        let spawn = w.rooms[1].objects.iter().find(|d| d.kind() == "spawn").unwrap();
        assert_eq!(field(&spawn.obj, "notifyWhenAttacked"), Some(&Val::Bool(false)));
    }
}
