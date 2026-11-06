//! mirror: reference/engine/src/processor.js
//! parity: FAITHFUL
//!
//! Room processing: entry, context, results (ARCHITECTURE.md §5.1–5.2, FROZEN signatures) and P4 install
//! (§5.5). Owner: WP09. The module block below is WP00/WP11's (a new file is an interface PR).
//! `processRoom` is ported statement by statement and replayed against the official processor.js by the
//! fixtures in crates/engine/tests/fixtures/creeps (generate.js); the handler files share the JS-semantics
//! helpers at the end of this file. `objectsToHistory` is built only when `EngineConfig::history` (a listener exists).

// ---- module tree (§5.3) ----
pub mod common {
    pub mod fake_runtime;
}
pub mod global;
pub mod global_intents {
    pub mod market;
    pub mod power;
}
pub mod intents {
    pub mod _calc_spawns;
    pub mod _create_energy;
    pub mod _damage;
    pub mod constructed_walls {
        pub mod tick;
    }
    pub mod construction_sites {
        pub mod tick;
    }
    pub mod containers {
        pub mod tick;
    }
    pub mod controllers {
        pub mod activate_safe_mode;
        pub mod tick;
        pub mod unclaim;
    }
    pub mod creeps {
        pub mod _add_fatigue;
        pub mod _clear_newbie_walls;
        pub mod _die;
        pub mod _drop_resources_without_space;
        pub mod _recalc_body;
        pub mod attack;
        pub mod attack_controller;
        pub mod build;
        pub mod claim_controller;
        pub mod dismantle;
        pub mod drop;
        pub mod generate_safe_mode;
        pub mod harvest;
        pub mod heal;
        pub mod intents;
        pub mod invaders {
            pub mod find_attack;
            pub mod flee;
            pub mod healer;
            pub mod pretick;
            pub mod shoot_at_will;
        }
        pub mod keepers {
            pub mod pretick;
        }
        pub mod r#move;
        pub mod pickup;
        pub mod pull;
        pub mod ranged_attack;
        pub mod ranged_heal;
        pub mod ranged_mass_attack;
        pub mod repair;
        pub mod reserve_controller;
        pub mod say;
        pub mod sign_controller;
        pub mod suicide;
        pub mod tick;
        pub mod transfer;
        pub mod upgrade_controller;
        pub mod withdraw;
    }
    pub mod deposits {
        pub mod tick;
    }
    pub mod energy {
        pub mod tick;
    }
    pub mod extensions {
        pub mod tick;
    }
    pub mod extractors {
        pub mod tick;
    }
    pub mod factories {
        pub mod produce;
        pub mod tick;
    }
    // flags/intents.js is dead code in vanilla (07 FLAG-OBJ-DEAD): not mirrored.
    pub mod invader_core {
        pub mod attack_controller;
        pub mod create_creep;
        pub mod destroy;
        pub mod intents;
        pub mod pretick;
        pub mod reserve_controller;
        pub mod stronghold {
            pub mod creeps;
            pub mod defence;
            #[allow(clippy::module_inception)]
            pub mod stronghold;
        }
        pub mod tick;
        pub mod transfer;
        pub mod upgrade_controller;
    }
    pub mod keeper_lairs {
        pub mod tick;
    }
    pub mod labs {
        pub mod boost_creep;
        pub mod intents;
        pub mod reverse_reaction;
        pub mod run_reaction;
        pub mod tick;
        pub mod unboost_creep;
    }
    pub mod links {
        pub mod intents;
        pub mod tick;
        pub mod transfer;
    }
    pub mod minerals {
        pub mod tick;
    }
    pub mod movement;
    pub mod nukers {
        pub mod launch_nuke;
    }
    pub mod nukes {
        pub mod pretick;
        pub mod tick;
    }
    pub mod portals {
        pub mod tick;
    }
    pub mod power_creeps {
        pub mod drop;
        pub mod enable_room;
        pub mod intents;
        pub mod r#move;
        pub mod pickup;
        pub mod renew;
        pub mod say;
        pub mod tick;
        pub mod transfer;
        pub mod use_power;
        pub mod withdraw;
    }
    pub mod power_spawns {
        pub mod intents;
        pub mod process_power;
    }
    pub mod ramparts {
        pub mod set_public;
        pub mod tick;
    }
    pub mod roads {
        pub mod tick;
    }
    pub mod room {
        pub mod create_construction_site;
        pub mod create_flag;
        pub mod destroy_structure;
        pub mod gen_energy;
        pub mod intents;
        pub mod remove_construction_site;
        pub mod remove_flag;
    }
    pub mod ruins {
        pub mod tick;
    }
    pub mod sources {
        pub mod tick;
    }
    pub mod spawns {
        pub mod _born_creep;
        pub mod _charge_energy;
        pub mod cancel_spawning;
        pub mod create_creep;
        pub mod intents;
        pub mod recycle_creep;
        pub mod renew_creep;
        pub mod set_spawn_directions;
        pub mod tick;
    }
    pub mod storages {
        pub mod tick;
    }
    pub mod structures {
        pub mod _destroy;
    }
    pub mod terminal {
        pub mod send;
        pub mod tick;
    }
    pub mod tombstones {
        pub mod tick;
    }
    pub mod towers {
        pub mod attack;
        pub mod heal;
        pub mod intents;
        pub mod repair;
        pub mod tick;
    }
}
// ---- end of module tree ----

/// `{k: v, …}` as a fresh live object.
macro_rules! obj {
    ($($k:literal => $v:expr),* $(,)?) => {{
        let o = $crate::live::O::new();
        $(o.set($crate::atom!($k), $v);)*
        $crate::live::V::Obj(o)
    }};
}
pub(crate) use obj;

/// A room object's `type`, for the tests every object goes through in the prescan and the tick loop.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum K {
    /// No type below (a mod's type, say).
    Other,
    Road,
    Rampart,
    ConstructedWall,
    Extension,
    Creep,
    ConstructionSite,
    Container,
    Controller,
    Deposit,
    Energy,
    Extractor,
    Factory,
    Flag,
    InvaderCore,
    KeeperLair,
    Lab,
    Link,
    Mineral,
    Nuke,
    Nuker,
    Observer,
    Portal,
    PowerBank,
    PowerCreep,
    PowerSpawn,
    Ruin,
    Source,
    Spawn,
    Storage,
    Terminal,
    Tombstone,
    Tower,
}

impl K {
    fn named(s: &str) -> K {
        match s {
            "road" => K::Road,
            "rampart" => K::Rampart,
            "constructedWall" => K::ConstructedWall,
            "extension" => K::Extension,
            "creep" => K::Creep,
            "constructionSite" => K::ConstructionSite,
            "container" => K::Container,
            "controller" => K::Controller,
            "deposit" => K::Deposit,
            "energy" => K::Energy,
            "extractor" => K::Extractor,
            "factory" => K::Factory,
            "flag" => K::Flag,
            "invaderCore" => K::InvaderCore,
            "keeperLair" => K::KeeperLair,
            "lab" => K::Lab,
            "link" => K::Link,
            "mineral" => K::Mineral,
            "nuke" => K::Nuke,
            "nuker" => K::Nuker,
            "observer" => K::Observer,
            "portal" => K::Portal,
            "powerBank" => K::PowerBank,
            "powerCreep" => K::PowerCreep,
            "powerSpawn" => K::PowerSpawn,
            "ruin" => K::Ruin,
            "source" => K::Source,
            "spawn" => K::Spawn,
            "storage" => K::Storage,
            "terminal" => K::Terminal,
            "tombstone" => K::Tombstone,
            "tower" => K::Tower,
            _ => K::Other,
        }
    }

    /// The type `ty` is `==` to: the only values `==` a name of letters are that string and an array joining to it.
    #[inline]
    fn of(ty: &V) -> K {
        match ty {
            V::Str(s) => K::named(s),
            V::Arr(_) => K::named(&to_js_string(ty)),
            _ => K::Other,
        }
    }
}

/// What the tick loop builds for `save`: the map view `{key: [[x, y], …]}`, keys in JS order. A point is its tile,
/// or `ODD_POINT` + its index in `odd` when x or y is not an integer tile coordinate.
pub struct MapView {
    lists: Vec<(Atom, Vec<u32>)>,
    odd: Vec<[V; 2]>,
    /// The last owner seen and its key (an owner's objects are many, and a key means hashing its id).
    user: Option<(Arc<str>, Atom)>,
}

const ODD_POINT: u32 = 2500;

impl Default for MapView {
    fn default() -> MapView {
        let lists = [atom!("w"), atom!("r"), atom!("pb"), atom!("p"), atom!("s"), atom!("c"), atom!("m"), atom!("k")].map(|k| (k, Vec::new())).into();
        MapView { lists, odd: Vec::new(), user: None }
    }
}

impl MapView {
    /// An owner's list. ponytail: user ids become map-view atoms (bounded by the user count).
    fn user_key(&mut self, user: &V) -> Atom {
        match user {
            V::Str(u) => self.user_str_key(u),
            _ => Atom::intern(&to_js_string(user)),
        }
    }

    fn user_str_key(&mut self, user: &Arc<str>) -> Atom {
        match &self.user {
            Some((id, k)) if id == user => *k,
            _ => {
                let k = Atom::intern(user);
                self.user = Some((user.clone(), k));
                k
            }
        }
    }

    fn list(&mut self, k: Atom) -> &mut Vec<u32> {
        let at = self.lists.iter().position(|(a, _)| *a == k).unwrap_or_else(|| {
            let at = crate::doc::js_insert_pos(&self.lists, k);
            self.lists.insert(at, (k, Vec::new()));
            at
        });
        &mut self.lists[at].1
    }

    fn push(&mut self, k: Atom, x: V, y: V) {
        let point = match (live::tile_coord(&x), live::tile_coord(&y)) {
            (Some(x), Some(y)) => (x * 50 + y) as u32,
            _ => {
                self.odd.push([x, y]);
                ODD_POINT + self.odd.len() as u32 - 1
            }
        };
        self.list(k).push(point);
    }

    /// `JSON.stringify`, ASCII-escaped, of what the object holds.
    fn to_json(&self) -> String {
        let digits = |out: &mut String, c: u32| {
            if c >= 10 {
                out.push((b'0' + (c / 10) as u8) as char);
            }
            out.push((b'0' + (c % 10) as u8) as char);
        };
        let points: usize = self.lists.iter().map(|(_, l)| l.len()).sum();
        let mut out = String::with_capacity(16 * self.lists.len() + 8 * points);
        out.push('{');
        for (i, (k, list)) in self.lists.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            crate::doc::write_str(&mut out, k.as_str(), true);
            out.push_str(":[");
            for (j, &p) in list.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                out.push('[');
                if p < ODD_POINT {
                    digits(&mut out, p / 50);
                    out.push(',');
                    digits(&mut out, p % 50);
                } else {
                    let [x, y] = &self.odd[(p - ODD_POINT) as usize];
                    live::write_json_item(&mut out, x, true);
                    out.push(',');
                    live::write_json_item(&mut out, y, true);
                }
                out.push(']');
            }
            out.push(']');
        }
        out.push('}');
        out
    }
}

use crate::atom;
use crate::bulk::{self, Bulk, DocStore, Target};
use crate::doc::{array_index, Atom, Doc, DocRef, Id, Obj, Val};
use crate::hooks;
use crate::intents::RoomIntents;
use crate::live::{self, is_equal, to_js_string, to_number, truthy, Crash, RoomObjects, A, O, V};
use crate::policy;
use crate::terrain::RoomTerrain;
use crate::world::{AccessSnapshot, EngineConfig, Inbox, Presence, RoomIdx, RoomSnapshot, RoomsMeta, TerrainStore, UserIdx, UserState, World};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Arc;

pub struct TickCtx<'w> {
    pub time: u64,
    pub vnow: i64,
    pub seed: u64,
    pub compat: &'w policy::Compat,
    pub config: &'w EngineConfig,
    pub meta: &'w RoomsMeta,
    pub terrain: &'w TerrainStore,
    pub access: &'w AccessSnapshot,
    /// As of P3 start.
    pub users: &'w [Arc<UserState>],
    pub user_ids: &'w crate::world::UserIds,
    pub hooks: &'w hooks::ProcessorHooks,
    pub order_override: Option<&'w [UserIdx]>,
}

/// One room's tick (§5.2). Never touches anything but `prev` and `ctx`; everything that crosses rooms is
/// returned in the result.
pub fn process_room(ctx: &TickCtx, room: RoomIdx, prev: &Arc<RoomSnapshot>, intents: crate::intents::RoomIntents) -> RoomResult {
    let mut out = run_room(ctx, room, ctx.terrain.get(room), prev, intents, OBJECT_TICKS);
    // §3.8: the object-index changes are worked out here, in parallel, and P4 only applies them
    if let Some(next) = &out.next {
        let mut ops = Vec::new();
        index_ops(room, &prev.objects, &next.objects, ctx.user_ids, &mut ops);
        out.index = Some((prev.clone(), ops));
    }
    out
}

/// P4 (§5.5): in RoomIdx order, ids/seq for inserts, install snapshots, user ops, indexes/presence,
/// activations (T+1), notifications, history; then a second pass for `foreign_inserts`.
pub fn install(w: &mut World, results: Vec<RoomResult>) {
    let mut results = results;
    results.sort_by_key(|r| r.room);
    let mut foreign = Vec::new();
    for r in results {
        let room = r.room;
        let draft = match (r.next, r.inserts.is_empty()) {
            (Some(d), _) => Some(d),
            (None, false) => Some(draft_of(&w.rooms[room as usize])),
            (None, true) => None,
        };
        if let Some(mut draft) = draft {
            let from_p3 = draft.objects.len();
            for doc in r.inserts {
                push_insert(w, &mut draft, doc);
            }
            // Flags inserted by room/intents (bulkFlags.insert) get their `_id` here, like objects.
            let flags: Vec<Arc<Obj>> = draft.flags.iter().map(|f| if f.get(atom!("_id")).is_some() { f.clone() } else { with_new_id(w, f) }).collect();
            draft.flags = flags.into();
            let next = Arc::new(RoomSnapshot::new(draft, w.time));
            let old = std::mem::replace(&mut w.rooms[room as usize], next);
            match r.index {
                // P3 compared the draft's docs with this very snapshot; the inserts (larger seqs) merge last
                Some((prev, mut ops)) if Arc::ptr_eq(&prev, &old) => {
                    index_ops(room, &[], &w.rooms[room as usize].objects[from_p3..], &w.user_ids, &mut ops);
                    apply_index(w, room, &old, ops);
                }
                _ => reindex(w, room, &old),
            }
        }
        apply_user_ops(w, r.user_ops);
        if r.activate_next {
            w.active.insert(room);
        }
        for (user, message) in r.notifications {
            send_notification(w, user, &message);
        }
        // ponytail: `history` and `stats` go to the history and stats mods, which have no sink in `World`
        // yet (interface request); they are dropped here.
        let costs = &mut w.index.room_cost_ns;
        if costs.len() <= room as usize {
            costs.resize(room as usize + 1, 0);
        }
        costs[room as usize] = r.cost_ns;
        foreign.extend(r.foreign_inserts);
    }
    // Nuke launches into other rooms: the target is not activated (XROOM-NUKE).
    for (room, doc) in foreign {
        let mut draft = draft_of(&w.rooms[room as usize]);
        push_insert(w, &mut draft, doc);
        let processed_at = w.rooms[room as usize].processed_at;
        let old = std::mem::replace(&mut w.rooms[room as usize], Arc::new(RoomSnapshot::new(draft, processed_at)));
        reindex(w, room, &old);
    }
}

pub struct RoomResult {
    pub room: RoomIdx,
    /// None ⇒ keep prev (unchanged, or crashed).
    pub next: Option<RoomDraft>,
    /// Bulk inserts in counter order; seq/_id in P4.
    pub inserts: Vec<Obj>,
    pub crashed: Option<live::Crash>,
    /// ACT-RULE 1–7 (false on crash).
    pub activate_next: bool,
    /// bulkUsers.execute()
    pub user_ops: Vec<bulk::DbOp>,
    /// Nuke launch (BULK-COLLECTION-WIDE).
    pub foreign_inserts: Vec<(RoomIdx, Obj)>,
    /// Fire-and-forget, kept on crash.
    pub notifications: Vec<(UserIdx, Box<str>)>,
    /// objectsToHistory JSON when ACT-RULE fired and the history mod is on.
    pub history: Option<Box<str>>,
    /// stats.inc → mods only (POST-STATS).
    pub stats: Vec<(UserIdx, Atom, f64)>,
    pub cost_ns: u32,
    /// `process_room`: the object-index changes from the snapshot it read to `next.objects` (§3.8). None ⇒ P4
    /// works them out itself.
    pub index: Option<(Arc<RoomSnapshot>, Vec<IndexOp>)>,
}

pub struct RoomDraft {
    pub info: Arc<Obj>,
    /// Without inserts.
    pub objects: Vec<DocRef>,
    pub flags: Arc<[Arc<Obj>]>,
    pub event_log: Arc<str>,
    pub map_view: Arc<str>,
    /// Over existing docs; P4 patches first_vision_seq for inserts.
    pub presence: Vec<(UserIdx, Presence)>,
    pub user_refs: Vec<UserIdx>,
    pub has_movers: bool,
    pub has_terminal_send: bool,
    pub has_power_creeps: bool,
}

/// processor.js `scope` (fields = its keys, snake_cased) + movement.js module state + RNG + outputs.
pub struct RoomScope<'a> {
    pub ctx: &'a TickCtx<'a>,
    pub room: RoomIdx,
    pub room_name: &'a str,
    pub room_objects: live::RoomObjects,
    pub room_terrain: &'a RoomTerrain,
    pub game_time: u64,
    pub room_info: live::O,
    pub room_controller: Option<live::O>,
    /// Per-room clones of referenced user docs.
    pub users: Vec<(UserIdx, live::O)>,
    pub flags: Vec<live::O>,
    pub bulk: Bulk,
    pub bulk_users: Bulk,
    pub bulk_flags: Bulk,
    pub event_log: Vec<live::V>,
    pub map_view: MapView,
    /// `room_objects.version()` when the prescan started: while it holds, `room_spawns` and `room_extensions` still
    /// are the room's objects of those types (a `process_object` hook may change `room_objects` during the prescan).
    pub prescan_version: u32,
    pub energy_available: f64,
    pub room_spawns: Vec<live::O>,
    pub room_extensions: Vec<live::O>,
    pub keepers: Vec<live::O>,
    pub invaders: Vec<live::O>,
    pub invader_core: Option<live::O>,
    pub objects_to_history: Vec<live::O>,
    pub movement: intents::movement::Movement,
    /// Room stream, seeded on first draw.
    pub rng: policy::LazyRng,
    pub out: RoomResult,
}

// ---- dispatch tables (processor.js; owner WP09) ----

/// `module.exports = function(object, intent, scope)`.
pub type IntentFn = fn(&O, &V, &mut RoomScope) -> Result<(), Crash>;
/// `module.exports = function(object, scope)` of a `tick.js`.
pub type TickFn = fn(&O, &mut RoomScope) -> Result<(), Crash>;

/// processor.js:248-305 in vanilla's order. `None`: the type's `intents.js` gets the object's whole intents
/// object; `Some(name)`: the handler gets `objectIntents[name]` when it is truthy. Not here: the observer's
/// inline `observeRoom` assignment (processor.js:283) and `notifyWhenAttacked` (processor.js:307).
pub const OBJECT_INTENTS: &[(&str, Option<&str>, IntentFn)] = &[
    ("creep", None, intents::creeps::intents::run),
    ("powerCreep", None, intents::power_creeps::intents::run),
    ("link", None, intents::links::intents::run),
    ("tower", None, intents::towers::intents::run),
    ("lab", None, intents::labs::intents::run),
    ("spawn", None, intents::spawns::intents::run),
    ("rampart", Some("setPublic"), intents::ramparts::set_public::run),
    ("terminal", Some("send"), intents::terminal::send::run),
    ("nuker", Some("launchNuke"), intents::nukers::launch_nuke::run),
    ("powerSpawn", None, intents::power_spawns::intents::run),
    ("invaderCore", None, intents::invader_core::intents::run),
    ("factory", Some("produce"), intents::factories::produce::run),
    ("controller", Some("unclaim"), intents::controllers::unclaim::run),
    ("controller", Some("activateSafeMode"), intents::controllers::activate_safe_mode::run),
];

