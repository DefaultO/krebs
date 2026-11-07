//! Oracle adapter: scenario JSON (ARCHITECTURE.md §12.2, format FROZEN) ⇄ process_room / global / cron.
//!
//! A room scenario is the input of one official `processRoom` call (tools/oracle/processor.js runs it under
//! node); [`RoomScenario::run`] builds the same input for [`processor::process_room`] and turns the
//! [`RoomResult`] into the oracle's `outcome` shape, so fixtures compare as JSON with key order significant
//! ([`first_diff`]). Outcome: `{objects, foreignInserts, users, flags, roomInfo, eventLog, mapView, activated,
//! notifications, stats, history, crashed}` = the collections after the bulk ops (inserts appended in counter
//! order, `_id` '#<n>' when the engine did not give one, '#f<n>' for objects inserted into another room),
//! the saved room doc, the parsed eventLog/mapView/history JSON (null when not saved) and the side effects.
//!
//! A global scenario ([`GlobalScenario`], tools/oracle/global.js) is a world fragment of collections; it is
//! turned into a [`crate::world::World`] for [`processor::global::run`], and the world after it back into
//! the collections (`_id`s the run created → '#<n>' per collection) plus the activated rooms.

use crate::bulk::{self, DocStore};
use crate::doc::{array_index, Doc, Id, Obj, Val};
use crate::hooks::ProcessorHooks;
use crate::intents::{RoomIntents, UserRoomIntents};
use crate::policy::{self, Compat, Profile, VClock};
use crate::processor::{self, RoomDraft, RoomResult, TickCtx};
use crate::room_name::RoomXY;
use crate::terrain::RoomTerrain;
use crate::world::{AccessSnapshot, CodeState, EngineConfig, Inbox, RoomIdx, RoomMeta, RoomSnapshot, RoomStatus, RoomsMeta, TerrainStore, UserIdx, UserState};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use util::json::Json;
use util::jsstr::JsStr;

/// One user's stored room intents (`rooms.intents.users[uid]`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScenarioIntents {
    pub user: String,
    /// `objects`: object id | "room" → intent name → sanitized value, in first-set order.
    pub objects: Option<Vec<(String, Obj)>>,
    /// `objectsManual` (INT-MANUAL).
    pub manual: Option<Vec<(String, Obj)>>,
}

/// `{"kind":"room", …}` (§12.2).
#[derive(Clone, Debug, PartialEq)]
pub struct RoomScenario {
    pub world_seed: u64,
    pub clock: VClock,
    pub game_time: u64,
    pub room: String,
    /// The `rooms` doc without `_id` (the oracle stores `{_id: room, …roomDoc}`).
    pub room_doc: Obj,
    pub terrain: String,
    /// Creation order.
    pub objects: Vec<Obj>,
    pub flags: Vec<Obj>,
    /// The users collection in JS key order of the scenario's `users` map (canonical array indices first).
    pub users: Vec<(String, Obj)>,
    /// Processing order, explicit.
    pub intents: Vec<ScenarioIntents>,
    pub accessible_rooms: Vec<String>,
    pub room_status_data: Json,
    pub world_size: u32,
    pub compat: Profile,
    pub tags: Vec<String>,
}

fn field<'a>(j: &'a Json, k: &str) -> Result<&'a Json, String> {
    j.get(k).ok_or_else(|| format!("scenario: missing `{k}`"))
}

fn num(j: &Json, k: &str) -> Result<f64, String> {
    field(j, k)?.as_f64().ok_or_else(|| format!("scenario: `{k}` is not a number"))
}

fn string(j: &Json, k: &str) -> Result<String, String> {
    field(j, k)?.as_str().map(str::to_string).ok_or_else(|| format!("scenario: `{k}` is not a string"))
}

fn obj(j: &Json) -> Result<Obj, String> {
    match Val::from_trusted_json(j) {
        Val::Obj(o) => Ok(o),
        _ => Err(format!("scenario: expected an object, got {}", j.stringify())),
    }
}

fn objs(j: &Json, k: &str) -> Result<Vec<Obj>, String> {
    field(j, k)?.as_array().ok_or_else(|| format!("scenario: `{k}` is not an array"))?.iter().map(obj).collect()
}

fn entries(j: &Json) -> Result<Option<Vec<(String, Obj)>>, String> {
    match j {
        Json::Null => Ok(None),
        Json::Obj(m) => m.iter().map(|(k, v)| Ok((k.clone(), obj(v)?))).collect::<Result<_, String>>().map(Some),
        _ => Err("scenario: intents must be objects or null".into()),
    }
}

fn entries_json(e: &Option<Vec<(String, Obj)>>) -> Json {
    match e {
        None => Json::Null,
        Some(e) => Json::Obj(e.iter().map(|(k, o)| (k.clone(), Val::Obj(o.clone()).to_json())).collect()),
    }
}

/// `"0x1"` (the JSON form, u64 does not fit a JS number) or a plain number.
fn parse_seed(j: &Json) -> Result<u64, String> {
    match j {
        Json::Str(s) => match s.strip_prefix("0x") {
            Some(h) => u64::from_str_radix(h, 16),
            None => s.parse(),
        }
        .map_err(|e| format!("scenario: worldSeed: {e}")),
        Json::Num(n) => Ok(*n as u64),
        _ => Err("scenario: worldSeed must be a string".into()),
    }
}

impl RoomScenario {
    pub fn from_json(j: &Json) -> Result<RoomScenario, String> {
        if j.get("kind").and_then(Json::as_str) != Some("room") {
            return Err("scenario: kind is not \"room\"".into());
        }
        let clock = field(j, "clock")?;
        let mut users: Vec<(String, Obj)> = match field(j, "users")? {
            Json::Obj(m) => m.iter().map(|(k, v)| Ok((k.clone(), obj(v)?))).collect::<Result<_, String>>()?,
            _ => return Err("scenario: `users` is not an object".into()),
        };
        // JS own-key order, as the oracle's `Object.values(users)` sees it.
        users.sort_by_key(|(k, _)| array_index(k).map_or(u64::MAX, u64::from));
        let intents = field(j, "intents")?
            .as_array()
            .ok_or("scenario: `intents` is not an array")?
            .iter()
            .map(|e| {
                let user = e.at(0).and_then(Json::as_str).ok_or("scenario: intents entry without a user id")?.to_string();
                let i = e.at(1).ok_or("scenario: intents entry without intents")?;
                Ok(ScenarioIntents {
                    user,
                    objects: entries(i.get("objects").unwrap_or(&Json::Null))?,
                    manual: entries(i.get("objectsManual").unwrap_or(&Json::Null))?,
                })
            })
            .collect::<Result<_, String>>()?;
        let strings = |k: &str| -> Result<Vec<String>, String> {
            match j.get(k) {
                None => Ok(Vec::new()),
                Some(a) => a.as_array().ok_or(format!("scenario: `{k}` is not an array"))?.iter().map(|s| s.as_str().map(str::to_string).ok_or(format!("scenario: `{k}` holds a non-string"))).collect(),
            }
        };
        let compat = j.get("compat").and_then(Json::as_str).unwrap_or("private");
        Ok(RoomScenario {
            world_seed: parse_seed(field(j, "worldSeed")?)?,
            clock: VClock { base_ms: num(clock, "baseMs")? as i64, base_tick: num(clock, "baseTick")? as u64, nominal_tick_ms: num(clock, "nominalTickMs")? as u32 },
            game_time: num(j, "gameTime")? as u64,
            room: string(j, "room")?,
            room_doc: obj(field(j, "roomDoc")?)?,
            terrain: string(j, "terrain")?,
            objects: objs(j, "objects")?,
            flags: objs(j, "flags")?,
            users,
            intents,
            accessible_rooms: strings("accessibleRooms")?,
            room_status_data: j.get("roomStatusData").cloned().unwrap_or(Json::Obj(Vec::new())),
            world_size: j.get("worldSize").and_then(Json::as_f64).unwrap_or(0.0) as u32,
            compat: Profile::parse(compat).ok_or_else(|| format!("scenario: unknown compat profile {compat}"))?,
            tags: strings("tags")?,
        })
    }

