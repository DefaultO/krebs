//! mirror: reference/engine/src/processor/global.js
//! parity: FAITHFUL
//!
//! The five `getInterRoom` query results (driver index.js:429-449) are separate object graphs, as in vanilla:
//! `creeps` (movers), `roomObjects` (terminals, power spawns, spawned power creeps; built lazily per touched doc,
//! so only what the stage reads is converted), `orders`, `userPowerCreeps`, `userIntents`, and `users`.
//! Deviation (BUG-CRASH-MKT-USER, F): `users` also holds the owners of the terminals in order and deal rooms,
//! where vanilla throws on `usersById[seller.user].money` (TICK-ERROR).

use crate::atom;
use crate::bulk::{self, Bulk, DbOp, DocStore, Target};
use crate::doc::{Atom, Doc, DocRef, Id, Obj, Val};
use crate::hooks::ProcessorHooks;
use crate::live::{self, to_js_string, O, V};
use crate::policy;
use crate::processor::global_intents::{market, power};
use crate::world::{room_accessible, MarketState, RoomIdx, RoomSnapshot, UserIdx, UserState, World};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use util::rng::Rng;

/// seed/compat/config come from World.
pub struct GlobalCtx<'h> {
    pub time: u64,
    pub vnow: i64,
    pub hooks: &'h ProcessorHooks,
    pub order_override: Option<&'h [UserIdx]>,
}

/// P5 (§5.6): inter-room moves → power → market → bulk writers → clear users.intents → hooks.global_stage.
pub fn run(w: &mut World, ctx: &GlobalCtx) {
    let mut bulk_objects = Bulk::new();

    // creeps (global.js:33-43), in collection order of the getInterRoom query
    for creep in movers(w) {
        let inter = match creep.get(atom!("interRoom")) {
            V::Obj(o) => o,
            _ => O::new(), // `creep.interRoom.room` of a non-object: undefined
        };
        let Some(dest) = accessible_room(w, &to_js_string(&inter.get(atom!("room"))), ctx.vnow) else {
            continue;
        };
        w.active.insert(dest);
        let data = O::new();
        data.set(atom!("room"), inter.get(atom!("room")));
        data.set(atom!("x"), inter.get(atom!("x")));
        data.set(atom!("y"), inter.get(atom!("y")));
        data.set(atom!("interRoom"), V::Null);
        // A creep doc is a plain JSON graph: removeHidden cannot throw on it.
        let _ = bulk_objects.update(Target::Obj(&creep), &V::Obj(data));
    }

    let ops = {
        let mut s = Scope::new(w, ctx, bulk_objects);
        power::run(&mut s);
        market::run(&mut s);
        s.into_ops()
    };
    apply(w, ops);

    w.global_intents.clear(); // driver.clearGlobalIntents()
    for hook in &ctx.hooks.global_stage {
        hook(w, ctx);
    }
}

/// `rooms.objects.find({type: {$in: ['creep','powerCreep']}, interRoom: {$ne: null}})` in collection (seq)
/// order; every result is its own object graph, as a query result is.
fn movers(w: &World) -> Vec<O> {
    let mut docs: Vec<&DocRef> = w
        .index
        .movers
        .iter()
        .flat_map(|&r| w.rooms[r as usize].objects.iter())
        .filter(|d| matches!(d.kind(), "creep" | "powerCreep") && !matches!(d.obj.get(atom!("interRoom")), None | Some(Val::Null)))
        .collect();
    docs.sort_by_key(|d| d.seq);
    docs.into_iter().map(|d| graph(&d.obj)).collect()
}

/// A stored doc as a fresh live object (a DB read).
pub(crate) fn graph(o: &Obj) -> O {
    match live::from_val(&Val::Obj(o.clone())) {
        V::Obj(o) => o,
        _ => unreachable!(),
    }
}

/// `accessibleRooms[name]`: the room whose `_id` is exactly `name`, status normal and open at `now`.
fn accessible_room(w: &World, name: &str, now: i64) -> Option<RoomIdx> {
    let r = exact_room(w, name)?;
    room_accessible(&w.rooms[r as usize].info, now).then_some(r)
}

