//! mirror: reference/backend/lib/game/api/user.js
//! parity: DIVERGENT(USER-SET-ACTIVE-BRANCH): branch switches are atomic; writes apply at the next tick boundary
//! (spec 10 §7); code timestamps use vclock(G); `world-start-room` falls back to an existing room when W5N5 is
//! missing (USER-WORLD-START-ROOM GOTCHA); segments are stored as strings (`null`/objects become their `""+v`,
//! an absent `data` removes the segment); `badge-svg` has no 1 h cache, and a badge colour must be the whole `#rrggbb`;
//! the modules of an upload and the console queue have limits (`MAX_MODULES`, `MAX_CONSOLE_COMMANDS`); a module is text or
//! `{binary: text}`, and a branch name or a console command is not an object or an array, where the original stores any
//! value: their keys would go into the names the server keeps until it restarts (`engine::doc`).
//! Every route is replayed against the official router in tests/fixtures/api (server.rs oracle), whose storage
//! calls return Q promises as the real RPC does: `respawn` therefore resets every room's status to 'normal'
//! (utils.js:147-150; spec 11 GEN-RESPAWN's "does nothing" is wrong, see `respawn_user`).
#![allow(unused_variables, clippy::manual_range_contains)] // JS range checks: NaN must pass them

use crate::http::{obj, Request, Response};
use crate::{edit_user, field, iso_date, obj_set, s, str_field, truthy, user_brief, vnow, write, ApiCtx};
use engine::doc::{json_truthy, Atom, Doc, DocRef, Obj, Val, OUTSIDE_NAME_BYTES};
use engine::world::{CodeState, RoomIdx, UserIdx, World, WorldView};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use util::json::Json;

/// The most code `POST /api/user/code` stores, in UTF-16 units: the game's 5 MB, unless a mod changes it.
pub static MAX_CODE_UNITS: AtomicUsize = AtomicUsize::new(5 * 1024 * 1024);

/// The most modules one upload holds. The original has no such limit, but a small upload could hold hundreds of thousands
/// of names, and each costs a stored key and a pass over the names, on the tick thread.
pub const MAX_MODULES: usize = 4096;

/// What one user's console queue holds. It empties when the user's code runs, so only someone whose code does not run
/// gets near this, and then it would grow for good: nothing reads it, and every checkpoint writes it. Room for a few of
/// the 1 MiB that `user/memory` takes.
const MAX_CONSOLE_COMMANDS: usize = 256;
const MAX_CONSOLE_BYTES: usize = 4 << 20;

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

/// Every `rooms.objects` doc owned by `u`, in collection (seq) order.
pub(crate) fn user_objects(view: &WorldView, u: UserIdx) -> Vec<&DocRef> {
    let id = view.users[u as usize].id.as_str();
    let mut out: Vec<&DocRef> = Vec::new();
    for &r in view.presence.get(u as usize).map(|p| p.keys().copied().collect::<Vec<_>>()).unwrap_or_default().iter() {
        out.extend(view.rooms[r as usize].objects.iter().filter(|d| str_field(&d.obj, "user") == Some(id)));
    }
    out.sort_by_key(|d| d.seq);
    out
}

// ---- utils.translateModulesToDb / translateModulesFromDb (CODE-MODULE-KEYS) ----

/// `o[k] = v` in JS own-property order.
fn js_set(m: &mut Vec<(String, Json)>, k: String, v: Json) {
    if let Some(slot) = m.iter_mut().find(|(x, _)| *x == k) {
        slot.1 = v;
        return;
    }
    let at = match engine::doc::array_index(&k) {
        None => m.len(),
        Some(i) => m.iter().position(|(x, _)| engine::doc::array_index(x).is_none_or(|j| j > i)).unwrap_or(m.len()),
    };
    m.insert(at, (k, v));
}

/// One pass, as `code_event` runs this on the tick thread. A renamed key holds an escape or a separator, so it is never an
/// array index, and it goes last, or takes the place of a key of that name.
fn rename_keys(modules: Json, f: impl Fn(&str) -> Option<String>) -> Vec<(String, Json)> {
    let Json::Obj(m) = modules else { return vec![] };
    let mut slots: HashMap<String, usize> = m.iter().enumerate().map(|(i, (k, _))| (k.clone(), i)).collect();
    let mut out: Vec<Option<(String, Json)>> = m.into_iter().map(Some).collect();
    for i in 0..out.len() {
        let Some((key, v)) = out[i].take() else { continue };
        match f(&key) {
            Some(new) if new == key => out[i] = Some((key, v)),
            Some(new) => {
                slots.remove(&key);
                match slots.get(&new) {
                    Some(&j) => out[j].as_mut().expect("a slot in the map is filled").1 = v,
                    None => {
                        slots.insert(new.clone(), out.len());
                        out.push(Some((new, v)));
                    }
                }
            }
            None => drop(slots.remove(&key)),
        }
    }
    out.into_iter().flatten().collect()
}

/// utils.translateModulesToDb: `.`/`/`/`\` escaped, names starting with `$` (after escaping) dropped, a falsy
/// `main` becomes `''`.
pub(crate) fn modules_to_db(modules: Json) -> Json {
    let mut m = rename_keys(modules, stored_name);
    let main_falsy = m.iter().find(|(k, _)| k == "main").is_none_or(|(_, v)| !json_truthy(v));
    if main_falsy {
        js_set(&mut m, "main".into(), Json::Str(String::new()));
    }
    Json::Obj(m)
}

/// A module's name as it is stored, or None for a name that is dropped (utils.translateModulesToDb).
fn stored_name(k: &str) -> Option<String> {
    let n = k.replace('.', "$DOT$").replace('/', "$SLASH$").replace('\\', "$BACKSLASH$");
    (!n.starts_with('$')).then_some(n)
}

/// Text, or `{binary: text}` and nothing else. The original stores any value, but stored keys are names the server keeps
/// until it restarts, so keys inside a module would let a request of any size spend all it will take (`engine::doc`).
fn is_module(m: &Json) -> bool {
    match m {
        Json::Str(_) => true,
        Json::Obj(o) => matches!(o.as_slice(), [(k, Json::Str(_))] if k == "binary"),
        _ => false,
    }
}

/// Why these modules cannot be stored, or None. The rule for their number, names and shape, for anything that takes code
/// from a player. Not their size: each route has its own. Modules that are not an object hold no names and store as empty.
pub fn module_problem(modules: &Json) -> Option<String> {
    let modules = modules.as_object()?;
    if modules.len() > MAX_MODULES {
        return Some("too many modules".into());
    }
    if modules.iter().any(|(k, _)| stored_name(k).is_some_and(|n| n.len() > OUTSIDE_NAME_BYTES)) {
        return Some("module name is too long".into());
    }
    if !modules.iter().all(|(_, m)| is_module(m)) {
        return Some("a module is text, or an object holding only text in binary".into());
    }
    None
}

/// Modules from a request, as stored, or the error for the reply. Every route that stores them comes through here.
pub(crate) fn checked_modules(modules: Json) -> Result<Val, String> {
    if modules.stringify().encode_utf16().count() > MAX_CODE_UNITS.load(Ordering::Relaxed) {
        return Err("code length exceeds 5 MB limit".into());
    }
    if let Some(problem) = module_problem(&modules) {
        return Err(problem);
    }
    let db = modules_to_db(modules);
    let names = db.as_object().map_or(&[][..], Vec::as_slice);
    // `Val::from_json` leaves a name out once the server has taken all it will: this is the one place that must not lose
    // one. `binary` is the one name inside a module.
    Atom::intern("binary");
    let stored = Val::from_json(&db);
    match &stored {
        Val::Obj(o) if o.len() == names.len() => Ok(stored),
        _ => Err("this server cannot take more new module names until it restarts".into()),
    }
}

/// utils.translateModulesFromDb.
pub(crate) fn modules_from_db(modules: Json) -> Json {
    Json::Obj(rename_keys(modules, |k| Some(k.replace("$DOT$", ".").replace("$SLASH$", "/").replace("$BACKSLASH$", "\\"))))
}

// ---- users.code helpers ----

/// The `users.code` query of code GET/POST: `$X` → `{user, X: true}`, else `{user, branch}`. `X` is the request's, so it
/// is compared and not looked up by `field`, which would keep it as a name for good.
fn code_matches(doc: &Obj, branch: &str) -> bool {
    match branch.strip_prefix('$') {
        Some(active) => doc.iter().any(|(k, v)| k.as_str() == active && *v == Val::Bool(true)),
        None => str_field(doc, "branch") == Some(branch),
    }
}