/// processor.js:341-394 tick dispatch in vanilla's order. Not here: observer `observeRoom` persistence,
/// the collapse timer and powerBank/deposit decay, which processor.js runs inline.
pub static OBJECT_TICKS: &[(&str, TickFn)] = &[
    ("invaderCore", intents::invader_core::tick::tick),
    ("energy", intents::energy::tick::tick),
    ("source", intents::sources::tick::tick),
    ("deposit", intents::deposits::tick::tick),
    ("mineral", intents::minerals::tick::tick),
    ("creep", intents::creeps::tick::tick),
    ("powerCreep", intents::power_creeps::tick::tick),
    ("spawn", intents::spawns::tick::tick),
    ("rampart", intents::ramparts::tick::tick),
    ("extension", intents::extensions::tick::tick),
    ("road", intents::roads::tick::tick),
    ("constructionSite", intents::construction_sites::tick::tick),
    ("keeperLair", intents::keeper_lairs::tick::tick),
    ("portal", intents::portals::tick::tick),
    ("constructedWall", intents::constructed_walls::tick::tick),
    ("link", intents::links::tick::tick),
    ("extractor", intents::extractors::tick::tick),
    ("tower", intents::towers::tick::tick),
    ("controller", intents::controllers::tick::tick),
    ("lab", intents::labs::tick::tick),
    ("container", intents::containers::tick::tick),
    ("terminal", intents::terminal::tick::tick),
    ("tombstone", intents::tombstones::tick::tick),
    ("ruin", intents::ruins::tick::tick),
    ("factory", intents::factories::tick::tick),
    ("nuke", intents::nukes::tick::tick),
    ("storage", intents::storages::tick::tick),
];

// ---- processRoom (processor.js:16-520) ----

/// KEEPER_ID / INVADER_ID (processor.js:13-14).
const KEEPER_ID: &str = "3";
const INVADER_ID: &str = "2";

/// `process_room` with an explicit tick table, so tests can stand in fakes for other packages' stubs.
pub(crate) fn run_room<'a>(ctx: &'a TickCtx<'a>, room: RoomIdx, terrain: &'a RoomTerrain, prev: &RoomSnapshot, intents: RoomIntents, ticks: &[(&str, TickFn)]) -> RoomResult {
    let started = std::time::Instant::now();
    let mut scope = load(ctx, room, terrain, prev);
    // oldRoomInfo = _.clone(roomInfo): shallow.
    let old_room_info = O::new();
    live::assign(&old_room_info, &scope.room_info);
    let mut out = match run_steps(&mut scope, intents, ticks) {
        Ok(activate) => save(scope, prev, &old_room_info, activate),
        Err(e) => {
            // PROC-ERROR / COMPAT-CRASH-ROOM: every buffered write is dropped, no activation; fire-and-forget
            // notifications and stats stay.
            let mut out = scope.out;
            out.crashed = Some(e);
            // A nuke launch is a bulk insert in vanilla: dropped with the rest.
            out.foreign_inserts.clear();
            out
        }
    };
    out.cost_ns = started.elapsed().as_nanos().min(u32::MAX as u128) as u32;
    out
}

/// PROC-INPUT: the room's docs (id-keyed, seq order), its info, flags, and the user docs its objects name.
fn load<'a>(ctx: &'a TickCtx<'a>, room: RoomIdx, terrain: &'a RoomTerrain, prev: &RoomSnapshot) -> RoomScope<'a> {
    let room_name: &str = &ctx.meta.rooms[room as usize].name;
    let mut room_objects = RoomObjects::with_capacity(prev.objects.len());
    let mut user_set = BTreeSet::new();
    let mut last_user = V::Undef;
    for d in prev.objects.iter() {
        let o = live::from_doc(d);
        // getRoomObjects: users[obj.user] = true for a truthy user.
        let user = o.get(atom!("user"));
        if truthy(&user) && !live::strict_eq(&user, &last_user) {
            if let Some(u) = Id::new(&to_js_string(&user)).and_then(|id| ctx.user_ids.get(&id)) {
                user_set.insert(*u);
            }
            last_user = user;
        }
        let id = o.get(atom!("_id"));
        room_objects.insert(if let V::Str(id) = id { id } else { to_js_string(&id) }, o);
    }
    room_objects.loaded = room_objects.slot_count();
    let users = user_set.into_iter().filter_map(|u| Some((u, live::from_shared(&ctx.users.get(u as usize)?.doc)))).collect();
    RoomScope {
        ctx,
        room,
        room_name,
        room_objects,
        room_terrain: terrain,
        game_time: ctx.time,
        room_info: live::from_shared(&prev.info),
        room_controller: None,
        users,
        flags: prev.flags.iter().map(live::from_shared).collect(),
        bulk: Bulk::new(),
        bulk_users: Bulk::new(),
        bulk_flags: Bulk::new(),
        event_log: Vec::new(),
        map_view: MapView::default(),
        prescan_version: 0,
        energy_available: 0.0,
        room_spawns: Vec::new(),
        room_extensions: Vec::new(),
        keepers: Vec::new(),
        invaders: Vec::new(),
        invader_core: None,
        objects_to_history: Vec::new(),
        movement: Default::default(),
        rng: policy::room_stream(ctx.seed, ctx.time, room_name),
        out: RoomResult {
            room,
            next: None,
            inserts: Vec::new(),
            crashed: None,
            activate_next: false,
            user_ops: Vec::new(),
            foreign_inserts: Vec::new(),
            notifications: Vec::new(),
            history: None,
            stats: Vec::new(),
            cost_ns: 0,
            index: None,
        },
    }
}

/// One user's entry of `intents.users` (`{objects, objectsManual}`), kept as id-ordered lists: object ids are
/// never interned.
struct UserEntry {
    uid: Arc<str>,
    idx: Option<UserIdx>,
    objects: Vec<(Arc<str>, V)>,
    manual: Option<Vec<(Arc<str>, V)>>,
}

/// processor.js:43-489, up to and including the `processRoom` emit. Returns `activateRoom`.
fn run_steps(scope: &mut RoomScope, intents: RoomIntents, ticks: &[(&str, TickFn)]) -> Result<bool, Crash> {
    let ctx = scope.ctx;
    let game_time = V::Num(scope.game_time as f64);
    let mut activate = false;
    let mut room_nukes = Vec::new();

    // PROC-PRESCAN (processor.js:43-171).
    let inserts_at_prescan = scope.room_objects.inserts();
    scope.prescan_version = scope.room_objects.version();
    let mut energy_slots = Vec::new();
    for slot in scope.room_objects.key_order() {
        let Some(object) = scope.room_objects.at(slot) else { continue };
        if matches!(prescan(scope, &object, &game_time, &mut activate, &mut room_nukes), K::Spawn | K::Extension) {
            energy_slots.push(slot);
        }
        for f in &ctx.hooks.activation_rules {
            activate |= f(&object, scope);
        }
        for f in &ctx.hooks.process_object {
            f(&object, scope);
        }
    }

    // intents = intents || {users: {}}  (driver.pathFinder.make is the thread's PathFinder, nothing to do).
    let mut users: Vec<UserEntry> = Vec::new();
    for u in intents.users {
        let Some(state) = ctx.users.get(u.user as usize) else { continue };
        let objects: Vec<(Arc<str>, V)> = u.objects.iter().map(|(id, o)| (Arc::from(&**id), V::Obj(live_obj(o)))).collect();
        let manual: Vec<(Arc<str>, V)> = u.manual.iter().map(|(id, o)| (Arc::from(&**id), V::Obj(live_obj(o)))).collect();
        let entry = user_entry(&mut users, state.id.as_str(), Some(u.user));
        // ponytail: one P2 entry per (room, user) is expected; a repeated user's ids extend the first.
        extend_by_id(&mut entry.objects, objects);
        if !manual.is_empty() {
            extend_by_id(entry.manual.get_or_insert_with(Vec::new), manual);
        }
    }

    // PROC-NUKE-PRETICK: the pretick sees vanilla's `intents` shape; the per-object intents are shared, so its
    // `createCreep = null` writes land in `users`.
    for nuke in &room_nukes {
        let shaped = intents_shape(&users);
        intents::nukes::pretick::pretick(nuke, &shaped, scope)?;
    }

    // PROC-NPC-PRETICK (processor.js:180-219).
    for keeper in scope.keepers.clone() {
        let i = intents::creeps::keepers::pretick::pretick(&keeper, scope)?;
        merge_npc_intents(ctx, &mut users, &keeper.get(atom!("user")), &i);
    }
    for invader in scope.invaders.clone() {
        let i = intents::creeps::invaders::pretick::pretick(&invader, scope)?;
        merge_npc_intents(ctx, &mut users, &invader.get(atom!("user")), &i);
    }
    if let Some(core) = scope.invader_core.clone() {
        if truthy(&core.get(atom!("user"))) {
            let i = intents::invader_core::pretick::pretick(&core, scope)?;
            merge_npc_intents(ctx, &mut users, &core.get(atom!("user")), &i);
        }
    }

    // PROC-CALC-SPAWNS.
    if !scope.room_spawns.is_empty() || !scope.room_extensions.is_empty() {
        let (spawns, extensions) = (scope.room_spawns.clone(), scope.room_extensions.clone());
        intents::_calc_spawns::calc_spawns(&spawns, &extensions, scope)?;
    }

    intents::movement::init(scope);

    // PROC-INTENTS (processor.js:227-322), users in ORD-POLICY order.
    order_users(ctx, &mut users);
    for entry in &mut users {
        if let Some(manual) = entry.manual.take() {
            // userIntents.objects = userIntents.objects || {}; _.extend(userIntents.objects, objectsManual)
            extend_by_id(&mut entry.objects, manual);
        }
        // for (var objectId in userIntents.objects): integer-like ids first, ascending.
        for i in js_key_order(entry.objects.iter().map(|(k, _)| &**k)) {
            let (object_id, object_intents) = entry.objects[i].clone();
            let object = scope.room_objects.get(&object_id);
            if &*object_id == "room" {
                if let Some(u) = entry.idx {
                    intents::room::intents::run(u, &object_intents, scope)?;
                }
                continue;
            }
            if let Some(u) = entry.idx {
                for f in &ctx.hooks.pre_object_intents {
                    f(object.as_ref(), u, &object_intents, scope);
                }
            }
            let Some(object) = object else { continue };
            let user = object.get(atom!("user"));
            if truthy(&object.get(atom!("_skip"))) || truthy(&user) && !eq_str(&user, &entry.uid) {
                continue;
            }
            dispatch_intents(&object, &object_intents, entry.idx, scope)?;
        }
    }

    // PROC-MOVE-CHECK.
    let safe_mode_user = match &scope.room_controller {
        Some(c) if gt(&c.get(atom!("safeMode")), &game_time) => c.get(atom!("user")),
        _ => V::Bool(false),
    };
    intents::movement::check(scope, &safe_mode_user)?;

    // PROC-ENERGY-AVAILABLE: _(roomObjects).filter(!off && (spawn || extension)).sum('store.energy').
    // Those objects are the prescan's unless something joined the room since.
    let mut energies = Vec::new();
    let mut add_energy = |o: &O| {
        if !truthy(&o.get(atom!("off"))) {
            energies.push(match o.get(atom!("store")) {
                V::Undef | V::Null => V::Undef,
                store => get_or_undef(&store, atom!("energy")),
            });
        }
    };
    if scope.room_objects.inserts() == inserts_at_prescan {
        energy_slots.iter().filter_map(|&s| scope.room_objects.peek(s)).for_each(&mut add_energy);
    } else {
        scope.room_objects.iter().filter(|o| o.eq_str(atom!("type"), "spawn") || o.eq_str(atom!("type"), "extension")).for_each(&mut add_energy);
    }
    scope.energy_available = lodash_sum(&energies);

    // PROC-TICK-LOOP (processor.js:342-483).
    let kinds: Vec<K> = ticks.iter().map(|(t, _)| K::named(t)).collect();
    let quiet_ok = std::ptr::eq(ticks.as_ptr(), OBJECT_TICKS.as_ptr()) && !ctx.config.history && ctx.hooks.object_ticks.is_empty() && ctx.hooks.post_object.is_empty();
    let mut quiet = Quiet { time: scope.game_time as f64, level: V::Undef, wall_hits_max: 0.0, rampart_hits_max: 0.0 };
    for slot in scope.room_objects.key_order() {
        if quiet_ok {
            if let Some(object) = scope.room_objects.peek(slot) {
                if quiet_tick(object, &mut quiet, &mut scope.map_view, scope.room_controller.as_ref()) {
                    continue;
                }
            }
        }
        let Some(object) = scope.room_objects.at(slot) else { continue };
        if truthy(&object.get(atom!("_skip"))) {
            continue;
        }
        tick_object(&object, &game_time, ticks, &kinds, scope)?;
    }

    for f in &ctx.hooks.room_end {
        f(scope);
    }
    Ok(activate)
}

/// What `quiet_tick` keeps from one object to the next.
struct Quiet {
    time: f64,
    /// `roomController.level` and what it allows, last worked out.
    level: V,
    wall_hits_max: f64,
    rampart_hits_max: f64,
}

/// The tick-loop body for a stored road, wall or rampart with nothing to do, which is most objects of a grown room,
/// on the values the handlers would read: true when it has run, which leaves the object's point on the map view.
/// Anything out of the ordinary, in value or in type, is left to `tick_object`.
fn quiet_tick(object: &O, q: &mut Quiet, map_view: &mut MapView, controller: Option<&O>) -> bool {
    let done = object.with_stored(|doc| {
        let Some(Val::Str(ty)) = doc.get(atom!("type")) else { return false };
        let kind = K::named(ty);
        if !matches!(kind, K::Road | K::ConstructedWall | K::Rampart) {
            return false;
        }
        let absent = |k: Atom| matches!(doc.get(k), None | Some(Val::Null));
        if !absent(atom!("_skip")) || !absent(atom!("effects")) {
            return false;
        }
        let decays_later = || matches!(doc.get(atom!("nextDecayTime")), Some(Val::Num(next)) if *next != 0.0 && !next.is_nan() && !(q.time >= *next - 1.0));
        if let Some(c) = controller {
            if kind != K::Road {
                let level = c.get(atom!("level"));
                if !live::strict_eq(&level, &q.level) {
                    let Ok(wall) = intents::_calc_spawns::support::controller_structures("constructedWall", &level) else { return false };
                    let rampart = intents::_calc_spawns::support::by_index(&crate::constants::RAMPART_HITS_MAX, &level);
                    q.wall_hits_max = if truthy(&wall) { crate::constants::WALL_HITS_MAX as f64 } else { 0.0 };
                    q.rampart_hits_max = if truthy(&rampart) { to_number(&rampart) } else { 0.0 };
                    q.level = level;
                }
            }
        }
        let hits_max_is = |want: f64| matches!(doc.get(atom!("hitsMax")), Some(Val::Num(h)) if *h == want);
        let key = match kind {
            K::Road => {
                if !decays_later() || !absent(atom!("user")) {
                    return false;
                }
                atom!("r")
            }
            K::ConstructedWall => {
                let walled = controller.is_none() || hits_max_is(q.wall_hits_max);
                if !walled || !absent(atom!("newbieWall")) || !absent(atom!("ticksToLive")) || !absent(atom!("decayTime")) || !absent(atom!("user")) {
                    return false;
                }
                atom!("w")
            }
            _ => {
                let Some(Val::Str(user)) = doc.get(atom!("user")) else { return false };
                let Some(c) = controller else { return false };
                if user.is_empty() || &**user == "2" || !c.is_str(atom!("user"), user) || !hits_max_is(q.rampart_hits_max) || !decays_later() || !absent(atom!("newbieWall")) {
                    return false;
                }
                match doc.get(atom!("isPublic")) {
                    None | Some(Val::Null | Val::Bool(false)) => map_view.user_str_key(user),
                    Some(Val::Bool(true)) => return true,
                    Some(_) => return false,
                }
            }
        };
        let (Some(Val::Num(x)), Some(Val::Num(y))) = (doc.get(atom!("x")), doc.get(atom!("y"))) else { return false };
        let (Some(x), Some(y)) = (live::tile_coord(&V::Num(*x)), live::tile_coord(&V::Num(*y))) else { return false };
        map_view.list(key).push((x * 50 + y) as u32);
        true
    });
    done == Some(true)
}

/// The prescan body for one object (processor.js:48-167).
fn prescan(scope: &mut RoomScope, object: &O, game_time: &V, activate: &mut bool, room_nukes: &mut Vec<O>) -> K {
    let ty = object.get(atom!("type"));
    let kind = K::of(&ty);
    let user = object.get(atom!("user"));
    let reset = |object: &O, keys: &[Atom]| {
        object.set(atom!("_actionLog"), object.get(atom!("actionLog")));
        object.set(atom!("actionLog"), V::Obj(O::nulls(keys)));
    };
    if kind == K::Creep {
        object.set(atom!("_actionLog"), object.get(atom!("actionLog")));
        object.set(atom!("_ticksToLive"), V::Num(to_number(&object.get(atom!("ageTime"))) - to_number(game_time)));
        let log = [
            atom!("attacked"),
            atom!("healed"),
            atom!("attack"),
            atom!("rangedAttack"),
            atom!("rangedMassAttack"),
            atom!("rangedHeal"),
            atom!("harvest"),
            atom!("heal"),
            atom!("repair"),
            atom!("build"),
            atom!("say"),
            atom!("upgradeController"),
            atom!("reserveController"),
        ];
        object.set(atom!("actionLog"), V::Obj(O::nulls(&log)));
        if eq_str(&user, KEEPER_ID) {
            scope.keepers.push(object.clone());
        } else if eq_str(&user, INVADER_ID) && !truthy(&object.get(atom!("strongholdId"))) {
            scope.invaders.push(object.clone());
        }
    }
    if kind == K::InvaderCore {
        scope.invader_core = Some(object.clone());
        reset(object, &[atom!("transferEnergy"), atom!("reserveController"), atom!("attackController"), atom!("upgradeController")]);
        if truthy(&object.get(atom!("deployTime"))) {
            *activate = true;
        }
    }
    if kind == K::Link {
        reset(object, &[atom!("transferEnergy")]);
    }
    if kind == K::Lab {
        reset(object, &[atom!("runReaction"), atom!("reverseReaction")]);
    }
    if kind == K::Tower {
        reset(object, &[atom!("attack"), atom!("heal"), atom!("repair")]);
    }
    if kind == K::Controller {
        scope.room_controller = Some(object.clone());
        let reservation = object.get(atom!("reservation"));
        if truthy(&reservation) {
            let r_user = get_or_undef(&reservation, atom!("user"));
            let left = to_number(&get_or_undef(&reservation, atom!("endTime"))) - to_number(game_time);
            let limit = crate::constants::CONTROLLER_RESERVE_MAX as f64 - crate::constants::INVADER_CORE_CONTROLLER_POWER as f64 * crate::constants::CONTROLLER_RESERVE as f64;
            if eq_str(&r_user, INVADER_ID) && left < limit {
                *activate = true;
            }
        }
        if truthy(&user) && !live::strict_eq(&user, &V::str(INVADER_ID)) {
            *activate = true;
        }
    }
    if kind == K::Observer {
        object.set(atom!("observeRoom"), V::Null);
    }
    if !*activate
        && truthy(&user)
        && !eq_str(&user, KEEPER_ID)
        && !truthy(&object.get(atom!("userNotActive")))
        && kind != K::Flag
        && !truthy(&object.get(atom!("strongholdId")))
        && !matches!(&ty, V::Str(t) if &**t == "controller")
    {
        *activate = true;
    }
    if (kind == K::PowerBank || kind == K::Deposit) && to_number(game_time) > to_number(&object.get(atom!("decayTime"))) - 500.0 {
        *activate = true;
    }
    if kind == K::Energy {
        *activate = true;
    }
    if kind == K::Nuke {
        *activate = true;
        room_nukes.push(object.clone());
    }
    if kind == K::Tombstone || kind == K::Portal {
        *activate = true;
    }
    // `hasNewbieWalls` (constructedWall with decayTime and user) is computed but never used.
    if kind == K::Extension {
        scope.room_extensions.push(object.clone());
    }
    if kind == K::Spawn {
        scope.room_spawns.push(object.clone());
    }
    if kind == K::PowerCreep {
        reset(object, &[atom!("spawned"), atom!("attack"), atom!("attacked"), atom!("healed"), atom!("power"), atom!("say")]);
    }
    if kind == K::Factory {
        reset(object, &[atom!("produce")]);
    }
    kind
}