    pub fn to_json(&self) -> Json {
        let s = |v: &str| Json::Str(v.to_string());
        let n = |v: f64| Json::Num(v);
        let docs = |d: &[Obj]| Json::Arr(d.iter().map(|o| Val::Obj(o.clone()).to_json()).collect());
        let strs = |v: &[String]| Json::Arr(v.iter().map(|x| s(x)).collect());
        Json::Obj(vec![
            ("kind".into(), s("room")),
            ("worldSeed".into(), s(&format!("0x{:x}", self.world_seed))),
            (
                "clock".into(),
                Json::Obj(vec![
                    ("baseMs".into(), n(self.clock.base_ms as f64)),
                    ("baseTick".into(), n(self.clock.base_tick as f64)),
                    ("nominalTickMs".into(), n(self.clock.nominal_tick_ms as f64)),
                ]),
            ),
            ("gameTime".into(), n(self.game_time as f64)),
            ("room".into(), s(&self.room)),
            ("roomDoc".into(), Val::Obj(self.room_doc.clone()).to_json()),
            ("terrain".into(), s(&self.terrain)),
            ("objects".into(), docs(&self.objects)),
            ("flags".into(), docs(&self.flags)),
            ("users".into(), Json::Obj(self.users.iter().map(|(k, o)| (k.clone(), Val::Obj(o.clone()).to_json())).collect())),
            (
                "intents".into(),
                Json::Arr(
                    self.intents
                        .iter()
                        .map(|i| Json::Arr(vec![s(&i.user), Json::Obj(vec![("objects".into(), entries_json(&i.objects)), ("objectsManual".into(), entries_json(&i.manual))])]))
                        .collect(),
                ),
            ),
            ("accessibleRooms".into(), strs(&self.accessible_rooms)),
            ("roomStatusData".into(), self.room_status_data.clone()),
            ("worldSize".into(), n(self.world_size as f64)),
            ("compat".into(), s(match self.compat {
                Profile::Private => "private",
                Profile::Mmo => "mmo",
            })),
            ("tags".into(), strs(&self.tags)),
        ])
    }

    /// The users collection as the oracle stores it: a doc without `_id` becomes `{_id: key, …doc}`.
    fn user_docs(&self) -> Vec<Obj> {
        self.users
            .iter()
            .map(|(id, doc)| {
                if doc.get(crate::atom!("_id")).is_some() {
                    return doc.clone();
                }
                let mut o = Obj::new();
                o.set(crate::atom!("_id"), Val::Str(id.as_str().into()));
                for (k, v) in doc.iter() {
                    o.set(k, v.clone());
                }
                o
            })
            .collect()
    }

    /// The `rooms` doc as stored: `{_id: room, …roomDoc}`.
    fn room_info(&self) -> Obj {
        let mut info = Obj::new();
        info.set(crate::atom!("_id"), Val::Str(self.room.as_str().into()));
        for (k, v) in self.room_doc.iter() {
            info.set(k, v.clone());
        }
        info
    }

    /// Runs `process_room` on this scenario and returns the outcome, or the panic message (a stub's
    /// `todo!()` included).
    pub fn run(&self) -> Result<Json, String> {
        self.run_room(engine_config())
    }

    /// `run` as a server without the history mod runs it: `process_room` then takes `quiet_tick` for plain roads,
    /// walls and ramparts, which `history: true` turns off. Only the outcome's `history` may differ.
    pub fn run_without_history(&self) -> Result<Json, String> {
        self.run_room(EngineConfig { history: false, ..engine_config() })
    }