/// Edits a user's code branches; recomputes the active indices and bumps `version` when the active world
/// branch changed (§3.5).
fn edit_code<R>(w: &mut World, u: UserIdx, f: impl FnOnce(&mut Vec<Obj>) -> R) -> R {
    let code = w.users[u as usize].code.clone();
    let before = code.active_world.map(|i| code.branches[i].clone());
    let mut docs: Vec<Obj> = code.branches.iter().map(|b| (**b).clone()).collect();
    let r = f(&mut docs);
    let active = |k: &str| docs.iter().position(|b| matches!(field(b, k), Some(Val::Bool(true))));
    let (aw, asim) = (active("activeWorld"), active("activeSim"));
    let changed = aw.map(|i| &docs[i]) != before.as_deref();
    let next = CodeState { active_world: aw, active_sim: asim, version: code.version + u64::from(changed), branches: docs.into_iter().map(Arc::new).collect() };
    edit_user(w, u, |st| st.code = Arc::new(next));
    r
}

/// socket/user.js `user:<id>/code` payload: `{branch, modules (decoded), timestamp, hash}`.
fn code_event(doc: &Obj, hash: Option<&Json>) -> String {
    let mut m = vec![];
    for k in ["branch", "modules", "timestamp"] {
        if let Some(v) = field(doc, k) {
            let v = if k == "modules" { modules_from_db(v.to_json()) } else { v.to_json() };
            m.push((k, v));
        }
    }
    if let Some(h) = hash {
        m.push(("hash", h.clone()));
    }
    obj(m).stringify()
}

fn emit(ctx: &ApiCtx, u: UserIdx, channel: &str, data: &str) {
    let id = ctx.published.get().users[u as usize].id;
    ctx.published.hub().emit(&format!("user:{id}/{channel}"), data);
}

// ---- routes ----

/// GET /api/user/world-start-room
pub fn world_start_room(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let controllers: Vec<&DocRef> = user_objects(&view, u).into_iter().filter(|d| d.kind() == "controller").collect();
    let room = if controllers.is_empty() {
        // 'W5N5', or the first room when the world has none (UI only)
        if crate::room_idx(&view, "W5N5").is_some() { "W5N5".to_string() } else { view.meta.rooms.iter().find(|r| !r.removed).map_or("W5N5".into(), |r| r.name.to_string()) }
    } else {
        let mut b = [0u8; 8];
        util::sys::os_random(&mut b);
        // _.shuffle(controllers)[0] is a uniform pick (Math.random)
        str_field(&controllers[(u64::from_le_bytes(b) % controllers.len() as u64) as usize].obj, "room").unwrap_or("").to_string()
    };
    Response::ok(obj(vec![("room", Json::Arr(vec![Json::Str(room)]))]))
}

/// utils.getUserWorldStatus.
pub(crate) fn world_status_of(view: &WorldView, u: UserIdx) -> &'static str {
    let objects = user_objects(view, u);
    if objects.is_empty() {
        return "empty";
    }
    let own: Vec<&&DocRef> = objects.iter().filter(|d| matches!(d.kind(), "spawn" | "controller")).collect();
    let normal = own.iter().any(|sp| {
        sp.kind() == "spawn" && own.iter().any(|c| c.kind() == "controller" && str_field(&c.obj, "room") == str_field(&sp.obj, "room") && str_field(&c.obj, "user") == str_field(&sp.obj, "user"))
    });
    if normal { "normal" } else { "lost" }
}

/// GET /api/user/world-status
pub fn world_status(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    Response::ok(obj(vec![("status", Json::Str(world_status_of(&ctx.published.get(), u).into()))]))
}

/// GET /api/user/branches
pub fn branches(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let has_default = |v: &WorldView| v.users[u as usize].code.branches.iter().any(|b| str_field(b, "branch") == Some("default"));
    if !has_default(&ctx.published.get()) {
        let r = write(ctx, move |w| {
            if !w.users[u as usize].code.branches.iter().any(|b| str_field(b, "branch") == Some("default")) {
                let id = w.users[u as usize].id;
                let mut c = Obj::new();
                obj_set(&mut c, "user", s(id.as_str()));
                obj_set(&mut c, "branch", s("default"));
                obj_set(&mut c, "modules", Val::from_trusted_json(&Json::parse(r#"{"main":""}"#).unwrap()));
                obj_set(&mut c, "timestamp", s(&iso_date(vnow(w))));
                let cid = w.ids.next();
                obj_set(&mut c, "_id", s(cid.as_str()));
                edit_code(w, u, |docs| docs.push(c));
            }
            Ok(())
        });
        if let Err(e) = r {
            return Response::error(&e);
        }
    }
    let view = ctx.published.get();
    let list = view.users[u as usize]
        .code
        .branches
        .iter()
        .map(|b| match b.to_json_obj() {
            Json::Obj(mut m) => {
                if let Some(slot) = m.iter_mut().find(|(k, _)| k == "modules") {
                    slot.1 = modules_from_db(std::mem::replace(&mut slot.1, Json::Null));
                }
                Json::Obj(m)
            }
            j => j,
        })
        .collect();
    Response::ok(obj(vec![("list", Json::Arr(list))]))
}

trait ToJsonObj {
    fn to_json_obj(&self) -> Json;
}

impl ToJsonObj for Obj {
    fn to_json_obj(&self) -> Json {
        Json::Obj(self.iter().map(|(k, v)| (k.as_str().to_string(), v.to_json())).collect())
    }
}

/// POST /api/user/code
pub fn code_post(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let Some(modules) = b.get("modules").cloned() else {
        return Response::error("TypeError: Cannot read properties of undefined (reading 'length')");
    };
    let modules = match checked_modules(modules) {
        Ok(m) => m,
        Err(e) => return Response::error(&e),
    };
    // `branch || '$activeWorld'`; a truthy non-string is queried as `{branch: value}` and matches no stored name
    let branch: Option<String> = match b.get("branch") {
        v if !v.is_some_and(json_truthy) => Some("$activeWorld".to_string()),
        Some(Json::Str(x)) => Some(x.clone()),
        _ => None,
    };
    let hash = b.get("_hash").cloned();
    let r = write(ctx, move |w| {
        edit_user(w, u, |st| {
            let mut d = (*st.doc).clone();
            obj_set(&mut d, "active", Val::Num(10000.0));
            st.doc = Arc::new(d);
        });
        let now = vnow(w);
        let first = edit_code(w, u, |docs| {
            let mut first = None;
            for d in docs.iter_mut().filter(|d| branch.as_deref().is_some_and(|br| code_matches(d, br))) {
                obj_set(d, "modules", modules.clone());
                obj_set(d, "timestamp", Val::Num(now as f64));
                first.get_or_insert_with(|| d.clone());
            }
            first
        });
        let first = first.ok_or("branch does not exist")?;
        Ok((code_event(&first, hash.as_ref()), now))
    });
    match r {
        Ok((event, now)) => {
            emit(ctx, u, "code", &event);
            Response::ok(obj(vec![("timestamp", Json::Num(now as f64))]))
        }
        Err(e) => Response::error(&e),
    }
}

/// GET /api/user/code
pub fn code_get(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let branch = req.param("branch").filter(|b| !b.is_empty()).unwrap_or("$activeWorld");
    let view = ctx.published.get();
    match view.users[u as usize].code.branches.iter().find(|d| code_matches(d, branch)) {
        None => Response::error("no code"),
        Some(d) => Response::ok(obj(vec![
            ("branch", field(d, "branch").map_or(Json::Null, Val::to_json)),
            ("modules", modules_from_db(field(d, "modules").map_or(Json::Null, Val::to_json))),
        ])),
    }
}

/// `+v` (ToNumber) of a JSON value.
pub(crate) fn to_number(v: Option<&Json>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Json::Null) => 0.0,
        Some(Json::Bool(b)) => f64::from(u8::from(*b)),
        Some(Json::Num(n)) => *n,
        Some(Json::Str(s)) => util::num::string_to_number(s),
        Some(a @ Json::Arr(_)) => util::num::string_to_number(&crate::socket::user::js_to_string(Some(a))),
        Some(Json::Obj(_)) => f64::NAN,
    }
}

/// `String(v)` for regex tests (`undefined` → "undefined").
fn to_string(v: Option<&Json>) -> String {
    crate::socket::user::js_to_string(v)
}

/// `/^#[a-f0-9]{6}$/i.test(v)`, on a string. The original has no `$`, and `badge-svg` writes the colour into the SVG, so
/// anything after the six digits was stored script.
fn color_ok(v: Option<&Json>) -> bool {
    v.and_then(Json::as_str).is_some_and(|s| s.len() == 7 && crate::api::badge::safe_color(s))
}

/// `_.isEqual` of two JSON values. Not through `Val`: one side is a request's, and that would keep its keys as names, so
/// one refused request could spend all the server will take. `y` is a stored badge, so the scan is short.
fn same_json(a: Option<&Json>, b: Option<&Json>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Json::Arr(x)), Some(Json::Arr(y))) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_json(Some(p), Some(q))),
        (Some(Json::Obj(x)), Some(Json::Obj(y))) => x.len() == y.len() && x.iter().all(|(k, v)| same_json(Some(v), y.iter().find(|(q, _)| q == k).map(|(_, w)| w))),
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// POST /api/user/badge
pub fn badge(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let body = req.body();
    let Some(badge @ (Json::Obj(_) | Json::Arr(_))) = body.get("badge") else { return Response::error("invalid params") };
    let param = to_number(badge.get("param"));
    if param.is_nan() || param < -100.0 || param > 100.0 || !color_ok(badge.get("color1")) || !color_ok(badge.get("color2")) || !color_ok(badge.get("color3")) {
        return Response::error("invalid params");
    }
    let ty = badge.get("type");
    if let Some(&Json::Num(t)) = ty {
        if t < 1.0 || t > 24.0 {
            return Response::error("invalid params");
        }
    } else {
        let custom = field(&ctx.published.get().users[u as usize].doc, "customBadge").map(Val::to_json);
        if !same_json(ty, custom.as_ref()) {
            return Response::error("invalid params");
        }
    }
    // From here `type` is a number or the user's own custom badge, and a colour is a string, so nothing new is named.
    let mut stored = Obj::new();
    let mut put = |k: &str, v: Option<&Json>| {
        if let Some(v) = v {
            obj_set(&mut stored, k, Val::from_json(v));
        }
    };
    put("type", ty);
    put("color1", badge.get("color1"));
    put("color2", badge.get("color2"));
    put("color3", badge.get("color3"));
    obj_set(&mut stored, "param", Val::Num(if param == 0.0 { 0.0 } else { param }));
    obj_set(&mut stored, "flip", Val::Bool(badge.get("flip").is_some_and(json_truthy)));
    reply(
        write(ctx, move |w| {
            edit_user(w, u, |st| {
                let mut d = (*st.doc).clone();
                obj_set(&mut d, "badge", Val::Obj(stored));
                st.doc = Arc::new(d);
            });
            Ok(())
        }),
        |_| obj(vec![("modified", Json::Num(1.0))]),
    )
}