/// `intents.users[uid] = intents.users[uid] || {}`.
fn user_entry<'u>(users: &'u mut Vec<UserEntry>, uid: &str, idx: Option<UserIdx>) -> &'u mut UserEntry {
    match users.iter().position(|e| &*e.uid == uid) {
        Some(i) => &mut users[i],
        None => {
            users.push(UserEntry { uid: uid.into(), idx, objects: Vec::new(), manual: None });
            users.last_mut().unwrap()
        }
    }
}

/// `_.extend(objects, more)`: an existing id keeps its position, its value is replaced.
fn extend_by_id(objects: &mut Vec<(Arc<str>, V)>, more: Vec<(Arc<str>, V)>) {
    for (id, v) in more {
        match objects.iter_mut().find(|(k, _)| *k == id) {
            Some(slot) => slot.1 = v,
            None => objects.push((id, v)),
        }
    }
}

/// `objectsIntents[objId] = _.assign(ii, objectsIntents[objId] || {})` for each NPC AI intent
/// (processor.js:183-191): stored intents win key by key.
fn merge_npc_intents(ctx: &TickCtx, users: &mut Vec<UserEntry>, npc_user: &V, ai: &V) {
    let uid = to_js_string(npc_user);
    let idx = Id::new(&uid).and_then(|id| ctx.user_ids.get(&id).copied());
    let entry = user_entry(users, &uid, idx);
    let V::Obj(ai) = ai else { return };
    for id in ai.keys() {
        let ii = ai.get(id);
        let existing = entry.objects.iter().find(|(k, _)| &**k == id.as_str()).map(|(_, v)| v.clone());
        if let (V::Obj(dst), Some(V::Obj(src))) = (&ii, &existing) {
            live::assign(dst, src);
        }
        extend_by_id(&mut entry.objects, vec![(id.as_str().into(), ii)]);
    }
}

/// Vanilla's `intents` object (`{users: {uid: {objects: {id: intents}}}}`) for the nuke pretick, which only
/// walks its values (`_.forEach`). Users and objects are keyed by position ("0", "1", … in the same order):
/// ids must never be interned (§3.1, atoms are never freed), and this runs on every tick a nuke is in flight.
/// The per-object intents are shared, so the pretick's writes land in `users`.
fn intents_shape(users: &[UserEntry]) -> V {
    let at = |i: usize| Atom::intern(&i.to_string());
    let by_user = O::new();
    for (u, e) in users.iter().enumerate() {
        let objects = O::new();
        for (i, (_, v)) in e.objects.iter().enumerate() {
            objects.set(at(i), v.clone());
        }
        let entry = O::new();
        entry.set(atom!("objects"), V::Obj(objects));
        by_user.set(at(u), V::Obj(entry));
    }
    let shaped = O::new();
    shaped.set(atom!("users"), V::Obj(by_user));
    V::Obj(shaped)
}

/// ORD-POLICY point 1: integer-like user ids ascending, then players by the order override (test hook) or
/// `(orderKey, uid bytes)`.
fn order_users(ctx: &TickCtx, users: &mut [UserEntry]) {
    users.sort_by_cached_key(|e| match array_index(&e.uid) {
        Some(i) => (0u8, i as u64, Vec::new()),
        None => match ctx.order_override.and_then(|o| e.idx.and_then(|u| o.iter().position(|&x| x == u))) {
            Some(p) => (1, p as u64, Vec::new()),
            None => (2, policy::order_key(ctx.seed, ctx.time, &e.uid), e.uid.as_bytes().to_vec()),
        },
    });
}

/// Indices of `keys` in `for…in` order: canonical array indices ascending, then insertion order.
pub(crate) fn js_key_order<'k>(keys: impl Iterator<Item = &'k str>) -> Vec<usize> {
    let mut order: Vec<(Option<u32>, usize)> = keys.enumerate().map(|(i, k)| (array_index(k), i)).collect();
    order.sort_by_key(|&(n, i)| (n.is_none(), n, i));
    order.into_iter().map(|(_, i)| i).collect()
}

/// processor.js:248-319 for one object of one user.
fn dispatch_intents(object: &O, object_intents: &V, user: Option<UserIdx>, scope: &mut RoomScope) -> Result<(), Crash> {
    let ctx = scope.ctx;
    let ty = object.get(atom!("type"));
    for &(t, name, f) in OBJECT_INTENTS {
        if !eq_str(&ty, t) {
            continue;
        }
        match name {
            None => f(object, object_intents, scope)?,
            Some(n) => {
                let intent = prop(object_intents, Atom::intern(n))?;
                if truthy(&intent) {
                    f(object, &intent, scope)?;
                }
            }
        }
    }
    if eq_str(&ty, "observer") {
        let observe = prop(object_intents, atom!("observeRoom"))?;
        if truthy(&observe) {
            object.set(atom!("observeRoom"), prop(&observe, atom!("roomName"))?);
        }
    }
    // INT-NOTIFY-ATTACKED.
    let notify = prop(object_intents, atom!("notifyWhenAttacked"))?;
    if truthy(&notify) && (crate::utils::construction_cost(&ty).is_some() || eq_str(&ty, "creep") || eq_str(&ty, "powerCreep")) {
        let enabled = truthy(&prop(&notify, atom!("enabled"))?);
        scope.bulk.update(Target::Obj(object), &obj! { "notifyWhenAttacked" => V::Bool(enabled) })?;
    }
    // Mod intent handlers run at vanilla's `processObjectIntents` emit.
    for &(t, name, f) in &ctx.hooks.object_intents {
        if eq_str(&ty, t) {
            let intent = prop(object_intents, Atom::intern(name))?;
            if truthy(&intent) {
                f(object, &intent, scope)?;
            }
        }
    }
    if let Some(u) = user {
        for f in &ctx.hooks.post_object_intents {
            f(object, u, object_intents, scope);
        }
    }
    Ok(())
}

/// The tick-loop body for one object (processor.js:348-482).
fn tick_object(object: &O, game_time: &V, ticks: &[(&str, TickFn)], kinds: &[K], scope: &mut RoomScope) -> Result<(), Crash> {
    let ctx = scope.ctx;
    let ty = object.get(atom!("type"));
    let kind = K::of(&ty);
    if kind == K::Other {
        for &(t, f) in ticks {
            if eq_str(&ty, t) {
                f(object, scope)?;
            }
        }
    } else {
        for (i, k) in kinds.iter().enumerate() {
            if *k == kind {
                (ticks[i].1)(object, scope)?;
            }
        }
    }
    for &(t, f) in &ctx.hooks.object_ticks {
        if eq_str(&ty, t) {
            f(object, scope)?;
        }
    }
    if kind == K::Observer {
        scope.bulk.update(Target::Obj(object), &obj! { "observeRoom" => object.get(atom!("observeRoom")) })?;
    }

    let effects = object.get(atom!("effects"));
    if truthy(&effects) {
        let collapse = lodash_find(&effects, |e| lodash_match(e, atom!("effect"), &V::Num(crate::constants::EFFECT_COLLAPSE_TIMER as f64)));
        if let Some(c) = collapse {
            if le(&prop(&c, atom!("endTime"))?, game_time) {
                bulk_remove(&mut scope.bulk, &object.get(atom!("_id")));
                scope.room_objects.remove(&to_js_string(&object.get(atom!("_id"))));
                return Ok(());
            }
        }
    }

    if (kind == K::PowerBank || kind == K::Deposit) && to_number(game_time) >= to_number(&object.get(atom!("decayTime"))) - 1.0 {
        bulk_remove(&mut scope.bulk, &object.get(atom!("_id")));
        scope.room_objects.remove(&to_js_string(&object.get(atom!("_id"))));
    }

    // objectsToHistory (processor.js:428-440): non-creeps by reference (stringified at room end), creeps as a JSON
    // copy without private fields. Only saveRoomHistory reads it, so it is skipped without a listener.
    if ctx.config.history && kind != K::Flag {
        let mut entry = object.clone();
        if kind == K::Creep || kind == K::PowerCreep {
            let copy = match live::from_val(&Val::from_json(&util::json::Json::parse(&live::stringify(&V::Obj(object.clone()))).expect("JSON.stringify output parses"))) {
                V::Obj(o) => o,
                _ => unreachable!("an object stringifies to an object"),
            };
            copy.set(atom!("_id"), V::str(&to_js_string(&object.get(atom!("_id")))));
            copy.delete(atom!("_actionLog"));
            copy.delete(Atom::intern("_ticksToLive"));
            let say = prop(&object.get(atom!("actionLog")), atom!("say"))?;
            if truthy(&say) && !truthy(&prop(&say, atom!("isPublic"))?) {
                if let V::Obj(log) = copy.get(atom!("actionLog")) {
                    log.delete(atom!("say"));
                }
            }
            entry = copy;
        }
        scope.objects_to_history.push(entry);
    }

    let user = object.get(atom!("user"));
    let key = if truthy(&user) {
        let skip = kind == K::ConstructionSite
            || truthy(&object.get(atom!("newbieWall")))
            || kind == K::Ruin
            || kind == K::Tombstone
            || kind == K::Rampart && truthy(&object.get(atom!("isPublic")));
        (!skip).then(|| scope.map_view.user_key(&user))
    } else if kind == K::ConstructedWall {
        Some(atom!("w"))
    } else if kind == K::Road {
        Some(atom!("r"))
    } else if kind == K::PowerBank {
        Some(atom!("pb"))
    } else if kind == K::Portal {
        Some(atom!("p"))
    } else if kind == K::Source {
        Some(atom!("s"))
    } else if kind == K::Mineral || kind == K::Deposit {
        Some(atom!("m"))
    } else if kind == K::Controller {
        Some(atom!("c"))
    } else if kind == K::KeeperLair {
        Some(atom!("k"))
    } else if kind == K::Energy && eq_str(&object.get(atom!("resourceType")), "power") {
        Some(atom!("pb"))
    } else {
        None
    };
    if let Some(k) = key {
        scope.map_view.push(k, object.get(atom!("x")), object.get(atom!("y")));
    }
    for f in &ctx.hooks.post_object {
        f(object, scope);
    }
    Ok(())
}

/// processor.js:493-518: execute the writers, event log, map view, activation, roomInfo.
fn save(scope: RoomScope, prev: &RoomSnapshot, old_room_info: &O, mut activate: bool) -> RoomResult {
    let RoomScope { ctx, room_objects, room_info, bulk, bulk_users, bulk_flags, event_log, map_view, objects_to_history, mut out, .. } = scope;
    let map_view = map_view.to_json();
    let object_ops = bulk.execute();
    out.user_ops = bulk_users.execute();
    let flag_ops = bulk_flags.execute();
    let event_log = json_ascii(&V::Arr(A::from_vec(event_log)));

    // roomInfo.active may be set in intents (ACT-RULE 7).
    if truthy(&room_info.get(atom!("active"))) {
        activate = true;
        room_info.delete(atom!("active"));
    }
    out.activate_next = activate;
    if activate && ctx.config.history {
        // saveRoomHistory: `JSON.stringify(objectsToHistory)`, an object keyed by `_id`
        let map = O::new();
        for o in objects_to_history {
            map.set(Atom::intern(&to_js_string(&o.get(atom!("_id")))), V::Obj(o));
        }
        out.history = Some(live::stringify(&V::Obj(map)).into());
    }
    let info_changed = !is_equal(&V::Obj(room_info.clone()), &V::Obj(old_room_info.clone()));

    if object_ops.is_empty() && flag_ops.is_empty() && !info_changed && *event_log == *prev.event_log && *map_view == *prev.map_view {
        return out;
    }

    // saveRoomInfo: `rooms.update({_id}, {$set: roomInfo})`.
    let info = if info_changed {
        let mut info = (*prev.info).clone();
        if let Some(Val::Obj(set)) = live::to_val(&V::Obj(room_info)) {
            for (k, v) in set.iter() {
                info.set(k, v.clone());
            }
        }
        Arc::new(info)
    } else {
        prev.info.clone()
    };

    let mut objects = CowStore::new(prev.objects.iter().cloned());
    objects.objects = Some(&room_objects).filter(|o| o.loaded == prev.objects.len());
    bulk::apply(&mut objects, object_ops);
    out.inserts = objects.inserts;
    let objects: Vec<DocRef> = objects.items.into_iter().flatten().collect();

    let mut flags = CowStore::new(prev.flags.iter().cloned());
    bulk::apply(&mut flags, flag_ops);
    let flags: Vec<Arc<Obj>> = flags.items.into_iter().flatten().chain(flags.inserts.into_iter().map(Arc::new)).collect();

    let mut draft = RoomDraft {
        info,
        objects: Vec::new(),
        flags: flags.into(),
        event_log: event_log.into(),
        map_view: map_view.into(),
        presence: Vec::new(),
        user_refs: Vec::new(),
        has_movers: false,
        has_terminal_send: false,
        has_power_creeps: false,
    };
    for d in &objects {
        derive_add(&mut draft, &d.obj, d.seq, ctx.user_ids);
    }
    draft.objects = objects;
    out.next = Some(draft);
    out
}

/// A `JSON.stringify` of a live value, ASCII-escaped (a one-byte V8 string).
fn json_ascii(v: &V) -> String {
    let mut s = String::new();
    live::write_json(&mut s, v, true);
    s
}

/// A doc collection being written: copy-on-write documents in order, inserts staged.
struct CowStore<'r, T> {
    items: Vec<Option<T>>,
    index: Option<util::hash::FxHashMap<Id, usize>>,
    inserts: Vec<Obj>,
    /// When these are a room's object docs: its `roomObjects`, whose first slots number them in the same order.
    objects: Option<&'r RoomObjects>,
}

trait CowDoc: Clone {
    fn obj(&self) -> &Obj;
    fn obj_mut(&mut self) -> &mut Obj;
}

impl CowDoc for DocRef {
    fn obj(&self) -> &Obj {
        &self.obj
    }
    fn obj_mut(&mut self) -> &mut Obj {
        Arc::make_mut(self).obj_mut()
    }
}

impl CowDoc for Arc<Obj> {
    fn obj(&self) -> &Obj {
        self
    }
    fn obj_mut(&mut self) -> &mut Obj {
        Arc::make_mut(self)
    }
}

impl<T: CowDoc> CowStore<'_, T> {
    fn new<'r>(items: impl Iterator<Item = T>) -> CowStore<'r, T> {
        CowStore { items: items.map(Some).collect(), index: None, inserts: Vec::new(), objects: None }
    }

    /// The position of the document with this `_id` (Loki's unique index; built on first use).
    fn find(&mut self, id: &Id) -> Option<usize> {
        if let Some(objects) = self.objects {
            let Some(i) = objects.first_slot(id.as_str(), self.items.len()) else { return None };
            if self.items[i].as_ref().is_none_or(|d| d.obj().get(atom!("_id")).and_then(Val::as_str) == Some(id.as_str())) {
                return Some(i);
            }
            self.objects = None;
        }
        let items = &self.items;
        let index = self.index.get_or_insert_with(|| {
            let mut m = util::hash::FxHashMap::with_capacity_and_hasher(items.len(), Default::default());
            for (i, d) in items.iter().enumerate() {
                if let Some(id) = d.as_ref().and_then(|d| d.obj().get(atom!("_id"))).and_then(Val::as_str).and_then(Id::new) {
                    m.entry(id).or_insert(i);
                }
            }
            m
        });
        index.get(id).copied()
    }
}

impl<T: CowDoc> DocStore for CowStore<'_, T> {
    fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj> {
        let i = self.find(id)?;
        self.items[i].as_mut().map(CowDoc::obj_mut)
    }
    fn remove(&mut self, id: &Id) {
        if let Some(i) = self.find(id) {
            self.items[i] = None;
        }
    }
    fn insert(&mut self, doc: Obj) {
        self.inserts.push(doc);
    }
}

/// Presence, user refs and mover flags of one document (§3.4 RoomSnapshot derived fields).
fn derive_add(draft: &mut RoomDraft, doc: &Obj, seq: u64, user_ids: &crate::world::UserIds) {
    let kind = doc.get(atom!("type")).and_then(Val::as_str).unwrap_or("");
    let uidx = |v: Option<&Val>| -> Option<UserIdx> { v.and_then(Val::as_str).and_then(Id::new).and_then(|id| user_ids.get(&id).copied()) };
    let user = uidx(doc.get(atom!("user")));
    if let Some(u) = user {
        let at = match draft.presence.binary_search_by_key(&u, |(x, _)| *x) {
            Ok(i) => i,
            Err(i) => {
                draft.presence.insert(i, (u, Presence::default()));
                i
            }
        };
        let p = &mut draft.presence[at].1;
        p.objects += 1;
        if kind != "flag" && kind != "constructionSite" {
            p.vision += 1;
            if p.vision == 1 {
                p.first_vision_seq = seq;
            }
            if kind == "observer" && p.observe.is_none() {
                let target = doc.get(atom!("observeRoom")).map_or(V::Undef, live::from_val);
                p.observe = Some((seq, (*to_js_string(&target)).into()));
            }
        }
    }
    let nested = |k: Atom| match doc.get(k) {
        Some(Val::Obj(o)) => uidx(o.get(atom!("user"))),
        _ => None,
    };
    for u in [user, nested(atom!("reservation")), nested(atom!("sign"))].into_iter().flatten() {
        if let Err(i) = draft.user_refs.binary_search(&u) {
            draft.user_refs.insert(i, u);
        }
    }
    let is_mover = kind == "creep" || kind == "powerCreep";
    draft.has_movers |= is_mover && !matches!(doc.get(atom!("interRoom")), None | Some(Val::Null));
    draft.has_power_creeps |= kind == "powerCreep";
    draft.has_terminal_send |= kind == "terminal" && doc.get(atom!("send")).is_some_and(|v| truthy(&live::from_val(v)));
}

// ---- P4 helpers ----

/// A draft that rebuilds `snap` unchanged (for inserts into a room that produced no draft).
fn draft_of(snap: &RoomSnapshot) -> RoomDraft {
    RoomDraft {
        info: snap.info.clone(),
        objects: snap.objects.to_vec(),
        flags: snap.flags.clone(),
        event_log: snap.event_log.clone(),
        map_view: snap.map_view.clone(),
        presence: snap.presence.to_vec(),
        user_refs: snap.user_refs.to_vec(),
        has_movers: snap.has_movers,
        has_terminal_send: snap.has_terminal_send,
        has_power_creeps: snap.has_power_creeps,
    }
}

/// An insert gets `seq` and, unless it carries one, `_id` (storage genId appends it last).
fn push_insert(w: &mut World, draft: &mut RoomDraft, mut doc: Obj) {
    let seq = w.next_seq;
    w.next_seq += 1;
    if doc.get(atom!("_id")).and_then(Val::as_str).and_then(Id::new).is_none() {
        doc.set(atom!("_id"), Val::Str(w.ids.next().as_str().into()));
    }
    derive_add(draft, &doc, seq, &w.user_ids);
    draft.objects.push(Doc::new(seq, doc));
}

fn with_new_id(w: &mut World, f: &Obj) -> Arc<Obj> {
    let mut f = f.clone();
    f.set(atom!("_id"), Val::Str(w.ids.next().as_str().into()));
    Arc::new(f)
}

/// One §3.8 object-index change. `RemoveIf` removes the entry only while it still names this room and seq.
pub enum IndexOp {
    Set(Id, crate::world::ObjLoc),
    RemoveIf(Id, u64),
}

/// The object-index changes from docs `a` to docs `b` of one room (both in seq order), in merge order.
pub(crate) fn index_ops(room: RoomIdx, a: &[DocRef], b: &[DocRef], ids: &crate::world::UserIds, ops: &mut Vec<IndexOp>) {
    let owner = |d: &Doc| d.obj.get(atom!("user")).and_then(Val::as_str).and_then(Id::new).and_then(|id| ids.get(&id).copied());
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        // most docs are unchanged: the same pointer is the same seq, nothing to index, and the doc is not read
        if i < a.len() && j < b.len() && Arc::ptr_eq(&a[i], &b[j]) {
            i += 1;
            j += 1;
            continue;
        }
        let order = match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) => x.seq.cmp(&y.seq),
            (Some(_), None) => Ordering::Less,
            _ => Ordering::Greater,
        };
        match order {
            Ordering::Equal => {
                ops.push(IndexOp::Set(b[j].id(), crate::world::ObjLoc { room, seq: b[j].seq, owner: owner(&b[j]) }));
                i += 1;
                j += 1;
            }
            Ordering::Less => {
                ops.push(IndexOp::RemoveIf(a[i].id(), a[i].seq));
                i += 1;
            }
            Ordering::Greater => {
                ops.push(IndexOp::Set(b[j].id(), crate::world::ObjLoc { room, seq: b[j].seq, owner: owner(&b[j]) }));
                j += 1;
            }
        }
    }
}

