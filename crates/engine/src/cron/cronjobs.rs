//! mirror: reference/backend/lib/cronjobs.js
//! parity: FAITHFUL
//!
//! CRON-STAGE jobs at vclock(G) (ARCHITECTURE.md §5.9). Each job reads the world through Loki-like queries in
//! collection order and applies every write at once (later queries of the same run see it, as with the
//! storage RPC). `Math.random` is the cron stream of (job, G, unit), the unit being the room / terminal room /
//! sector centre / core the job's outer loop is on (RNG-SEED, tools/oracle/cron.js). `sendNotifications` runs on
//! the backend's real 60 s timer (CLK-NOTIFY), not here.

use crate::atom;
use crate::bulk::DbOp;
use crate::constants as C;
use crate::doc::{Atom, DocRef, Id, Obj, Val};
use crate::live::{from_val, to_js_string, to_number, truthy, V};
use crate::policy;
use crate::processor::global::{apply, exact_room, Ops};
use crate::world::{RoomIdx, World};
use std::sync::Arc;
use util::rng::Rng;

/// What a cron job (built-in or mod) gets besides `&mut World`: the stored game time G it runs at, `Date.now()`
/// (= vclock(G)) and the cron RNG streams.
pub struct CronCtx {
    pub g: u64,
    pub now: i64,
    seed: u64,
    job: &'static str,
    unit: Option<Rng>,
}

impl CronCtx {
    pub fn new(seed: u64, g: u64, now: i64, job: &'static str) -> CronCtx {
        CronCtx { g, now, seed, job, unit: None }
    }

    /// The job's outer loop moved on to `unit`: its own stream from now on.
    pub fn unit(&mut self, unit: &str) {
        self.unit = Some(policy::cron_stream(self.seed, self.g, self.job, unit));
    }

    /// `Math.random()`.
    pub fn random(&mut self) -> f64 {
        let (seed, g, job) = (self.seed, self.g, self.job);
        self.unit.get_or_insert_with(|| policy::cron_stream(seed, g, job, "<none>")).next_f64()
    }

    pub fn rng(&mut self) -> &mut Rng {
        let (seed, g, job) = (self.seed, self.g, self.job);
        self.unit.get_or_insert_with(|| policy::cron_stream(seed, g, job, "<none>"))
    }
}

/// A job: `Err` is a rejected job promise (vanilla logs it; the rest of that run is skipped).
pub type JobFn = fn(&mut World, &mut CronCtx) -> Result<(), String>;

/// `config.cronjobs` in key order (period in seconds of vclock), without sendNotifications.
pub const JOBS: &[(&str, u32, JobFn)] = &[
    ("roomsForceUpdate", 20, rooms_force_update),
    ("genPowerBanks", 5 * 60, gen_power_banks),
    ("genInvaders", 5 * 60, gen_invaders),
    ("purgeTransactions", 60 * 60, purge_transactions),
    ("recreateNpcOrders", 5 * 60, recreate_npc_orders),
    ("calcMarketStats", 60 * 60, calc_market_stats),
    ("deletePowerCreeps", 10 * 60, delete_power_creeps),
    ("genDeposits", 5 * 60, gen_deposits),
    ("genStrongholds", 5 * 60, super::strongholds::gen_strongholds),
    ("expandStrongholds", 15 * 60, super::strongholds::expand_strongholds),
];

/// Runs one built-in job by name (oracle replays, CLI).
pub fn run_job(name: &str, w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    let (_, _, f) = JOBS.iter().find(|(n, _, _)| *n == name).ok_or_else(|| format!("unknown cron job {name}"))?;
    f(w, cx)
}

// ---- Loki-like reads and immediate writes ----

/// `o[k]` as a JS value.
pub(crate) fn field(o: &Obj, k: &str) -> V {
    o.get(Atom::intern(k)).map_or(V::Undef, from_val)
}

pub(crate) fn sv(s: &str) -> Val {
    Val::Str(s.into())
}

pub(crate) fn obj(pairs: Vec<(&str, Val)>) -> Obj {
    let mut o = Obj::new();
    for (k, v) in pairs {
        o.set(Atom::intern(k), v);
    }
    o
}

/// The `rooms` collection in collection order: (RoomIdx, doc).
pub(crate) fn rooms(w: &World) -> Vec<(RoomIdx, Arc<Obj>)> {
    (0..w.rooms.len()).filter(|&r| w.meta.rooms.get(r).is_some_and(|m| !m.removed)).map(|r| (r as RoomIdx, w.rooms[r].info.clone())).collect()
}

pub(crate) fn name_of(w: &World, r: RoomIdx) -> String {
    w.meta.rooms[r as usize].name.to_string()
}

/// `rooms.objects.find(pred)` across the world, collection (seq) order.
pub(crate) fn objects(w: &World, pred: impl Fn(&DocRef) -> bool) -> Vec<DocRef> {
    let mut v: Vec<DocRef> = w.rooms.iter().flat_map(|r| r.objects.iter()).filter(|d| pred(d)).cloned().collect();
    v.sort_by_key(|d| d.seq);
    v
}