    fn run_room(&self, config: EngineConfig) -> Result<Json, String> {
        let names = self.room_names();
        let meta = rooms_meta(&names, self.world_size)?;
        let terrain = RoomTerrain::from_db_str(&self.terrain).ok_or("scenario: terrain is not 2500 codes 0..3")?;
        let terrains = TerrainStore::new(names.iter().enumerate().map(|(i, _)| if i == 0 { terrain.clone() } else { RoomTerrain::default() }).collect());
        let access = self.access(&names);
        if let Some(o) = self.objects.iter().chain(&self.flags).find(|o| o.get(crate::atom!("room")).and_then(Val::as_str) != Some(self.room.as_str())) {
            return Err(format!("scenario: a doc of another room ({}); the oracle only reads `room` = {}", Val::Obj(o.clone()).to_json().stringify(), self.room));
        }
        let users: Vec<Arc<UserState>> = self.users.iter().zip(self.user_docs()).enumerate().map(|(i, ((id, _), doc))| user_state(id, i as u64 + 1, &doc)).collect::<Result<_, _>>()?;
        let user_ids: crate::world::UserIds = users.iter().enumerate().map(|(i, u)| (u.id, i as UserIdx)).collect();
        let (intents, override_order) = self.room_intents(&user_ids)?;

        let compat = Compat::new(self.compat);
        let hooks = ProcessorHooks::default();
        let ctx = TickCtx {
            time: self.game_time,
            vnow: self.clock.at(self.game_time),
            seed: self.world_seed,
            compat: &compat,
            config: &config,
            meta: &meta,
            terrain: &terrains,
            access: &access,
            users: &users,
            user_ids: &user_ids,
            hooks: &hooks,
            order_override: if override_order.is_empty() { None } else { Some(&override_order) },
        };
        let prev = Arc::new(RoomSnapshot::new(
            RoomDraft {
                info: Arc::new(self.room_info()),
                objects: self.objects.iter().enumerate().map(|(i, o)| Doc::new(i as u64 + 1, o.clone())).collect(),
                flags: self.flags.iter().cloned().map(Arc::new).collect(),
                // Never equal to a processed room's JSON, so a processed room always yields a draft.
                event_log: "".into(),
                map_view: "".into(),
                presence: Vec::new(),
                user_refs: Vec::new(),
                has_movers: false,
                has_terminal_send: false,
                has_power_creeps: false,
            },
            self.game_time.saturating_sub(1),
        ));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| processor::process_room(&ctx, 0, &prev, intents))).map_err(panic_text)?;
        Ok(self.outcome(&r))
    }

    /// The intents `process_room` gets and the `order_override` (empty when ORD-POLICY already gives the
    /// scenario's processing order).
    fn room_intents(&self, user_ids: &crate::world::UserIds) -> Result<(RoomIntents, Vec<UserIdx>), String> {
        let idx = |uid: &str| -> Result<UserIdx, String> {
            Id::new(uid).and_then(|id| user_ids.get(&id).copied()).ok_or_else(|| format!("scenario: intents of {uid}, who is not in `users`"))
        };
        // The oracle keys `rooms.intents.users` by user id, so `_.forEach` (and so vanilla) runs canonical
        // array-index ids first, ascending, then the scenario's order.
        let mut in_order: Vec<&ScenarioIntents> = self.intents.iter().collect();
        in_order.sort_by_key(|i| array_index(&i.user).map_or(u64::MAX, u64::from));
        let mut intents = RoomIntents { users: Vec::new() };
        for i in &in_order {
            let user = idx(&i.user)?;
            if intents.users.iter().any(|u| u.user == user) {
                return Err(format!("scenario: two intents entries for user {}", i.user));
            }
            let boxed = |e: &Option<Vec<(String, Obj)>>| e.iter().flatten().map(|(k, o)| (k.as_str().into(), o.clone())).collect();
            intents.users.push(UserRoomIntents { user, objects: boxed(&i.objects), manual: boxed(&i.manual) });
        }
        // That order is authoritative; pass it only when it is not already ORD-POLICY's.
        let order: Vec<&str> = in_order.iter().map(|i| i.user.as_str()).collect();
        let mut policy_order = order.clone();
        policy::sort_users(self.world_seed, self.game_time, &mut policy_order, |u| u);
        let override_order: Vec<UserIdx> = if order != policy_order { order.iter().map(|u| idx(u)).collect::<Result<_, _>>()? } else { Vec::new() };
        Ok((intents, override_order))
    }

    fn access(&self, names: &[&str]) -> AccessSnapshot {
        let mut accessible = vec![0u64; names.len().div_ceil(64)];
        for (i, n) in names.iter().enumerate() {
            if self.accessible_rooms.iter().any(|r| r == n) {
                accessible[i / 64] |= 1 << (i % 64);
            }
        }
        // driver updateRoomStatusData: {novice: {room: ts}, respawn: {…}, closed: {…}}; Infinity arrives as null.
        let mut status = HashMap::new();
        for key in ["novice", "respawn", "closed"] {
            for (room, ts) in self.room_status_data.get(key).and_then(Json::as_object).into_iter().flatten() {
                if let Some(i) = names.iter().position(|n| n == room) {
                    let st = match key {
                        "novice" => RoomStatus::Novice,
                        "respawn" => RoomStatus::Respawn,
                        _ => RoomStatus::Closed,
                    };
                    status.insert(i as RoomIdx, (st, ts.as_f64().map(|t| t as i64)));
                }
            }
        }
        AccessSnapshot { version: 0, accessible, status }
    }

    /// RoomIdx order of the scenario: the room first, then the other accessible rooms.
    fn room_names(&self) -> Vec<&str> {
        std::iter::once(self.room.as_str()).chain(self.accessible_rooms.iter().map(String::as_str).filter(|r| *r != self.room)).collect()
    }

    /// A [`RoomResult`] of this scenario → the oracle's `outcome`.
    pub fn outcome(&self, r: &RoomResult) -> Json {
        let names = self.room_names();
        let s = |v: &str| Json::Str(v.to_string());
        let doc = |o: &Obj| Val::Obj(o.clone()).to_json();
        let parse = |t: &str| Json::parse(t).unwrap_or_else(|e| Json::Str(format!("<invalid JSON: {e}>")));
        let uid = |u: UserIdx| self.users.get(u as usize).map_or_else(|| format!("<user {u}>"), |(id, _)| id.clone());

        let (mut objects, flags, info, event_log, map_view) = match &r.next {
            Some(d) => (
                d.objects.iter().map(|x| x.obj.clone()).collect::<Vec<_>>(),
                d.flags.iter().map(|f| (**f).clone()).collect::<Vec<_>>(),
                (*d.info).clone(),
                parse(&d.event_log),
                parse(&d.map_view),
            ),
            None => (self.objects.clone(), self.flags.clone(), self.room_info(), Json::Null, Json::Null),
        };
        let mut n = 0;
        for o in &r.inserts {
            objects.push(with_id(o, "#", &mut n));
        }
        let mut nf = 0;
        let foreign = r.foreign_inserts.iter().map(|(room, o)| Json::Arr(vec![s(names.get(*room as usize).copied().unwrap_or("<room?>")), doc(&with_id(o, "#f", &mut nf))])).collect();
        let mut nflags = 0;
        let flags: Vec<Obj> = flags.iter().map(|f| with_id(f, "#", &mut nflags)).collect();
        let mut users = Coll(self.user_docs());
        bulk::apply(&mut users, r.user_ops.clone());

        Json::Obj(vec![
            ("objects".into(), Json::Arr(objects.iter().map(doc).collect())),
            ("foreignInserts".into(), Json::Arr(foreign)),
            ("users".into(), Json::Arr(users.0.iter().map(doc).collect())),
            ("flags".into(), Json::Arr(flags.iter().map(doc).collect())),
            ("roomInfo".into(), doc(&info)),
            ("eventLog".into(), event_log),
            ("mapView".into(), map_view),
            ("activated".into(), Json::Bool(r.activate_next)),
            ("notifications".into(), Json::Arr(r.notifications.iter().map(|(u, m)| Json::Arr(vec![s(&uid(*u)), s(m)])).collect())),
            ("stats".into(), Json::Arr(r.stats.iter().map(|(u, k, v)| Json::Arr(vec![s(&uid(*u)), s(k.as_str()), num_json(*v)])).collect())),
            ("history".into(), r.history.as_deref().map_or(Json::Null, parse)),
            ("crashed".into(), Json::Bool(r.crashed.is_some())),
        ])
    }
}


/// NaN/±Infinity cross the storage RPC as null.
fn num_json(v: f64) -> Json {
    if v.is_finite() {
        Json::Num(if v == 0.0 { 0.0 } else { v })
    } else {
        Json::Null
    }
}

/// An inserted doc with the placeholder `_id` the oracle's genId gives (`dbBulk` only fills a falsy `_id`).
fn with_id(o: &Obj, prefix: &str, n: &mut u32) -> Obj {
    let mut o = o.clone();
    let id = crate::atom!("_id");
    let missing = match o.get(id) {
        None | Some(Val::Null) | Some(Val::Bool(false)) => true,
        Some(Val::Str(s)) => s.is_empty(),
        Some(Val::Num(x)) => *x == 0.0 || x.is_nan(),
        _ => false,
    };
    if missing {
        *n += 1;
        o.set(id, Val::Str(format!("{prefix}{n}").into()));
    }
    o
}