pub(crate) fn exact_room(w: &World, name: &str) -> Option<RoomIdx> {
    w.meta.by_name(name).filter(|&r| w.meta.rooms.get(r as usize).is_some_and(|m| !m.removed && &*m.name == name))
}

/// The global stage's scope (global.js:16-29): query results, the bulk writers, gameTime, `Date.now()`.
pub(crate) struct Scope<'w> {
    pub w: &'w World,
    /// `gameTime` (getGametime: a number).
    pub game_time: f64,
    /// `Date.now()` / `new Date()` = vclock(T) (CLOCK-POLICY).
    pub now: f64,
    pub world_size: f64,
    /// `shardName: ''` (getInterRoom).
    pub shard_name: &'static str,
    pub users_by_id: HashMap<Arc<str>, O>,
    /// users.intents docs `{user, intents}` in processing order (ORD-POINTS 2/3).
    pub user_intents: Vec<O>,
    /// users.power_creeps, collection order.
    pub user_power_creeps: Vec<O>,
    /// market.orders, collection order.
    pub orders: Vec<O>,
    /// roomObjects (terminal/powerSpawn/powerCreep docs), converted on first touch, by seq.
    room_objects: HashMap<u64, O>,
    rng: Option<Rng>,
    seed: u64,
    time: u64,
    pub bulk_objects: Bulk,
    pub bulk_users: Bulk,
    pub bulk_transactions: Bulk,
    pub bulk_users_money: Bulk,
    pub bulk_users_resources: Bulk,
    pub bulk_users_power_creeps: Bulk,
    pub bulk_market_orders: Bulk,
    pub bulk_market_intershard_orders: Bulk,
}

/// Executed ops per collection, applied in global.js's execute order (the crons reuse it for their writes).
#[derive(Default)]
pub(crate) struct Ops {
    pub objects: Vec<DbOp>,
    pub rooms: Vec<DbOp>,
    pub users: Vec<DbOp>,
    pub market_orders: Vec<DbOp>,
    pub intershard_orders: Vec<DbOp>,
    pub users_money: Vec<DbOp>,
    pub transactions: Vec<DbOp>,
    pub users_resources: Vec<DbOp>,
    pub users_power_creeps: Vec<DbOp>,
    pub market_stats: Vec<DbOp>,
}