/// `rooms.objects.find({room, …pred})`.
pub(crate) fn objects_in(w: &World, r: RoomIdx, pred: impl Fn(&DocRef) -> bool) -> Vec<DocRef> {
    w.rooms[r as usize].objects.iter().filter(|d| pred(d)).cloned().collect()
}

pub(crate) fn set_room(w: &mut World, r: RoomIdx, fields: Obj) {
    let Some(id) = Id::new(&name_of(w, r)) else { return };
    apply(w, Ops { rooms: vec![DbOp::Set(id, fields)], ..Ops::default() });
}

pub(crate) fn insert_objects(w: &mut World, docs: Vec<Obj>) {
    apply(w, Ops { objects: docs.into_iter().map(DbOp::Insert).collect(), ..Ops::default() });
}

pub(crate) fn set_object(w: &mut World, d: &DocRef, fields: Obj) {
    apply(w, Ops { objects: vec![DbOp::Set(d.id(), fields)], ..Ops::default() });
}

/// `utils.activateRoom(room)`.
pub(crate) fn activate(w: &mut World, r: RoomIdx) {
    w.active.insert(r);
}

/// `parseInt(terrain.charAt(i)) & 1` (an index off the string reads NaN: not a wall).
pub(crate) fn wall_at(w: &World, r: RoomIdx, i: i64) -> bool {
    (0..2500).contains(&i) && w.terrain.raw(r)[i as usize] & C::TERRAIN_MASK_WALL != 0
}

/// `common.checkTerrain(terrain, x, y, WALL)`.
pub(crate) fn wall_xy(w: &World, r: RoomIdx, x: i64, y: i64) -> bool {
    wall_at(w, r, y * 50 + x)
}

/// `{k: v}` matched with Loki `===` against a stored value.
fn is(o: &Obj, k: &str, v: &str) -> bool {
    matches!(o.get(Atom::intern(k)), Some(Val::Str(s)) if &**s == v)
}

/// backend utils.js:301 isCenter.
pub(crate) fn is_center(x: f64, y: f64) -> bool {
    let c = |v: f64| (v < 0.0 && ((v + 1.0).abs() % 10.0) >= 4.0 && ((v + 1.0).abs() % 10.0) <= 6.0) || (v >= 0.0 && (v.abs() % 10.0) >= 4.0 && (v.abs() % 10.0) <= 6.0);
    c(x) && c(y)
}

/// backend utils.js:306 isVeryCenter.
pub(crate) fn is_very_center(x: f64, y: f64) -> bool {
    let c = |v: f64| (v < 0.0 && (v + 1.0).abs() % 10.0 == 5.0) || (v >= 0.0 && v.abs() % 10.0 == 5.0);
    c(x) && c(y)
}

/// backend utils.js:12 roomNameFromXY.
pub(crate) fn room_name_from_xy(x: f64, y: f64) -> String {
    let xs = if x < 0.0 { format!("W{}", util::num::to_js_string(-x - 1.0)) } else { format!("E{}", util::num::to_js_string(x)) };
    let ys = if y < 0.0 { format!("N{}", util::num::to_js_string(-y - 1.0)) } else { format!("S{}", util::num::to_js_string(y)) };
    xs + &ys
}

/// lodash 3 `_.sum(list, fn)` (arraySum: from the last element, `+x || 0`).
fn lodash_sum(xs: impl DoubleEndedIterator<Item = f64>) -> f64 {
    xs.rev().fold(0.0, |a, x| a + if x.is_nan() { 0.0 } else { x })
}

// ---- jobs ----

/// cronjobs.js:178 roomsForceUpdate.
fn rooms_force_update(w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    let g = cx.g as f64;
    let active: Vec<RoomIdx> = w.active.list().to_vec();
    for (r, room) in rooms(w) {
        if is(&room, "status", "out of borders") || active.contains(&r) {
            continue;
        }
        cx.unit(&name_of(w, r));
        let next = field(&room, "nextForceUpdateTime");
        if !truthy(&next) || g >= to_number(&next) {
            activate(w, r);
            let t = g + 90.0 + (cx.random() * 20.0).floor();
            set_room(w, r, obj(vec![("nextForceUpdateTime", Val::Num(t))]));
        }
    }
    Ok(())
}