/// §3.8 index upkeep for one replaced snapshot: objects (by a seq-ordered merge), presence, mover/terminal/power-creep
/// room sets.
fn reindex(w: &mut World, room: RoomIdx, old: &RoomSnapshot) {
    let mut ops = Vec::new();
    index_ops(room, &old.objects, &w.rooms[room as usize].objects, &w.user_ids, &mut ops);
    apply_index(w, room, old, ops);
}

/// `reindex` with the object-index changes already worked out.
fn apply_index(w: &mut World, room: RoomIdx, old: &RoomSnapshot, ops: Vec<IndexOp>) {
    let new = w.rooms[room as usize].clone();
    for op in ops {
        match op {
            IndexOp::Set(id, loc) => {
                w.index.objects.insert(id, loc);
            }
            IndexOp::RemoveIf(id, seq) => {
                if w.index.objects.get(&id).is_some_and(|l| l.room == room && l.seq == seq) {
                    w.index.objects.remove(&id);
                }
            }
        }
    }
    let presence = &mut w.index.presence;
    if presence.len() < w.users.len() {
        presence.resize_with(w.users.len(), Default::default);
    }
    for (u, _) in old.presence.iter() {
        if new.presence.binary_search_by_key(u, |(x, _)| *x).is_err() {
            if let Some(m) = presence.get_mut(*u as usize) {
                Arc::make_mut(m).remove(&room);
            }
        }
    }
    for (u, p) in new.presence.iter() {
        if let Some(m) = presence.get_mut(*u as usize) {
            Arc::make_mut(m).insert(room, p.clone());
        }
    }
    let flag = |set: &mut std::collections::BTreeSet<RoomIdx>, on: bool| {
        if on {
            set.insert(room);
        } else {
            set.remove(&room);
        }
    };
    flag(&mut w.index.movers, new.has_movers);
    flag(&mut w.index.terminal_sends, new.has_terminal_send);
    flag(&mut w.index.power_creep_rooms, new.has_power_creeps);
}

/// `UserState` has no `Clone` (FROZEN in world.rs); every field is cheap to copy.
pub(crate) fn clone_user(u: &UserState) -> UserState {
    UserState {
        id: u.id,
        seq: u.seq,
        doc: u.doc.clone(),
        auth: u.auth.clone(),
        code: u.code.clone(),
        memory: u.memory.clone(),
        memory_version: u.memory_version,
        segments: u.segments.clone(),
        public_segments: u.public_segments.clone(),
        inter_shard: u.inter_shard.clone(),
        console_queue: u.console_queue.clone(),
        power_creeps: u.power_creeps.clone(),
        inbox: u.inbox.clone(),
    }
}

/// bulkUsers ops on the users collection (Loki semantics, copy-on-write user docs).
pub(crate) fn apply_user_ops(w: &mut World, ops: Vec<bulk::DbOp>) {
    if ops.is_empty() {
        return;
    }
    struct Users<'w> {
        w: &'w World,
        touched: Vec<(UserIdx, Obj)>,
    }
    impl DocStore for Users<'_> {
        fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj> {
            let u = *self.w.user_ids.get(id)?;
            let i = match self.touched.iter().position(|(x, _)| *x == u) {
                Some(i) => i,
                None => {
                    self.touched.push((u, (*self.w.users.get(u as usize)?.doc).clone()));
                    self.touched.len() - 1
                }
            };
            Some(&mut self.touched[i].1)
        }
        // Room processing never removes or inserts users.
        fn remove(&mut self, _: &Id) {}
        fn insert(&mut self, _: Obj) {}
    }
    let mut store = Users { w, touched: Vec::new() };
    bulk::apply(&mut store, ops);
    for (u, doc) in store.touched {
        let next = UserState { doc: Arc::new(doc), ..clone_user(&w.users[u as usize]) };
        w.users[u as usize] = Arc::new(next);
    }
}

/// driver.sendNotification (POST-NOTIFICATIONS): `users.notifications.update({user, message, date ≤ now,
/// type:'msg'}, {$set: {…, date: now}, $inc: {count: 1}}, {upsert: true})`: every matching doc is updated
/// (runner notifications are dated ahead by `groupInterval`, so same-text docs can coexist), else one is
/// inserted; real clock (CLK-NOTIFY).
fn send_notification(w: &mut World, user: UserIdx, message: &str) {
    let Some(state) = w.users.get(user as usize) else { return };
    let now = policy::real_now_ms("CLK-NOTIFY") as f64;
    let uid: Arc<str> = state.id.as_str().into();
    let upsert = |mut doc: Obj| {
        doc.set(atom!("user"), Val::Str(uid.clone()));
        doc.set(atom!("message"), Val::Str(message.into()));
        doc.set(atom!("date"), Val::Num(now));
        doc.set(atom!("type"), Val::Str("msg".into()));
        // $inc: doc[key] = (doc[key] || 0) + 1
        let count = js_add(&or0(doc.get(atom!("count")).map_or(V::Undef, live::from_val)), &V::Num(1.0));
        doc.set(atom!("count"), live::to_val(&count).unwrap_or(Val::Null));
        doc
    };
    let mut notifications = state.inbox.notifications.clone();
    let mut hit = false;
    for n in notifications.iter_mut() {
        if n.get(atom!("user")).and_then(Val::as_str) == Some(&*uid)
            && n.get(atom!("message")).and_then(Val::as_str) == Some(message)
            && n.get(atom!("type")).and_then(Val::as_str) == Some("msg")
            && n.get(atom!("date")).and_then(Val::as_f64).is_some_and(|d| d <= now)
        {
            *n = Arc::new(upsert((**n).clone()));
            hit = true;
        }
    }
    if !hit {
        let mut doc = upsert(Obj::new());
        doc.set(atom!("_id"), Val::Str(w.ids.next().as_str().into()));
        notifications.push(Arc::new(doc));
    }
    let inbox = Arc::new(Inbox { notifications, messages: state.inbox.messages.clone() });
    let next = UserState { inbox, ..clone_user(state) };
    w.users[user as usize] = Arc::new(next);
}

// ---- scope services shared by the handler ports ----

impl RoomScope<'_> {
    /// `users[id]` of the room's user docs (`undefined` when not loaded).
    pub fn user_doc(&self, id: &V) -> V {
        let id = to_js_string(id);
        self.users.iter().find(|(_, o)| eq_str(&o.get(atom!("_id")), &id)).map_or(V::Undef, |(_, o)| V::Obj(o.clone()))
    }

    /// The world user a JS user id names.
    pub fn user_idx(&self, id: &V) -> Option<UserIdx> {
        Id::new(&to_js_string(id)).and_then(|id| self.ctx.user_ids.get(&id).copied())
    }

    /// `stats.inc(name, userId, amount)` (POST-STATS: mods only).
    pub fn stats_inc(&mut self, name: Atom, user: &V, amount: f64) {
        if let Some(u) = self.user_idx(user) {
            self.out.stats.push((u, name, amount));
        }
    }

    /// `driver.sendNotification(userId, message)`: fire-and-forget, kept on a crash.
    pub fn send_notification(&mut self, user: &V, message: String) {
        if let Some(u) = self.user_idx(user) {
            self.out.notifications.push((u, message.into()));
        }
    }
}

// ---- JS value semantics for the handler ports ----

/// A stored document as a fresh live object (a DB read).
pub(crate) fn live_obj(o: &Obj) -> O {
    live::from_obj(o)
}

/// V8's TypeError text for a property read from `undefined`/`null`.
pub(crate) fn read_crash(base: &V, key: &str) -> Crash {
    let what = if matches!(base, V::Null) { "null" } else { "undefined" };
    Crash(format!("Cannot read properties of {what} (reading '{key}')").into())
}

/// `base[key]`; reading from `undefined`/`null` throws.
pub(crate) fn prop(base: &V, key: Atom) -> Result<V, Crash> {
    match base {
        V::Undef | V::Null => Err(read_crash(base, key.as_str())),
        _ => Ok(get_or_undef(base, key)),
    }
}

/// `base[key]` where the base is known not to be nullish (lodash's tolerant reads): `undefined` otherwise.
pub(crate) fn get_or_undef(base: &V, key: Atom) -> V {
    match base {
        V::Obj(o) => o.get(key),
        V::Arr(a) if key == atom!("length") => V::Num(a.len() as f64),
        V::Arr(a) => key.array_index().map_or(V::Undef, |i| a.get(i as usize)),
        V::Str(s) if key == atom!("length") => V::Num(s.encode_utf16().count() as f64),
        V::Str(s) => key.array_index().and_then(|i| s.encode_utf16().nth(i as usize)).map_or(V::Undef, |u| V::Str(String::from_utf16_lossy(&[u]).into())),
        _ => V::Undef,
    }
}

/// `base[i]` for an integer `i` >= 0.
pub(crate) fn get_index(base: &V, i: f64) -> V {
    match base {
        V::Arr(a) => a.get(i as usize),
        _ => get_or_undef(base, Atom::intern(&util::num::to_js_string(i))),
    }
}

/// `base[key] = v`; writing to `undefined`/`null` throws, to other primitives is ignored (sloppy mode).
pub(crate) fn put(base: &V, key: Atom, v: V) -> Result<(), Crash> {
    match base {
        V::Undef | V::Null => {
            let what = if matches!(base, V::Null) { "null" } else { "undefined" };
            Err(Crash(format!("Cannot set properties of {what} (setting '{}')", key.as_str()).into()))
        }
        V::Obj(o) => {
            o.set(key, v);
            Ok(())
        }
        V::Arr(a) => {
            if let Some(i) = key.array_index() {
                a.set(i as usize, v);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `a == b` (abstract equality).
pub(crate) fn loose_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Undef | V::Null, V::Undef | V::Null) => true,
        (V::Undef | V::Null, _) | (_, V::Undef | V::Null) => false,
        (V::Num(x), V::Num(y)) => x == y,
        (V::Str(x), V::Str(y)) => x == y,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Obj(_) | V::Arr(_), V::Obj(_) | V::Arr(_)) => live::strict_eq(a, b),
        (V::Bool(_), _) => loose_eq(&V::Num(to_number(a)), b),
        (_, V::Bool(_)) => loose_eq(a, &V::Num(to_number(b))),
        (V::Obj(_) | V::Arr(_), _) => loose_eq(&V::Str(to_js_string(a)), b),
        (_, V::Obj(_) | V::Arr(_)) => loose_eq(a, &V::Str(to_js_string(b))),
        (V::Num(x), V::Str(_)) => *x == to_number(b),
        (V::Str(_), V::Num(y)) => to_number(a) == *y,
    }
}

/// `v == "literal"` without allocating.
pub(crate) fn eq_str(v: &V, s: &str) -> bool {
    match v {
        V::Str(x) => &**x == s,
        V::Undef | V::Null => false,
        V::Num(n) => *n == util::num::string_to_number(s),
        V::Bool(b) => (*b as u8 as f64) == util::num::string_to_number(s),
        V::Obj(_) | V::Arr(_) => &*to_js_string(v) == s,
    }
}

/// Abstract relational comparison: two strings compare by UTF-16 units, everything else as numbers.
fn js_cmp(a: &V, b: &V) -> Option<Ordering> {
    if let (V::Num(x), V::Num(y)) = (a, b) {
        return x.partial_cmp(y);
    }
    let prim = |v: &V| match v {
        V::Obj(_) | V::Arr(_) => V::Str(to_js_string(v)),
        _ => v.clone(),
    };
    match (prim(a), prim(b)) {
        (V::Str(x), V::Str(y)) => Some(x.encode_utf16().cmp(y.encode_utf16())),
        (x, y) => to_number(&x).partial_cmp(&to_number(&y)),
    }
}

pub(crate) fn lt(a: &V, b: &V) -> bool {
    js_cmp(a, b) == Some(Ordering::Less)
}
pub(crate) fn le(a: &V, b: &V) -> bool {
    matches!(js_cmp(a, b), Some(Ordering::Less | Ordering::Equal))
}
pub(crate) fn gt(a: &V, b: &V) -> bool {
    js_cmp(a, b) == Some(Ordering::Greater)
}
pub(crate) fn ge(a: &V, b: &V) -> bool {
    matches!(js_cmp(a, b), Some(Ordering::Greater | Ordering::Equal))
}

/// `a + b`: string concatenation when either side is (or converts to) a string, else numeric.
pub(crate) fn js_add(a: &V, b: &V) -> V {
    match (a, b) {
        (V::Str(_) | V::Obj(_) | V::Arr(_), _) | (_, V::Str(_) | V::Obj(_) | V::Arr(_)) => V::Str(format!("{}{}", to_js_string(a), to_js_string(b)).into()),
        _ => V::Num(to_number(a) + to_number(b)),
    }
}

/// `x || 0`.
pub(crate) fn or0(v: V) -> V {
    if truthy(&v) {
        v
    } else {
        V::Num(0.0)
    }
}

/// `Math.min(a, b)`: NaN wins, -0 < +0.
pub(crate) fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() {
            a
        } else {
            b
        }
    } else {
        a.min(b)
    }
}

/// `Math.max(a, b)`: NaN wins, +0 > -0.
pub(crate) fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() {
            b
        } else {
            a
        }
    } else {
        a.max(b)
    }
}

/// `Math.round(x)`: halves round up.
pub(crate) fn js_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let f = x.floor();
    let r = if x - f >= 0.5 { f + 1.0 } else { f };
    if r == 0.0 && (x < 0.0 || x.is_sign_negative()) {
        -0.0
    } else {
        r
    }
}

fn hi_word(x: f64) -> i32 {
    (x.to_bits() >> 32) as i32
}

fn lo_word(x: f64) -> u32 {
    x.to_bits() as u32
}

fn with_hi(x: f64, hi: i32) -> f64 {
    f64::from_bits(((hi as u32 as u64) << 32) | lo_word(x) as u64)
}

fn with_lo(x: f64, lo: u32) -> f64 {
    f64::from_bits((x.to_bits() & 0xffff_ffff_0000_0000) | lo as u64)
}

fn from_words(hi: i32, lo: u32) -> f64 {
    f64::from_bits(((hi as u32 as u64) << 32) | lo as u64)
}

/// `Math.pow`: V8's fdlibm `__ieee754_pow` (e_pow.c), with ECMAScript's NaN for `(±1) ** ±∞` and NaN `y`.
/// Rust's `powf` (the platform libm) differs in the last ulp for some inputs.
#[allow(clippy::excessive_precision, clippy::approx_constant, clippy::eq_op)] // fdlibm verbatim: constants, NaN as (z-z)/(z-z)
pub(crate) fn js_pow(x: f64, y: f64) -> f64 {
    if y.is_nan() || (x.abs() == 1.0 && y.is_infinite()) {
        return f64::NAN;
    }
    const BP: [f64; 2] = [1.0, 1.5];
    const DP_H: [f64; 2] = [0.0, 5.84962487220764160156e-01];
    const DP_L: [f64; 2] = [0.0, 1.35003920212974897128e-08];
    const TWO53: f64 = 9007199254740992.0;
    const HUGE: f64 = 1.0e300;
    const TINY: f64 = 1.0e-300;
    const L1: f64 = 5.99999999999994648725e-01;
    const L2: f64 = 4.28571428578550184252e-01;
    const L3: f64 = 3.33333329818377432918e-01;
    const L4: f64 = 2.72728123808534006489e-01;
    const L5: f64 = 2.30660745775561754067e-01;
    const L6: f64 = 2.06975017800338417784e-01;
    const P1: f64 = 1.66666666666666019037e-01;
    const P2: f64 = -2.77777777770155933842e-03;
    const P3: f64 = 6.61375632143793436117e-05;
    const P4: f64 = -1.65339022054652515390e-06;
    const P5: f64 = 4.13813679705723846039e-08;
    const LG2: f64 = 6.93147180559945286227e-01;
    const LG2_H: f64 = 6.93147182464599609375e-01;
    const LG2_L: f64 = -1.90465429995776804525e-09;
    const OVT: f64 = 8.0085662595372944372e-17;
    const CP: f64 = 9.61796693925975554329e-01;
    const CP_H: f64 = 9.61796700954437255859e-01;
    const CP_L: f64 = -7.02846165095275826516e-09;
    const IVLN2: f64 = 1.44269504088896338700e+00;
    const IVLN2_H: f64 = 1.44269502162933349609e+00;
    const IVLN2_L: f64 = 1.92596299112661746887e-08;

    let (hx, lx) = (hi_word(x), lo_word(x));
    let (hy, ly) = (hi_word(y), lo_word(y));
    let mut ix = hx & 0x7fffffff;
    let iy = hy & 0x7fffffff;

    // y == 0: x**0 = 1
    if (iy as u32 | ly) == 0 {
        return 1.0;
    }
    // x == 1: 1**y = 1
    if hx == 0x3ff00000 && lx == 0 {
        return 1.0;
    }
    // NaN arguments
    if ix > 0x7ff00000 || (ix == 0x7ff00000 && lx != 0) || iy > 0x7ff00000 || (iy == 0x7ff00000 && ly != 0) {
        return (x + 0.0) + (y + 0.0);
    }

    // yisint: 0 not an integer, 1 odd, 2 even (only needed when x < 0)
    let mut yisint = 0;
    if hx < 0 {
        if iy >= 0x43400000 {
            yisint = 2;
        } else if iy >= 0x3ff00000 {
            let k = (iy >> 20) - 0x3ff;
            if k > 20 {
                let j = ly >> (52 - k);
                if (j << (52 - k)) == ly {
                    yisint = 2 - (j & 1) as i32;
                }
            } else if ly == 0 {
                let j = iy >> (20 - k);
                if (j << (20 - k)) == iy {
                    yisint = 2 - (j & 1);
                }
            }
        }
    }

    // special values of y
    if ly == 0 {
        if iy == 0x7ff00000 {
            // y is ±inf
            return if ((ix - 0x3ff00000) as u32 | lx) == 0 {
                1.0
            } else if ix >= 0x3ff00000 {
                if hy >= 0 {
                    y
                } else {
                    0.0
                }
            } else if hy < 0 {
                -y
            } else {
                0.0
            };
        }
        if iy == 0x3ff00000 {
            return if hy < 0 { 1.0 / x } else { x };
        }
        if hy == 0x40000000 {
            return x * x;
        }
        if hy == 0x3fe00000 && hx >= 0 {
            return x.sqrt();
        }
    }

    let mut ax = x.abs();
    // special values of x
    if lx == 0 && (ix == 0x7ff00000 || ix == 0 || ix == 0x3ff00000) {
        let mut z = ax;
        if hy < 0 {
            z = 1.0 / z;
        }
        if hx < 0 {
            if ((ix - 0x3ff00000) | yisint) == 0 {
                z = (z - z) / (z - z);
            } else if yisint == 1 {
                z = -z;
            }
        }
        return z;
    }

    let mut n: i32 = ((hx as u32) >> 31) as i32 - 1;
    // (x<0)**(non-int) is NaN
    if (n | yisint) == 0 {
        return (x - x) / (x - x);
    }
    let mut s = 1.0;
    if (n | (yisint - 1)) == 0 {
        s = -1.0;
    }

    let (t1, t2);
    if iy > 0x41e00000 {
        // |y| > 2**31
        if iy > 0x43f00000 {
            if ix <= 0x3fefffff {
                return if hy < 0 { HUGE * HUGE } else { TINY * TINY };
            }
            if ix >= 0x3ff00000 {
                return if hy > 0 { HUGE * HUGE } else { TINY * TINY };
            }
        }
        if ix < 0x3fefffff {
            return if hy < 0 { s * HUGE * HUGE } else { s * TINY * TINY };
        }
        if ix > 0x3ff00000 {
            return if hy > 0 { s * HUGE * HUGE } else { s * TINY * TINY };
        }
        let t = ax - 1.0;
        let w = (t * t) * (0.5 - t * (3.3333333333333331e-01 - t * 0.25));
        let u = IVLN2_H * t;
        let v = t * IVLN2_L - w * IVLN2;
        let a = with_lo(u + v, 0);
        t1 = a;
        t2 = v - (a - u);
    } else {
        n = 0;
        if ix < 0x00100000 {
            ax *= TWO53;
            n -= 53;
            ix = hi_word(ax);
        }
        n += (ix >> 20) - 0x3ff;
        let j = ix & 0x000fffff;
        ix = j | 0x3ff00000;
        let k = if j <= 0x3988E {
            0
        } else if j < 0xBB67A {
            1
        } else {
            n += 1;
            ix -= 0x00100000;
            0
        };
        ax = with_hi(ax, ix);

        let u = ax - BP[k];
        let v = 1.0 / (ax + BP[k]);
        let ss = u * v;
        let s_h = with_lo(ss, 0);
        let t_h = from_words(((ix >> 1) | 0x20000000) + 0x00080000 + ((k as i32) << 18), 0);
        let t_l = ax - (t_h - BP[k]);
        let s_l = v * ((u - s_h * t_h) - s_h * t_l);
        let mut s2 = ss * ss;
        let mut r = s2 * s2 * (L1 + s2 * (L2 + s2 * (L3 + s2 * (L4 + s2 * (L5 + s2 * L6)))));
        r += s_l * (s_h + ss);
        s2 = s_h * s_h;
        let t_h = with_lo(3.0 + s2 + r, 0);
        let t_l = r - ((t_h - 3.0) - s2);
        let u = s_h * t_h;
        let v = s_l * t_h + t_l * ss;
        let p_h = with_lo(u + v, 0);
        let p_l = v - (p_h - u);
        let z_h = CP_H * p_h;
        let z_l = CP_L * p_h + p_l * CP + DP_L[k];
        let t = n as f64;
        let a = with_lo(((z_h + z_l) + DP_H[k]) + t, 0);
        t1 = a;
        t2 = z_l - (((a - t) - DP_H[k]) - z_h);
    }

    // (y1+y2)*(t1+t2)
    let y1 = with_lo(y, 0);
    let p_l = (y - y1) * t1 + y * t2;
    let mut p_h = y1 * t1;
    let mut z = p_l + p_h;
    let (j, i) = (hi_word(z), lo_word(z));
    if j >= 0x40900000 {
        if ((j - 0x40900000) as u32 | i) != 0 || p_l + OVT > z - p_h {
            return s * HUGE * HUGE;
        }
    } else if (j & 0x7fffffff) >= 0x4090cc00 && ((j.wrapping_sub(0xc090cc00u32 as i32)) as u32 | i != 0 || p_l <= z - p_h) {
        return s * TINY * TINY;
    }

    // 2**(p_h+p_l)
    let i = j & 0x7fffffff;
    let mut k = (i >> 20) - 0x3ff;
    let mut n = 0;
    if i > 0x3fe00000 {
        n = j + (0x00100000 >> (k + 1));
        k = ((n & 0x7fffffff) >> 20) - 0x3ff;
        let t = from_words(n & !(0x000fffff >> k), 0);
        n = ((n & 0x000fffff) | 0x00100000) >> (20 - k);
        if j < 0 {
            n = -n;
        }
        p_h -= t;
    }
    let t = with_lo(p_l + p_h, 0);
    let u = t * LG2_H;
    let v = (p_l - (t - p_h)) * LG2 + t * LG2_L;
    z = u + v;
    let w = v - (z - u);
    let t = z * z;
    let t1 = z - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    // V8 (src/base/ieee754.cc) groups this as `Divide(z * t1, (t1 - two) - (w + z * w))`, unlike fdlibm's
    // `(z*t1)/(t1-two)-(w+z*w)`; Math.pow follows V8.
    let r = (z * t1) / ((t1 - 2.0) - (w + z * w));
    z = 1.0 - (r - z);
    let j = hi_word(z) + (n << 20);
    if (j >> 20) <= 0 {
        z = scalbn(z, n);
    } else {
        z = with_hi(z, j);
    }
    s * z
}