/// A Loki collection for `bulk::apply`: data[] order, lookup by `_id`.
struct Coll(Vec<Obj>);

impl Coll {
    fn find(&self, id: &Id) -> Option<usize> {
        self.0.iter().position(|d| d.get(crate::atom!("_id")).and_then(Val::as_str) == Some(id.as_str()))
    }
}

impl DocStore for Coll {
    fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj> {
        self.find(id).map(|i| &mut self.0[i])
    }
    fn remove(&mut self, id: &Id) {
        if let Some(i) = self.find(id) {
            self.0.remove(i);
        }
    }
    fn insert(&mut self, doc: Obj) {
        self.0.push(doc);
    }
}

/// RoomsMeta over the given rooms (RoomIdx = position).
fn rooms_meta(names: &[&str], world_size: u32) -> Result<RoomsMeta, String> {
    let xys: Vec<RoomXY> = names.iter().map(|n| RoomXY::parse_engine(n).ok_or_else(|| format!("scenario: bad room name {n}"))).collect::<Result<_, _>>()?;
    let (x0, y0) = (xys.iter().map(|p| p.x).min().unwrap_or(0), xys.iter().map(|p| p.y).min().unwrap_or(0));
    let w = (xys.iter().map(|p| p.x).max().unwrap_or(0) - x0 + 1) as u32;
    let h = (xys.iter().map(|p| p.y).max().unwrap_or(0) - y0 + 1) as u32;
    let mut grid = vec![0u32; (w * h) as usize];
    for (i, p) in xys.iter().enumerate() {
        grid[((p.y - y0) as u32 * w + (p.x - x0) as u32) as usize] = i as u32 + 1;
    }
    Ok(RoomsMeta {
        x0,
        y0,
        w,
        h,
        grid,
        rooms: names.iter().zip(&xys).map(|(n, xy)| RoomMeta { name: (*n).into(), xy: *xy, removed: false }).collect(),
        world_size,
    })
}

/// CFG-ENGINE-DEFAULTS (reference/driver/lib/index.js:19-33).
fn engine_config() -> EngineConfig {
    EngineConfig {
        main_loop_min_duration_ms: 1000,
        main_loop_reset_interval_ms: 5000,
        cpu_max_per_tick: 500,
        cpu_bucket_size: 10000,
        history_chunk_size: 20,
        socket_update_throttle_ms: 200,
        history: true,
        disabled_cronjobs: Vec::new(),
    }
}

fn user_state(id: &str, seq: u64, doc: &Obj) -> Result<Arc<UserState>, String> {
    Ok(Arc::new(UserState {
        id: Id::new(id).ok_or_else(|| format!("scenario: bad user id {id}"))?,
        seq,
        doc: Arc::new(doc.clone()),
        auth: Arc::new(Obj::new()),
        code: Arc::new(CodeState { branches: Vec::new(), active_world: None, active_sim: None, version: 0 }),
        memory: JsStr::from_str(""),
        memory_version: 0,
        segments: Arc::new(BTreeMap::new()),
        public_segments: "".into(),
        inter_shard: None,
        console_queue: Arc::new([]),
        power_creeps: Arc::new([]),
        inbox: Arc::new(Inbox { notifications: Vec::new(), messages: Vec::new() }),
    }))
}

// ---- global stage ----

/// The collections of a global scenario (tools/oracle/global.js COLLECTIONS), in outcome order.
pub const GLOBAL_COLLECTIONS: &[&str] = &["rooms", "rooms.objects", "users", "market.orders", "users.power_creeps", "users.intents", "transactions", "users.money", "users.resources"];

/// `{"kind":"global", …, "db": {collection: [docs]}}`: the world fragment the five vanilla `getInterRoom`
/// queries read; `users.intents` order is the processing order.
#[derive(Clone, Debug, PartialEq)]
pub struct GlobalScenario {
    pub world_seed: u64,
    pub clock: VClock,
    pub game_time: u64,
    pub world_size: u32,
    pub compat: Profile,
    pub tags: Vec<String>,
    /// In the scenario's key order.
    pub db: Vec<(String, Vec<Obj>)>,
}

fn id_of(o: &Obj) -> Option<&str> {
    o.get(crate::atom!("_id")).and_then(Val::as_str)
}

fn str_of<'a>(o: &'a Obj, k: &str) -> Option<&'a str> {
    o.get(crate::doc::Atom::intern(k)).and_then(Val::as_str)
}

fn num_of(o: &Obj, k: &str) -> Option<f64> {
    o.get(crate::doc::Atom::intern(k)).and_then(Val::as_f64)
}

fn val_truthy(v: Option<&Val>) -> bool {
    match v {
        None | Some(Val::Null) | Some(Val::Bool(false)) => false,
        Some(Val::Num(n)) => *n != 0.0 && !n.is_nan(),
        Some(Val::Str(s)) => !s.is_empty(),
        _ => true,
    }
}

impl GlobalScenario {
    pub fn from_json(j: &Json) -> Result<GlobalScenario, String> {
        if j.get("kind").and_then(Json::as_str) != Some("global") {
            return Err("scenario: kind is not \"global\"".into());
        }
        let clock = field(j, "clock")?;
        let db = match field(j, "db")? {
            Json::Obj(m) => m.iter().map(|(k, v)| Ok((k.clone(), v.as_array().ok_or(format!("scenario: db.{k} is not an array"))?.iter().map(obj).collect::<Result<_, _>>()?))).collect::<Result<_, String>>()?,
            _ => return Err("scenario: `db` is not an object".into()),
        };
        let compat = j.get("compat").and_then(Json::as_str).unwrap_or("private");
        Ok(GlobalScenario {
            world_seed: parse_seed(field(j, "worldSeed")?)?,
            clock: VClock { base_ms: num(clock, "baseMs")? as i64, base_tick: num(clock, "baseTick")? as u64, nominal_tick_ms: num(clock, "nominalTickMs")? as u32 },
            game_time: num(j, "gameTime")? as u64,
            world_size: j.get("worldSize").and_then(Json::as_f64).unwrap_or(0.0) as u32,
            compat: Profile::parse(compat).ok_or_else(|| format!("scenario: unknown compat profile {compat}"))?,
            tags: j.get("tags").and_then(Json::as_array).map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default(),
            db,
        })
    }