/// cronjobs.js:194 genPowerBanks.
fn gen_power_banks(w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    let g = cx.g as f64;
    let candidates: Vec<(RoomIdx, Arc<Obj>)> = rooms(w).into_iter().filter(|(_, room)| matches!(room.get(atom!("bus")), Some(Val::Bool(true))) && is(room, "status", "normal")).collect();
    for (r, room) in candidates {
        cx.unit(&name_of(w, r));
        let respawn = crate::processor::js_round(cx.random() * C::POWER_BANK_RESPAWN_TIME as f64 / 2.0 + C::POWER_BANK_RESPAWN_TIME as f64 * 0.75);
        if !truthy(&field(&room, "powerBankTime")) {
            let mut doc = (*room).clone();
            doc.set(Atom::intern("powerBankTime"), Val::Num(g + respawn));
            set_room(w, r, doc);
            continue;
        }
        if g < to_number(&field(&room, "powerBankTime")) {
            continue;
        }
        let bank_time = g + respawn;
        let (mut x, mut y);
        loop {
            x = (cx.random() * 40.0 + 5.0).floor() as i64;
            y = (cx.random() * 40.0 + 5.0).floor() as i64;
            let is_wall = wall_at(w, r, y * 50 + x);
            let has_exit = (-1..=1).any(|dx| (-1..=1).any(|dy| !wall_at(w, r, (y + dy) * 50 + x + dx)));
            if is_wall && has_exit {
                break;
            }
        }
        let mut power = (cx.random() * (C::POWER_BANK_CAPACITY_MAX - C::POWER_BANK_CAPACITY_MIN) as f64 + C::POWER_BANK_CAPACITY_MIN as f64).floor();
        if cx.random() < C::POWER_BANK_CAPACITY_CRIT {
            power += C::POWER_BANK_CAPACITY_MAX as f64;
        }
        let bank = obj(vec![
            ("type", sv("powerBank")),
            ("x", Val::Num(x as f64)),
            ("y", Val::Num(y as f64)),
            ("room", sv(&name_of(w, r))),
            ("store", Val::Obj(obj(vec![("power", Val::Num(power))]))),
            ("hits", Val::Num(C::POWER_BANK_HITS as f64)),
            ("hitsMax", Val::Num(C::POWER_BANK_HITS as f64)),
            ("decayTime", Val::Num(g + C::POWER_BANK_DECAY as f64)),
        ]);
        insert_objects(w, vec![bank]);
        set_room(w, r, obj(vec![("powerBankTime", Val::Num(bank_time))]));
        activate(w, r);
    }
    Ok(())
}