/// fdlibm `scalbn`: `x * 2**n` without double rounding.
#[allow(clippy::excessive_precision)]
fn scalbn(x: f64, n: i32) -> f64 {
    const TWO54: f64 = 1.80143985094819840000e+16;
    const TWOM54: f64 = 5.55111512312578270212e-17;
    const HUGE: f64 = 1.0e300;
    const TINY: f64 = 1.0e-300;
    let mut x = x;
    let mut hx = hi_word(x);
    let lx = lo_word(x);
    let mut k = (hx & 0x7ff00000) >> 20;
    if k == 0 {
        if (lx | (hx & 0x7fffffff) as u32) == 0 {
            return x;
        }
        x *= TWO54;
        hx = hi_word(x);
        k = ((hx & 0x7ff00000) >> 20) - 54;
        if n < -50000 {
            return TINY * x;
        }
    }
    if k == 0x7ff {
        return x + x;
    }
    let k = k as i64 + n as i64;
    if k > 0x7fe {
        return HUGE * HUGE.copysign(x);
    }
    if k > 0 {
        return with_hi(x, (hx & !0x7ff00000) | ((k as i32) << 20));
    }
    if k <= -54 {
        return if n > 50000 { HUGE * HUGE.copysign(x) } else { TINY * TINY.copysign(x) };
    }
    with_hi(x, (hx & !0x7ff00000) | (((k + 54) as i32) << 20)) * TWOM54
}

/// `Math.exp`: V8's fdlibm `__ieee754_exp` (e_exp.c).
#[allow(clippy::excessive_precision, clippy::approx_constant)]
pub(crate) fn js_exp(x: f64) -> f64 {
    const HALF: [f64; 2] = [0.5, -0.5];
    const O_THRESHOLD: f64 = 7.09782712893383973096e+02;
    const U_THRESHOLD: f64 = -7.45133219101941108420e+02;
    const LN2_HI: [f64; 2] = [6.93147180369123816490e-01, -6.93147180369123816490e-01];
    const LN2_LO: [f64; 2] = [1.90821492927058770002e-10, -1.90821492927058770002e-10];
    const INVLN2: f64 = 1.44269504088896338700e+00;
    const P1: f64 = 1.66666666666666019037e-01;
    const P2: f64 = -2.77777777770155933842e-03;
    const P3: f64 = 6.61375632143793436117e-05;
    const P4: f64 = -1.65339022054652515390e-06;
    const P5: f64 = 4.13813679705723846039e-08;
    const TWOM1000: f64 = 9.33263618503218878990e-302;
    const HUGE: f64 = 1.0e300;

    let mut x = x;
    let hx_full = hi_word(x) as u32;
    let xsb = ((hx_full >> 31) & 1) as usize;
    let hx = hx_full & 0x7fffffff;
    let (mut hi, mut lo, mut k) = (0.0, 0.0, 0i32);

    if hx >= 0x40862E42 {
        if hx >= 0x7ff00000 {
            if ((hx & 0xfffff) | lo_word(x)) != 0 {
                return x + x;
            }
            return if xsb == 0 { x } else { 0.0 };
        }
        if x > O_THRESHOLD {
            return HUGE * HUGE;
        }
        if x < U_THRESHOLD {
            return TWOM1000 * TWOM1000;
        }
    }

    if hx > 0x3fd62e42 {
        if hx < 0x3FF0A2B2 {
            // V8's addition: Math.exp(1) === Math.E.
            if x == 1.0 {
                return std::f64::consts::E;
            }
            hi = x - LN2_HI[xsb];
            lo = LN2_LO[xsb];
            k = 1 - xsb as i32 - xsb as i32;
        } else {
            k = (INVLN2 * x + HALF[xsb]) as i32;
            let t = k as f64;
            hi = x - t * LN2_HI[0];
            lo = t * LN2_LO[0];
        }
        x = hi - lo;
    } else if hx < 0x3e300000 {
        if HUGE + x > 1.0 {
            return 1.0 + x;
        }
    } else {
        k = 0;
    }

    let t = x * x;
    let twopk = if k >= -1021 { from_words(0x3ff00000 + (k << 20), 0) } else { from_words(0x3ff00000 + ((k + 1000) << 20), 0) };
    let c = x - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    if k == 0 {
        return 1.0 - ((x * c) / (c - 2.0) - x);
    }
    let y = 1.0 - ((lo - (x * c) / (2.0 - c)) - hi);
    if k >= -1021 {
        if k == 1024 {
            return y * 2.0 * f64::from_bits(0x7fe0000000000000);
        }
        y * twopk
    } else {
        y * twopk * TWOM1000
    }
}

/// ToIntegerOrInfinity (slice bounds).
pub(crate) fn to_integer(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.trunc()
    }
}

/// `arr.slice(0, end)` of a JS array of numbers.
pub(crate) fn slice_to(v: &[f64], end: f64) -> &[f64] {
    let n = v.len() as f64;
    let e = to_integer(end);
    let e = if e < 0.0 { (n + e).max(0.0) } else { e.min(n) };
    &v[..e as usize]
}

/// lodash `_.sum(values)`: summed from the last element, `+v || 0` each.
pub(crate) fn lodash_sum(values: &[V]) -> f64 {
    let mut total = 0.0;
    for v in values.iter().rev() {
        let n = to_number(v);
        total += if n.is_nan() || n == 0.0 { 0.0 } else { n };
    }
    total
}

/// The elements lodash iterates for a collection: array items, object values, string characters.
pub(crate) fn lodash_values(v: &V) -> Vec<V> {
    match v {
        V::Arr(a) => a.to_vec(),
        V::Obj(o) => o.keys().into_iter().map(|k| o.get(k)).collect(),
        V::Str(s) => s.encode_utf16().map(|u| V::Str(String::from_utf16_lossy(&[u]).into())).collect(),
        _ => Vec::new(),
    }
}