    pub fn to_json(&self) -> Json {
        let n = |v: f64| Json::Num(v);
        Json::Obj(vec![
            ("kind".into(), Json::Str("global".into())),
            ("worldSeed".into(), Json::Str(format!("0x{:x}", self.world_seed))),
            (
                "clock".into(),
                Json::Obj(vec![("baseMs".into(), n(self.clock.base_ms as f64)), ("baseTick".into(), n(self.clock.base_tick as f64)), ("nominalTickMs".into(), n(self.clock.nominal_tick_ms as f64))]),
            ),
            ("gameTime".into(), n(self.game_time as f64)),
            ("worldSize".into(), n(self.world_size as f64)),
            ("compat".into(), Json::Str(if self.compat == Profile::Mmo { "mmo" } else { "private" }.into())),
            ("tags".into(), Json::Arr(self.tags.iter().map(|t| Json::Str(t.clone())).collect())),
            ("db".into(), Json::Obj(self.db.iter().map(|(k, d)| (k.clone(), Json::Arr(d.iter().map(|o| Val::Obj(o.clone()).to_json()).collect()))).collect())),
        ])
    }

    pub fn coll(&self, name: &str) -> &[Obj] {
        self.db.iter().find(|(k, _)| k == name).map_or(&[], |(_, d)| d)
    }