/// GET /api/user/respawn-prohibited-rooms
pub fn respawn_prohibited_rooms(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    Response::ok(obj(vec![("rooms", Json::Arr(vec![]))]))
}

/// POST /api/user/respawn
pub fn respawn(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    if !matches!(world_status_of(&ctx.published.get(), u), "normal" | "lost") {
        return Response::error("invalid status");
    }
    reply(write(ctx, move |w| respawn_user(w, u)), |_| obj(vec![("modified", Json::Num(1.0))]))
}

/// utils.respawnUser (GEN-RESPAWN): creeps, power creeps and sites go, owned structures become the system
/// user's ruins, other owned objects and flags go, every room's status becomes 'normal' (the un-awaited loop,
/// see below), controllers are released, `rooms` is emptied. Rooms are not activated.
pub fn respawn_user(w: &mut World, u: UserIdx) -> Result<(), String> {
    // users.findOne({username: C.SYSTEM_USERNAME}): exact match, first in creation order
    let system = w.users.iter().position(|s| str_field(&s.doc, "username") == Some(engine::constants::SYSTEM_USERNAME)).ok_or("no system user")?;
    let sys = w.users[system].id.as_str().to_string();
    let uid = w.users[u as usize].id.as_str().to_string();
    let game_time = w.time as f64;
    let structure_types: Vec<&str> = engine::constants::CONSTRUCTION_COST.iter().map(|(t, _)| t.as_str()).collect();
    let rooms: Vec<RoomIdx> = w.index.presence.get(u as usize).map(|p| p.keys().copied().collect()).unwrap_or_default();
    // {user, type: {$in: CONSTRUCTION_COST keys}}: the user index, creation order across rooms
    let mut owned: Vec<(RoomIdx, DocRef)> = rooms
        .iter()
        .flat_map(|&r| w.rooms[r as usize].objects.iter().map(move |d| (r, d.clone())))
        .filter(|(_, d)| str_field(&d.obj, "user") == Some(uid.as_str()))
        .collect();
    owned.sort_by_key(|(_, d)| d.seq);
    let mut ruins: Vec<(RoomIdx, Obj)> = Vec::new();
    for (r, d) in owned.iter().filter(|(_, d)| structure_types.contains(&d.kind())) {
        let o = &d.obj;
        let mut ruin = Obj::new();
        obj_set(&mut ruin, "type", s("ruin"));
        obj_set(&mut ruin, "user", s(&sys));
        for k in ["room", "x", "y"] {
            if let Some(v) = field(o, k) {
                obj_set(&mut ruin, k, v.clone());
            }
        }
        let mut st = Obj::new();
        obj_set(&mut st, "id", s(d.id().as_str()));
        obj_set(&mut st, "type", s(d.kind()));
        obj_set(&mut st, "hits", Val::Num(0.0));
        if let Some(h) = field(o, "hitsMax") {
            obj_set(&mut st, "hitsMax", h.clone());
        }
        obj_set(&mut st, "user", s(&sys));
        obj_set(&mut ruin, "structure", Val::Obj(st));
        obj_set(&mut ruin, "store", field(o, "store").filter(|v| truthy(Some(v))).cloned().unwrap_or(Val::Obj(Obj::new())));
        obj_set(&mut ruin, "destroyTime", Val::Num(game_time));
        obj_set(&mut ruin, "decayTime", Val::Num(game_time + 500000.0));
        ruins.push((*r, ruin));
    }
    let mut new_docs: Vec<(RoomIdx, DocRef)> = Vec::new();
    for (r, ruin) in ruins {
        new_docs.push((r, crate::new_doc(w, ruin)));
    }
    let mut touched: Vec<RoomIdx> = rooms.clone();
    touched.extend(new_docs.iter().map(|(r, _)| *r));
    touched.sort_unstable();
    touched.dedup();
    for r in touched {
        let mut objs: Vec<DocRef> = Vec::new();
        for d in w.rooms[r as usize].objects.iter() {
            let mine = str_field(&d.obj, "user") == Some(uid.as_str());
            match (mine, d.kind()) {
                (false, _) => objs.push(d.clone()),
                // existing ruins change hands; everything else but controllers goes
                (true, "ruin") => {
                    let mut o = d.obj.clone();
                    obj_set(&mut o, "user", s(&sys));
                    objs.push(Doc::new(d.seq, o));
                }
                (true, "controller") => {
                    let mut o = d.obj.clone();
                    for (k, v) in [("level", Val::Num(0.0)), ("hits", Val::Num(0.0)), ("hitsMax", Val::Num(0.0)), ("progress", Val::Num(0.0)), ("progressTotal", Val::Num(0.0)), ("user", Val::Null), ("downgradeTime", Val::Null), ("safeMode", Val::Null), ("safeModeAvailable", Val::Num(0.0)), ("safeModeCooldown", Val::Null), ("isPowerEnabled", Val::Bool(false))] {
                        obj_set(&mut o, k, v);
                    }
                    objs.push(Doc::new(d.seq, o));
                }
                (true, _) => {}
            }
        }
        objs.extend(new_docs.iter().filter(|(x, _)| *x == r).map(|(_, d)| d.clone()));
        let flags: Vec<Arc<Obj>> = w.rooms[r as usize].flags.iter().filter(|f| str_field(f, "user") != Some(uid.as_str())).cloned().collect();
        crate::install_room(w, r, objs, Some(flags.into()), None);
    }
    // flags in rooms without objects of the user
    for r in 0..w.rooms.len() as RoomIdx {
        if w.rooms[r as usize].flags.iter().any(|f| str_field(f, "user") == Some(uid.as_str())) {
            let objs = w.rooms[r as usize].objects.to_vec();
            let flags: Vec<Arc<Obj>> = w.rooms[r as usize].flags.iter().filter(|f| str_field(f, "user") != Some(uid.as_str())).cloned().collect();
            crate::install_room(w, r, objs, Some(flags.into()), None);
        }
    }
    // utils.js:147-150: `controllers` is the un-awaited storage promise, a Q promise whose 52 prototype methods are
    // enumerable, so `for (i in controllers)` runs `rooms.update({_id: controllers[i].room}, {$set: {status:
    // 'normal'}})` with `_id: undefined`; the RPC's JSON drops it and every room gets status 'normal' (the
    // out-of-borders ones included). Spec 11 GEN-RESPAWN's "does nothing" assumed a native Promise.
    let mut reopened = false;
    for r in 0..w.rooms.len() as RoomIdx {
        if w.meta.rooms[r as usize].removed || str_field(&w.rooms[r as usize].info, "status") == Some("normal") {
            continue;
        }
        let mut info = (*w.rooms[r as usize].info).clone();
        obj_set(&mut info, "status", s("normal"));
        let objs = w.rooms[r as usize].objects.to_vec();
        crate::install_room(w, r, objs, None, Some(Arc::new(info)));
        reopened = true;
    }
    if reopened {
        let e = &w.env;
        w.env = Arc::new(engine::world::EnvState { paused: e.paused, tick_rate_ms: e.tick_rate_ms, last_accessible_update: e.last_accessible_update, status_dirty: true });
    }
    edit_user(w, u, |st| {
        st.power_creeps = Arc::from(Vec::new());
        let mut d = (*st.doc).clone();
        obj_set(&mut d, "rooms", Val::Arr(vec![]));
        st.doc = Arc::new(d);
    });
    Ok(())
}