/// `lodash_values(v).find(f)` is something, without the copy; `f` may not change `v`.
pub(crate) fn lodash_any(v: &V, mut f: impl FnMut(&V) -> Result<bool, Crash>) -> Result<bool, Crash> {
    match v {
        V::Arr(a) => a.with_items(|items| {
            for x in items {
                if f(x)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }),
        _ => {
            for x in lodash_values(v) {
                if f(&x)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

/// `v.type == t`.
pub(crate) fn part_is(part: &V, t: &str) -> bool {
    match part {
        V::Obj(o) => o.eq_str(atom!("type"), t),
        _ => eq_str(&get_or_undef(part, atom!("type")), t),
    }
}

/// `_.find(collection, pred)`.
pub(crate) fn lodash_find(v: &V, mut pred: impl FnMut(&V) -> bool) -> Option<V> {
    lodash_values(v).into_iter().find(|x| pred(x))
}

/// `lodash_match(o, key, &V::str(s))` without making the string.
pub(crate) fn lodash_match_str(o: &V, key: Atom, s: &str) -> bool {
    match o {
        V::Obj(x) => x.is_str(key, s),
        V::Arr(_) | V::Str(_) => matches!(get_or_undef(o, key), V::Str(x) if &*x == s),
        _ => false,
    }
}

/// One `_.matches({key: value})` test: `===` for comparable values, `_.isEqual` otherwise (NaN, objects).
pub(crate) fn lodash_match(o: &V, key: Atom, value: &V) -> bool {
    let have = match o {
        V::Obj(_) | V::Arr(_) | V::Str(_) => get_or_undef(o, key),
        _ => return false,
    };
    let comparable = !matches!(value, V::Obj(_) | V::Arr(_)) && !matches!(value, V::Num(n) if n.is_nan());
    if comparable {
        live::strict_eq(&have, value) && (!matches!(value, V::Undef) || matches!(o, V::Obj(x) if x.has(key)))
    } else {
        is_equal(&have, value)
    }
}

/// `bulk.remove(id)`, a no-op for a falsy id (bulk.js:79).
pub(crate) fn bulk_remove(bulk: &mut Bulk, id: &V) {
    if truthy(id) {
        bulk.remove(&to_js_string(id));
    }
}

// ---- small shared idioms of the handler files ----

/// `object.type == t`.
pub(crate) fn type_is(o: &O, t: &str) -> bool {
    o.eq_str(atom!("type"), t)
}

/// `Math.abs(target.x - object.x) > r || Math.abs(target.y - object.y) > r`.
pub(crate) fn out_of_range(object: &O, target: &O, r: f64) -> bool {
    let d = |k| (to_number(&target.get(k)) - to_number(&object.get(k))).abs();
    d(atom!("x")) > r || d(atom!("y")) > r
}

/// `{x: o.x, y: o.y}`.
pub(crate) fn xy_of(o: &O) -> V {
    obj! { "x" => o.get(atom!("x")), "y" => o.get(atom!("y")) }
}

/// `{[k]: v}`.
pub(crate) fn obj1(k: Atom, v: V) -> V {
    let o = O::new();
    o.set(k, v);
    V::Obj(o)
}

/// `eventLog.push({event, objectId, data})`.
pub(crate) fn push_event(scope: &mut RoomScope, event: u32, object_id: V, data: V) {
    scope.event_log.push(obj! { "event" => V::Num(event as f64), "objectId" => object_id, "data" => data });
}

/// `bulk.update(object, data)` on the objects writer.
pub(crate) fn update(scope: &mut RoomScope, object: &O, data: &V) -> Result<(), Crash> {
    scope.bulk.update(Target::Obj(object), data)
}

/// The `Object.prototype` members a plain object inherits (V8/node 22).
const OBJECT_PROTOTYPE_KEYS: &[&str] = &[
    "__proto__",
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "toLocaleString",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// `roomObjects[id]` for a JS key value. `roomObjects` is a plain object, so an intent naming an
/// `Object.prototype` member gets that inherited (truthy) value, which has none of a room object's
/// properties; a fresh empty object stands in for it (e.g. withdraw from `"constructor"` crashes as vanilla).
pub(crate) fn room_object(scope: &RoomScope, id: &V) -> Option<O> {
    let key = to_js_string(id);
    scope.room_objects.get(&key).or_else(|| OBJECT_PROTOTYPE_KEYS.contains(&&*key).then(O::new))
}

/// `delete roomObjects[id]`.
pub(crate) fn delete_room_object(scope: &mut RoomScope, id: &V) {
    scope.room_objects.remove(&to_js_string(id));
}

/// Every object of the room in key order (an `_.forEach`/`_.find` snapshot).
pub(crate) fn room_object_list(scope: &RoomScope) -> Vec<O> {
    scope.room_objects.iter().cloned().collect()
}

/// `C.TABLE[key]` for a numeric table indexed by a JS value (RCL, level): `undefined` → NaN.
pub(crate) fn by_level<T: Copy + Into<f64>>(table: &[Option<T>], key: &V) -> f64 {
    array_index(&to_js_string(key)).and_then(|i| table.get(i as usize).copied().flatten()).map_or(f64::NAN, Into::into)
}

/// `arr.sort((a,b) => b-a).slice(0, n)` summed with `_.sum` (BODY-WORK-BOOST-BONUS).
pub(crate) fn top_boosts_sum(mut parts: Vec<f64>, n: f64) -> f64 {
    parts.sort_by(|a, b| b.partial_cmp(a).unwrap_or(Ordering::Equal));
    lodash_sum(&slice_to(&parts, n).iter().map(|&x| V::Num(x)).collect::<Vec<_>>())
}

// ---- constants.js lookups with JS property-read semantics ----
// ponytail: inherited Object.prototype keys (`C.BOOSTS.work.constructor`, …) are not modelled; game data
// never names them.

/// `C.BOOSTS[part][boost]`; reading `[boost]` of a missing `C.BOOSTS[part]` row throws.
pub(crate) fn boosts_entry(part: &str, boost: &V) -> Result<Option<crate::constants::BoostEffects>, Crash> {
    use std::str::FromStr;
    let row = crate::constants::BodyPart::from_str(part).ok().and_then(|p| crate::constants::BOOSTS.iter().find(|(q, _)| *q == p)).map(|(_, r)| *r);
    let Some(row) = row else { return Err(read_crash(&V::Undef, &to_js_string(boost))) };
    let key = to_js_string(boost);
    Ok(crate::constants::ResourceType::from_str(&key).ok().and_then(|r| row.iter().find(|(x, _)| *x == r)).map(|(_, e)| *e))
}

/// `C.BOOSTS[part][boost][method]` where the code reads it unguarded: a missing `[boost]` throws.
pub(crate) fn boost_effect(part: &str, boost: &V, method: &str) -> Result<V, Crash> {
    match boosts_entry(part, boost)? {
        Some(e) => Ok(e.iter().find(|(m, _)| *m == method).map_or(V::Undef, |(_, x)| V::Num(*x))),
        None => Err(read_crash(&V::Undef, method)),
    }
}

/// `_.contains(C.RESOURCES_ALL, v)`.
pub(crate) fn resource_type(v: &V) -> Option<crate::constants::ResourceType> {
    use std::str::FromStr;
    match v {
        V::Str(s) => crate::constants::ResourceType::from_str(s).ok(),
        _ => None,
    }
}

/// `_.contains(C.OBSTACLE_OBJECT_TYPES, v)`.
pub(crate) fn is_obstacle_type(v: &V) -> bool {
    matches!(v, V::Str(s) if crate::constants::OBSTACLE_OBJECT_TYPES.contains(&&**s))
}

#[cfg(test)]
pub(crate) mod tests {
    //! Hand-written room scenarios whose expected outcomes were derived by reading the vanilla sources
    //! (cited per test). They run `run_room` with the other packages' tick handlers replaced by no-ops, so
    //! only processor.js, movement.js and the creep files execute; oracle fixtures (WP03) replace them.
    use super::*;
    use crate::doc::Val;
    use crate::intents::UserRoomIntents;
    use crate::world::{CodeState, ObjLoc, RoomMeta, RoomStatus, WorldIndex};
    use std::collections::{BTreeMap, HashMap};
    use util::jsstr::JsStr;

    pub const T: u64 = 1000;
    pub const ROOM: &str = "W1N1";

    pub fn obj_of(json: &str) -> Obj {
        match Val::from_json(&util::json::Json::parse(json).unwrap_or_else(|e| panic!("{e:?}: {json}"))) {
            Val::Obj(o) => o,
            _ => panic!("not an object: {json}"),
        }
    }

    pub fn user_state(i: usize, id: &str) -> Arc<UserState> {
        Arc::new(UserState {
            id: Id::new(id).unwrap(),
            seq: i as u64,
            doc: Arc::new(obj_of(&format!(r#"{{"_id":"{id}","username":"{id}","gcl":0}}"#))),
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
        })
    }

    /// The world around one room "W1N1": users "2", "3", "u1", "u2" (UserIdx 0..3).
    pub struct Fixture {
        pub config: EngineConfig,
        pub meta: RoomsMeta,
        pub terrain_store: TerrainStore,
        pub terrain: RoomTerrain,
        pub access: AccessSnapshot,
        pub users: Vec<Arc<UserState>>,
        pub user_ids: crate::world::UserIds,
        pub hooks: hooks::ProcessorHooks,
        pub compat: policy::Compat,
        pub order: Option<Vec<UserIdx>>,
    }

    pub fn engine_config() -> EngineConfig {
        EngineConfig {
            main_loop_min_duration_ms: 1000,
            main_loop_reset_interval_ms: 5000,
            cpu_max_per_tick: 500,
            cpu_bucket_size: 10000,
            history_chunk_size: 20,
            socket_update_throttle_ms: 200,
            history: false,
            disabled_cronjobs: Vec::new(),
        }
    }

    pub fn rooms_meta() -> RoomsMeta {
        let xy = crate::room_name::RoomXY::parse_engine(ROOM).unwrap();
        RoomsMeta { x0: 0, y0: 0, w: 1, h: 1, grid: vec![1], rooms: vec![RoomMeta { name: ROOM.into(), xy, removed: false }], world_size: 4 }
    }

    impl Fixture {
        pub fn new() -> Fixture {
            let users: Vec<Arc<UserState>> = ["2", "3", "u1", "u2"].iter().enumerate().map(|(i, id)| user_state(i, id)).collect();
            let user_ids = users.iter().enumerate().map(|(i, u)| (u.id, i as UserIdx)).collect();
            Fixture {
                config: engine_config(),
                meta: rooms_meta(),
                terrain_store: TerrainStore::new(vec![RoomTerrain::default()]),
                terrain: RoomTerrain::default(),
                access: AccessSnapshot { version: 0, accessible: vec![1], status: HashMap::<RoomIdx, (RoomStatus, Option<i64>)>::new() },
                users,
                user_ids,
                hooks: hooks::ProcessorHooks::default(),
                compat: policy::Compat::new(policy::Profile::Private),
                order: None,
            }
        }

        pub fn ctx(&self) -> TickCtx<'_> {
            TickCtx {
                time: T,
                vnow: 0,
                seed: 1,
                compat: &self.compat,
                config: &self.config,
                meta: &self.meta,
                terrain: &self.terrain_store,
                access: &self.access,
                users: &self.users,
                user_ids: &self.user_ids,
                hooks: &self.hooks,
                order_override: self.order.as_deref(),
            }
        }

        /// One room tick with every non-creep tick handler replaced by a no-op.
        pub fn run(&self, prev: &RoomSnapshot, intents: RoomIntents) -> RoomResult {
            fn noop(_: &O, _: &mut RoomScope) -> Result<(), Crash> {
                Ok(())
            }
            let ticks: Vec<(&str, TickFn)> = OBJECT_TICKS.iter().map(|&(t, f)| (t, if t == "creep" { f } else { noop as TickFn })).collect();
            let ctx = self.ctx();
            run_room(&ctx, 0, &self.terrain, prev, intents, &ticks)
        }
    }

    /// Runs `f` on the scope of an empty room of a fresh fixture.
    pub fn with_scope<R>(f: impl FnOnce(&mut RoomScope) -> R) -> R {
        let fx = Fixture::new();
        let ctx = fx.ctx();
        let prev = snapshot(&[]);
        let mut scope = load(&ctx, 0, &fx.terrain, &prev);
        f(&mut scope)
    }

    pub fn snapshot(docs: &[String]) -> RoomSnapshot {
        RoomSnapshot::new(
            RoomDraft {
                info: Arc::new(obj_of(r#"{"_id":"W1N1","status":"normal"}"#)),
                objects: docs.iter().enumerate().map(|(i, d)| Doc::new(i as u64 + 1, obj_of(d))).collect(),
                flags: Arc::new([]),
                event_log: "[]".into(),
                map_view: "".into(),
                presence: Vec::new(),
                user_refs: Vec::new(),
                has_movers: false,
                has_terminal_send: false,
                has_power_creeps: false,
            },
            T - 1,
        )
    }

    /// The map view's JSON is what an object of arrays of `[x, y]` stringifies to.
    #[test]
    fn map_view_json() {
        let user_keys = [V::str("5f0a"), V::str("3"), V::str("2"), V::str("5f0a"), V::Num(7.0)];
        let (mut view, object) = (MapView::default(), O::new());
        for k in [atom!("w"), atom!("r"), atom!("pb"), atom!("p"), atom!("s"), atom!("c"), atom!("m"), atom!("k")] {
            object.set(k, V::Arr(A::new()));
        }
        let points = [(V::Num(0.0), V::Num(49.0)), (V::Num(-0.0), V::Num(12.0)), (V::Num(1.5), V::Undef), (V::str("7"), V::Null), (V::Num(50.0), V::Num(f64::NAN)), (V::Arr(A::from_vec(vec![V::Num(1.0)])), V::Num(3.0))];
        for (i, (x, y)) in points.iter().enumerate() {
            let k = view.user_key(&user_keys[i % user_keys.len()]);
            view.push(k, x.clone(), y.clone());
            view.push(atom!("r"), x.clone(), y.clone());
            for k in [k, atom!("r")] {
                let list = match object.get(k) {
                    V::Arr(a) => a,
                    _ => {
                        let a = A::new();
                        object.set(k, V::Arr(a.clone()));
                        a
                    }
                };
                list.push(V::Arr(A::from_vec(vec![x.clone(), y.clone()])));
            }
        }
        assert_eq!(view.to_json(), json_ascii(&V::Obj(object)));
    }

    pub fn intents(list: &[(UserIdx, &[(&str, &str)])]) -> RoomIntents {
        RoomIntents {
            users: list
                .iter()
                .map(|(u, objects)| UserRoomIntents { user: *u, objects: objects.iter().map(|(id, j)| (Box::from(*id), obj_of(j))).collect(), manual: Vec::new() })
                .collect(),
        }
    }

    pub const NULL_LOG: &str = r#"{"attacked":null,"healed":null,"attack":null,"rangedAttack":null,"rangedMassAttack":null,"rangedHeal":null,"harvest":null,"heal":null,"repair":null,"build":null,"say":null,"upgradeController":null,"reserveController":null}"#;

    /// A creep doc owned by u1 with a reset actionLog (so the tick does not persist it).
    pub fn creep(id: &str, x: i32, y: i32, parts: &[&str], store: &str, capacity: u32, extra: &str) -> String {
        let body: Vec<String> = parts.iter().map(|p| format!(r#"{{"type":"{p}","hits":100}}"#)).collect();
        let hits = parts.len() * 100;
        format!(
            r#"{{"_id":"{id}","type":"creep","x":{x},"y":{y},"room":"W1N1","name":"{id}","user":"u1","body":[{}],"hits":{hits},"hitsMax":{hits},"spawning":false,"fatigue":0,"store":{store},"storeCapacity":{capacity},"ageTime":2500,"actionLog":{NULL_LOG}{extra}}}"#,
            body.join(",")
        )
    }

    pub fn doc(r: &RoomResult, id: &str) -> Option<String> {
        r.next.as_ref().expect("room changed").objects.iter().find(|d| d.id().as_str() == id).map(|d| d.json().to_string())
    }

    pub fn inserts(r: &RoomResult) -> Vec<String> {
        r.inserts
            .iter()
            .map(|o| {
                let mut s = String::new();
                Val::Obj(o.clone()).write_json(&mut s);
                s
            })
            .collect()
    }

    pub fn event_log(r: &RoomResult) -> String {
        r.next.as_ref().expect("room changed").event_log.to_string()
    }

    pub const U1: UserIdx = 2;
    pub const U2: UserIdx = 3;

    /// creeps/move.js + movement.js: a plain step costs 0 fatigue for a MOVE-only creep (execute, _add-fatigue).
    #[test]
    fn move_one_step() {
        let f = Fixture::new();
        let prev = snapshot(&[creep("c1", 10, 10, &["move"], "{}", 0, "")]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"move":{"direction":3,"id":"undefined"}}"#)])]));
        assert!(r.crashed.is_none());
        assert_eq!(doc(&r, "c1").unwrap(), creep("c1", 11, 10, &["move"], "{}", 0, ""));
        assert!(r.activate_next, "an owned creep activates the room (processor.js:119)");
        assert_eq!(event_log(&r), "[]");
        assert!(r.inserts.is_empty() && r.user_ops.is_empty());
    }

    /// harvest.js:25-64 then upgradeController.js in creepActions order; GCL through bulkUsers.inc.
    #[test]
    fn harvest_then_upgrade() {
        let f = Fixture::new();
        let source = r#"{"_id":"s1","type":"source","x":11,"y":10,"room":"W1N1","energy":3000,"energyCapacity":3000}"#;
        let ctrl = r#"{"_id":"ctrl","type":"controller","x":12,"y":12,"room":"W1N1","level":1,"user":"u1","progress":0,"downgradeTime":20000}"#;
        let prev = snapshot(&[creep("c1", 10, 10, &["work", "carry", "move"], r#"{"energy":0}"#, 50, ""), source.into(), ctrl.into()]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"harvest":{"id":"s1"},"upgradeController":{"id":"ctrl"}}"#)])]));
        assert!(r.crashed.is_none(), "{:?}", r.crashed);
        let log = NULL_LOG.replace(r#""harvest":null"#, r#""harvest":{"x":11,"y":10}"#).replace(r#""upgradeController":null"#, r#""upgradeController":{"x":12,"y":12}"#);
        let want = creep("c1", 10, 10, &["work", "carry", "move"], r#"{"energy":1}"#, 50, "").replace(NULL_LOG, &log);
        assert_eq!(doc(&r, "c1").unwrap(), want);
        assert_eq!(doc(&r, "s1").unwrap(), r#"{"_id":"s1","type":"source","x":11,"y":10,"room":"W1N1","energy":2998,"energyCapacity":3000,"invaderHarvested":2}"#);
        assert_eq!(doc(&r, "ctrl").unwrap(), ctrl.replace(r#""progress":0"#, r#""progress":1"#));
        assert_eq!(r.user_ops, vec![bulk::DbOp::Inc(Id::new("u1").unwrap(), vec![atom!("gcl")], 1.0)]);
        assert_eq!(
            event_log(&r),
            r#"[{"event":5,"objectId":"c1","data":{"targetId":"s1","amount":2}},{"event":9,"objectId":"c1","data":{"amount":1,"energySpent":1}}]"#
        );
        assert_eq!(&*r.next.as_ref().unwrap().map_view, r#"{"w":[],"r":[],"pb":[],"p":[],"s":[[11,10]],"c":[],"m":[],"k":[],"u1":[[10,10],[12,12]]}"#);
        assert_eq!(r.stats, vec![(U1, atom!("energyHarvested"), 2.0), (U1, atom!("energyControl"), 1.0)]);
    }

    /// pickup.js: the whole pile fits, so it is removed from the DB and roomObjects.
    #[test]
    fn pickup_whole_pile() {
        let f = Fixture::new();
        let pile = r#"{"_id":"d1","type":"energy","x":11,"y":10,"room":"W1N1","energy":30,"resourceType":"energy"}"#;
        let prev = snapshot(&[creep("c1", 10, 10, &["carry", "move"], "{}", 50, ""), pile.into()]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"pickup":{"id":"d1"}}"#)])]));
        assert_eq!(doc(&r, "c1").unwrap(), creep("c1", 10, 10, &["carry", "move"], r#"{"energy":30}"#, 50, ""));
        assert!(doc(&r, "d1").is_none());
    }

    /// build.js:9-107 progress, then build.js:108-293 completion (road on plain).
    #[test]
    fn build_progress_and_completion() {
        let f = Fixture::new();
        let site = |progress: u32| format!(r#"{{"_id":"cs1","type":"constructionSite","x":10,"y":12,"room":"W1N1","user":"u1","structureType":"road","progress":{progress},"progressTotal":300}}"#);
        let body = ["work", "work", "carry", "move"];
        let log = NULL_LOG.replace(r#""build":null"#, r#""build":{"x":10,"y":12}"#);

        let prev = snapshot(&[creep("c1", 10, 10, &body, r#"{"energy":50}"#, 50, ""), site(0)]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"build":{"id":"cs1","x":10,"y":12}}"#)])]));
        assert_eq!(doc(&r, "cs1").unwrap(), site(10));
        assert_eq!(doc(&r, "c1").unwrap(), creep("c1", 10, 10, &body, r#"{"energy":40}"#, 50, "").replace(NULL_LOG, &log));
        assert_eq!(event_log(&r), r#"[{"event":4,"objectId":"c1","data":{"targetId":"cs1","amount":10,"structureType":"road","x":10,"y":12,"incomplete":true}}]"#);

        let prev = snapshot(&[creep("c1", 10, 10, &body, r#"{"energy":50}"#, 50, ""), site(295)]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"build":{"id":"cs1","x":10,"y":12}}"#)])]));
        assert!(doc(&r, "cs1").is_none());
        assert_eq!(doc(&r, "c1").unwrap(), creep("c1", 10, 10, &body, r#"{"energy":45}"#, 50, "").replace(NULL_LOG, &log));
        assert_eq!(inserts(&r), vec![r#"{"type":"road","x":10,"y":12,"room":"W1N1","notifyWhenAttacked":true,"hits":5000,"hitsMax":5000,"nextDecayTime":2000}"#]);
        assert!(event_log(&r).contains(r#""amount":5,"structureType":"road","x":10,"y":12,"incomplete":false"#));
        // The new road is ticked under its createdStructure key and lands in the map view.
        assert!(r.next.as_ref().unwrap().map_view.contains(r#""r":[[10,12]]"#));
    }

    /// repair.js: 1 WORK repairs 100 hits for ceil(100 × 0.01) = 1 energy.
    #[test]
    fn repair_road() {
        let f = Fixture::new();
        let road = r#"{"_id":"r1","type":"road","x":10,"y":11,"room":"W1N1","hits":4000,"hitsMax":5000,"nextDecayTime":5000}"#;
        let prev = snapshot(&[creep("c1", 10, 10, &["work", "carry", "move"], r#"{"energy":10}"#, 50, ""), road.into()]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"repair":{"id":"r1","x":10,"y":11}}"#)])]));
        assert_eq!(doc(&r, "r1").unwrap(), road.replace("4000", "4100"));
        assert!(doc(&r, "c1").unwrap().contains(r#""store":{"energy":9}"#));
        assert_eq!(event_log(&r), r#"[{"event":7,"objectId":"c1","data":{"targetId":"r1","amount":100,"energySpent":1}}]"#);
    }

    /// transfer.js clips to the container's free capacity; withdraw.js clips to the creep's free space and
    /// logs EVENT_TRANSFER with the ids reversed.
    #[test]
    fn transfer_and_withdraw() {
        let f = Fixture::new();
        let container = |e: u32| format!(r#"{{"_id":"ct1","type":"container","x":11,"y":10,"room":"W1N1","store":{{"energy":{e}}},"storeCapacity":2000,"hits":250000,"hitsMax":250000}}"#);

        let prev = snapshot(&[creep("c1", 10, 10, &["carry", "move"], r#"{"energy":50}"#, 50, ""), container(1990)]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"transfer":{"id":"ct1","amount":50,"resourceType":"energy"}}"#)])]));
        assert_eq!(doc(&r, "ct1").unwrap(), container(2000));
        assert!(doc(&r, "c1").unwrap().contains(r#""store":{"energy":40}"#));
        assert_eq!(event_log(&r), r#"[{"event":12,"objectId":"c1","data":{"targetId":"ct1","resourceType":"energy","amount":10}}]"#);

        let prev = snapshot(&[creep("c1", 10, 10, &["carry", "move"], "{}", 50, ""), container(100)]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"withdraw":{"id":"ct1","amount":100,"resourceType":"energy"}}"#)])]));
        assert_eq!(doc(&r, "ct1").unwrap(), container(50));
        assert!(doc(&r, "c1").unwrap().contains(r#""store":{"energy":50}"#));
        assert_eq!(event_log(&r), r#"[{"event":12,"objectId":"ct1","data":{"targetId":"c1","resourceType":"energy","amount":50}}]"#);
    }

    /// suicide.js → _die.js: lifeRate = 0.2 × 750 / 1500; body energy 10 + 5 + 5, plus the carried 7.
    #[test]
    fn suicide_tombstone() {
        let f = Fixture::new();
        let prev = snapshot(&[creep("c1", 10, 10, &["work", "carry", "move"], r#"{"energy":7}"#, 50, "").replace(r#""ageTime":2500"#, r#""ageTime":1750"#)]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"suicide":{}}"#)])]));
        assert!(doc(&r, "c1").is_none());
        assert_eq!(
            inserts(&r),
            vec![r#"{"type":"tombstone","room":"W1N1","x":10,"y":10,"user":"u1","deathTime":1000,"decayTime":1015,"creepId":"c1","creepName":"c1","creepTicksToLive":750,"creepBody":["work","carry","move"],"store":{"energy":27}}"#]
        );
        assert_eq!(event_log(&r), r#"[{"event":2,"objectId":"c1","data":{"type":"creep"}}]"#);
        assert!(r.stats.is_empty(), "suicide is not violent");
    }

    /// say.js: the first 10 UTF-16 units; the changed actionLog is persisted by the creep tick.
    #[test]
    fn say_is_persisted() {
        let f = Fixture::new();
        let prev = snapshot(&[creep("c1", 10, 10, &["move"], "{}", 0, "")]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"say":{"message":"hello world!!","isPublic":true}}"#)])]));
        assert!(doc(&r, "c1").unwrap().contains(r#""say":{"message":"hello worl","isPublic":true}"#));
    }

    fn two_creeps(c1: (i32, i32), c2: (i32, i32), c2_user: &str) -> RoomSnapshot {
        snapshot(&[creep("c1", c1.0, c1.1, &["move"], "{}", 0, ""), creep("c2", c2.0, c2.1, &["move"], "{}", 0, "").replace(r#""user":"u1""#, &format!(r#""user":"{c2_user}""#))])
    }

    /// `x` after the tick (the prev value when the room did not change).
    fn x_of(r: &RoomResult, prev: &RoomSnapshot, id: &str) -> f64 {
        let docs = r.next.as_ref().map_or(&prev.objects[..], |d| &d.objects[..]);
        let d = docs.iter().find(|d| d.id().as_str() == id).unwrap();
        d.obj.get(atom!("x")).and_then(Val::as_f64).unwrap()
    }

    /// movement.js:107-150: equal rates keep registration order (MOVE-TIEBREAK); the order override
    /// (ORD-POLICY point 1 test hook) decides which user registers first.
    #[test]
    fn move_conflict_tiebreak() {
        let mut f = Fixture::new();
        let prev = two_creeps((10, 10), (12, 10), "u1");
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"move":{"direction":3}}"#), ("c2", r#"{"move":{"direction":7}}"#)])]));
        assert_eq!((x_of(&r, &prev, "c1"), x_of(&r, &prev, "c2")), (11.0, 12.0));

        let prev = two_creeps((10, 10), (12, 10), "u2");
        let both = [(U1, &[("c1", r#"{"move":{"direction":3}}"#)][..]), (U2, &[("c2", r#"{"move":{"direction":7}}"#)][..])];
        f.order = Some(vec![U2, U1]);
        let r = f.run(&prev, intents(&both));
        assert_eq!((x_of(&r, &prev, "c1"), x_of(&r, &prev, "c2")), (10.0, 11.0));
        f.order = Some(vec![U1, U2]);
        let r = f.run(&prev, intents(&both));
        assert_eq!((x_of(&r, &prev, "c1"), x_of(&r, &prev, "c2")), (11.0, 12.0));
    }

    /// MOVE-SWAP-CYCLE and MOVE-CHAIN-CANCEL (movement.js:154-185); terrain walls block.
    #[test]
    fn move_swap_and_blocked_chain() {
        let f = Fixture::new();
        let prev = two_creeps((10, 10), (11, 10), "u1");
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"move":{"direction":3}}"#), ("c2", r#"{"move":{"direction":7}}"#)])]));
        assert_eq!((x_of(&r, &prev, "c1"), x_of(&r, &prev, "c2")), (11.0, 10.0));

        let prev = snapshot(&[creep("c1", 10, 10, &["move"], "{}", 0, ""), creep("c2", 11, 10, &["move"], "{}", 0, ""), creep("c3", 12, 10, &["move"], "{}", 0, "")]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"move":{"direction":3}}"#), ("c2", r#"{"move":{"direction":3}}"#)])]));
        assert_eq!((x_of(&r, &prev, "c1"), x_of(&r, &prev, "c2")), (10.0, 11.0), "the whole chain stays");

        let mut f = Fixture::new();
        f.terrain.set_raw(11, 10, 1);
        let prev = snapshot(&[creep("c1", 10, 10, &["move"], "{}", 0, "")]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"move":{"direction":3}}"#)])]));
        assert_eq!(x_of(&r, &prev, "c1"), 10.0);
    }

    /// creeps/tick.js:52-78: stepping onto x = 0 costs no fatigue and requests W2N1 at x = 49.
    #[test]
    fn edge_exit() {
        let f = Fixture::new();
        let prev = snapshot(&[creep("c1", 1, 10, &["move"], "{}", 0, "")]);
        let r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"move":{"direction":7}}"#)])]));
        let want = creep("c1", 0, 10, &["move"], "{}", 0, r#","interRoom":{"room":"W2N1","x":49,"y":10}"#);
        assert_eq!(doc(&r, "c1").unwrap(), want);
        assert_eq!(event_log(&r), r#"[{"event":10,"objectId":"c1","data":{"room":"W2N1","x":49,"y":10}}]"#);
        assert!(r.next.as_ref().unwrap().has_movers);
    }

    /// creeps/tick.js:87-96 and _die.js: age death keeps ticking the detached object (map view entry).
    #[test]
    fn age_death() {
        let f = Fixture::new();
        let prev = snapshot(&[creep("c1", 10, 10, &["move"], "{}", 0, "").replace(r#""ageTime":2500"#, r#""ageTime":1001"#)]);
        let r = f.run(&prev, RoomIntents { users: Vec::new() });
        assert!(doc(&r, "c1").is_none());
        assert_eq!(
            inserts(&r),
            vec![r#"{"type":"tombstone","room":"W1N1","x":10,"y":10,"user":"u1","deathTime":1000,"decayTime":1005,"creepId":"c1","creepName":"c1","creepTicksToLive":1,"creepBody":["move"],"store":{}}"#]
        );
        assert!(r.next.as_ref().unwrap().map_view.contains(r#""u1":[[10,10]]"#));
    }

    /// BUG-DIE-DOUBLE (R): hits ≤ 0 calls _die at tick.js:110 and again at tick.js:134.
    #[test]
    fn violent_death_twice() {
        let f = Fixture::new();
        let prev = snapshot(&[creep("c1", 10, 10, &["move"], r#"{"energy":5}"#, 50, "").replace(r#""hits":100,"hitsMax""#, r#""hits":0,"hitsMax""#)]);
        let r = f.run(&prev, RoomIntents { users: Vec::new() });
        let ins = inserts(&r);
        assert_eq!(ins.len(), 2, "{ins:?}");
        assert_eq!(ins[0], ins[1]);
        assert!(ins[0].ends_with(r#""store":{"energy":15}}"#), "10 body energy (0.2 × 1500/1500 × 50) + 5 carried: {}", ins[0]);
        assert_eq!(r.stats, vec![(U1, atom!("creepsLost"), 1.0), (U1, atom!("creepsLost"), 1.0)]);
    }

    /// movement.js:204-251 and _add-fatigue.js: cost 2 per non-MOVE part on plain (10 on swamp, 1 on a
    /// road), then −2 per MOVE part.
    #[test]
    fn fatigue_by_terrain() {
        let body = ["work", "work", "move"];
        let fatigue = |terrain: Option<u8>, road: bool| {
            let mut f = Fixture::new();
            if let Some(code) = terrain {
                f.terrain.set_raw(11, 10, code);
            }
            let mut docs = vec![creep("c1", 10, 10, &body, "{}", 0, "")];
            if road {
                docs.push(r#"{"_id":"r1","type":"road","x":11,"y":10,"room":"W1N1","hits":5000,"hitsMax":5000,"nextDecayTime":5000}"#.into());
            }
            let r = f.run(&snapshot(&docs), intents(&[(U1, &[("c1", r#"{"move":{"direction":3}}"#)])]));
            let d = doc(&r, "c1").unwrap();
            (obj_of(&d).get(atom!("fatigue")).and_then(Val::as_f64).unwrap(), doc(&r, "r1"))
        };
        assert_eq!(fatigue(None, false).0, 2.0);
        assert_eq!(fatigue(Some(2), false).0, 18.0);
        let (f, road) = fatigue(Some(2), true);
        assert_eq!(f, 0.0);
        assert!(road.unwrap().contains(r#""nextDecayTime":4997"#));
    }

    /// pull.js + movement.js:176-181 + _add-fatigue.js: the pulled creep's cost goes to the head, whose
    /// recovery already ran this tick (tick-order dependent).
    #[test]
    fn pull_train() {
        let f = Fixture::new();
        let prev = snapshot(&[creep("a", 10, 10, &["move"], "{}", 0, ""), creep("b", 9, 10, &["work"], "{}", 0, "")]);
        let r = f.run(&prev, intents(&[(U1, &[("a", r#"{"move":{"direction":3,"id":"undefined"},"pull":{"id":"b"}}"#), ("b", r#"{"move":{"id":"a","direction":null}}"#)])]));
        assert_eq!((x_of(&r, &prev, "a"), x_of(&r, &prev, "b")), (11.0, 10.0));
        for id in ["a", "b"] {
            assert!(doc(&r, id).unwrap().contains(r#""fatigue":0"#), "{id}");
        }
    }

    /// BUG-CRASH-WITHDRAW (C): private replicates the room crash, mmo skips the intent.
    #[test]
    fn withdraw_crash_by_profile() {
        let mut f = Fixture::new();
        let road = r#"{"_id":"r1","type":"road","x":11,"y":10,"room":"W1N1","hits":5000,"hitsMax":5000}"#;
        let prev = snapshot(&[creep("c1", 10, 10, &["carry", "move"], "{}", 50, ""), road.into()]);
        let i = || intents(&[(U1, &[("c1", r#"{"withdraw":{"id":"r1","amount":10,"resourceType":"energy"}}"#)])]);
        let r = f.run(&prev, i());
        assert_eq!(r.crashed, Some(Crash("Cannot read properties of undefined (reading 'energy')".into())));
        assert!(r.next.is_none() && !r.activate_next && r.inserts.is_empty());
        f.compat = policy::Compat::new(policy::Profile::Mmo);
        let r = f.run(&prev, i());
        assert!(r.crashed.is_none() && r.activate_next);
    }

    /// An unchanged room keeps its snapshot (§5.2 step 11).
    #[test]
    fn unchanged_room_keeps_prev() {
        let f = Fixture::new();
        let source = r#"{"_id":"s1","type":"source","x":11,"y":10,"room":"W1N1","energy":3000,"energyCapacity":3000}"#;
        let prev = snapshot(&[source.into()]);
        let r = f.run(&prev, RoomIntents { users: Vec::new() });
        assert!(!r.activate_next, "a lone source does not activate");
        let d = r.next.expect("the map view changed");
        let again = RoomSnapshot::new(d, T);
        assert!(f.run(&again, RoomIntents { users: Vec::new() }).next.is_none());
    }

    /// A room with the controller at (1, 1): what `quiet_tick` may take, then what looks like it and may not.
    fn quiet_room() -> Vec<String> {
        let at = |id: &str, x: u32, rest: &str| format!(r#"{{"_id":"{id}","x":{x},"y":5,"room":"W1N1",{rest}}}"#);
        let road = r#""type":"road","hits":5000,"hitsMax":5000"#;
        let wall = r#""type":"constructedWall","hits":1000"#;
        let rampart = r#""type":"rampart","user":"u1","hits":1000"#;
        vec![
            r#"{"_id":"ctrl","type":"controller","x":1,"y":1,"room":"W1N1","user":"u1","level":4,"progress":0,"downgradeTime":100000}"#.into(),
            at("road", 2, &format!(r#"{road},"nextDecayTime":1100"#)),
            at("wall", 3, &format!(r#"{wall},"hitsMax":300000000"#)),
            at("rampart", 4, &format!(r#"{rampart},"hitsMax":3000000,"nextDecayTime":1100"#)),
            at("public", 5, &format!(r#"{rampart},"hitsMax":3000000,"nextDecayTime":1100,"isPublic":true"#)),
            at("road-due", 6, &format!(r#"{road},"nextDecayTime":1001"#)),
            at("road-no-decay", 7, road),
            at("road-effects", 8, &format!(r#"{road},"nextDecayTime":1100,"effects":[]"#)),
            at("wall-newbie", 9, &format!(r#"{wall},"hitsMax":300000000,"newbieWall":true"#)),
            at("wall-hits", 10, &format!(r#"{wall},"hitsMax":7"#)),
            at("wall-decays", 11, &format!(r#"{wall},"hitsMax":300000000,"ticksToLive":50"#)),
            at("rampart-newbie", 12, &format!(r#"{rampart},"hitsMax":3000000,"nextDecayTime":1100,"newbieWall":true"#)),
            at("rampart-foreign", 13, r#""type":"rampart","user":"u2","hits":1000,"hitsMax":3000000,"nextDecayTime":1100"#),
            at("rampart-due", 14, &format!(r#"{rampart},"hitsMax":3000000,"nextDecayTime":1001"#)),
        ]
    }

    #[test]
    fn quiet_tick_takes_only_plain_roads_walls_and_ramparts() {
        let f = Fixture::new();
        let ctx = f.ctx();
        let prev = snapshot(&quiet_room());
        let scope = load(&ctx, 0, &f.terrain, &prev);
        let controller = scope.room_objects.iter().find(|o| o.is_str(atom!("type"), "controller")).cloned();
        let (mut view, mut quiet) = (MapView::default(), Quiet { time: T as f64, level: V::Undef, wall_hits_max: 0.0, rampart_hits_max: 0.0 });
        let took: Vec<String> = scope.room_objects.iter().filter(|o| quiet_tick(o, &mut quiet, &mut view, controller.as_ref())).map(|o| to_js_string(&o.get(atom!("_id"))).to_string()).collect();
        assert_eq!(took, ["road", "wall", "rampart", "public"]);
    }

    /// The history mod turns `quiet_tick` off, and nothing else in a tick without intents: both ways end alike.
    #[test]
    fn quiet_tick_leaves_the_room_as_the_full_tick_does() {
        let docs = quiet_room();
        let run = |history: bool| {
            let mut f = Fixture::new();
            f.config.history = history;
            let ctx = f.ctx();
            let r = process_room(&ctx, 0, &Arc::new(snapshot(&docs)), RoomIntents { users: Vec::new() });
            assert!(r.crashed.is_none(), "{:?}", r.crashed);
            let d = r.next.as_ref().expect("the map view changed");
            (d.map_view.to_string(), d.event_log.to_string(), d.objects.iter().map(|o| o.json().to_string()).collect::<Vec<_>>(), inserts(&r), r.activate_next)
        };
        let (quiet, full) = (run(false), run(true));
        assert_eq!(quiet, full);
        assert!(quiet.0.contains(r#""r":[[2,5]"#) && quiet.0.contains(r#""w":[[3,5]"#), "{}", quiet.0);
        assert!(!quiet.0.contains("[12,5]"), "an owned newbie wall is not on the map: {}", quiet.0);
    }

    /// A `process_object` hook may change `room_objects` during the prescan: the spawn limit (spawns/tick.js) then
    /// counts what is there, not what the prescan listed. At level 1 only the spawn nearest the controller works, so a
    /// removed `near` counted by mistake would stop `far` from regaining energy.
    #[test]
    fn spawn_limit_sees_a_spawn_removed_during_the_prescan() {
        fn remove_near(object: &O, scope: &mut RoomScope) {
            if object.is_str(atom!("_id"), "near") {
                scope.room_objects.remove("near");
            }
        }
        let spawn = |id: &str, x: u32, energy: u32| format!(r#"{{"_id":"{id}","type":"spawn","x":{x},"y":{x},"room":"W1N1","name":"{id}","user":"u1","hits":5000,"hitsMax":5000,"spawning":null,"store":{{"energy":{energy}}},"storeCapacityResource":{{"energy":300}}}}"#);
        let prev = Arc::new(snapshot(&[
            r#"{"_id":"ctrl","type":"controller","x":1,"y":1,"room":"W1N1","user":"u1","level":1,"progress":0,"downgradeTime":100000}"#.into(),
            spawn("near", 2, 0),
            spawn("far", 40, 100),
        ]));
        let mut f = Fixture::new();
        f.hooks.process_object.push(remove_near);
        let ctx = f.ctx();
        let r = process_room(&ctx, 0, &prev, RoomIntents { users: Vec::new() });
        assert!(doc(&r, "far").unwrap().contains(r#""energy":101"#), "{:?}", doc(&r, "far"));
    }

    /// `c_r` (§2.9, target ≤ 20 µs for a woken idle room; ≤ 300 µs for ~600 objects). Other packages' tick
    /// handlers are no-ops here, so this is processor.js + save overhead only.
    /// `cargo test --release -p engine --lib -- --ignored --nocapture bench_c_r`
    #[test]
    #[ignore]
    fn bench_c_r() {
        let f = Fixture::new();
        let idle = snapshot(&[
            r#"{"_id":"ctrl","type":"controller","x":20,"y":20,"room":"W1N1","level":0}"#.into(),
            r#"{"_id":"s1","type":"source","x":10,"y":10,"room":"W1N1","energy":3000,"energyCapacity":3000,"ticksToRegeneration":300}"#.into(),
            r#"{"_id":"s2","type":"source","x":40,"y":40,"room":"W1N1","energy":3000,"energyCapacity":3000}"#.into(),
            r#"{"_id":"m1","type":"mineral","x":5,"y":45,"room":"W1N1","mineralType":"H","mineralAmount":70000,"density":3}"#.into(),
        ]);
        // Steady state: the previous tick's map view is stored, so the room comes back unchanged.
        let warm = f.run(&idle, RoomIntents { users: Vec::new() }).next.unwrap();
        let idle = RoomSnapshot::new(warm, T);
        let mut docs: Vec<String> = Vec::new();
        for i in 0..600 {
            let (x, y) = (i % 48 + 1, (i / 48) % 48 + 1);
            docs.push(match i % 6 {
                0 => creep(&format!("c{i}"), x, y, &["work", "work", "carry", "carry", "move", "move"], r#"{"energy":20}"#, 100, ""),
                1 => format!(r#"{{"_id":"t{i}","type":"tower","x":{x},"y":{y},"room":"W1N1","user":"u1","store":{{"energy":200}},"storeCapacityResource":{{"energy":1000}},"hits":3000,"hitsMax":3000}}"#),
                2 => format!(r#"{{"_id":"r{i}","type":"road","x":{x},"y":{y},"room":"W1N1","hits":5000,"hitsMax":5000,"nextDecayTime":5000}}"#),
                3 => format!(r#"{{"_id":"w{i}","type":"constructedWall","x":{x},"y":{y},"room":"W1N1","hits":300000,"hitsMax":300000000}}"#),
                4 => format!(r#"{{"_id":"ra{i}","type":"rampart","x":{x},"y":{y},"room":"W1N1","user":"u1","hits":300000,"hitsMax":3000000,"nextDecayTime":5000}}"#),
                _ => format!(r#"{{"_id":"ct{i}","type":"container","x":{x},"y":{y},"room":"W1N1","store":{{"energy":500}},"storeCapacity":2000,"hits":250000,"hitsMax":250000}}"#),
            });
        }
        let big = snapshot(&docs);
        let moves: Vec<(String, String)> = (0..600).step_by(6).map(|i| (format!("c{i}"), r#"{"move":{"direction":5}}"#.to_string())).collect();
        let list: Vec<(&str, &str)> = moves.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        for (name, prev, with_intents, n) in [("idle room", &idle, false, 20000), ("600 objects, no intents", &big, false, 200), ("600 objects, 100 movers", &big, true, 200)] {
            let started = std::time::Instant::now();
            for _ in 0..n {
                let i = if with_intents { intents(&[(U1, &list[..])]) } else { RoomIntents { users: Vec::new() } };
                std::hint::black_box(f.run(prev, i));
            }
            println!("c_r {name}: {:.1} µs", started.elapsed().as_secs_f64() * 1e6 / n as f64);
        }
    }

    /// processor.js:183-191: `objects[id] = _.assign(aiIntents, stored || {})`: stored intents win key by key,
    /// an existing id keeps its place, a new id is appended, and a missing NPC user entry is created.
    #[test]
    fn npc_intents_merge() {
        let f = Fixture::new();
        let ctx = f.ctx();
        let mut users = vec![UserEntry {
            uid: "3".into(),
            idx: Some(1),
            objects: vec![("k1".into(), V::Obj(obj_live(r#"{"attack":{"id":"a"}}"#))), ("k0".into(), V::Obj(obj_live(r#"{"say":{}}"#)))],
            manual: None,
        }];
        let ai = V::Obj(obj_live(r#"{"k1":{"move":{"direction":1},"attack":{"id":"b"}},"k2":{"move":{"direction":2}}}"#));
        merge_npc_intents(&ctx, &mut users, &V::str("3"), &ai);
        let got: Vec<String> = users[0].objects.iter().map(|(k, v)| format!("{k}={}", live::stringify(v))).collect();
        assert_eq!(got, [r#"k1={"move":{"direction":1},"attack":{"id":"a"}}"#, r#"k0={"say":{}}"#, r#"k2={"move":{"direction":2}}"#]);
        merge_npc_intents(&ctx, &mut users, &V::str("2"), &V::Obj(obj_live(r#"{"i1":{"suicide":{}}}"#)));
        assert_eq!((&*users[1].uid, users[1].idx, users[1].objects.len()), ("2", Some(0), 1));
        // ORD-POLICY: integer-like ids first, ascending.
        order_users(&ctx, &mut users);
        assert_eq!(users.iter().map(|u| &*u.uid).collect::<Vec<_>>(), ["2", "3"]);
    }

    fn obj_live(json: &str) -> O {
        live_obj(&obj_of(json))
    }

    /// The nuke pretick gets vanilla's `intents` shape over the same per-object intents (PROC-NUKE-PRETICK),
    /// keyed by position so no id is interned.
    #[test]
    fn intents_shape_shares_objects() {
        let entry = |uid: &str, ids: &[&str]| UserEntry {
            uid: uid.into(),
            idx: None,
            objects: ids.iter().map(|id| (Arc::from(*id), V::Obj(obj_live(r#"{"createCreep":{"name":"x"}}"#)))).collect(),
            manual: None,
        };
        let users = vec![entry("u-nuke-shape-a", &["id-nuke-shape-1"]), entry("u-nuke-shape-b", &["id-nuke-shape-2", "id-nuke-shape-3"])];
        let shaped = intents_shape(&users);
        // _.forEach(intents.users, u => _.forEach(u.objects, i => { if(!!i.createCreep) i.createCreep = null; }))
        for u in lodash_values(&get_or_undef(&shaped, atom!("users"))) {
            for i in lodash_values(&get_or_undef(&u, atom!("objects"))) {
                put(&i, atom!("createCreep"), V::Null).unwrap();
            }
        }
        let after: Vec<String> = users.iter().flat_map(|e| e.objects.iter().map(|(_, v)| live::stringify(v))).collect();
        assert_eq!(after, vec![r#"{"createCreep":null}"#; 3]);
        assert_eq!(live::stringify(&shaped).matches("nuke-shape").count(), 0, "keyed by position, not by id");
    }

    thread_local! {
        static CALLS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    fn log_call(s: String) {
        CALLS.with(|c| c.borrow_mut().push(s));
    }

    /// Mod hooks run at vanilla's emit points (PROC-MOD-HOOKS), custom intents and custom object types dispatch.
    #[test]
    fn hooks_run_in_order() {
        fn process_object(o: &O, _: &mut RoomScope) {
            log_call(format!("processObject {}", to_js_string(&o.get(atom!("_id")))));
        }
        fn pre(o: Option<&O>, _: UserIdx, _: &V, _: &mut RoomScope) {
            log_call(format!("pre {}", o.map_or("-".into(), |o| to_js_string(&o.get(atom!("_id"))).to_string())));
        }
        fn custom_intent(o: &O, intent: &V, _: &mut RoomScope) -> Result<(), Crash> {
            log_call(format!("glow {} {}", to_js_string(&o.get(atom!("_id"))), live::stringify(intent)));
            Ok(())
        }
        fn post(o: &O, _: UserIdx, _: &V, _: &mut RoomScope) {
            log_call(format!("post {}", to_js_string(&o.get(atom!("_id")))));
        }
        fn custom_tick(o: &O, scope: &mut RoomScope) -> Result<(), Crash> {
            log_call(format!("tick {}", to_js_string(&o.get(atom!("_id")))));
            update(scope, o, &obj! { "ticked" => V::Bool(true) })
        }
        fn post_object(o: &O, _: &mut RoomScope) {
            log_call(format!("postProcessObject {}", to_js_string(&o.get(atom!("_id")))));
        }
        fn room_end(_: &mut RoomScope) {
            log_call("processRoom".into());
        }
        fn activate(o: &O, _: &RoomScope) -> bool {
            eq_str(&o.get(atom!("type")), "beacon")
        }
        let mut f = Fixture::new();
        f.hooks.process_object.push(process_object);
        f.hooks.pre_object_intents.push(pre);
        f.hooks.object_intents.push(("beacon", "glow", custom_intent));
        f.hooks.post_object_intents.push(post);
        f.hooks.object_ticks.push(("beacon", custom_tick));
        f.hooks.post_object.push(post_object);
        f.hooks.room_end.push(room_end);
        f.hooks.activation_rules.push(activate);
        let prev = snapshot(&[r#"{"_id":"b1","type":"beacon","x":1,"y":1,"room":"W1N1"}"#.into()]);
        CALLS.with(|c| c.borrow_mut().clear());
        let r = f.run(&prev, intents(&[(U1, &[("b1", r#"{"glow":{"power":2}}"#), ("gone", r#"{"glow":{}}"#)])]));
        assert_eq!(
            CALLS.with(|c| c.borrow().clone()),
            ["processObject b1", "pre b1", r#"glow b1 {"power":2}"#, "post b1", "pre -", "tick b1", "postProcessObject b1", "processRoom"]
        );
        assert!(r.activate_next, "a mod activation rule");
        assert_eq!(doc(&r, "b1").unwrap(), r#"{"_id":"b1","type":"beacon","x":1,"y":1,"room":"W1N1","ticked":true}"#);
    }

    /// A world over `rooms` (named W1N1, W2N1, ...) with the fixture's users.
    fn test_world(f: &Fixture, rooms: Vec<RoomSnapshot>) -> World {
        let mut meta = rooms_meta();
        for i in 1..rooms.len() {
            let name = format!("W{}N1", i + 1);
            let xy = crate::room_name::RoomXY::parse_engine(&name).unwrap();
            meta.rooms.push(RoomMeta { name: name.into(), xy, removed: false });
        }
        let terrain = TerrainStore::new(rooms.iter().map(|_| RoomTerrain::default()).collect());
        let clock = policy::VClock { base_ms: 0, base_tick: 0, nominal_tick_ms: 1000 };
        let mut w = World::new(1, clock, policy::Compat::new(policy::Profile::Private), meta, terrain, rooms.into_iter().map(Arc::new).collect(), f.users.clone());
        w.time = T;
        w.config = Arc::new(engine_config());
        w.access = Arc::new(AccessSnapshot { version: 0, accessible: vec![u64::MAX], status: HashMap::new() });
        w.user_ids = Arc::new(f.user_ids.clone());
        w.usernames = Arc::new(HashMap::new());
        w.index = WorldIndex::default();
        w.next_seq = 10;
        w
    }

    /// P4: a crashed room installs nothing but its notifications; foreign inserts land in their target
    /// room after every room's own inserts, with new seq/_id (XROOM-NUKE).
    #[test]
    fn install_foreign_inserts_and_crash() {
        let f = Fixture::new();
        let a = snapshot(&[creep("c1", 10, 10, &["move"], "{}", 0, "")]);
        let b = snapshot(&[]);
        let mut w = test_world(&f, vec![a, b]);
        let before = w.rooms[0].clone();
        let empty = |room| RoomResult {
            room,
            next: None,
            inserts: Vec::new(),
            crashed: None,
            activate_next: false,
            user_ops: Vec::new(),
            foreign_inserts: Vec::new(),
            notifications: Vec::new(),
            history: None,
            stats: Vec::new(),
            cost_ns: 7,
            index: None,
        };
        let mut crashed = empty(0);
        crashed.crashed = Some(Crash("boom".into()));
        crashed.notifications = vec![(U2, "attacked".into())];
        let mut launcher = empty(1);
        launcher.foreign_inserts = vec![(0, obj_of(r#"{"type":"nuke","room":"W1N1","x":5,"y":5,"landTime":51000}"#))];
        install(&mut w, vec![launcher, crashed]);
        assert_eq!(w.rooms[0].objects.len(), 2);
        assert!(Arc::ptr_eq(&w.rooms[0].objects[0], &before.objects[0]), "untouched docs stay shared");
        let nuke = &w.rooms[0].objects[1];
        assert_eq!((nuke.seq, nuke.kind()), (10, "nuke"));
        assert_eq!(w.rooms[0].processed_at, before.processed_at, "the target room was not processed");
        assert_eq!(w.users[U2 as usize].inbox.notifications.len(), 1);
        assert_eq!(w.index.room_cost_ns, vec![7, 7]);
        assert!(w.index.objects.contains_key(&nuke.id()));
    }

    /// driver.sendNotification is a Loki upsert: every doc matching `{user, message, date ≤ now, type}` is
    /// updated (`$set date`, `$inc count`); a same-text doc dated ahead (a runner `groupInterval` bucket) is
    /// not, and nothing is inserted while one matches.
    #[test]
    fn notification_upsert_updates_every_match() {
        let f = Fixture::new();
        let mut w = test_world(&f, vec![snapshot(&[])]);
        let msg = "Your creep c1 in room W1N1 is under attack!";
        let old = |id: &str, date: f64, count: &str| Arc::new(obj_of(&format!(r#"{{"_id":"{id}","user":"u1","message":"{msg}","date":{date},"type":"msg","count":{count}}}"#)));
        let future = 1e15;
        let notifications = vec![old("n1", 5.0, "2"), old("n2", future, "1"), old("n3", 7.0, r#""4""#)];
        w.users[U1 as usize] = Arc::new(UserState { inbox: Arc::new(Inbox { notifications, messages: Vec::new() }), ..clone_user(&w.users[U1 as usize]) });
        send_notification(&mut w, U1, msg);
        let n = &w.users[U1 as usize].inbox.notifications;
        assert_eq!(n.len(), 3, "a match exists: no insert");
        let field = |i: usize, k| n[i].get(k).cloned().unwrap();
        assert_eq!(field(0, atom!("count")), Val::Num(3.0));
        assert!(field(0, atom!("date")).as_f64().unwrap() > 7.0);
        assert_eq!((field(1, atom!("count")), field(1, atom!("date"))), (Val::Num(1.0), Val::Num(future)), "dated ahead: untouched");
        assert_eq!(field(2, atom!("count")), Val::Str("41".into()), "$inc is `(doc.count || 0) + 1`");
    }

    /// P4: insert ids/seq, user ops, notifications, index upkeep.
    #[test]
    fn install_applies_results() {
        let f = Fixture::new();
        let prev = snapshot(&[creep("c1", 10, 10, &["carry", "move"], r#"{"energy":20}"#, 50, "")]);
        let mut r = f.run(&prev, intents(&[(U1, &[("c1", r#"{"drop":{"amount":20,"resourceType":"energy"}}"#)])]));
        assert_eq!(inserts(&r), vec![r#"{"type":"energy","x":10,"y":10,"room":"W1N1","energy":20,"resourceType":"energy"}"#]);
        r.activate_next = false; // ActiveSet is WP01's stub
        r.user_ops = vec![bulk::DbOp::Inc(Id::new("u1").unwrap(), vec![atom!("gcl")], 5.0)];
        r.notifications = vec![(U1, "hello".into())];

        let mut w = test_world(&f, vec![prev]);
        w.index.objects.insert(Id::new("c1").unwrap(), ObjLoc { room: 0, seq: 1, owner: Some(U1) });
        install(&mut w, vec![r]);
        let room = &w.rooms[0];
        assert_eq!(room.objects.len(), 2);
        let drop = &room.objects[1];
        assert_eq!(drop.seq, 10);
        assert_eq!(w.next_seq, 11);
        let id = drop.id();
        assert_eq!(id.as_str().len(), 15);
        assert!(drop.json().ends_with(&format!(r#""resourceType":"energy","_id":"{}"}}"#, id.as_str())));
        assert!(w.index.objects.contains_key(&id) && w.index.objects.contains_key(&Id::new("c1").unwrap()));
        assert_eq!(w.users[U1 as usize].doc.get(atom!("gcl")).and_then(Val::as_f64), Some(5.0));
        let n = &w.users[U1 as usize].inbox.notifications;
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].get(atom!("message")).and_then(Val::as_str), Some("hello"));
        assert_eq!(n[0].get(atom!("count")).and_then(Val::as_f64), Some(1.0));
        assert_eq!(w.index.presence[U1 as usize].get(&0).map(|p| p.objects), Some(1), "the drop is ownerless; c1 counts");
    }

    /// P4 applies the index changes `process_room` worked out in P3 exactly as its own merge would: a changed owner,
    /// a removed doc whose entry now names another room (kept), an unchanged doc, an insert.
    #[test]
    fn install_applies_p3_index_ops() {
        let f = Fixture::new();
        let e1 = r#"{"_id":"e1","type":"energy","x":1,"y":1,"room":"W1N1","energy":5}"#.to_string();
        let s1 = r#"{"_id":"s1","type":"source","x":2,"y":2,"room":"W1N1","energy":5}"#.to_string();
        let installed = |p3_ops: bool| {
            let mut w = test_world(&f, vec![snapshot(&[creep("c1", 10, 10, &["move"], "{}", 0, ""), e1.clone(), s1.clone()]), snapshot(&[])]);
            for (id, loc) in [("c1", ObjLoc { room: 0, seq: 1, owner: Some(U1) }), ("e1", ObjLoc { room: 1, seq: 9, owner: None }), ("s1", ObjLoc { room: 0, seq: 3, owner: None })] {
                w.index.objects.insert(Id::new(id).unwrap(), loc);
            }
            let prev = w.rooms[0].clone();
            let mut draft = draft_of(&prev);
            let mut c1 = draft.objects[0].obj.clone();
            c1.set(atom!("user"), Val::Str("u2".into()));
            draft.objects[0] = Doc::new(1, c1);
            draft.objects.remove(1);
            let index = p3_ops.then(|| {
                let mut ops = Vec::new();
                index_ops(0, &prev.objects, &draft.objects, &w.user_ids, &mut ops);
                (prev.clone(), ops)
            });
            let r = RoomResult {
                room: 0,
                next: Some(draft),
                inserts: vec![obj_of(r#"{"type":"energy","x":3,"y":3,"room":"W1N1","energy":1}"#)],
                crashed: None,
                activate_next: false,
                user_ops: Vec::new(),
                foreign_inserts: Vec::new(),
                notifications: Vec::new(),
                history: None,
                stats: Vec::new(),
                cost_ns: 0,
                index,
            };
            install(&mut w, vec![r]);
            let mut ix: Vec<(String, ObjLoc)> = w.index.objects.iter().map(|(k, v)| (k.as_str().to_string(), *v)).collect();
            ix.sort_by(|a, b| a.0.cmp(&b.0));
            ix
        };
        let ix = installed(true);
        assert_eq!(ix, installed(false));
        let loc = |id: &str| ix.iter().find(|e| e.0 == id).map(|e| e.1);
        assert_eq!(loc("c1"), Some(ObjLoc { room: 0, seq: 1, owner: Some(U2) }));
        assert_eq!(loc("e1"), Some(ObjLoc { room: 1, seq: 9, owner: None }), "another room's entry is not removed");
        assert_eq!(ix.len(), 4, "c1, e1, s1 and the insert");
    }
}

#[cfg(test)]
mod oracle {
    //! Replays crates/engine/tests/fixtures/creeps/cases.ndjson: the official processor.js over the same
    //! scenarios with every non-creep tick handler replaced by a no-op on both sides (generate.js).
    use super::tests::{engine_config, rooms_meta, user_state};
    use super::*;
    use std::collections::HashMap;
    use crate::doc::Val;
    use crate::intents::UserRoomIntents;
    use crate::world::RoomStatus;
    use util::json::Json;

    const CASES: &str = include_str!("../tests/fixtures/creeps/cases.ndjson");

    fn norm_val(v: &Val) -> String {
        let mut s = String::new();
        v.write_json(&mut s);
        s
    }

    fn norm(j: &Json) -> String {
        norm_val(&Val::from_json(j))
    }

    fn obj(j: &Json) -> Obj {
        match Val::from_json(j) {
            Val::Obj(o) => o,
            _ => panic!("not an object"),
        }
    }

    fn op_json(op: &bulk::DbOp) -> String {
        use bulk::DbOp::*;
        let q = |s: &str| {
            let mut o = String::new();
            util::json::write_quoted(&mut o, s);
            o
        };
        let v = |x: &Val| norm_val(x);
        match op {
            Remove(id) => format!(r#"{{"op":"remove","id":{}}}"#, q(id.as_str())),
            Inc(id, path, n) => {
                let key = path.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(".");
                format!(r#"{{"op":"update","id":{},"update":{{"$inc":{{{}:{}}}}}}}"#, q(id.as_str()), q(&key), v(&Val::Num(*n)))
            }
            AddToSet(id, k, x) => format!(r#"{{"op":"update","id":{},"update":{{"$addToSet":{{{}:{}}}}}}}"#, q(id.as_str()), q(k.as_str()), v(x)),
            Pull(id, k, x) => format!(r#"{{"op":"update","id":{},"update":{{"$pull":{{{}:{}}}}}}}"#, q(id.as_str()), q(k.as_str()), v(x)),
            Set(id, o) => format!(r#"{{"op":"update","id":{},"update":{{"$set":{}}}}}"#, q(id.as_str()), v(&Val::Obj(o.clone()))),
            Insert(o) => format!(r#"{{"op":"insert","data":{}}}"#, v(&Val::Obj(o.clone()))),
            Replace(..) => unreachable!("room processing never replaces a document"),
        }
    }

    /// Replays one case; `Err` names the first difference.
    fn replay(case: &Json) -> Result<(), String> {
        let g = |k: &str| case.get(k).unwrap_or_else(|| panic!("missing {k}"));
        let result = g("result");
        let r = |k: &str| result.get(k).unwrap_or_else(|| panic!("missing result.{k}"));

        let users_json = g("users").as_object().unwrap();
        let users: Vec<Arc<UserState>> = users_json
            .iter()
            .enumerate()
            .map(|(i, (id, doc))| {
                let u = user_state(i, id);
                Arc::new(UserState { doc: Arc::new(obj(doc)), ..clone_user(&u) })
            })
            .collect();
        let user_ids: crate::world::UserIds = users.iter().enumerate().map(|(i, u)| (u.id, i as UserIdx)).collect();
        let idx = |uid: &str| user_ids[&Id::new(uid).unwrap()];

        let mut intents = RoomIntents { users: Vec::new() };
        let mut order = Vec::new();
        for entry in g("intents").as_array().unwrap() {
            let uid = entry.at(0).unwrap().as_str().unwrap();
            let u = entry.at(1).unwrap();
            let list = |k: &str| match u.get(k) {
                Some(Json::Obj(m)) => m.iter().map(|(id, o)| (Box::from(id.as_str()), obj(o))).collect(),
                _ => Vec::new(),
            };
            intents.users.push(UserRoomIntents { user: idx(uid), objects: list("objects"), manual: list("objectsManual") });
            order.push(idx(uid));
        }

        let docs: Vec<DocRef> = g("objects").as_array().unwrap().iter().enumerate().map(|(i, d)| Doc::new(i as u64 + 1, obj(d))).collect();
        let prev = RoomSnapshot::new(
            RoomDraft {
                info: Arc::new(obj(g("roomDoc"))),
                objects: docs,
                flags: g("flags").as_array().unwrap().iter().map(|f| Arc::new(obj(f))).collect::<Vec<_>>().into(),
                event_log: "[]".into(),
                map_view: "".into(),
                presence: Vec::new(),
                user_refs: Vec::new(),
                has_movers: false,
                has_terminal_send: false,
                has_power_creeps: false,
            },
            0,
        );
        let terrain = RoomTerrain::from_db_str(g("terrain").as_str().unwrap()).unwrap();
        let (config, meta) = (engine_config(), rooms_meta());
        let terrain_store = TerrainStore::new(vec![terrain.clone()]);
        let access = AccessSnapshot { version: 0, accessible: vec![1], status: HashMap::<RoomIdx, (RoomStatus, Option<i64>)>::new() };
        let hooks = hooks::ProcessorHooks::default();
        let compat = policy::Compat::new(policy::Profile::parse(g("compat").as_str().unwrap()).unwrap());
        let time = g("gameTime").as_f64().unwrap() as u64;
        let ctx = TickCtx {
            time,
            vnow: time as i64 * 1000,
            seed: 1,
            compat: &compat,
            config: &config,
            meta: &meta,
            terrain: &terrain_store,
            access: &access,
            users: &users,
            user_ids: &user_ids,
            hooks: &hooks,
            order_override: Some(&order),
        };
        fn noop(_: &O, _: &mut RoomScope) -> Result<(), Crash> {
            Ok(())
        }
        let ticks: Vec<(&str, TickFn)> = OBJECT_TICKS.iter().map(|&(t, f)| (t, if t == "creep" { f } else { noop as TickFn })).collect();
        let out = run_room(&ctx, 0, &terrain, &prev, intents, &ticks);

        let id_of = |u: UserIdx| users[u as usize].id.as_str().to_string();
        let check = |what: &str, ours: String, theirs: String| if ours == theirs { Ok(()) } else { Err(format!("{what}:\n  ours   {ours}\n  theirs {theirs}")) };

        match (r("crashed"), &out.crashed) {
            (Json::Null, None) => {}
            // The oracle logs `error.stack`, whose first line is "<Name>: <message>".
            (Json::Str(m), Some(Crash(c))) => check("crash message", c.to_string(), m.split_once("Error: ").map_or(m.as_str(), |(_, rest)| rest).to_string())?,
            (theirs, ours) => return Err(format!("crashed: ours {ours:?}, theirs {theirs:?}")),
        }
        let notes: Vec<String> = out.notifications.iter().map(|(u, m)| format!("{}:{m}", id_of(*u))).collect();
        let their_notes: Vec<String> = r("notifications").as_array().unwrap().iter().map(|n| format!("{}:{}", n.at(0).unwrap().as_str().unwrap(), n.at(1).unwrap().as_str().unwrap())).collect();
        check("notifications", notes.join("|"), their_notes.join("|"))?;
        let stats: Vec<String> = out.stats.iter().map(|(u, a, n)| format!("{}:{}:{}", a.as_str(), id_of(*u), util::num::to_js_string(*n))).collect();
        let their_stats: Vec<String> = r("stats")
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| user_ids.contains_key(&Id::new(s.at(1).unwrap().as_str().unwrap()).unwrap_or(Id::EMPTY)))
            .map(|s| format!("{}:{}:{}", s.at(0).unwrap().as_str().unwrap(), s.at(1).unwrap().as_str().unwrap(), util::num::to_js_string(s.at(2).unwrap().as_f64().unwrap_or(f64::NAN))))
            .collect();
        check("stats", stats.join("|"), their_stats.join("|"))?;
        if out.crashed.is_some() {
            return check("crash leaves no draft", format!("{}", out.next.is_some()), "false".into());
        }

        check("activated", out.activate_next.to_string(), r("activated").as_bool().unwrap().to_string())?;
        let user_ops: Vec<String> = out.user_ops.iter().map(op_json).collect();
        let their_user_ops: Vec<String> = r("ops").get("users").unwrap().as_array().unwrap().iter().map(norm).collect();
        check("user ops", user_ops.join("\n"), their_user_ops.join("\n"))?;

        let (docs, inserts, info, event_log, map_view) = match &out.next {
            Some(d) => (&d.objects[..], &out.inserts[..], d.info.clone(), d.event_log.to_string(), d.map_view.to_string()),
            None => (&prev.objects[..], &[][..], prev.info.clone(), prev.event_log.to_string(), prev.map_view.to_string()),
        };
        let ours: Vec<String> = docs.iter().map(|d| d.json().to_string()).chain(inserts.iter().map(|o| norm_val(&Val::Obj(o.clone())))).collect();
        let theirs: Vec<String> = r("docsAfter").as_array().unwrap().iter().map(norm).collect();
        for (i, (a, b)) in ours.iter().zip(&theirs).enumerate() {
            check(&format!("doc {i}"), a.clone(), b.clone())?;
        }
        check("doc count", ours.len().to_string(), theirs.len().to_string())?;
        check("event log", norm(&Json::parse(&event_log).unwrap()), norm(&Json::parse(r("eventLog").as_str().unwrap()).unwrap()))?;
        check("map view", map_view, norm(&Json::parse(r("mapView").as_str().unwrap()).unwrap()))?;
        let mut want_info = obj(g("roomDoc"));
        if let Json::Obj(set) = r("roomInfo") {
            for (k, v) in set {
                want_info.set(Atom::intern(k), Val::from_json(v));
            }
        }
        check("room info", norm_val(&Val::Obj((*info).clone())), norm_val(&Val::Obj(want_info)))?;
        let flags: Vec<String> = out.next.as_ref().map_or(prev.flags.to_vec(), |d| d.flags.to_vec()).iter().map(|f| norm_val(&Val::Obj((**f).clone()))).collect();
        let their_flags: Vec<String> = r("flagsAfter").as_array().unwrap().iter().map(norm).collect();
        check("flags", flags.join("\n"), their_flags.join("\n"))
    }

    #[test]
    fn oracle_cases() {
        let mut failures = Vec::new();
        let mut n = 0;
        for line in CASES.lines().filter(|l| !l.trim().is_empty()) {
            let case = Json::parse(line).unwrap();
            n += 1;
            let name = case.get("name").and_then(Json::as_str).unwrap_or("?").to_string();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| replay(&case)));
            match r {
                Ok(Ok(())) => {}
                Ok(Err(e)) => failures.push(format!("{name}: {e}")),
                Err(p) => failures.push(format!("{name}: panic {:?}", p.downcast_ref::<String>().map(String::as_str).or(p.downcast_ref::<&str>().copied()))),
            }
        }
        assert!(n > 300, "{n} cases");
        assert!(failures.is_empty(), "{} of {n} cases differ:\n{}", failures.len(), failures.iter().take(8).cloned().collect::<Vec<_>>().join("\n\n"));
    }
}