    /// The world the global stage of T sees: rooms in `rooms` order (then rooms only named by objects,
    /// as removed slots), objects/orders/power creeps with seq = their collection position, AccessSnapshot
    /// by the getInterRoom rule at vclock(T), indexes by brute force.
    pub fn world(&self) -> Result<crate::world::World, String> {
        use crate::world::*;
        let now = self.clock.at(self.game_time);
        let mut names: Vec<String> = self.coll("rooms").iter().map(|r| id_of(r).unwrap_or("").to_string()).collect();
        let listed = names.len();
        for o in self.coll("rooms.objects") {
            if let Some(r) = str_of(o, "room") {
                if !names.iter().any(|n| n == r) {
                    names.push(r.to_string());
                }
            }
        }
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut meta = rooms_meta(&refs, self.world_size)?;
        for m in meta.rooms.iter_mut().skip(listed) {
            m.removed = true;
        }
        let mut seq = 0u64;
        let mut next = || {
            seq += 1;
            seq
        };
        let mut objects: Vec<Vec<crate::doc::DocRef>> = vec![Vec::new(); names.len()];
        let mut index_objects = HashMap::new();
        let user_idx: HashMap<&str, UserIdx> = self.coll("users").iter().enumerate().filter_map(|(i, u)| id_of(u).map(|id| (id, i as UserIdx))).collect();
        let (mut movers, mut terminal_sends, mut pc_rooms) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
        let mut presence: Vec<BTreeMap<RoomIdx, Presence>> = vec![BTreeMap::new(); user_idx.len()];
        for o in self.coll("rooms.objects") {
            let r = names.iter().position(|n| Some(n.as_str()) == str_of(o, "room")).ok_or("scenario: an object without a room")? as RoomIdx;
            let s = next();
            let kind = str_of(o, "type").unwrap_or("");
            let owner = str_of(o, "user").and_then(|u| user_idx.get(u).copied());
            if let Some(id) = id_of(o).and_then(Id::new) {
                index_objects.insert(id, ObjLoc { room: r, seq: s, owner });
            }
            // getInterRoom's `interRoom: {$ne: null}` (storage rewrites it to also exclude undefined).
            if (kind == "creep" || kind == "powerCreep") && !matches!(o.get(crate::atom!("interRoom")), None | Some(Val::Null)) {
                movers.insert(r);
            }
            if kind == "terminal" && val_truthy(o.get(crate::atom!("send"))) {
                terminal_sends.insert(r);
            }
            if kind == "powerCreep" {
                pc_rooms.insert(r);
            }
            if let Some(u) = owner {
                let p = presence[u as usize].entry(r).or_default();
                p.objects += 1;
                if kind != "flag" && kind != "constructionSite" {
                    if p.vision == 0 {
                        p.first_vision_seq = s;
                    }
                    p.vision += 1;
                }
            }
            objects[r as usize].push(Doc::new(s, o.clone()));
        }
        let room_docs = self.coll("rooms");
        let rooms: Vec<Arc<RoomSnapshot>> = (0..names.len())
            .map(|i| {
                let info = room_docs.get(i).cloned().unwrap_or_else(|| {
                    let mut o = Obj::new();
                    o.set(crate::atom!("_id"), Val::Str(names[i].as_str().into()));
                    o
                });
                let has = |set: &std::collections::BTreeSet<RoomIdx>| set.contains(&(i as RoomIdx));
                Arc::new(RoomSnapshot::new(
                    RoomDraft {
                        info: Arc::new(info),
                        objects: std::mem::take(&mut objects[i]),
                        flags: Arc::new([]),
                        event_log: "[]".into(),
                        map_view: "".into(),
                        presence: Vec::new(),
                        user_refs: Vec::new(),
                        has_movers: has(&movers),
                        has_terminal_send: has(&terminal_sends),
                        has_power_creeps: has(&pc_rooms),
                    },
                    self.game_time.saturating_sub(1),
                ))
            })
            .collect();
        // getInterRoom: status 'normal' and (!openTime || openTime < now); updateRoomStatusData for the rest.
        let mut accessible = vec![0u64; names.len().div_ceil(64)];
        let mut status = HashMap::new();
        for (i, r) in room_docs.iter().enumerate() {
            let open = num_of(r, "openTime");
            if str_of(r, "status") == Some("normal") && open.is_none_or(|t| t == 0.0 || t < now as f64) {
                accessible[i / 64] |= 1 << (i % 64);
            }
            let ts = |k: &str| num_of(r, k).filter(|t| *t > now as f64);
            let st = if let Some(t) = ts("novice") {
                Some((RoomStatus::Novice, Some(t as i64)))
            } else if let Some(t) = ts("respawnArea") {
                Some((RoomStatus::Respawn, Some(t as i64)))
            } else if let Some(t) = ts("openTime") {
                Some((RoomStatus::Closed, Some(t as i64)))
            } else if str_of(r, "status") == Some("out of borders") {
                Some((RoomStatus::Closed, None))
            } else {
                None
            };
            if let Some(st) = st {
                status.insert(i as RoomIdx, st);
            }
        }
        let mut pcs: Vec<Vec<crate::doc::DocRef>> = vec![Vec::new(); user_idx.len()];
        for pc in self.coll("users.power_creeps") {
            let s = next();
            if let Some(&u) = str_of(pc, "user").and_then(|u| user_idx.get(u)) {
                pcs[u as usize].push(Doc::new(s, pc.clone()));
            }
        }
        let users: Vec<Arc<UserState>> = self
            .coll("users")
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let mut u = Arc::try_unwrap(user_state(id_of(d).unwrap_or(""), i as u64 + 1, d)?).ok().unwrap();
                u.power_creeps = std::mem::take(&mut pcs[i]).into();
                Ok(Arc::new(u))
            })
            .collect::<Result<_, String>>()?;
        let docs = |c: &str, next: &mut dyn FnMut() -> u64| -> Arc<[crate::doc::DocRef]> { self.coll(c).iter().map(|o| Doc::new(next(), o.clone())).collect() };
        let orders = docs("market.orders", &mut next);
        let transactions = docs("transactions", &mut next);
        let plain = |c: &str| -> Arc<[Arc<Obj>]> { self.coll(c).iter().cloned().map(Arc::new).collect() };
        let global_intents = self
            .coll("users.intents")
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let user = str_of(d, "user").and_then(|u| user_idx.get(u).copied()).ok_or("scenario: users.intents doc of an unknown user")?;
                let intents = d.get(crate::atom!("intents")).and_then(Val::as_obj).cloned().unwrap_or_default();
                Ok(GlobalIntentsDoc { user, from_api: false, arrival: i as u32, intents })
            })
            .collect::<Result<_, String>>()?;
        let mut ids = crate::ids::IdGen::new(self.world_seed, self.compat);
        for (_, d) in &self.db {
            for id in d.iter().filter_map(id_of).filter_map(Id::new) {
                ids.mark_imported(id);
            }
        }
        let next_seq = next();
        // World::new (private view buffer) then the scenario's own derived state: the snapshots carry no
        // presence, so the index comes from the scan above.
        // rooms.terrain docs (cron scenarios) give the rooms their terrain; plain otherwise.
        let terrain_docs = self.coll("rooms.terrain");
        let terrains = names.iter().map(|n| terrain_docs.iter().find(|t| str_of(t, "room") == Some(n.as_str())).and_then(|t| str_of(t, "terrain")).and_then(RoomTerrain::from_db_str).unwrap_or_default()).collect();
        let mut w = World::new(self.world_seed, self.clock, Compat::new(self.compat), meta, TerrainStore::new(terrains), rooms, users);
        w.time = self.game_time;
        w.config = Arc::new(engine_config());
        w.access = Arc::new(AccessSnapshot { version: 0, accessible, status });
        w.market = Arc::new(MarketState::new(orders, Arc::new([]), transactions, plain("users.money"), plain("users.resources")));
        w.env = Arc::new(EnvState { paused: false, tick_rate_ms: 1000, last_accessible_update: 0, status_dirty: false });
        w.global_intents = global_intents;
        w.index.presence = presence.into_iter().map(Arc::new).collect();
        w.index.objects = index_objects;
        w.index.movers = movers;
        w.index.terminal_sends = terminal_sends;
        w.index.power_creep_rooms = pc_rooms;
        w.index.force_update = Default::default();
        w.index.room_cost_ns = vec![0; names.len()];
        w.ids = ids;
        w.next_seq = next_seq;
        Ok(w)
    }

    /// The owners of the `users.intents` docs in processing (= collection) order. A user order can only
    /// express it when each owner's docs are adjacent: vanilla runs [a, b, a] as three docs, while
    /// `GlobalCtx::order_override` ranks users (ORD-POINTS 2), so such a scenario is not replayable.
    pub fn intent_owners(&self) -> Result<Vec<&str>, String> {
        let owners: Vec<&str> = self.coll("users.intents").iter().filter_map(|d| str_of(d, "user")).collect();
        if let Some(u) = owners.iter().enumerate().find(|&(i, u)| i > 0 && owners[i - 1] != *u && owners[..i - 1].contains(u)).map(|(_, u)| u) {
            return Err(format!("scenario: users.intents docs of {u} are not adjacent; no user order reproduces that processing order"));
        }
        Ok(owners)
    }

    /// Runs `processor::global::run` and returns the outcome, or the panic message.
    pub fn run(&self) -> Result<Json, String> {
        let owners = self.intent_owners()?;
        let mut w = self.world()?;
        // The doc order is authoritative; pass it only when it is not ORD-POLICY's (owner rank, stable).
        let mut ranked = owners.clone();
        policy::sort_users(self.world_seed, self.game_time, &mut ranked, |u| u);
        let mut order: Vec<UserIdx> = Vec::new();
        if owners != ranked {
            for u in &owners {
                let i = w.user_ids.get(&Id::new(u).ok_or("scenario: bad user id")?).copied().ok_or("scenario: unknown user")?;
                if !order.contains(&i) {
                    order.push(i);
                }
            }
        }
        let hooks = ProcessorHooks::default();
        let ctx = processor::global::GlobalCtx { time: self.game_time, vnow: self.clock.at(self.game_time), hooks: &hooks, order_override: if order.is_empty() { None } else { Some(&order) } };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| processor::global::run(&mut w, &ctx))).map_err(panic_text)?;
        let activated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.active.take())).map_err(|p| format!("ActiveSet::take {}", panic_text(p)))?;
        Ok(self.outcome(&w, &activated))
    }

    /// The world after the global stage → the oracle's outcome (new `_id`s → '#<n>' per collection).
    pub fn outcome(&self, w: &crate::world::World, activated: &[RoomIdx]) -> Json {
        let known: std::collections::HashSet<&str> = self.db.iter().flat_map(|(_, d)| d.iter().filter_map(id_of)).collect();
        let normalize = |docs: Vec<Obj>| -> Json {
            let mut n = 0;
            Json::Arr(
                docs.into_iter()
                    .map(|mut o| {
                        if id_of(&o).is_none_or(|id| !known.contains(id)) {
                            n += 1;
                            o.set(crate::atom!("_id"), Val::Str(format!("#{n}").into()));
                        }
                        Val::Obj(o).to_json()
                    })
                    .collect(),
            )
        };
        let by_seq = |mut v: Vec<&crate::doc::DocRef>| -> Vec<Obj> {
            v.sort_by_key(|d| d.seq);
            v.into_iter().map(|d| d.obj.clone()).collect()
        };
        let listed = self.coll("rooms").len();
        let names: Vec<&str> = w.meta.rooms.iter().map(|m| &*m.name).collect();
        let mut act: Vec<&str> = activated.iter().filter_map(|r| names.get(*r as usize).copied()).collect();
        act.sort_unstable();
        act.dedup();
        Json::Obj(vec![
            ("rooms".into(), Json::Arr(w.rooms.iter().take(listed).map(|r| Val::Obj((*r.info).clone()).to_json()).collect())),
            ("rooms.objects".into(), normalize(by_seq(w.rooms.iter().flat_map(|r| r.objects.iter()).collect()))),
            ("users".into(), Json::Arr(w.users.iter().map(|u| Val::Obj((*u.doc).clone()).to_json()).collect())),
            ("market.orders".into(), normalize(by_seq(w.market.orders.iter().collect()))),
            ("users.power_creeps".into(), normalize(by_seq(w.users.iter().flat_map(|u| u.power_creeps.iter()).collect()))),
            (
                "users.intents".into(),
                Json::Arr(
                    w.global_intents
                        .iter()
                        .map(|d| Json::Obj(vec![("user".into(), Json::Str(w.users[d.user as usize].id.as_str().into())), ("intents".into(), Val::Obj(d.intents.clone()).to_json())]))
                        .collect(),
                ),
            ),
            ("transactions".into(), normalize(by_seq(w.market.transactions.iter().collect()))),
            ("users.money".into(), normalize(w.market.money.iter().map(|o| (**o).clone()).collect())),
            ("users.resources".into(), normalize(w.market.resources.iter().map(|o| (**o).clone()).collect())),
            ("activated".into(), Json::Arr(act.into_iter().map(|r| Json::Str(r.into())).collect())),
            ("crashed".into(), Json::Bool(false)),
        ])
    }
}