impl<'w> Scope<'w> {
    fn new(w: &'w World, ctx: &GlobalCtx, bulk_objects: Bulk) -> Scope<'w> {
        let user_intents = intent_docs(w, ctx);
        let user_power_creeps: Vec<O> = {
            let mut pcs: Vec<&DocRef> = w.users.iter().flat_map(|u| u.power_creeps.iter()).collect();
            pcs.sort_by_key(|d| d.seq);
            pcs.into_iter().map(|d| graph(&d.obj)).collect()
        };
        let orders: Vec<O> = w.market.orders.iter().map(|d| graph(&d.obj)).collect();

        // db.users.find({_id: {$in: _.map(_.flatten([orders, userPowerCreeps, userIntents]), 'user')}}), plus F rows
        let mut wanted: BTreeSet<UserIdx> = BTreeSet::new();
        let mut want = |v: &V| {
            if let Some(&u) = Id::new(&to_js_string(v)).and_then(|id| w.user_ids.get(&id)) {
                wanted.insert(u);
            }
        };
        for o in orders.iter().chain(&user_power_creeps).chain(&user_intents) {
            want(&o.get(atom!("user")));
        }
        let terminal_owner = |room: &V| -> V {
            let Some(r) = exact_room(w, &to_js_string(room)) else { return V::Undef };
            w.rooms[r as usize].objects.iter().rev().find(|d| d.kind() == "terminal").and_then(|d| d.obj.get(atom!("user"))).map_or(V::Undef, live::from_val)
        };
        for o in &orders {
            want(&terminal_owner(&o.get(atom!("roomName"))));
        }
        for doc in &user_intents {
            if let V::Arr(deals) = crate::processor::get_or_undef(&doc.get(atom!("intents")), atom!("deal")) {
                for d in deals.to_vec() {
                    want(&terminal_owner(&crate::processor::get_or_undef(&d, atom!("targetRoomName"))));
                }
            }
        }
        let users_by_id = wanted.into_iter().map(|u| (Arc::from(w.users[u as usize].id.as_str()), graph(&w.users[u as usize].doc))).collect();

        Scope {
            w,
            game_time: ctx.time as f64,
            now: ctx.vnow as f64,
            world_size: w.meta.world_size as f64,
            shard_name: "",
            users_by_id,
            user_intents,
            user_power_creeps,
            orders,
            room_objects: HashMap::new(),
            rng: None,
            seed: w.seed,
            time: ctx.time,
            bulk_objects,
            bulk_users: Bulk::new(),
            bulk_transactions: Bulk::new(),
            bulk_users_money: Bulk::new(),
            bulk_users_resources: Bulk::new(),
            bulk_users_power_creeps: Bulk::new(),
            bulk_market_orders: Bulk::new(),
            bulk_market_intershard_orders: Bulk::new(),
        }
    }

    fn into_ops(self) -> Ops {
        Ops {
            objects: self.bulk_objects.execute(),
            rooms: Vec::new(),
            market_stats: Vec::new(),
            users: self.bulk_users.execute(),
            market_orders: self.bulk_market_orders.execute(),
            intershard_orders: self.bulk_market_intershard_orders.execute(),
            users_money: self.bulk_users_money.execute(),
            transactions: self.bulk_transactions.execute(),
            users_resources: self.bulk_users_resources.execute(),
            users_power_creeps: self.bulk_users_power_creeps.execute(),
        }
    }

    /// The roomObjects graph object for a doc (one object per doc for the whole stage).
    fn room_object(&mut self, d: &DocRef) -> O {
        self.room_objects.entry(d.seq).or_insert_with(|| graph(&d.obj)).clone()
    }

    /// `terminalsByRoom[room]` (`_.indexBy(terminals, 'room')`: the last terminal of the room).
    pub fn terminal_in(&mut self, room: &V) -> Option<O> {
        let w = self.w;
        let r = exact_room(w, &to_js_string(room))?;
        let d = w.rooms[r as usize].objects.iter().rev().find(|d| d.kind() == "terminal")?;
        Some(self.room_object(d))
    }

    /// `_.filter(terminals, i => !!i.send)`, collection order.
    pub fn terminals_with_send(&mut self) -> Vec<O> {
        let w = self.w;
        let mut docs: Vec<&DocRef> =
            w.index.terminal_sends.iter().flat_map(|&r| w.rooms[r as usize].objects.iter()).filter(|d| d.kind() == "terminal" && val_truthy(d.obj.get(atom!("send")))).collect();
        docs.sort_by_key(|d| d.seq);
        docs.into_iter().map(|d| self.room_object(d)).collect()
    }

    /// `_.find(roomObjectsByType.powerSpawn, i => i._id == id)`.
    pub fn power_spawn(&mut self, id: &V) -> Option<O> {
        let w = self.w;
        let loc = Id::new(&to_js_string(id)).and_then(|id| w.index.objects.get(&id))?;
        let d = w.rooms[loc.room as usize].objects.iter().find(|d| d.seq == loc.seq && d.kind() == "powerSpawn")?;
        Some(self.room_object(d))
    }

    /// `roomObjectsByType.powerCreep` (spawned power creeps), collection order.
    pub fn power_creeps(&mut self) -> Vec<O> {
        let w = self.w;
        let mut docs: Vec<&DocRef> = w.index.power_creep_rooms.iter().flat_map(|&r| w.rooms[r as usize].objects.iter()).filter(|d| d.kind() == "powerCreep").collect();
        docs.sort_by_key(|d| d.seq);
        docs.into_iter().map(|d| self.room_object(d)).collect()
    }

    /// `Math.random` of the global stage: the global stream of T (RNG-SEED), created on first draw.
    pub fn rng(&mut self) -> &mut Rng {
        let (seed, time) = (self.seed, self.time);
        self.rng.get_or_insert_with(|| policy::global_stream(seed, time))
    }

    /// `new Date()` as it reaches the database: its ISO string.
    pub fn date(&self) -> V {
        V::str(&util::num::iso_date(self.now as i64))
    }
}

fn val_truthy(v: Option<&Val>) -> bool {
    v.is_some_and(|v| live::truthy(&live::from_val(v)))
}

/// `db['users.intents'].find()` in processing order: owners by ORD-POLICY (or the replay's order), a user's
/// API docs (arrival order) before its runner doc (ORD-POINTS 2/3).
fn intent_docs(w: &World, ctx: &GlobalCtx) -> Vec<O> {
    let rank: HashMap<UserIdx, usize> = match ctx.order_override {
        Some(order) => order.iter().enumerate().map(|(i, &u)| (u, i)).collect(),
        None => {
            let mut users: Vec<UserIdx> = w.global_intents.iter().map(|d| d.user).collect();
            users.sort_unstable();
            users.dedup();
            let mut owners: Vec<(String, UserIdx)> = users.into_iter().map(|u| (w.users[u as usize].id.as_str().to_string(), u)).collect();
            policy::sort_users(w.seed, ctx.time, &mut owners, |o| o.0.as_str());
            owners.into_iter().enumerate().map(|(i, (_, u))| (u, i)).collect()
        }
    };
    let mut docs: Vec<_> = w.global_intents.iter().collect();
    docs.sort_by_key(|d| (rank.get(&d.user).copied().unwrap_or(usize::MAX), !d.from_api, d.arrival));
    docs.into_iter()
        .map(|d| {
            let o = O::new();
            o.set(atom!("user"), V::str(w.users[d.user as usize].id.as_str()));
            o.set(atom!("intents"), live::from_val(&Val::Obj(d.intents.clone())));
            o
        })
        .collect()
}

/// The executes of global.js:48-58 against the world, in order. Inserts get seq/_id in this order.
pub(crate) fn apply(w: &mut World, ops: Ops) {
    let mut store = ObjectsStore { w, drafts: BTreeMap::new(), inserts: Vec::new() };
    bulk::apply(&mut store, ops.objects);
    store.finish();

    if !ops.rooms.is_empty() {
        let mut rooms = RoomsStore { w, touched: BTreeMap::new() };
        bulk::apply(&mut rooms, ops.rooms);
        for (r, info) in rooms.touched {
            let prev = &w.rooms[r as usize];
            let next = RoomSnapshot::from_docs(Arc::new(info), prev.objects.to_vec(), prev.flags.clone(), prev.event_log.clone(), prev.map_view.clone(), prev.processed_at, &w.user_ids.clone());
            w.set_room(r, Arc::new(next));
        }
    }

    crate::processor::apply_user_ops(w, ops.users);

    let market_touched = !(ops.market_orders.is_empty()
        && ops.intershard_orders.is_empty()
        && ops.users_money.is_empty()
        && ops.transactions.is_empty()
        && ops.users_resources.is_empty()
        && ops.market_stats.is_empty());
    if market_touched {
        let m = w.market.clone();
        // Intershard orders share market.orders on a single-shard server (the oracle does the same).
        let mut orders = SeqStore::new(&m.orders);
        bulk::apply(&mut orders, ops.market_orders);
        bulk::apply(&mut orders, ops.intershard_orders);
        let orders = orders.finish(w);
        let mut money = PlainStore::new(&m.money);
        bulk::apply(&mut money, ops.users_money);
        let money = money.finish(w);
        let mut transactions = SeqStore::new(&m.transactions);
        bulk::apply(&mut transactions, ops.transactions);
        let transactions = transactions.finish(w);
        let mut resources = PlainStore::new(&m.resources);
        bulk::apply(&mut resources, ops.users_resources);
        let resources = resources.finish(w);
        let mut stats = PlainStore::new(&m.stats);
        bulk::apply(&mut stats, ops.market_stats);
        let stats = stats.finish(w);
        w.market = Arc::new(MarketState::new(orders, stats, transactions, money, resources));
    }

    if !ops.users_power_creeps.is_empty() {
        let mut pcs = SeqStore::new(&w.users.iter().flat_map(|u| u.power_creeps.iter().cloned()).collect::<Vec<_>>());
        bulk::apply(&mut pcs, ops.users_power_creeps);
        let all = pcs.finish(w);
        let mut per_user: Vec<Vec<DocRef>> = vec![Vec::new(); w.users.len()];
        for d in all.iter() {
            let owner = d.obj.get(atom!("user")).and_then(Val::as_str).and_then(Id::new).and_then(|id| w.user_ids.get(&id).copied());
            if let Some(u) = owner {
                per_user[u as usize].push(d.clone());
            }
        }
        for (u, mut docs) in per_user.into_iter().enumerate() {
            docs.sort_by_key(|d| d.seq);
            let unchanged = docs.len() == w.users[u].power_creeps.len() && docs.iter().zip(w.users[u].power_creeps.iter()).all(|(a, b)| Arc::ptr_eq(a, b));
            if !unchanged {
                w.users[u] = Arc::new(UserState { power_creeps: docs.into(), ..crate::processor::clone_user(&w.users[u]) });
            }
        }
    }
}

/// A collection kept in seq order (market.orders, transactions, users.power_creeps). Inserts get seq and, unless
/// they carry one, `_id`, in call order (§3.7).
struct SeqStore {
    docs: Vec<DocRef>,
    inserts: Vec<Obj>,
    changed: bool,
}

impl SeqStore {
    fn new(docs: &[DocRef]) -> SeqStore {
        SeqStore { docs: docs.to_vec(), inserts: Vec::new(), changed: false }
    }