/// POST /api/user/set-active-branch
pub fn set_active_branch(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let active = match b.get("activeName").and_then(Json::as_str) {
        Some(a @ ("activeWorld" | "activeSim")) => a.to_string(),
        _ => return Response::error("invalid params"),
    };
    let branch = b.get("branch").cloned();
    let (a2, b2) = (active.clone(), branch.clone());
    let r = write(ctx, move |w| {
        edit_code(w, u, |docs| {
            if !docs.iter().any(|d| clause(d, "branch", b2.as_ref())) {
                return Err("no branch".to_string());
            }
            // one atomic switch (the original's two updates could leave no active branch for a tick)
            for d in docs.iter_mut() {
                let on = clause(d, "branch", b2.as_ref());
                obj_set(d, &a2, Val::Bool(on));
            }
            Ok(())
        })
    });
    match r {
        Ok(()) => {
            let mut ev = vec![("activeName", Json::Str(active))];
            if let Some(b) = branch {
                ev.push(("branch", b));
            }
            emit(ctx, u, "set-active-branch", &obj(ev).stringify());
            Response::ok(Json::Null)
        }
        Err(e) => Response::error(&e),
    }
}

/// A Loki `{k: v}` clause as the storage process receives it: the RPC's JSON drops an undefined `v`, which then
/// matches every doc; otherwise `$eq` is `===` (objects never match a copy).
pub(crate) fn clause(doc: &Obj, k: &str, v: Option<&Json>) -> bool {
    match v {
        None => true,
        Some(Json::Obj(_) | Json::Arr(_)) => false,
        Some(v) => field(doc, k).map(Val::to_json).as_ref() == Some(v),
    }
}

/// `/^tutorial-\d$/`
fn tutorial_branch(name: &str) -> bool {
    name.strip_prefix("tutorial-").is_some_and(|d| d.len() == 1 && d.as_bytes()[0].is_ascii_digit())
}