// ---- crons ----

/// `{"kind":"cron", …, "job": name, "db": {collection: [docs]}, "env": {key: value}}` (tools/oracle/cron.js):
/// one CRON-STAGE job at stored game time `game_time` (= G) on a world fragment.
#[derive(Clone, Debug, PartialEq)]
pub struct CronScenario {
    pub job: String,
    /// The world fragment (its `db`, clock, seed, G).
    pub world: GlobalScenario,
    /// env keys (`activeRooms`, …).
    pub env: Json,
}

impl CronScenario {
    pub fn from_json(j: &Json) -> Result<CronScenario, String> {
        if j.get("kind").and_then(Json::as_str) != Some("cron") {
            return Err("scenario: kind is not \"cron\"".into());
        }
        let mut g = j.clone();
        if let Json::Obj(m) = &mut g {
            m.retain(|(k, _)| k != "job" && k != "env");
            if let Some((_, kind)) = m.iter_mut().find(|(k, _)| k == "kind") {
                *kind = Json::Str("global".into());
            }
        }
        Ok(CronScenario { job: string(j, "job")?, world: GlobalScenario::from_json(&g)?, env: j.get("env").cloned().unwrap_or(Json::Obj(Vec::new())) })
    }

    pub fn to_json(&self) -> Json {
        let Json::Obj(g) = self.world.to_json() else { unreachable!() };
        let mut out = Vec::new();
        for (k, v) in g {
            match k.as_str() {
                "kind" => out.push((k, Json::Str("cron".into()))),
                "worldSize" => {}
                "db" => {
                    out.push(("job".into(), Json::Str(self.job.clone())));
                    out.push((k, v));
                    out.push(("env".into(), self.env.clone()));
                }
                _ => out.push((k, v)),
            }
        }
        // cron.js writes: kind, worldSeed, clock, gameTime, job, db, env, compat, tags.
        let order = ["kind", "worldSeed", "clock", "gameTime", "job", "db", "env", "compat", "tags"];
        out.sort_by_key(|(k, _)| order.iter().position(|o| o == k).unwrap_or(order.len()));
        Json::Obj(out)
    }

    /// Runs the job on the scenario world (env `activeRooms` as the active set) and returns the oracle outcome.
    pub fn run(&self) -> Result<Json, String> {
        let mut w = self.world.world()?;
        if let Some(Json::Arr(names)) = self.env.get("activeRooms") {
            for n in names.iter().filter_map(Json::as_str) {
                if let Some(r) = w.meta.by_name(n).filter(|&r| &*w.meta.rooms[r as usize].name == n) {
                    w.active.insert(r);
                }
            }
        }
        let job = crate::cron::cronjobs::JOBS.iter().find(|(n, _, _)| *n == self.job).map(|(n, _, _)| *n).ok_or_else(|| format!("unknown cron job {}", self.job))?;
        let g = self.world.game_time;
        let mut cx = crate::cron::CronCtx::new(self.world.world_seed, g, self.world.clock.at(g), job);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::cron::cronjobs::run_job(job, &mut w, &mut cx))).map_err(panic_text)?;
        Ok(self.outcome(&w, res.is_err()))
    }

    /// cron.js's outcome: every collection of the scenario in its order (new `_id`s → '#<n>' per collection), `env`
    /// (gameTime, the scenario env keys, `activeRooms` sorted), `crashed`.
    pub fn outcome(&self, w: &crate::world::World, crashed: bool) -> Json {
        let base = self.world.outcome(w, &[]);
        let known: std::collections::HashSet<&str> = self.world.db.iter().flat_map(|(_, d)| d.iter().filter_map(id_of)).collect();
        let mut out = Vec::new();
        // lokijs listCollections: newest first; calcMarketStats creates market.stats when the scenario lacks it
        let empty = Vec::new();
        let mut colls: Vec<(&str, &Vec<Obj>)> = self.world.db.iter().rev().map(|(k, d)| (k.as_str(), d)).collect();
        if self.job == "calcMarketStats" && !colls.iter().any(|(k, _)| *k == "market.stats") {
            colls.insert(0, ("market.stats", &empty));
        }
        for (name, docs) in colls {
            let name = name.to_string();
            let original = || Json::Arr(docs.iter().map(|o| Val::Obj(o.clone()).to_json()).collect());
            let v = match name.as_str() {
                "users.intents" | "rooms.terrain" => original(),
                "market.stats" => {
                    let mut n = 0;
                    Json::Arr(
                        w.market
                            .stats
                            .iter()
                            .map(|o| {
                                let mut o = (**o).clone();
                                if id_of(&o).is_none_or(|id| !known.contains(id)) {
                                    n += 1;
                                    o.set(crate::atom!("_id"), Val::Str(format!("#{n}").into()));
                                }
                                Val::Obj(o).to_json()
                            })
                            .collect(),
                    )
                }
                n => base.get(n).cloned().unwrap_or_else(original),
            };
            out.push((name.clone(), v));
        }
        let mut active: Vec<String> = w.active.list().iter().map(|&r| w.meta.rooms[r as usize].name.to_string()).collect();
        active.sort();
        let mut env = vec![("gameTime".to_string(), Json::Num(self.world.game_time as f64))];
        if let Json::Obj(m) = &self.env {
            env.extend(m.iter().filter(|(k, _)| k != "gameTime").cloned());
        }
        let list = Json::Arr(active.into_iter().map(Json::Str).collect());
        match env.iter_mut().find(|(k, _)| k == "activeRooms") {
            Some((_, v)) => *v = list,
            None if !w.active.list().is_empty() => env.push(("activeRooms".into(), list)),
            None => {}
        }
        out.push(("env".into(), Json::Obj(env)));
        out.push(("crashed".into(), Json::Bool(crashed)));
        Json::Obj(out)
    }
}

fn panic_text(p: Box<dyn std::any::Any + Send>) -> String {
    let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
    format!("panicked: {msg}")
}

// ---- comparison ----

/// The first difference between an oracle outcome and ours, as `path: expected …, got …`, with `_id`s
/// named in array paths. Key order is significant. `history` is compared only when we produced one (the
/// history mod is optional).
pub fn first_diff(expected: &Json, actual: &Json) -> Option<String> {
    diff(expected, actual, "outcome")
}

fn short(j: &Json) -> String {
    let s = j.stringify();
    if s.len() > 300 {
        format!("{}…", &s[..s.char_indices().nth(300).map_or(s.len(), |(i, _)| i)])
    } else {
        s
    }
}