    fn finish(self, w: &mut World) -> Arc<[DocRef]> {
        let SeqStore { mut docs, inserts, .. } = self;
        for mut o in inserts {
            ensure_id(w, &mut o);
            let seq = w.next_seq;
            w.next_seq += 1;
            docs.push(Doc::new(seq, o));
        }
        docs.into()
    }
}

impl DocStore for SeqStore {
    fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj> {
        let d = self.docs.iter_mut().find(|d| d.id() == *id)?;
        self.changed = true;
        Some(Arc::make_mut(d).obj_mut())
    }
    fn remove(&mut self, id: &Id) {
        self.docs.retain(|d| d.id() != *id);
    }
    fn insert(&mut self, doc: Obj) {
        self.inserts.push(doc);
    }
}

/// The `rooms` collection: `$set` on room docs by `_id` (the room name).
struct RoomsStore<'w> {
    w: &'w World,
    touched: BTreeMap<RoomIdx, Obj>,
}

impl DocStore for RoomsStore<'_> {
    fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj> {
        let r = exact_room(self.w, id.as_str())?;
        let w = self.w;
        Some(self.touched.entry(r).or_insert_with(|| (*w.rooms[r as usize].info).clone()))
    }
    // Neither the global stage nor the crons remove or insert rooms.
    fn remove(&mut self, _: &Id) {}
    fn insert(&mut self, _: Obj) {}
}