const INVADER_BODIES: &[(&str, &[&str])] = &[
    ("bigHealer", &["move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "heal", "move"]),
    ("bigRanged", &["tough", "tough", "tough", "tough", "tough", "tough", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "ranged_attack", "work", "move"]),
    ("bigMelee", &["tough", "tough", "tough", "tough", "tough", "tough", "tough", "tough", "tough", "tough", "tough", "tough", "tough", "tough", "tough", "tough", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "move", "ranged_attack", "ranged_attack", "ranged_attack", "work", "work", "work", "work", "attack", "attack", "move"]),
    ("smallHealer", &["move", "move", "move", "move", "heal", "heal", "heal", "heal", "heal", "move"]),
    ("smallRanged", &["tough", "tough", "move", "move", "move", "move", "ranged_attack", "ranged_attack", "ranged_attack", "move"]),
    ("smallMelee", &["tough", "tough", "move", "move", "move", "move", "ranged_attack", "work", "attack", "move"]),
];

/// cronjobs.js:244 createCreep.
fn invader(kind: &str, room: &str, square: (i64, i64), boosted: bool, cx: &mut CronCtx) -> Obj {
    let (x, y) = crate::utils::room_name_to_xy(room);
    let body_types = INVADER_BODIES.iter().find(|(k, _)| *k == kind).map_or(&[][..], |(_, b)| *b);
    let center = is_center(x, y);
    let body: Vec<Val> = body_types
        .iter()
        .map(|&t| {
            let mut p = obj(vec![("type", sv(t)), ("hits", Val::Num(100.0))]);
            if boosted {
                let boost = match t {
                    "heal" => Some(if center { "XLHO2" } else { "LO" }),
                    "ranged_attack" => Some(if center { "XKHO2" } else { "KO" }),
                    "work" => Some(if center { "XZH2O" } else { "ZH" }),
                    "attack" => Some(if center { "XUH2O" } else { "UH" }),
                    "tough" => Some(if center { "XGHO2" } else { "GO" }),
                    _ => None,
                };
                if let Some(b) = boost {
                    p.set(Atom::intern("boost"), sv(b));
                }
            }
            Val::Obj(p)
        })
        .collect();
    let n = body.len() as f64;
    let name = format!("invader_{room}_{}", util::num::to_js_string((cx.random() * 1000.0).floor()));
    obj(vec![
        ("type", sv("creep")),
        ("user", sv("2")),
        ("body", Val::Arr(body)),
        ("hits", Val::Num(n * 100.0)),
        ("hitsMax", Val::Num(n * 100.0)),
        ("ticksToLive", Val::Num(1500.0)),
        ("x", Val::Num(square.0 as f64)),
        ("y", Val::Num(square.1 as f64)),
        ("room", sv(room)),
        ("fatigue", Val::Num(0.0)),
        ("store", Val::Obj(Obj::new())),
        ("storeCapacity", Val::Num(0.0)),
        ("name", sv(&name)),
    ])
}

/// cronjobs.js:317 createRaid.
fn create_raid(controller_level: &V, room: &str, mut exits: Vec<(i64, i64)>, cx: &mut CronCtx) -> Vec<Obj> {
    let level = to_number(controller_level);
    let big = truthy(controller_level) && level >= 4.0;
    let kind = if big { "big" } else { "small" };
    let (mut count, mut boost_chance) = (1.0, 0.5);
    let (x, y) = crate::utils::room_name_to_xy(room);
    let center = is_center(x, y);
    if cx.random() > 0.9 || center {
        let mut max = 2.0;
        boost_chance = if big { 0.0 } else { 0.5 };
        if cx.random() > 0.8 || center {
            max = 5.0;
            if big {
                if level < 5.0 || level < 6.0 {
                    max = 2.0;
                } else if level < 7.0 {
                    max = 3.0;
                } else if level < 8.0 {
                    boost_chance = 0.4;
                    max = 3.0;
                } else {
                    boost_chance = 0.4;
                    max = 5.0;
                }
            }
        }
        count = (cx.random() * (max - 1.0)).floor() + 2.0;
        count = crate::processor::js_min(count, exits.len() as f64);
    }
    let mut creeps = Vec::new();
    let mut last: Option<(i64, i64)> = None;
    let mut i = 0.0;
    while i < count {
        let subtype = if i == 0.0 && !center {
            "Melee"
        } else if i == 0.0 || ((i == 1.0 || (i == 2.0 && count == 5.0)) && cx.random() > 0.5) {
            "Ranged"
        } else {
            "Healer"
        };
        let square = match last {
            // _.sortBy(exits, chebyshev)[0]: the first closest
            Some(l) => exits.iter().copied().min_by_key(|e| (e.0 - l.0).abs().max((e.1 - l.1).abs())),
            None => {
                let k = (cx.random() * exits.len() as f64).floor() as usize;
                exits.get(k).copied()
            }
        };
        let Some(square) = square else { break };
        let boosted = cx.random() < boost_chance;
        creeps.push(invader(&format!("{kind}{subtype}"), room, square, boosted, cx));
        if let Some(p) = exits.iter().position(|e| *e == square) {
            exits.remove(p);
        }
        last = Some(square);
        i += 1.0;
    }
    creeps
}

/// The sector regex `^p1\dp2\d$` of a room (cronjobs.js:390), as the prefixes; None when the name does not have
/// that shape (the regex is then the name itself).
pub(crate) fn sector_prefixes(name: &str) -> Option<(String, String)> {
    let b = name.as_bytes();
    if b.len() < 4 || !matches!(b[0], b'W' | b'E') {
        return None;
    }
    let ns = b.iter().position(|c| matches!(c, b'N' | b'S'))?;
    let (xd, yd) = (&b[1..ns], &b[ns + 1..]);
    if xd.is_empty() || yd.is_empty() || !xd.iter().chain(yd).all(u8::is_ascii_digit) {
        return None;
    }
    Some((name[..ns - 1].to_string(), name[ns..b.len() - 1].to_string()))
}

/// `name` matches `^p1\dp2\d$`.
pub(crate) fn in_sector(name: &str, p: &Option<(String, String)>, own: &str) -> bool {
    let Some((p1, p2)) = p else { return name == own };
    let rest = match name.strip_prefix(p1.as_str()) {
        Some(r) => r,
        None => return false,
    };
    let mut c = rest.chars();
    if !c.next().is_some_and(|d| d.is_ascii_digit()) {
        return false;
    }
    let rest = c.as_str();
    let Some(rest) = rest.strip_prefix(p2.as_str()) else { return false };
    rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit()
}

/// cronjobs.js:376 genInvaders.
fn gen_invaders(w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    let active: Vec<RoomIdx> = w.active.list().to_vec();
    let candidates: Vec<(RoomIdx, Arc<Obj>)> = rooms(w).into_iter().filter(|(r, room)| is(room, "status", "normal") && !active.contains(r)).collect();
    for (r, room) in candidates {
        let name = name_of(w, r);
        cx.unit(&name);
        let objs = objects_in(w, r, |d| matches!(d.kind(), "source" | "controller" | "creep"));
        if objs.iter().any(|d| d.kind() == "creep" && is(&d.obj, "user", "2")) {
            continue;
        }
        let sources: Vec<&DocRef> = objs.iter().filter(|d| d.kind() == "source").collect();
        let harvested = lodash_sum(sources.iter().map(|d| to_number(&field(&d.obj, "invaderHarvested"))));
        let goal = {
            let g = field(&room, "invaderGoal");
            if truthy(&g) {
                to_number(&g)
            } else {
                C::INVADERS_ENERGY_GOAL as f64
            }
        };
        if goal != 1.0 && harvested < goal {
            continue;
        }
        let sector = sector_prefixes(&name);
        let cores = objects(w, |d| {
            d.kind() == "invaderCore"
                && matches!(d.obj.get(atom!("level")), Some(Val::Num(l)) if *l > 0.0)
                && d.obj.get(atom!("room")).and_then(Val::as_str).is_some_and(|rn| in_sector(rn, &sector, &name))
        });
        if cores.is_empty() {
            continue;
        }
        let mut exits: Vec<&str> = Vec::new();
        let mut squares: [Vec<(i64, i64)>; 4] = Default::default(); // top, bottom, left, right
        for i in 0..49 {
            let checks = [("top", i, 0, 0), ("bottom", i, 49, 1), ("left", 0, i, 2), ("right", 49, i, 3)];
            for (side, x, y, k) in checks {
                if !wall_xy(w, r, x, y) {
                    if !exits.contains(&side) {
                        exits.push(side);
                    }
                    squares[k].push((x, y));
                }
            }
        }
        // checkExit: a neighbour with an owned or reserved controller closes that side
        let (x, y) = crate::utils::room_name_to_xy(&name);
        exits.retain(|&side| {
            let (nx, ny) = match side {
                "top" => (x, y - 1.0),
                "right" => (x + 1.0, y),
                "bottom" => (x, y + 1.0),
                _ => (x - 1.0, y),
            };
            let next = room_name_from_xy(nx, ny);
            let controller = exact_room(w, &next).and_then(|nr| objects_in(w, nr, |d| d.kind() == "controller").into_iter().next());
            !controller.is_some_and(|c| truthy(&field(&c.obj, "user")) || truthy(&field(&c.obj, "reservation")))
        });
        if exits.is_empty() {
            continue;
        }
        let exit = exits[(cx.random() * exits.len() as f64).floor() as usize];
        let k = match exit {
            "top" => 0,
            "bottom" => 1,
            "left" => 2,
            _ => 3,
        };
        let controller = objs.iter().find(|d| d.kind() == "controller");
        // `controller && controller.user && controller.level`
        let level = match controller {
            None => V::Undef,
            Some(c) => {
                let u = field(&c.obj, "user");
                if truthy(&u) {
                    field(&c.obj, "level")
                } else {
                    u
                }
            }
        };
        let creeps = create_raid(&level, &name, squares[k].clone(), cx);
        insert_objects(w, creeps);
        let mut invader_goal = (C::INVADERS_ENERGY_GOAL as f64 * (cx.random() * 0.6 + 0.7)).floor();
        if cx.random() < 0.1 {
            invader_goal *= if cx.random() > 0.5 { 2.0 } else { 0.0 }; // Math.floor(0.5) is 0
        }
        set_room(w, r, obj(vec![("invaderGoal", Val::Num(invader_goal))]));
        let sources: Vec<DocRef> = objects_in(w, r, |d| d.kind() == "source");
        for s in &sources {
            set_object(w, s, obj(vec![("invaderHarvested", Val::Num(0.0))]));
        }
        activate(w, r);
    }
    Ok(())
}

/// cronjobs.js:443 purgeTransactions: keep the newest 100 per sender and per recipient.
fn purge_transactions(w: &mut World, _: &mut CronCtx) -> Result<(), String> {
    let mut data: Vec<DocRef> = w.market.transactions.to_vec();
    data.sort_by(|a, b| {
        let t = |d: &DocRef| -crate::live::parse_int(&field(&d.obj, "time"));
        t(a).partial_cmp(&t(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
    let (mut senders, mut recipients) = (std::collections::HashMap::<String, usize>::new(), std::collections::HashMap::<String, usize>::new());
    let mut delete = Vec::new();
    for d in &data {
        let s = senders.entry(to_js_string(&field(&d.obj, "sender")).to_string()).or_default();
        let keep1 = *s < 100;
        if keep1 {
            *s += 1;
        }
        let rc = recipients.entry(to_js_string(&field(&d.obj, "recipient")).to_string()).or_default();
        let keep2 = *rc < 100;
        if keep2 {
            *rc += 1;
        }
        if !keep1 && !keep2 {
            delete.push(DbOp::Remove(d.id()));
        }
    }
    if !delete.is_empty() {
        apply(w, Ops { transactions: delete, ..Ops::default() });
    }
    Ok(())
}

/// cronjobs.js:38 recreateNpcOrders.
fn recreate_npc_orders(w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    const MINERALS: [&str; 17] = ["X", "Z", "K", "L", "U", "O", "O", "H", "H", "Z", "K", "L", "U", "O", "O", "H", "H"];
    let sell_price = |m: &str| match m {
        "H" | "O" => 3000.0,
        "X" => 18000.0,
        _ => 6000.0,
    };
    let (sell_mineral_amount, sell_energy_amount, buy_mineral_amount, period) = (40.0, 40.0, 20.0, 5000.0);
    let g = cx.g as f64;
    // {type: 'terminal', user: {$eq: null}}: storage matches null and undefined
    let terminals = objects(w, |d| d.kind() == "terminal" && matches!(d.obj.get(atom!("user")), None | Some(Val::Null)));
    for t in terminals {
        let room_name = to_js_string(&field(&t.obj, "room")).to_string();
        cx.unit(&room_name);
        let Some(r) = exact_room(w, &room_name) else { return Err(format!("Cannot read properties of null (reading 'status') ({room_name})")) };
        let room = w.rooms[r as usize].info.clone();
        if !is(&room, "status", "normal") {
            continue;
        }
        let next = field(&room, "nextNpcMarketOrder");
        if truthy(&next) && to_number(&next) > g {
            continue;
        }
        let sell = MINERALS[(cx.random() * MINERALS.len() as f64).floor() as usize];
        let buy = MINERALS[(cx.random() * MINERALS.len() as f64).floor() as usize];
        let now = cx.now as f64;
        let order = |kind: &str, amount: f64, resource: &str, price: f64| {
            obj(vec![
                ("created", Val::Num(g)),
                ("createdTimestamp", Val::Num(now)),
                ("active", Val::Bool(true)),
                ("type", sv(kind)),
                ("amount", Val::Num(period * amount)),
                ("remainingAmount", Val::Num(period * amount)),
                ("totalAmount", Val::Num(period * amount)),
                ("resourceType", sv(resource)),
                ("price", Val::Num(price)),
                ("roomName", sv(&room_name)),
            ])
        };
        let mut orders = vec![order("sell", sell_mineral_amount, sell, sell_price(sell))];
        if cx.random() < 0.5 {
            orders.push(order("sell", sell_energy_amount, "energy", 1000.0));
        }
        if cx.random() < 0.25 {
            orders.push(order("buy", buy_mineral_amount, buy, 1000.0));
        }
        let mut ops: Vec<DbOp> = w.market.orders.iter().filter(|o| is(&o.obj, "roomName", &room_name)).map(|o| DbOp::Remove(o.id())).collect();
        ops.extend(orders.into_iter().map(DbOp::Insert));
        apply(w, Ops { market_orders: ops, ..Ops::default() });
        let next = g + crate::processor::js_round(period * (0.8 + 0.4 * cx.random()));
        set_room(w, r, obj(vec![("nextNpcMarketOrder", Val::Num(next))]));
    }
    Ok(())
}

/// cronjobs.js:472 calcMarketStats over the last 14 days of `market.sell` (calendar days of vclock, UTC).
fn calc_market_stats(w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    let mut ops: Vec<DbOp> = w.market.stats.iter().filter_map(|s| s.get(atom!("_id")).and_then(Val::as_str).and_then(Id::new)).map(DbOp::Remove).collect();
    // date stored as an ISO string; Loki compares it with the ISO string of the cut-off
    let since = util::num::iso_date(cx.now - 14 * 24 * 3600 * 1000);
    let data: Vec<Arc<Obj>> =
        w.market.money.iter().filter(|m| matches!(m.get(atom!("date")), Some(Val::Str(d)) if **d > *since) && is(m, "type", "market.sell")).cloned().collect();
    // result[resourceType][dateStr], both in insertion order
    let mut result: Vec<(String, Vec<(String, [f64; 4])>)> = Vec::new(); // [sumPrice, sumAmount, stddev, cnt]
    let mut keyed = Vec::new();
    for m in &data {
        let date = to_js_string(&field(m, "date"));
        let date_str = date.get(..10).unwrap_or("").to_string(); // getFullYear-getMonth+1-getDate (TZ=UTC), zero-padded
        let market = m.get(atom!("market")).and_then(Val::as_obj).cloned().unwrap_or_default();
        let r = to_js_string(&field(&market, "resourceType")).to_string();
        let ri = match result.iter().position(|(k, _)| *k == r) {
            Some(i) => i,
            None => {
                result.push((r.clone(), Vec::new()));
                result.len() - 1
            }
        };
        let di = match result[ri].1.iter().position(|(k, _)| *k == date_str) {
            Some(i) => i,
            None => {
                result[ri].1.push((date_str.clone(), [0.0; 4]));
                result[ri].1.len() - 1
            }
        };
        let e = &mut result[ri].1[di].1;
        e[0] += to_number(&field(m, "change"));
        e[1] += to_number(&field(&market, "amount"));
        e[3] += 1.0;
        keyed.push((ri, di, to_number(&field(&market, "amount")), to_number(&field(&market, "price"))));
    }
    let avg = |e: &[f64; 4]| e[0] / e[1];
    for (ri, di, amount, price) in keyed {
        let e = result[ri].1[di].1;
        result[ri].1[di].1[2] += amount * crate::processor::js_pow(price - avg(&e), 2.0) / e[1];
    }
    let fixed3 = |x: f64| util::num::string_to_number(&util::num::to_fixed(x, 3));
    for (resource, dates) in &result {
        for (date, e) in dates {
            ops.push(DbOp::Insert(obj(vec![
                ("resourceType", sv(resource)),
                ("date", sv(date)),
                ("transactions", Val::Num(e[3])),
                ("volume", Val::Num(e[1])),
                ("avgPrice", Val::Num(fixed3(avg(e)))),
                ("stddevPrice", Val::Num(fixed3(e[2].sqrt()))),
            ])));
        }
    }
    apply(w, Ops { market_stats: ops, ..Ops::default() });
    Ok(())
}

/// cronjobs.js:533 deletePowerCreeps: refund the creep's share of the power level.
fn delete_power_creeps(w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    let now = cx.now as f64;
    let mut pcs: Vec<DocRef> = w.users.iter().flat_map(|u| u.power_creeps.iter().cloned()).collect();
    pcs.sort_by_key(|d| d.seq);
    let due: Vec<DocRef> = pcs.into_iter().filter(|d| matches!(d.obj.get(Atom::intern("deleteTime")), Some(Val::Num(t)) if *t < now)).collect();
    let (mul, pow) = (C::POWER_LEVEL_MULTIPLY as f64, C::POWER_LEVEL_POW as f64);
    let base = |level: f64| crate::processor::js_pow(level, pow) * mul;
    for pc in due {
        if !truthy(&field(&pc.obj, "deleteTime")) {
            continue;
        }
        let uid = to_js_string(&field(&pc.obj, "user"));
        let Some(&u) = Id::new(&uid).and_then(|id| w.user_ids.get(&id)) else { return Err("Cannot read properties of null (reading 'power')".into()) };
        let power = to_number(&field(&w.users[u as usize].doc, "power"));
        let p = if power.is_nan() || power == 0.0 { 0.0 } else { power };
        let level = crate::processor::js_pow(p / mul, 1.0 / pow).floor();
        let (prev, cur, next) = (base(level - 1.0), base(level), base(level + 1.0));
        let change = crate::processor::js_round(power - prev - (power - cur) * (cur - prev) / (next - cur));
        let id = w.users[u as usize].id;
        apply(w, Ops { users: vec![DbOp::Inc(id, vec![atom!("power")], -change)], users_power_creeps: vec![DbOp::Remove(pc.id())], ..Ops::default() });
    }
    Ok(())
}

/// cronjobs.js:556 genDeposits.
fn gen_deposits(w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    let g = cx.g as f64;
    // BUG-DEPOSIT-REGEX: the string '^[WE]\d*5[NS]\d*5$' is the pattern ^[WE]d*5[NS]d*5$ (replicated unless fixed)
    let fixed = w.compat.fix("BUG-DEPOSIT-REGEX");
    let is_central = |name: &str| {
        let b = name.as_bytes();
        let Some(ns) = b.iter().position(|c| matches!(c, b'N' | b'S')) else { return false };
        let ok = |s: &[u8]| !s.is_empty() && *s.last().unwrap() == b'5' && s[..s.len() - 1].iter().all(|c| if fixed { c.is_ascii_digit() } else { *c == b'd' });
        matches!(b.first(), Some(b'W' | b'E')) && ok(&b[1..ns]) && ok(&b[ns + 1..])
    };
    let centrals: Vec<(RoomIdx, Arc<Obj>)> = rooms(w).into_iter().filter(|(r, room)| is_central(&name_of(w, *r)) && !is(room, "status", "out of borders")).collect();
    let deposits = objects(w, |d| d.kind() == "deposit");
    for (cr, _) in centrals {
        let center = name_of(w, cr);
        cx.unit(&center);
        // getSectorEdges: /^([WE])(\d*)5([NS])(\d*)5$/
        let Some(edges) = sector_edges(&center) else { continue };
        let (cxr, cyr) = crate::utils::room_name_to_xy(&center);
        let (cxx, cyy) = (24.0 + 50.0 * cxr, 24.0 + 50.0 * cyr);
        let in_sector = |room: &str, x: f64, y: f64| {
            let (xx, yy) = crate::utils::room_name_to_xy(room);
            (50.0 * xx + x - cxx).abs() < 250.0 && (50.0 * yy + y - cyy).abs() < 250.0
        };
        let sector_deposits: Vec<&DocRef> = deposits
            .iter()
            .filter(|d| {
                let room = to_js_string(&field(&d.obj, "room"));
                edges.iter().any(|e| *e == *room) && in_sector(&room, to_number(&field(&d.obj, "x")), to_number(&field(&d.obj, "y")))
            })
            .collect();
        let throughput = lodash_sum(sector_deposits.iter().map(|d| {
            let harvested = field(&d.obj, "harvested");
            let h = if truthy(&harvested) { to_number(&harvested) } else { 0.0 };
            20.0 / crate::processor::js_max(1.0, C::DEPOSIT_EXHAUST_MULTIPLY * crate::processor::js_pow(h, C::DEPOSIT_EXHAUST_POW))
        }));
        if throughput >= 2.5 {
            continue;
        }
        let bus: Vec<(RoomIdx, Arc<Obj>)> = rooms(w)
            .into_iter()
            .filter(|(r, room)| edges.contains(&name_of(w, *r)) && matches!(room.get(atom!("bus")), Some(Val::Bool(true))) && is(room, "status", "normal") && room.get(Atom::intern("depositType")).is_some())
            .collect();
        if bus.is_empty() {
            return Err(format!("No normal bus rooms found for the sector of {center}"));
        }
        let busy: Vec<String> = sector_deposits.iter().map(|d| to_js_string(&field(&d.obj, "room")).to_string()).collect();
        let free: Vec<(RoomIdx, Arc<Obj>)> = bus.into_iter().filter(|(r, _)| !busy.contains(&name_of(w, *r))).collect();
        if free.is_empty() {
            return Ok(());
        }
        let Some((r, room)) = policy::lodash_sample(cx.rng(), &free).cloned() else { return Ok(()) };
        let room_name = name_of(w, r);
        let objs = objects_in(w, r, |_| true);
        let mut cnt = 0;
        let (mut x, mut y);
        loop {
            cnt += 1;
            x = (cx.random() * 40.0 + 5.0).floor() as i64;
            y = (cx.random() * 40.0 + 5.0).floor() as i64;
            let is_wall = wall_at(w, r, y * 50 + x);
            let has_exit = (-1..=1).any(|dx| (-1..=1).any(|dy| !wall_at(w, r, (y + dy) * 50 + x + dx)));
            let near = objs.iter().any(|o| (to_number(&field(&o.obj, "x")) - x as f64).abs() <= 2.0 && (to_number(&field(&o.obj, "y")) - y as f64).abs() <= 2.0);
            if !((!is_wall || !has_exit || near) && cnt < 1000) {
                break;
            }
        }
        if cnt >= 1000 {
            continue;
        }
        let deposit_type = field(&room, "depositType");
        if truthy(&deposit_type) {
            let doc = obj(vec![
                ("type", sv("deposit")),
                ("depositType", room.get(Atom::intern("depositType")).cloned().unwrap_or(Val::Null)),
                ("x", Val::Num(x as f64)),
                ("y", Val::Num(y as f64)),
                ("room", sv(&room_name)),
                ("harvested", Val::Num(0.0)),
                ("decayTime", Val::Num(C::DEPOSIT_DECAY_TIME as f64 + g)),
            ]);
            insert_objects(w, vec![doc]);
            activate(w, r);
        }
    }
    Ok(())
}

/// cronjobs.js:557 getSectorEdges: the 40 bus rooms around a sector centre, deduplicated, in push order.
fn sector_edges(center: &str) -> Option<Vec<String>> {
    let b = center.as_bytes();
    let ns = b.iter().position(|c| matches!(c, b'N' | b'S'))?;
    let (we, ns_c) = (&center[..1], &center[ns..ns + 1]);
    let (xs, ys) = (&center[1..ns], &center[ns + 1..]);
    if !matches!(we, "W" | "E") || !xs.ends_with('5') || !ys.ends_with('5') {
        return None;
    }
    let (xd, yd) = (&xs[..xs.len() - 1], &ys[..ys.len() - 1]);
    if !xd.bytes().chain(yd.bytes()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    // `10*xx` of the captured digit string ('' → 0)
    let num = |s: &str| if s.is_empty() { 0.0 } else { s.parse::<f64>().unwrap_or(f64::NAN) };
    let (xx, yy) = (num(xd), num(yd));
    let f = |v: f64| util::num::to_js_string(v);
    let mut names: Vec<String> = Vec::new();
    for i in 0..=10 {
        let i = i as f64;
        for n in [
            format!("{we}{}{ns_c}{}", f(10.0 * xx + i), f(10.0 * yy)),
            format!("{we}{}{ns_c}{}", f(10.0 * xx + 10.0), f(10.0 * yy + i)),
            format!("{we}{}{ns_c}{}", f(10.0 * xx), f(10.0 * yy + i)),
            format!("{we}{}{ns_c}{}", f(10.0 * xx + i), f(10.0 * yy + 10.0)),
        ] {
            if !names.contains(&n) {
                names.push(n);
            }
        }
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sectors_and_names() {
        let p = sector_prefixes("W12N34");
        assert_eq!(p, Some(("W1".into(), "N3".into())));
        assert!(in_sector("W15N39", &p, "W12N34") && !in_sector("W25N34", &p, "W12N34") && !in_sector("W1N3", &p, "W12N34"));
        assert_eq!(sector_prefixes("W5N5"), Some(("W".into(), "N".into())));
        assert_eq!(room_name_from_xy(-1.0, -1.0), "W0N0");
        assert_eq!(room_name_from_xy(3.0, 4.0), "E3S4");
        let e = sector_edges("W5N5").unwrap();
        assert_eq!(e.len(), 40);
        assert_eq!(e[0], "W0N0");
        assert!(e.contains(&"W10N10".to_string()) && e.contains(&"W0N10".to_string()));
        assert!(is_center(-5.0, 4.0) && !is_center(0.0, 4.0) && is_very_center(5.0, -6.0));
    }
}