fn diff(e: &Json, a: &Json, path: &str) -> Option<String> {
    match (e, a) {
        (Json::Obj(me), Json::Obj(ma)) => {
            let (ke, ka): (Vec<&str>, Vec<&str>) = (me.iter().map(|(k, _)| k.as_str()).collect(), ma.iter().map(|(k, _)| k.as_str()).collect());
            for (k, ve) in me {
                if path == "outcome" && k == "history" && a.get("history").is_some_and(Json::is_null) {
                    continue;
                }
                match a.get(k) {
                    None => return Some(format!("{path}.{k}: expected {}, missing", short(ve))),
                    Some(va) => {
                        if let Some(d) = diff(ve, va, &format!("{path}.{k}")) {
                            return Some(d);
                        }
                    }
                }
            }
            if let Some(k) = ka.iter().find(|k| !ke.contains(k)) {
                return Some(format!("{path}.{k}: unexpected, got {}", short(a.get(k).unwrap())));
            }
            (ke != ka).then(|| format!("{path}: key order expected {ke:?}, got {ka:?}"))
        }
        (Json::Arr(ve), Json::Arr(va)) => {
            for (i, (x, y)) in ve.iter().zip(va).enumerate() {
                let id = x.get("_id").and_then(Json::as_str).map_or(String::new(), |id| format!("(_id={id})"));
                if let Some(d) = diff(x, y, &format!("{path}[{i}]{id}")) {
                    return Some(d);
                }
            }
            (ve.len() != va.len()).then(|| {
                let extra = if ve.len() > va.len() { format!("missing {}", short(&ve[va.len()])) } else { format!("extra {}", short(&va[ve.len()])) };
                format!("{path}: expected {} items, got {} ({extra})", ve.len(), va.len())
            })
        }
        _ => (e != a).then(|| format!("{path}: expected {}, got {}", short(e), short(a))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Atom;

    #[test]
    fn diff_paths() {
        let e = Json::parse(r#"{"objects":[{"_id":"a","x":1,"y":2}],"history":{"a":1}}"#).unwrap();
        assert_eq!(first_diff(&e, &e), None);
        let a = Json::parse(r#"{"objects":[{"_id":"a","x":1,"y":3}],"history":null}"#).unwrap();
        assert_eq!(first_diff(&e, &a).unwrap(), "outcome.objects[0](_id=a).y: expected 2, got 3");
        let a = Json::parse(r#"{"objects":[{"_id":"a","y":2,"x":1}],"history":null}"#).unwrap();
        assert!(first_diff(&e, &a).unwrap().contains("key order"));
        let a = Json::parse(r#"{"objects":[],"history":{"a":1}}"#).unwrap();
        assert!(first_diff(&e, &a).unwrap().starts_with("outcome.objects: expected 1 items, got 0"));
    }

    #[test]
    fn placeholder_ids() {
        let mut n = 0;
        let mut o = Obj::new();
        o.set(Atom::intern("type"), Val::Str("energy".into()));
        let a = with_id(&o, "#", &mut n);
        o.set(Atom::intern("_id"), Val::Str("given".into()));
        let b = with_id(&o, "#", &mut n);
        let c = with_id(&a, "#", &mut n);
        assert_eq!(Val::Obj(a).to_json().stringify(), r##"{"type":"energy","_id":"#1"}"##);
        assert_eq!(b.get(Atom::intern("_id")).and_then(Val::as_str), Some("given"));
        assert_eq!(c.get(Atom::intern("_id")).and_then(Val::as_str), Some("#1"));
    }

    fn room(users: &str, intents: &str, objects: &str) -> RoomScenario {
        let j = format!(
            r#"{{"kind":"room","worldSeed":"0x1","clock":{{"baseMs":0,"baseTick":0,"nominalTickMs":1000}},"gameTime":5,"room":"W1N1","roomDoc":{{}},"terrain":"{}","objects":{objects},"flags":[],"users":{users},"intents":{intents}}}"#,
            "0".repeat(2500)
        );
        RoomScenario::from_json(&Json::parse(&j).unwrap()).unwrap()
    }

    /// The oracle stores intents in an object keyed by user id: "3" runs before "u9" whatever the array says.
    #[test]
    fn intents_follow_js_key_order() {
        let s = room(r#"{"u9":{"_id":"u9"},"3":{"_id":"3"}}"#, r#"[["u9",{"objects":{"a":{}},"objectsManual":null}],["3",{"objects":{"b":{}},"objectsManual":null}]]"#, "[]");
        let ids: crate::world::UserIds = s.users.iter().enumerate().map(|(i, (u, _))| (Id::new(u).unwrap(), i as UserIdx)).collect();
        let (intents, over) = s.room_intents(&ids).unwrap();
        let order: Vec<&str> = intents.users.iter().map(|u| s.users[u.user as usize].0.as_str()).collect();
        assert_eq!(order, ["3", "u9"]);
        assert!(over.is_empty(), "ORD-POLICY already puts \"3\" first");
        let dup = room(r#"{"u9":{"_id":"u9"}}"#, r#"[["u9",{"objects":null,"objectsManual":null}],["u9",{"objects":null,"objectsManual":null}]]"#, "[]");
        assert!(dup.room_intents(&ids).err().unwrap().contains("two intents entries"));
    }

    #[test]
    fn users_get_their_id_and_foreign_docs_are_refused() {
        let s = room(r#"{"u1":{"cpu":1}}"#, "[]", r#"[{"_id":"a","type":"source","room":"W2N2","x":1,"y":1}]"#);
        assert_eq!(Val::Obj(s.user_docs()[0].clone()).to_json().stringify(), r#"{"_id":"u1","cpu":1}"#);
        assert!(s.run().unwrap_err().contains("another room"));
    }

    #[test]
    fn split_global_intent_owners_are_not_replayable() {
        let g = |docs: &str| {
            let j = format!(r#"{{"kind":"global","worldSeed":"0x1","clock":{{"baseMs":0,"baseTick":0,"nominalTickMs":1000}},"gameTime":5,"db":{{"users.intents":{docs}}}}}"#);
            GlobalScenario::from_json(&Json::parse(&j).unwrap()).unwrap()
        };
        assert_eq!(g(r#"[{"user":"a"},{"user":"a"},{"user":"b"}]"#).intent_owners().unwrap(), ["a", "a", "b"]);
        assert!(g(r#"[{"user":"a"},{"user":"b"},{"user":"a"}]"#).intent_owners().is_err());
        assert!(g(r#"[{"user":"a"},{"user":"b"},{"user":"b"},{"user":"a"}]"#).run().unwrap_err().contains("not adjacent"));
    }

    #[test]
    fn meta_grid() {
        let m = rooms_meta(&["W1N1", "E0S0", "W2N3"], 12).unwrap();
        assert_eq!((m.x0, m.y0, m.w, m.h), (-3, -4, 4, 5));
        for (i, r) in m.rooms.iter().enumerate() {
            assert_eq!(m.grid[((r.xy.y - m.y0) as u32 * m.w + (r.xy.x - m.x0) as u32) as usize], i as u32 + 1);
        }
    }
}