/// A history collection without creation order (users.money, users.resources).
struct PlainStore {
    docs: Vec<Arc<Obj>>,
    inserts: Vec<Obj>,
}

impl PlainStore {
    fn new(docs: &[Arc<Obj>]) -> PlainStore {
        PlainStore { docs: docs.to_vec(), inserts: Vec::new() }
    }

    fn finish(self, w: &mut World) -> Arc<[Arc<Obj>]> {
        let PlainStore { mut docs, inserts } = self;
        for mut o in inserts {
            ensure_id(w, &mut o);
            docs.push(Arc::new(o));
        }
        docs.into()
    }
}

impl DocStore for PlainStore {
    fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj> {
        let d = self.docs.iter_mut().find(|d| d.get(atom!("_id")).and_then(Val::as_str) == Some(id.as_str()))?;
        Some(Arc::make_mut(d))
    }
    fn remove(&mut self, id: &Id) {
        self.docs.retain(|d| d.get(atom!("_id")).and_then(Val::as_str) != Some(id.as_str()));
    }
    fn insert(&mut self, doc: Obj) {
        self.inserts.push(doc);
    }
}

/// storage genId: an insert without `_id` gets one, appended last.
fn ensure_id(w: &mut World, o: &mut Obj) {
    if o.get(atom!("_id")).and_then(Val::as_str).and_then(Id::new).is_none() {
        o.set(atom!("_id"), Val::Str(w.ids.next().as_str().into()));
    }
}