/// POST /api/user/clone-branch
pub fn clone_branch(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let source = b.get("branch").filter(|v| json_truthy(v)).cloned();
    if let Some(src) = &source {
        match src.as_str() {
            Some(x) if x.encode_utf16().count() <= 30 => {}
            _ => return Response::error("invalid branch name"),
        }
    }
    let defaults = match b.get("defaultModules").filter(|v| json_truthy(v)).map(|m| checked_modules(m.clone())).transpose() {
        Ok(m) => m,
        Err(e) => return Response::error(&e),
    };
    let new_name = b.get("newName").cloned();
    if matches!(new_name, Some(Json::Obj(_) | Json::Arr(_))) {
        return Response::error("invalid branch name");
    }
    let r = write(ctx, move |w| {
        let id = w.users[u as usize].id;
        let now = vnow(w);
        let cid = w.ids.next();
        let doc = edit_code(w, u, |docs| {
            let exists = docs.iter().any(|d| clause(d, "branch", new_name.as_ref()));
            let tutorial = tutorial_branch(&crate::socket::user::js_to_string(new_name.as_ref()));
            if !exists && !tutorial && docs.len() >= 30 {
                return Err("too many branches".to_string());
            }
            let src_modules = source.as_ref().and_then(|s| docs.iter().find(|d| str_field(d, "branch") == s.as_str())).and_then(|d| field(d, "modules").cloned());
            let modules = src_modules.or(defaults).unwrap_or_else(|| Val::from_trusted_json(&Json::parse(r#"{"main":""}"#).unwrap()));
            let stamp = s(&iso_date(now));
            // update(..., {upsert: true}): every match (all branches when newName is undefined), else insert
            let mut first = None;
            for d in docs.iter_mut().filter(|d| clause(d, "branch", new_name.as_ref())) {
                obj_set(d, "modules", modules.clone());
                obj_set(d, "timestamp", stamp.clone());
                first.get_or_insert_with(|| d.clone());
            }
            if let Some(d) = first {
                return Ok(d);
            }
            let mut d = Obj::new();
            obj_set(&mut d, "user", s(id.as_str()));
            if let Some(n) = &new_name {
                obj_set(&mut d, "branch", Val::from_json(n));
            }
            obj_set(&mut d, "modules", modules);
            obj_set(&mut d, "timestamp", stamp);
            obj_set(&mut d, "_id", s(cid.as_str()));
            docs.push(d.clone());
            Ok(d)
        })?;
        Ok((code_event(&doc, None), now))
    });
    match r {
        Ok((event, now)) => {
            emit(ctx, u, "code", &event);
            Response::ok(obj(vec![("timestamp", Json::Num(now as f64))]))
        }
        Err(e) => Response::error(&e),
    }
}

/// POST /api/user/delete-branch
pub fn delete_branch(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let branch = req.body().get("branch").cloned();
    reply(
        write(ctx, move |w| {
            let now = vnow(w);
            edit_code(w, u, |docs| docs.retain(|d| !(clause(d, "branch", branch.as_ref()) && field(d, "activeWorld") != Some(&Val::Bool(true)) && field(d, "activeSim") != Some(&Val::Bool(true)))));
            Ok(now)
        }),
        |now| obj(vec![("timestamp", Json::Num(now as f64))]),
    )
}

/// GET /api/user/memory
pub fn memory_get(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let text = view.users[u as usize].memory.to_string_lossy();
    let data = (|| {
        let memory = Json::parse(if text.is_empty() { "{}" } else { &text }).map_err(|_| ())?;
        let cur = match req.param("path").filter(|p| !p.is_empty()) {
            Some(path) => crate::socket::user::walk(&memory, path)?,
            None => Some(Cow::Borrowed(&memory)),
        };
        Ok::<_, ()>(cur.map(|v| format!("gz:{}", util::base64::encode(&util::deflate::gzip_compress(v.stringify().as_bytes())))))
    })();
    match data {
        Ok(None) => Response::ok(Json::Null),
        Ok(Some(gz)) => Response::ok(obj(vec![("data", Json::Str(gz))])),
        Err(()) => Response::ok(obj(vec![("data", Json::Str("Incorrect memory path".into()))])),
    }
}

/// The hidden console expression of POST /api/user/memory.
pub(crate) fn memory_expression(path: Option<&str>, value: Option<&Json>) -> String {
    let s = value.map(Json::stringify);
    match path {
        Some(p) => {
            let parts: String = p.split('.').map(|i| format!("['{}']", i.replace('\'', "\\'"))).collect();
            match &s {
                Some(v) => format!("Memory{parts}= {v}"),
                None => format!("delete Memory{parts}"),
            }
        }
        None => format!("for(var i in Memory) {{ delete Memory[i]; }} _.extend(Memory,{});", s.as_deref().unwrap_or("undefined")),
    }
}

/// Inserts a `users.console` doc (newest first, ORDER-CONSOLE-COMMANDS) and returns it.
fn insert_console(ctx: &ApiCtx, u: UserIdx, expression: Val, hidden: bool) -> Response {
    let r = write(ctx, move |w| {
        let mut d = Obj::new();
        obj_set(&mut d, "user", s(w.users[u as usize].id.as_str()));
        obj_set(&mut d, "expression", expression);
        if hidden {
            obj_set(&mut d, "hidden", Val::Bool(true));
        }
        // A command that is not a string is at most 1024 long (`console`).
        let size = |d: &Obj| str_field(d, "expression").map_or(1024, str::len);
        let queue = &w.users[u as usize].console_queue;
        if queue.len() >= MAX_CONSOLE_COMMANDS || queue.iter().map(|c| size(c)).sum::<usize>() + size(&d) > MAX_CONSOLE_BYTES {
            return Err("console queue is full".to_string());
        }
        let id = w.ids.next();
        obj_set(&mut d, "_id", s(id.as_str()));
        let out = d.to_json_obj();
        edit_user(w, u, |st| {
            let mut q: Vec<Arc<Obj>> = vec![Arc::new(d)];
            q.extend(st.console_queue.iter().cloned());
            st.console_queue = q.into();
        });
        Ok(out)
    });
    reply(r, |d| d)
}

/// POST /api/user/memory
pub fn memory_post(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let value = b.get("value");
    if value.is_some_and(|v| v.stringify().encode_utf16().count() > 1024 * 1024) {
        return Response::error("memory size is too large");
    }
    let path = match b.get("path") {
        Some(Json::Str(p)) if !p.is_empty() => Some(p.as_str()),
        Some(v) if json_truthy(v) => return Response::error("TypeError: request.body.path.split is not a function"),
        _ => None,
    };
    insert_console(ctx, u, s(&memory_expression(path, value)), true)
}

/// `parseInt(v)` of a query/body value; NaN when it does not start with a number.
fn parse_int(v: Option<&Json>) -> f64 {
    let s = to_string(v);
    let t = s.trim_start_matches(util::num::is_js_whitespace);
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    let n: String = digits.chars().take_while(char::is_ascii_digit).collect();
    if n.is_empty() {
        return f64::NAN;
    }
    let x: f64 = n.parse().unwrap_or(f64::NAN);
    if neg { -x } else { x }
}

pub(crate) fn segment_id(v: Option<&Json>) -> Option<u8> {
    let id = parse_int(v);
    (!id.is_nan() && (0.0..=99.0).contains(&id)).then_some(id as u8)
}

/// GET /api/user/memory-segment
pub fn memory_segment_get(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let Some(id) = segment_id(req.param("segment").map(|s| Json::Str(s.into())).as_ref()) else { return Response::error("invalid segment ID") };
    match ctx.published.get().users[u as usize].segments.get(&id) {
        Some(d) => Response::ok(obj(vec![("data", Json::Str(d.to_string_lossy()))])),
        None => Response::ok(Json::Null),
    }
}

/// POST /api/user/memory-segment
pub fn memory_segment_post(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let Some(id) = segment_id(b.get("segment")) else { return Response::error("invalid segment ID") };
    let data = to_string(b.get("data"));
    if data.encode_utf16().count() > 100 * 1024 {
        return Response::error("length limit exceeded");
    }
    // `hset(key, id, undefined)` stores undefined: every reader then sees no segment
    let data = b.get("data").is_some().then_some(data);
    reply(
        write(ctx, move |w| {
            edit_user(w, u, |st| {
                let mut m = (*st.segments).clone();
                match &data {
                    Some(d) => m.insert(id, util::jsstr::JsStr::from_str(d)),
                    None => m.remove(&id),
                };
                st.segments = Arc::new(m);
            });
            Ok(())
        }),
        |_| Json::Null,
    )
}

/// POST /api/user/console
pub fn console(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let Some(expr) = b.get("expression") else { return Response::error("TypeError: Cannot read properties of undefined (reading 'length')") };
    if expr.stringify().encode_utf16().count() > 1024 {
        return Response::error("expression size is too large");
    }
    if matches!(expr, Json::Obj(_) | Json::Arr(_)) {
        return Response::error("invalid params");
    }
    insert_console(ctx, u, Val::from_json(expr), false)
}

/// GET /api/user/find
pub fn find(req: &Request, ctx: &ApiCtx) -> Response {
    let view = ctx.published.get();
    let by_name = req.param("username").filter(|v| !v.is_empty()).map(|n| view.usernames.get(n.to_lowercase().as_str()).copied());
    let by_id = req.param("id").filter(|v| !v.is_empty()).map(|i| crate::user_by_id(&view, i));
    let u = match (by_id, by_name) {
        (Some(x), _) => x,
        (None, Some(x)) => x,
        (None, None) => (!view.users.is_empty()).then_some(0),
    };
    let Some(u) = u else { return Response::error("TypeError: Cannot read properties of null (reading 'steam')") };
    let st = &view.users[u as usize];
    let mut out: Vec<(&str, Json)> = vec![("_id", Json::Str(st.id.as_str().into()))];
    for k in ["username", "badge", "gcl", "power"] {
        if let Some(v) = field(&st.doc, k) {
            out.push((k, v.to_json()));
        }
    }
    // `if (user.steam && !user.steam.steamProfileLinkHidden) result.steam = {id: user.steam.id}` (undefined id: `{}`)
    if let Some(steam) = field(&st.doc, "steam").filter(|v| truthy(Some(v))) {
        let o = steam.as_obj();
        if !truthy(o.and_then(|o| field(o, "steamProfileLinkHidden"))) {
            out.push(("steam", Json::Obj(o.and_then(|o| field(o, "id")).map(|v| vec![("id".to_string(), v.to_json())]).unwrap_or_default())));
        }
    }
    Response::ok(obj(vec![("user", obj(out))]))
}

/// GET /api/user/stats
pub fn stats(req: &Request, ctx: &ApiCtx) -> Response {
    let i = to_number(req.param("interval").map(|s| Json::Str(s.into())).as_ref());
    let i32v = if i.is_finite() { i.trunc() as i64 as i32 } else { 0 };
    if ![8, 180, 1440].contains(&i32v) {
        return Response::error("invalid params");
    }
    Response::ok(obj(vec![("stats", Json::Obj(vec![]))]))
}

/// Rooms of `id`'s controllers, sorted by level descending (stable); `None` matches every controller (the RPC drops
/// `{user: undefined}`).
fn controller_rooms(view: &WorldView, id: Option<&str>) -> Vec<Json> {
    let q = id.map(|i| Json::Str(i.into()));
    let mut list: Vec<(&DocRef, f64)> = view
        .rooms
        .iter()
        .flat_map(|r| r.objects.iter())
        .filter(|d| d.kind() == "controller" && clause(&d.obj, "user", q.as_ref()))
        .map(|d| (d, field(&d.obj, "level").and_then(Val::as_f64).unwrap_or(f64::NAN)))
        .collect();
    list.sort_by_key(|(d, _)| d.seq);
    list.sort_by(|a, b| {
        let x = b.1 - a.1;
        if x > 0.0 { std::cmp::Ordering::Greater } else if x < 0.0 { std::cmp::Ordering::Less } else { std::cmp::Ordering::Equal }
    });
    list.into_iter().map(|(d, _)| field(&d.obj, "room").map_or(Json::Null, Val::to_json)).collect()
}

/// GET /api/user/rooms
pub fn rooms(req: &Request, ctx: &ApiCtx) -> Response {
    let view = ctx.published.get();
    Response::ok(obj(vec![("rooms", Json::Arr(controller_rooms(&view, req.param("id"))))]))
}

/// POST /api/user/notify-prefs
pub fn notify_prefs(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let mut prefs = match field(&ctx.published.get().users[u as usize].doc, "notifyPrefs") {
        Some(Val::Obj(o)) => o.clone(),
        _ => Obj::new(),
    };
    for k in ["disabled", "disabledOnMessages", "sendOnline"] {
        if let Some(v) = b.get(k) {
            obj_set(&mut prefs, k, Val::Bool(json_truthy(v)));
        }
    }
    let interval = to_number(b.get("interval"));
    if [5.0, 10.0, 30.0, 60.0, 180.0, 360.0, 720.0, 1440.0, 4320.0].contains(&interval) {
        obj_set(&mut prefs, "interval", Val::Num(interval));
    }
    let errors = to_number(b.get("errorsInterval"));
    if [0.0, 5.0, 10.0, 30.0, 60.0, 180.0, 360.0, 720.0, 1440.0, 4320.0, 100000.0].contains(&errors) {
        obj_set(&mut prefs, "errorsInterval", Val::Num(errors));
    }
    reply(
        write(ctx, move |w| {
            edit_user(w, u, |st| {
                let mut d = (*st.doc).clone();
                obj_set(&mut d, "notifyPrefs", Val::Obj(prefs));
                st.doc = Arc::new(d);
            });
            Ok(())
        }),
        |_| obj(vec![("modified", Json::Num(1.0))]),
    )
}

/// GET /api/user/overview
pub fn overview(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let rooms = controller_rooms(&view, Some(view.users[u as usize].id.as_str()));
    Response::ok(obj(vec![("rooms", Json::Arr(rooms)), ("stats", Json::Obj(vec![])), ("totals", Json::Obj(vec![])), ("gametimes", Json::Arr(vec![]))]))
}

/// POST /api/user/tutorial-done
pub fn tutorial_done(req: &Request, ctx: &ApiCtx) -> Response {
    let _ = auth!(ctx);
    Response::ok(Json::Null)
}

/// POST /api/user/email
pub fn email(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let raw = to_string(req.body().get("email"));
    if !crate::api::register::valid_email(&raw) {
        return Response::error("invalid email");
    }
    let email = raw.to_lowercase();
    reply(
        write(ctx, move |w| {
            if w.users.iter().any(|x| str_field(&x.doc, "email") == Some(email.as_str())) {
                return Err("email already exists".into());
            }
            edit_user(w, u, |st| {
                let mut d = (*st.doc).clone();
                obj_set(&mut d, "email", s(&email));
                st.doc = Arc::new(d);
            });
            Ok(())
        }),
        |_| obj(vec![("modified", Json::Num(1.0))]),
    )
}

/// GET /api/user/money-history
pub fn money_history(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let id = view.users[u as usize].id;
    let page = parse_int(req.param("page").map(|s| Json::Str(s.into())).as_ref());
    let page = if page.is_nan() || page == 0.0 { 0 } else { page as i64 };
    let mut list: Vec<&Arc<Obj>> = view.market.money.iter().filter(|d| str_field(d, "user") == Some(id.as_str())).collect();
    let date = |d: &Obj| field(d, "date").map_or(Json::Null, Val::to_json);
    list.sort_by(|a, b| match (date(b), date(a)) {
        (Json::Num(x), Json::Num(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
        (x, y) => to_string(Some(&x)).cmp(&to_string(Some(&y))),
    });
    let rest: Vec<Json> = list.into_iter().skip((page.max(0) * 20) as usize).take(21).map(|d| d.to_json_obj()).collect();
    let has_more = rest.len() > 20;
    Response::ok(obj(vec![("page", Json::Num(page as f64)), ("list", Json::Arr(rest.into_iter().take(20).collect())), ("hasMore", Json::Bool(has_more))]))
}

/// Opened as a page, an SVG runs what is in it, on this server's origin. The client and the pages load it as an image,
/// which runs nothing and is not touched by these.
fn svg_reply(svg: &str) -> Response {
    let mut reply = Response::text(200, "image/svg+xml; charset=utf-8", svg);
    reply.headers.push(("Content-Security-Policy".into(), "sandbox".into()));
    reply.headers.push(("X-Content-Type-Options".into(), "nosniff".into()));
    reply
}

/// GET /api/user/badge-svg (not cached: the original's 1 h process cache only delays badge changes)
pub fn badge_svg(req: &Request, ctx: &ApiCtx) -> Response {
    let Some(name) = req.param("username") else {
        // a synchronous TypeError outside the promise chain: Express' 500 page
        return Response::text(500, "text/html; charset=utf-8", "TypeError: Cannot read properties of undefined (reading 'toLowerCase')");
    };
    let border = to_number(req.param("border").map(|b| Json::Str(b.into())).as_ref()) == 1.0;
    let view = ctx.published.get();
    let svg = view.usernames.get(name.to_lowercase().as_str()).and_then(|&u| match field(&view.users[u as usize].doc, "badge") {
        b if truthy(b) => crate::api::badge::get_badge_svg(&b.unwrap().to_json(), border),
        _ => Some("<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>".into()),
    });
    match svg {
        Some(svg) => svg_reply(&svg),
        None => Response::text(404, "text/plain; charset=utf-8", "Not Found"),
    }
}

/// POST /api/user/set-steam-visible
pub fn set_steam_visible(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let hidden = if req.body().get("visible").is_some_and(json_truthy) { 0.0 } else { 1.0 };
    reply(
        write(ctx, move |w| {
            edit_user(w, u, |st| {
                let mut d = (*st.doc).clone();
                let mut steam = match field(&d, "steam") {
                    Some(Val::Obj(o)) => o.clone(),
                    _ => Obj::new(),
                };
                obj_set(&mut steam, "steamProfileLinkHidden", Val::Num(hidden));
                obj_set(&mut d, "steam", Val::Obj(steam));
                st.doc = Arc::new(d);
            });
            Ok(())
        }),
        |_| obj(vec![("modified", Json::Num(1.0))]),
    )
}

/// GET /api/user/auth-token (STUB-AUTH-TOKENS: the account page lists persistent tokens; there are none)
pub fn auth_tokens(req: &Request, ctx: &ApiCtx) -> Response {
    Response::ok(obj(vec![("tokens", Json::Arr(vec![]))]))
}

/// GET /api/user/decorations/inventory (STUB-DECO-INVENTORY)
pub fn decorations_inventory(req: &Request, ctx: &ApiCtx) -> Response {
    Response::ok(obj(vec![("list", Json::Arr(vec![]))]))
}

/// GET /api/user/decorations/themes (STUB-DECO-THEMES)
pub fn decorations_themes(req: &Request, ctx: &ApiCtx) -> Response {
    Response::ok(obj(vec![("list", Json::Arr(vec![]))]))
}

#[allow(dead_code)]
fn _brief(v: &WorldView, u: UserIdx) -> Json {
    user_brief(v, u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    fn err(j: &Json) -> Option<&str> {
        j.get("error").and_then(Json::as_str)
    }

    /// node: utils.translateModulesToDb/FromDb on tests/fixtures/user/modules.json inputs.
    #[test]
    fn module_keys_match_node() {
        let fx = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/user/modules.json")).unwrap();
        for c in Json::parse(&fx).unwrap().as_array().unwrap() {
            let input = c.get("in").unwrap().clone();
            assert_eq!(&modules_to_db(input.clone()), c.get("toDb").unwrap(), "toDb {input}");
            assert_eq!(&modules_from_db(input.clone()), c.get("fromDb").unwrap(), "fromDb {input}");
        }
    }

    #[test]
    fn memory_get_matches_node() {
        let fx = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/user/memory.json")).unwrap();
        let fx = Json::parse(&fx).unwrap();
        let mem = Json::parse(fx.get("memory").unwrap().as_str().unwrap()).unwrap();
        for c in fx.get("get").unwrap().as_array().unwrap() {
            let (path, want) = (c.at(0).unwrap().as_str().unwrap(), c.at(1).unwrap());
            let got = if path.is_empty() { Ok(Some(Cow::Borrowed(&mem))) } else { crate::socket::user::walk(&mem, path) };
            match (got, want) {
                (Err(()), Json::Str(w)) => assert_eq!(w, "Incorrect memory path"),
                (Ok(None), Json::Null) => {}
                (Ok(Some(v)), Json::Str(w)) => assert_eq!(&Json::parse(w).unwrap(), v.as_ref(), "{path}"),
                (g, w) => panic!("{path}: {:?} vs {w}", g.map(|o| o.map(|v| v.stringify()))),
            }
        }
        // through the handler: gz payload decodes to the JSON text
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        let (_, j) = fake::run(memory_get, &ctx, "GET", "/x?path=a.b", "");
        let gz = j.get("data").and_then(Json::as_str).unwrap().strip_prefix("gz:").unwrap();
        let raw = util::deflate::gzip_decompress(&util::base64::decode(gz).unwrap(), 1 << 20).unwrap();
        assert_eq!(String::from_utf8(raw).unwrap(), r#"[1,{"c":null}]"#);
        assert_eq!(fake::run(memory_get, &ctx, "GET", "/x?path=zz", "").1.stringify(), r#"{"ok":1}"#);
        assert_eq!(err(&fake::run(memory_get, &ctx, "GET", "/x?path=zz.y", "").1), None);
        assert_eq!(fake::run(memory_get, &ctx, "GET", "/x?path=zz.y", "").1.get("data").and_then(Json::as_str), Some("Incorrect memory path"));
    }

    #[test]
    fn memory_post_expressions() {
        assert_eq!(memory_expression(Some("a.b'c"), Some(&Json::Num(5.0))), r#"Memory['a']['b\'c']= 5"#);
        assert_eq!(memory_expression(Some("x"), None), "delete Memory['x']");
        assert_eq!(memory_expression(None, Some(&Json::parse(r#"{"a":1}"#).unwrap())), r#"for(var i in Memory) { delete Memory[i]; } _.extend(Memory,{"a":1});"#);
        assert_eq!(memory_expression(None, None), "for(var i in Memory) { delete Memory[i]; } _.extend(Memory,undefined);");
        let (ctx, h) = fake::ctx(Some(fake::ALICE));
        let (_, j) = fake::run(memory_post, &ctx, "POST", "/x", r#"{"path":"a","value":[1]}"#);
        let keys: Vec<&str> = j.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["ok", "user", "expression", "hidden", "_id"]);
        let (_, j2) = fake::run(console, &ctx, "POST", "/x", r#"{"expression":"Game.time"}"#);
        assert_eq!(j2.get("expression").and_then(Json::as_str), Some("Game.time"));
        assert!(j2.get("hidden").is_none());
        let long = format!(r#"{{"expression":"{}"}}"#, "x".repeat(1023));
        assert_eq!(err(&fake::run(console, &ctx, "POST", "/x", &long).1), Some("expression size is too large"));
        drop(ctx);
        let w = h.join().unwrap();
        let q = &w.users[fake::ALICE as usize].console_queue;
        assert_eq!(str_field(&q[0], "expression"), Some("Game.time"));
        assert_eq!(str_field(&q[1], "expression"), Some("Memory['a']= [1]"));
    }

    #[test]
    fn module_uploads_have_limits() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        let names = |n: usize| (0..n).map(|i| format!(r#""lim{i}":"""#)).collect::<Vec<_>>().join(",");
        let upload = |modules: String| fake::run(code_post, &ctx, "POST", "/x", &format!(r#"{{"branch":"default","modules":{{{modules}}}}}"#)).1;
        assert_eq!(err(&upload(names(MAX_MODULES + 1))), Some("too many modules"));
        assert_eq!(err(&upload(names(MAX_MODULES - 1))), None);
        assert_eq!(err(&upload(format!(r#""{}":"""#, "n".repeat(OUTSIDE_NAME_BYTES + 1)))), Some("module name is too long"));
        assert_eq!(err(&upload(format!(r#""{}":"""#, "n".repeat(OUTSIDE_NAME_BYTES)))), None);

        // defaultModules skipped these, so a branch could hold any amount of code.
        let clone = |modules: String| fake::run(clone_branch, &ctx, "POST", "/x", &format!(r#"{{"newName":"lim","defaultModules":{{{modules}}}}}"#)).1;
        let too_long = format!(r#""main":"{}""#, "x".repeat(5 * 1024 * 1024));
        assert_eq!(err(&clone(too_long)), Some("code length exceeds 5 MB limit"));
        assert_eq!(err(&clone(names(MAX_MODULES + 1))), Some("too many modules"));
        assert_eq!(err(&clone(r#""main":"x""#.into())), None);
    }

    /// The rule mods use for code that comes to them, and what the routes refuse.
    #[test]
    fn a_module_is_text_or_binary() {
        let problem = |modules: &str| module_problem(&Json::parse(modules).unwrap());
        for fine in [r#"{"main":"x"}"#, r#"{"main":"x","w":{"binary":"AAAA"}}"#, r#"{"main":""}"#, "{}", "5", "null", "[1]"] {
            assert_eq!(problem(fine), None, "{fine}");
        }
        let shape = Some("a module is text, or an object holding only text in binary".to_string());
        for bad in [r#"{"a":{}}"#, r#"{"a":{"k":"x"}}"#, r#"{"a":{"binary":"x","k":"y"}}"#, r#"{"a":{"binary":1}}"#, r#"{"a":{"binary":{"k":1}}}"#, r#"{"a":[{"k":1}]}"#, r#"{"a":[]}"#, r#"{"a":1}"#, r#"{"a":true}"#, r#"{"main":null}"#] {
            assert_eq!(problem(bad), shape, "{bad}");
        }
        // the length of the stored name, in which a dot is five bytes
        let dots = |n: usize| format!(r#"{{"{}":""}}"#, ".".repeat(n));
        assert_eq!(problem(&format!(r#"{{"a{}":""}}"#, ".".repeat(51))), None);
        assert_eq!(problem(&format!(r#"{{"a{}":""}}"#, ".".repeat(52))), Some("module name is too long".into()));
        assert_eq!(problem(&dots(300)), None, "a name starting with $ once stored is dropped");

        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        let upload = |modules: &str| fake::run(code_post, &ctx, "POST", "/x", &format!(r#"{{"branch":"default","modules":{modules}}}"#)).1;
        assert_eq!(err(&upload(r#"{"main":"x","w":{"binary":"AAAA"}}"#)), None);
        assert_eq!(err(&upload(r#"{"main":"x","w":{"nested":"AAAA"}}"#)), shape.as_deref());
    }

    /// Each of these kept every key it was sent as a name for good, so one request could spend all the server will take
    /// from outside, and after that no player could upload a module with a new name.
    #[test]
    fn refused_requests_leave_no_names() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        let n = 60_000;
        let keys = |tag: &str| (0..n).map(|i| format!(r#""{tag}{i}":1"#)).collect::<Vec<_>>().join(",");
        let spent = |what: &str, handler: fn(&Request, &ApiCtx) -> Response, body: String| {
            let before = engine::doc::outside_names_added();
            let (_, reply) = fake::run(handler, &ctx, "POST", "/x", &body);
            assert!(err(&reply).is_some(), "{what}: {reply}");
            // Other tests add names at the same time, but far fewer than this.
            assert!(engine::doc::outside_names_added() - before < n / 2, "{what}");
        };
        spent("badge", badge, format!(r##"{{"badge":{{"type":{{{}}},"param":0,"color1":"#000000","color2":"#000000","color3":"#000000"}}}}"##, keys("bk")));
        spent("module", code_post, format!(r#"{{"modules":{{"main":"","w":{{"binary":"","x":{{{}}}}}}}}}"#, keys("mk")));
        spent("module in an array", code_post, format!(r#"{{"modules":{{"main":"","w":[{{{}}}]}}}}"#, keys("ak")));
        spent("branch name", clone_branch, format!(r#"{{"newName":{{{}}}}}"#, keys("nk")));
        spent("console", console, r#"{"expression":{"ck1":1,"ck2":2}}"#.to_string());
    }

    #[test]
    fn a_custom_badge_still_matches() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        let post = |ty: &str| fake::run(badge, &ctx, "POST", "/x", &format!(r##"{{"badge":{{"type":{ty},"param":0,"color1":"#000000","color2":"#000000","color3":"#000000"}}}}"##)).1;
        assert_eq!(err(&post(r#"{"path1":"M0","path2":"M1"}"#)), Some("invalid params"), "no custom badge yet");
        crate::write(&ctx, |w| {
            edit_user(w, fake::ALICE, |st| {
                let mut d = (*st.doc).clone();
                obj_set(&mut d, "customBadge", Val::from_trusted_json(&Json::parse(r#"{"path1":"M0","path2":"M1"}"#).unwrap()));
                st.doc = Arc::new(d);
            });
            Ok(())
        })
        .unwrap();
        assert_eq!(err(&post(r#"{"path1":"M0","path2":"M1"}"#)), None);
        assert_eq!(err(&post(r#"{"path2":"M1","path1":"M0"}"#)), None, "_.isEqual ignores the order of keys");
        assert_eq!(err(&post(r#"{"path1":"other","path2":"M1"}"#)), Some("invalid params"));
        assert_eq!(err(&post(r#"{"path1":"M0"}"#)), Some("invalid params"));
        assert_eq!(err(&post("null")), Some("invalid params"));
        assert_eq!(err(&post("[1]")), Some("invalid params"));
    }

    #[test]
    fn the_console_queue_stops_growing() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        for _ in 0..MAX_CONSOLE_COMMANDS {
            assert_eq!(err(&fake::run(console, &ctx, "POST", "/x", r#"{"expression":"1"}"#).1), None);
        }
        assert_eq!(err(&fake::run(console, &ctx, "POST", "/x", r#"{"expression":"1"}"#).1), Some("console queue is full"));

        let ctx = crate::ApiCtx { published: ctx.published.clone(), commands: ctx.commands.clone(), user: Some(fake::BOB) };
        let value = format!(r#"{{"value":"{}"}}"#, "x".repeat(1024 * 1024 - 2));
        for _ in 0..MAX_CONSOLE_BYTES / (1024 * 1024) - 1 {
            assert_eq!(err(&fake::run(memory_post, &ctx, "POST", "/x", &value).1), None);
        }
        assert_eq!(err(&fake::run(memory_post, &ctx, "POST", "/x", &value).1), Some("console queue is full"));
        assert_eq!(err(&fake::run(console, &ctx, "POST", "/x", r#"{"expression":"1"}"#).1), None, "a short one still fits");
    }

    #[test]
    fn a_badge_colour_is_a_whole_colour() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        let post = |c: &str| fake::run(badge, &ctx, "POST", "/x", &format!(r##"{{"badge":{{"type":1,"color1":{c},"color2":"#000000","color3":"#ABCDEF","param":0}}}}"##)).1;
        assert_eq!(err(&post(r##""#00ff00""##)), None);
        for bad in [r##""#000000\"/><script>alert(1)</script>""##, r##""#0000000""##, r##""#00000""##, r##"["#000000"]"##, "5"] {
            assert_eq!(err(&post(bad)), Some("invalid params"), "{bad}");
        }
        let reply = svg_reply("<svg/>");
        assert_eq!(reply.header("content-security-policy"), Some("sandbox"));
        assert_eq!(reply.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(reply.header("content-type"), Some("image/svg+xml; charset=utf-8"));
    }

    #[test]
    fn code_flow() {
        let (ctx, h) = fake::ctx(Some(fake::ALICE));
        let (_, b) = fake::run(branches, &ctx, "GET", "/x", "");
        let first = b.get("list").unwrap().at(0).unwrap();
        assert_eq!(first.get("modules").unwrap().stringify(), r#"{"main":"module.exports.loop=function(){}","lib.util":"x"}"#);
        let (_, g) = fake::run(code_get, &ctx, "GET", "/x?branch=$activeSim", "");
        assert_eq!(g.get("branch").and_then(Json::as_str), Some("default"));
        assert_eq!(err(&fake::run(code_get, &ctx, "GET", "/x?branch=nope", "").1), Some("no code"));

        let (_, p) = fake::run(code_post, &ctx, "POST", "/x", r#"{"branch":"default","modules":{"main":"x","a.b/c":"y",".hidden":"z"},"_hash":"h1"}"#);
        assert!(p.get("timestamp").and_then(Json::as_f64).is_some(), "{p}");
        let (_, g) = fake::run(code_get, &ctx, "GET", "/x", "");
        assert_eq!(g.get("modules").unwrap().stringify(), r#"{"main":"x","a.b/c":"y"}"#);
        assert_eq!(err(&fake::run(code_post, &ctx, "POST", "/x", r#"{"branch":"zzz","modules":{}}"#).1), Some("branch does not exist"));

        let (_, c) = fake::run(clone_branch, &ctx, "POST", "/x", r#"{"branch":"default","newName":"two"}"#);
        assert!(c.get("timestamp").is_some(), "{c}");
        assert_eq!(err(&fake::run(set_active_branch, &ctx, "POST", "/x", r#"{"activeName":"activeWorld","branch":"nope"}"#).1), Some("no branch"));
        assert_eq!(err(&fake::run(set_active_branch, &ctx, "POST", "/x", r#"{"activeName":"bad","branch":"two"}"#).1), Some("invalid params"));
        assert_eq!(fake::run(set_active_branch, &ctx, "POST", "/x", r#"{"activeName":"activeWorld","branch":"two"}"#).1.stringify(), r#"{"ok":1}"#);
        let (_, g) = fake::run(code_get, &ctx, "GET", "/x", "");
        assert_eq!(g.get("branch").and_then(Json::as_str), Some("two"));
        // the active branch cannot be deleted, the other can
        fake::run(delete_branch, &ctx, "POST", "/x", r#"{"branch":"two"}"#);
        fake::run(set_active_branch, &ctx, "POST", "/x", r#"{"activeName":"activeSim","branch":"two"}"#);
        drop(ctx);
        let w = h.join().unwrap();
        let code = &w.users[fake::ALICE as usize].code;
        assert_eq!(code.branches.len(), 2);
        assert_eq!(code.active_world, Some(1));
        assert!(code.version >= 3, "version bumps on active world changes: {}", code.version);
        assert_eq!(field(&w.users[fake::ALICE as usize].doc, "active"), Some(&Val::Num(10000.0)));
    }

    #[test]
    fn badge_validation() {
        let (ctx, h) = fake::ctx(Some(fake::BOB));
        let bad = [
            r#"{"badge":5}"#,
            r##"{"badge":{"type":1,"color1":"#ff0000","color2":"#ff0000","color3":"#ff0000"}}"##,
            r##"{"badge":{"type":25,"param":0,"color1":"#ff0000","color2":"#ff0000","color3":"#ff0000"}}"##,
            r##"{"badge":{"type":1,"param":101,"color1":"#ff0000","color2":"#ff0000","color3":"#ff0000"}}"##,
            r##"{"badge":{"type":1,"param":0,"color1":"ff0000","color2":"#ff0000","color3":"#ff0000"}}"##,
            r##"{"badge":{"type":{"path1":"x"},"param":0,"color1":"#ff0000","color2":"#ff0000","color3":"#ff0000"}}"##,
        ];
        for b in bad {
            assert_eq!(err(&fake::run(badge, &ctx, "POST", "/x", b).1), Some("invalid params"), "{b}");
        }
        let ok = r##"{"badge":{"type":3,"param":"-5","color1":"#AbCdEf","color2":"#ff0000","color3":"#000000","flip":1}}"##;
        assert_eq!(fake::run(badge, &ctx, "POST", "/x", ok).1.stringify(), r#"{"ok":1,"modified":1}"#);
        drop(ctx);
        let w = h.join().unwrap();
        let mut stored = String::new();
        field(&w.users[fake::BOB as usize].doc, "badge").unwrap().write_json(&mut stored);
        assert_eq!(stored, r##"{"type":3,"color1":"#AbCdEf","color2":"#ff0000","color3":"#000000","param":-5,"flip":true}"##);
    }

    #[test]
    fn world_status_and_start_room() {
        let w = fake::world();
        let v = fake::view(&w);
        assert_eq!(world_status_of(&v, fake::ALICE), "normal");
        assert_eq!(world_status_of(&v, fake::BOB), "empty");
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        assert_eq!(fake::run(world_start_room, &ctx, "GET", "/x", "").1.stringify(), r#"{"ok":1,"room":["W2N1"]}"#);
        let (ctx, _h) = fake::ctx(Some(fake::BOB));
        assert_eq!(fake::run(world_start_room, &ctx, "GET", "/x", "").1.stringify(), r#"{"ok":1,"room":["W1N1"]}"#);
        assert_eq!(fake::run(rooms, &ctx, "GET", "/x?id=a11ce0000000001", "").1.stringify(), r#"{"ok":1,"rooms":["W2N1"]}"#);
        assert_eq!(fake::run(rooms, &ctx, "GET", "/x", "").1.stringify(), r#"{"ok":1,"rooms":["W2N1","W1N1","W1N2"]}"#);
        let (_, f) = fake::run(find, &ctx, "GET", "/x?username=Alice", "");
        assert_eq!(f.get("user").unwrap().stringify(), r##"{"_id":"a11ce0000000001","username":"alice","badge":{"type":1,"color1":"#ff0000","color2":"#00ff00","color3":"#0000ff","param":0,"flip":false},"gcl":1000,"steam":{"id":"7656"}}"##);
        assert_eq!(err(&fake::run(stats, &ctx, "GET", "/x?interval=7", "").1), Some("invalid params"));
        assert_eq!(fake::run(stats, &ctx, "GET", "/x?interval=8.9", "").1.stringify(), r#"{"ok":1,"stats":{}}"#);
    }

    /// Review regressions: non-string branch, segment without data, steam without id, respawn reopening rooms.
    #[test]
    fn review_edge_cases() {
        let (ctx, h) = fake::ctx(Some(fake::ALICE));
        // `{branch: 5}` is queried as such: no branch matches (the active branch is left alone), active is still set
        assert_eq!(err(&fake::run(code_post, &ctx, "POST", "/x", r#"{"branch":5,"modules":{"main":"evil"}}"#).1), Some("branch does not exist"));
        assert_eq!(err(&fake::run(code_post, &ctx, "POST", "/x", r#"{"branch":true,"modules":{"main":"evil"}}"#).1), Some("branch does not exist"));
        let (_, g) = fake::run(code_get, &ctx, "GET", "/x", "");
        assert_eq!(g.get("modules").unwrap().get("main").and_then(Json::as_str), Some("module.exports.loop=function(){}"));
        assert!(fake::run(code_post, &ctx, "POST", "/x", r#"{"branch":0,"modules":{"main":"ok"}}"#).1.get("timestamp").is_some(), "falsy → $activeWorld");
        // hset(…, undefined): the segment reads as absent
        fake::run(memory_segment_post, &ctx, "POST", "/x", r#"{"segment":4,"data":"x"}"#);
        assert_eq!(fake::run(memory_segment_post, &ctx, "POST", "/x", r#"{"segment":4}"#).1.stringify(), r#"{"ok":1}"#);
        assert_eq!(fake::run(memory_segment_get, &ctx, "GET", "/x?segment=4", "").1.stringify(), r#"{"ok":1}"#);
        // steam without an id: `{id: undefined}` → {}
        crate::write(&ctx, |w| {
            edit_user(w, fake::BOB, |st| {
                let mut d = (*st.doc).clone();
                obj_set(&mut d, "steam", Val::from_json(&Json::parse(r#"{"displayName":"B"}"#).unwrap()));
                st.doc = Arc::new(d);
            });
            Ok(())
        })
        .unwrap();
        assert_eq!(fake::run(find, &ctx, "GET", "/x?username=bob", "").1.get("user").unwrap().get("steam").unwrap().stringify(), "{}");
        drop(ctx);
        h.join().unwrap();

        // utils.js:147-150 (Q promise for-in): every room becomes 'normal' and the access snapshot is marked dirty
        let mut w = fake::world();
        let mut info = (*w.rooms[2].info).clone();
        obj_set(&mut info, "status", s("out of borders"));
        let objs = w.rooms[2].objects.to_vec();
        crate::install_room(&mut w, 2, objs, None, Some(Arc::new(info)));
        assert_eq!(respawn_user(&mut w, fake::ALICE), Err("no system user".into()));
        crate::add_user(&mut w, fake::obj(r#"{"username":"Screeps","usernameLower":"screeps","cpu":0}"#), Obj::new(), vec![]);
        respawn_user(&mut w, fake::ALICE).unwrap();
        assert_eq!(str_field(&w.rooms[2].info, "status"), Some("normal"));
        assert!(w.env.status_dirty);
    }

    #[test]
    fn segments() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        assert_eq!(err(&fake::run(memory_segment_post, &ctx, "POST", "/x", r#"{"segment":100,"data":"x"}"#).1), Some("invalid segment ID"));
        assert_eq!(fake::run(memory_segment_post, &ctx, "POST", "/x", r#"{"segment":"7abc","data":"hello"}"#).1.stringify(), r#"{"ok":1}"#);
        assert_eq!(fake::run(memory_segment_get, &ctx, "GET", "/x?segment=7", "").1.stringify(), r#"{"ok":1,"data":"hello"}"#);
        assert_eq!(fake::run(memory_segment_get, &ctx, "GET", "/x?segment=8", "").1.stringify(), r#"{"ok":1}"#);
        assert_eq!(err(&fake::run(memory_segment_get, &ctx, "GET", "/x?segment=x", "").1), Some("invalid segment ID"));
    }
}