/// The `rooms.objects` collection for the global stage's bulk writer: copy-on-write per touched room. A doc whose
/// `room` field names another room afterwards moves there, keeping its seq (creation order).
struct ObjectsStore<'w> {
    w: &'w mut World,
    drafts: BTreeMap<RoomIdx, Vec<DocRef>>,
    inserts: Vec<Obj>,
}

impl ObjectsStore<'_> {
    fn draft(&mut self, r: RoomIdx) -> &mut Vec<DocRef> {
        let rooms = &self.w.rooms;
        self.drafts.entry(r).or_insert_with(|| rooms[r as usize].objects.to_vec())
    }

    fn finish(self) {
        let ObjectsStore { w, mut drafts, inserts } = self;
        let mut moved = Vec::new();
        for (&r, docs) in drafts.iter_mut() {
            docs.retain(|d| match d.obj.get(atom!("room")).and_then(Val::as_str).and_then(|n| exact_room(w, n)) {
                Some(dest) if dest != r => {
                    moved.push((dest, d.clone()));
                    false
                }
                _ => true,
            });
        }
        for (dest, doc) in moved {
            let docs = drafts.entry(dest).or_insert_with(|| w.rooms[dest as usize].objects.to_vec());
            let at = docs.partition_point(|d| d.seq < doc.seq);
            docs.insert(at, doc);
        }
        for mut doc in inserts {
            // ponytail: an insert naming no known room would be an orphan doc in vanilla; it is dropped here.
            let Some(r) = doc.get(atom!("room")).and_then(Val::as_str).and_then(|n| exact_room(w, n)) else { continue };
            ensure_id(w, &mut doc);
            let seq = w.next_seq;
            w.next_seq += 1;
            drafts.entry(r).or_insert_with(|| w.rooms[r as usize].objects.to_vec()).push(Doc::new(seq, doc));
        }
        let user_ids = w.user_ids.clone();
        for (r, docs) in drafts {
            let prev = &w.rooms[r as usize];
            let next = RoomSnapshot::from_docs(prev.info.clone(), docs, prev.flags.clone(), prev.event_log.clone(), prev.map_view.clone(), prev.processed_at, &user_ids);
            w.set_room(r, Arc::new(next));
        }
    }
}

impl DocStore for ObjectsStore<'_> {
    fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj> {
        let loc = *self.w.index.objects.get(id)?;
        let docs = self.draft(loc.room);
        let i = docs.binary_search_by_key(&loc.seq, |d| d.seq).ok()?;
        Some(Arc::make_mut(&mut docs[i]).obj_mut())
    }

    fn remove(&mut self, id: &Id) {
        let Some(loc) = self.w.index.objects.get(id).copied() else { return };
        let docs = self.draft(loc.room);
        if let Ok(i) = docs.binary_search_by_key(&loc.seq, |d| d.seq) {
            docs.remove(i);
        }
    }

    fn insert(&mut self, doc: Obj) {
        self.inserts.push(doc);
    }
}

// ---- small JS helpers shared by global-intents ----

/// `{k1: v1, k2: v2, …}` (static keys).
pub(crate) fn obj(pairs: Vec<(&'static str, V)>) -> V {
    let o = O::new();
    for (k, v) in pairs {
        o.set(Atom::intern(k), v);
    }
    V::Obj(o)
}

/// A property key from a runtime value (`o[key]`), only for resource types and intershard resources (never
/// interns arbitrary strings; other keys never reach the global stage after the room-phase checks).
pub(crate) fn resource_key(v: &V) -> Option<Atom> {
    let s = to_js_string(v);
    let known = s.parse::<crate::constants::ResourceType>().is_ok() || crate::constants::INTERSHARD_RESOURCES.contains(&&*s);
    known.then(|| Atom::intern(&s))
}
