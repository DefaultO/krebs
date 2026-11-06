//! mirror: reference/driver/lib/runtime/make.js
//! parity: FAITHFUL (tests/fixtures/make: the official runner path, 345 cases + the Loki operator matrix)
//!
//! Not modelled, unobservable to players: inserted `users.notifications` docs carry no `_id` yet (CLK-NOTIFY
//! docs are excluded from parity), and `cpu: 'error'` travels as `CPU_ERROR` in `TickReport.cpu`.
//! Not modelled, a storage artifact (COMPAT-PROFILES "Loki operators"): a foreign-segment username that is
//! itself a Loki query object (`setActiveForeignSegment({$gt: ''})`) finds no user here instead of being
//! evaluated as a query.
//!
//! Host half of make.js (ARCHITECTURE.md §2.3, §4.3, §6.10) and the driver code around it that decides who
//! runs and saves what a run produced:
//! * P1 `users_to_run`: index.js getAllUsers (TICK-GET-USERS, Loki operators), make.js getUserData
//!   (RUN-SKIP, CPU-NEGATIVE-SKIP), data.js:103-109 (RUN-DEACTIVATE), data.js:151-165 (CPU-LIMIT).
//! * P2 `save_run`: make.js:192-273 (CPU-BUCKET-UPDATE, segment settings, visuals, cpu stats) and
//!   runner.js saveResult with index.js saveUserMemory / saveUserMemorySegments / saveUserIntents
//!   (INT-SAVE, INT-NOTIFY) / sendConsoleMessages / sendConsoleError, plus INT-MANUAL routing.
//!
//! P2 runs serially in user creation order; outputs are routed into per-room buckets whose keys are the rooms
//! the caller activates (ACT-SOURCES row 1).

use super::{RunJob, RunOutput, RunStatus};
use crate::atom;
use crate::doc::{Atom, Obj, Val};
use crate::intents::{RoomIntents, UserRoomIntents};
use crate::live::{self, V};
use crate::policy;
use crate::world::{GlobalIntentsDoc, Inbox, RoomIdx, UserIdx, UserState, World};
use std::collections::BTreeMap;
use std::sync::Arc;

/// make.js:69 (RUN-SKIP).
pub const PENALTY_ERROR: &str = "Your script is temporary blocked due to a hard reset inflicted to the runtime process.\nPlease try to change your code in order to prevent causing hard timeout resets.";
/// make.js:81 (CPU-NEGATIVE-SKIP).
pub const BUCKET_ERROR: &str = "Script execution has been terminated: CPU bucket is empty";
/// `cpu: 'error'` in a `user:<id>/cpu` push (make.js:263-266, 306): `TickReport.cpu` carries numbers only.
pub const CPU_ERROR: u32 = u32::MAX;

/// Eligibility `active != 0 && cpu > 0` with Loki operator semantics, RUN-SKIP penalty, CPU-NEGATIVE-SKIP,
/// RUN-DEACTIVATE. Skipped users get their `RunOutput` directly.
pub fn users_to_run(w: &World, time: u64) -> (Vec<RunJob>, Vec<RunOutput>) {
    let _ = time; // the runner phase reads `w.time`; kept for the FROZEN signature
    let max = w.config.cpu_max_per_tick as f64;
    let (mut jobs, mut skipped) = (Vec::new(), Vec::new());
    for (i, u) in w.users.iter().enumerate() {
        let (i, d) = (i as UserIdx, &*u.doc);
        // vanilla: driver/lib/index.js:145 users.find({$and: [{active: {$ne: 0}}, {cpu: {$gt: 0}}]})
        if !(loki_ne_zero(d.get(atom!("active"))) && loki_cmp(d.get(atom!("cpu")), 0.0, false, true)) {
            continue;
        }
        // vanilla: make.js:125 getUserData runs before data.js:103 (the deactivation check)
        match pre_run(d, max) {
            PreRun::Penalty => skipped.push(output(i, RunStatus::Skipped, Some(PENALTY_ERROR))),
            PreRun::Negative => skipped.push(output(i, RunStatus::Skipped, Some(BUCKET_ERROR))),
            PreRun::Run(_) if !has_objects(w, i) => skipped.push(output(i, RunStatus::Deactivated, None)),
            PreRun::Run(_) => {
                let (tick_limit, bucket) = cpu_limit(d, max);
                let dirty = live::to_number(&or(get(d, "lastUsedDirtyTime"), V::Num(0.0)));
                let expected = if dirty.is_nan() { 0.0 } else { dirty };
                jobs.push(RunJob { user: i, expected_ms: expected as f32, limit: num(d.get(atom!("cpu"))), tick_limit, bucket });
            }
        }
    }
    // vanilla: driver/lib/index.js:147 sorts by lastUsedDirtyTime desc. A measured time is never a game order
    // (ORD-POLICY): it only schedules the LPT runner queue. Stable, so ties keep creation order.
    jobs.sort_by(|a, b| b.expected_ms.total_cmp(&a.expected_ms));
    (jobs, skipped)
}

/// What P2 hands the tick loop: the rooms with intents (each to activate, then processed this tick) and the
/// `TickReport` rows, which are not world state.
#[derive(Default)]
#[allow(clippy::type_complexity)] // the TickReport row shapes
pub struct Saved {
    /// `rooms.intents` of this tick, by room; users in arrival order (API docs first).
    pub buckets: BTreeMap<RoomIdx, RoomIntents>,
    /// (user, log, results, error): `user:<id>/console`.
    pub console: Vec<(UserIdx, Vec<String>, Vec<String>, Option<String>)>,
    /// (user, used | CPU_ERROR, Memory length | CPU_ERROR when absent): `user:<id>/cpu`.
    pub cpu: Vec<(UserIdx, u32, u32)>,
    pub visuals: Vec<(UserIdx, Box<str>, Arc<str>)>,
    /// Users whose Memory was written this tick.
    pub memory_changed: Vec<UserIdx>,
}

/// P2: save every run of this tick in user creation order (§2.2). `world.manual_intents` (INT-MANUAL) are
/// consumed into the buckets first: the API calls arrived in P0, before any runner save.
pub fn save_run(w: &mut World, outs: Vec<RunOutput>) -> Saved {
    save_run_at(w, outs, policy::real_now_ms("CLK-NOTIFY") as f64)
}

/// `save_run` with the real-clock reading of CLK-NOTIFY passed in (tests pin it).
fn save_run_at(w: &mut World, mut outs: Vec<RunOutput>, now: f64) -> Saved {
    let mut s = Saved::default();
    for (room, user, id, intent) in std::mem::take(&mut w.manual_intents) {
        // vanilla: backend/lib/game/api/game.js:522 $merge {users: {[uid]: {objectsManual: {[id]: intent}}}}
        let manual = &mut user_entry(&mut s.buckets, room, user).manual;
        match manual.iter_mut().find(|(k, _)| *k == id) {
            Some((_, prev)) => *prev = merge(prev, &intent),
            None => manual.push((id, intent)),
        }
    }
    outs.sort_by_key(|o| o.user);
    for o in outs {
        save_one(w, o, &mut s, now);
    }
    s
}

fn save_one(w: &mut World, o: RunOutput, s: &mut Saved, now: f64) {
    let u = o.user;
    let max = w.config.cpu_max_per_tick as f64;
    let bucket_size = w.config.cpu_bucket_size as f64;
    let fix_seg0 = w.compat.fix("ID-FOREIGN-SEG0");
    let npc = matches!(w.users[u as usize].id.as_str(), "2" | "3");
    match o.status {
        RunStatus::Deactivated => {
            // vanilla: data.js:107 users.update({_id}, {$set: {active: 0}}); reject(false): nothing else is saved
            Arc::make_mut(&mut user_mut(w, u).doc).set(atom!("active"), Val::Num(0.0));
            return;
        }
        RunStatus::Skipped => {
            // vanilla: make.js:61-82; P1 left the doc untouched, so the same check decides which skip it was
            let doc = Arc::make_mut(&mut user_mut(w, u).doc);
            let set0 = |d: &mut Obj| {
                d.set(atom!("lastUsedCpu"), Val::Num(0.0));
                d.set(atom!("lastUsedDirtyTime"), Val::Num(0.0));
            };
            match pre_run(doc, max) {
                PreRun::Penalty => {
                    set0(doc);
                    // Loki $inc: doc[k] = (doc[k] || 0) + n
                    let p = js_add(&or(get(doc, "skipTicksPenalty"), V::Num(0.0)), &V::Num(-1.0));
                    put(doc, atom!("skipTicksPenalty"), &p);
                }
                PreRun::Negative => {
                    let (avail, cpu) = (get(doc, "cpuAvailable"), get(doc, "cpu"));
                    let floor = -live::to_number(&cpu) * 2.0;
                    let v = if live::to_number(&avail) < floor { V::Num(floor) } else { js_add(&avail, &cpu) };
                    set0(doc);
                    put(doc, atom!("cpuAvailable"), &v);
                }
                PreRun::Run(_) => {}
            }
        }
        RunStatus::Done | RunStatus::PartialError => bill(w, &o, s, max, bucket_size, fix_seg0),
        RunStatus::TotalError | RunStatus::Reset(_) => {}
    }
    if matches!(o.status, RunStatus::Reset(super::ResetReason::HardTimeout)) {
        s.cpu.push((u, CPU_ERROR, CPU_ERROR)); // make.js:306 {cpu: 'error'}
    }
    if !matches!(o.status, RunStatus::Skipped) {
        // vanilla: data.js:149 consumed console commands as soon as the run read them (not for skips)
        let user = user_mut(w, u);
        if !user.console_queue.is_empty() {
            user.console_queue = Arc::from(Vec::new());
        }
    }

    // vanilla: runner.js:18-46 saveResult
    let billed = matches!(o.status, RunStatus::Done | RunStatus::PartialError);
    let error = o.error.filter(|e| !e.is_empty());
    if npc {
        // vanilla: index.js:355-366, 377-385 NPC consoles go to stdout only
        if billed && !o.console_log.is_empty() {
            println!("{} console {:?}", if w.users[u as usize].id.as_str() == "3" { "Source Keeper" } else { "Invader" }, o.console_log);
        }
        if let Some(e) = &error {
            println!("{} error {e}", if w.users[u as usize].id.as_str() == "3" { "Source Keeper" } else { "Invader" });
        }
    } else if billed {
        if let Some(e) = &error {
            error_notification(w, u, e, now);
        }
        s.console.push((u, o.console_log, o.console_results, error));
    } else if let Some(e) = error {
        // a failed make() resolves to `{error}` alone (make.js:137, :283-293, :303): the run's console is lost
        error_notification(w, u, &e, now);
        s.console.push((u, Vec::new(), Vec::new(), Some(e)));
    }
    if !billed {
        return;
    }
    if let Some(m) = o.memory {
        // vanilla: index.js:155 saveUserMemory (RUN-MEMORY-LIMIT: rejected, runner log only)
        if m.utf16_len() > 2 * 1024 * 1024 {
            eprintln!("user {}: Script execution has been terminated: memory allocation limit reached", w.users[u as usize].id);
        } else {
            let user = user_mut(w, u);
            user.memory = m;
            user.memory_version += 1;
            s.memory_changed.push(u);
        }
    }
    if !o.segments.is_empty() {
        // vanilla: index.js:162-168 hmset
        let segs = Arc::make_mut(&mut user_mut(w, u).segments);
        for (id, data) in o.segments {
            segs.insert(id, data);
        }
    }
    if let Some(ism) = o.inter_shard {
        user_mut(w, u).inter_shard = Some(ism); // mmo only; the runtime never produces it under `private` (RUN-INTERSHARD)
    }
    if let Some(ui) = o.intents {
        // vanilla: index.js:180-243 saveUserIntents
        if !ui.notify.is_empty() {
            notify(w, u, &ui.notify, now);
        }
        if let Some(g) = ui.global {
            let arrival = w.global_intents.len() as u32;
            w.global_intents.push(GlobalIntentsDoc { user: u, from_api: false, arrival, intents: g });
        }
        for (room, objects) in ui.rooms {
            // $merge {users: {[uid]: {objects}}}: one run per user and tick, so `objects` is still empty
            user_entry(&mut s.buckets, room, u).objects = objects;
        }
    }
}

/// make.js:192-273: billing `$set`, segment settings, public segments, visuals, cpu stats.
fn bill(w: &mut World, o: &RunOutput, s: &mut Saved, max: f64, bucket_size: f64, fix_seg0: bool) {
    let u = o.user;
    let count = o.intents.as_ref().map_or(0, |i| i.count);
    let cpu_used = (o.used_clean_ms + 0.2 * count as f64).ceil();
    let seg = |k: &str| o.seg_settings.as_ref().and_then(Val::as_obj).and_then(|x| x.get(Atom::intern(k)).cloned());
    let doc = Arc::make_mut(&mut user_mut(w, u).doc);
    let run_cpu = match pre_run(doc, max) {
        PreRun::Run(c) => c,
        _ => V::Num(f64::INFINITY), // unreachable: a billed run passed the same check in P1
    };
    doc.set(atom!("lastUsedCpu"), Val::Num(o.used_time as f64));
    doc.set(atom!("lastUsedDirtyTime"), Val::Num(o.used_dirty_ms as f64));
    if let Some(a) = seg("activeSegments").filter(|v| live::truthy(&live::from_val(v))) {
        doc.set(atom!("activeSegments"), a);
    }
    if let Some(d) = seg("defaultPublicSegment") {
        doc.set(atom!("defaultPublicSegment"), d);
    }
    if live::to_number(&run_cpu) < f64::INFINITY {
        // CPU-BUCKET-UPDATE: user.cpuAvailable + user.cpu - cpuUsed, capped, no lower bound
        let sum = js_add(&get(doc, "cpuAvailable"), &get(doc, "cpu"));
        let mut avail = live::to_number(&sum) - cpu_used;
        if avail > bucket_size {
            avail = bucket_size;
        }
        put(doc, atom!("cpuAvailable"), &V::Num(avail));
    }
    if let Some(afs) = seg("activeForeignSegment") {
        active_foreign_segment(w, u, afs, fix_seg0);
    }
    if let Some(p) = seg("publicSegments") {
        if live::truthy(&live::from_val(&p)) {
            user_mut(w, u).public_segments = live::to_js_string(&live::from_val(&p));
        }
    }
    for (room, data) in &o.visuals {
        s.visuals.push((u, room.clone(), data.clone()));
    }
    let used = if o.error.as_deref().is_some_and(|e| e.contains("CPU limit reached")) { CPU_ERROR } else { o.used_time };
    let mem = o.memory.as_ref().unwrap_or(&w.users[u as usize].memory).utf16_len();
    s.cpu.push((u, used, mem.min(CPU_ERROR as usize - 1) as u32));
}

/// make.js:216-247.
fn active_foreign_segment(w: &mut World, u: UserIdx, afs: Val, fix_seg0: bool) {
    let set = |v: Option<&Val>| match v {
        None => false,
        Some(x) if fix_seg0 => !matches!(x, Val::Null), // ID-FOREIGN-SEG0 fixed: 0 is an id
        Some(x) => live::truthy(&live::from_val(x)),
    };
    let doc = Arc::make_mut(&mut user_mut(w, u).doc);
    let Val::Obj(mut afs) = afs else {
        if afs == Val::Null {
            doc.remove(atom!("activeForeignSegment")); // $unset
        }
        return;
    };
    let name = afs.get(atom!("username")).cloned();
    let cur = doc.get(atom!("activeForeignSegment")).cloned();
    if let Some(cur) = cur.filter(|c| live::truthy(&live::from_val(c))) {
        let same = loose_eq(&v(name.as_ref()), &v(cur.as_obj().and_then(|c| c.get(atom!("username")))));
        if same && set(afs.get(atom!("id"))) {
            // $merge {activeForeignSegment: {id}}
            let mut patch = Obj::new();
            patch.set(atom!("id"), afs.get(atom!("id")).cloned().unwrap());
            let mut m = Obj::new();
            m.set(atom!("activeForeignSegment"), Val::Obj(patch));
            *doc = merge(doc, &m);
            return;
        }
    }
    // db.users.findOne({username}), then $set in `finally` even when the lookup threw. The query crosses the
    // storage RPC as JSON, so an undefined username is `{}`: the first users doc. Otherwise Loki $eq (exact).
    let target = match name.as_ref() {
        None => (!w.users.is_empty()).then_some(0),
        Some(n) => n.as_str().and_then(|n| {
            let t = *w.usernames.get(n.to_lowercase().as_str())?;
            (w.users[t as usize].doc.get(atom!("username")).and_then(Val::as_str) == Some(n)).then_some(t)
        }),
    };
    if let Some(t) = target {
        let td = &w.users[t as usize];
        let dps = td.doc.get(atom!("defaultPublicSegment")).cloned();
        // vanilla sets user_id first, but runtime.js:186 `{username, id}` always holds an `id` slot (undefined
        // survives ExternalCopy), so `id` precedes `user_id` in the stored doc
        if !set(afs.get(atom!("id"))) && set(dps.as_ref()) {
            afs.set(atom!("id"), dps.unwrap());
        }
        afs.set(atom!("user_id"), Val::Str(td.id.as_str().into()));
    }
    Arc::make_mut(&mut user_mut(w, u).doc).set(atom!("activeForeignSegment"), Val::Obj(afs));
}

/// index.js:185-227 (INT-NOTIFY). Real clock: CLK-NOTIFY.
fn notify(w: &mut World, u: UserIdx, list: &[Val], now: f64) {
    let uid: Arc<str> = w.users[u as usize].id.as_str().into();
    for n in list.iter().take(20) {
        let n = n.as_obj();
        let field = |k: &str| v(n.and_then(|o| o.get(Atom::intern(k))));
        // `< 0 → 0`, `> 1440 → 1440` (NaN passes through, as `clamp` keeps it), then `*= 60 * 1000`
        let gi = (live::to_number(&field("groupInterval")).clamp(0.0, 1440.0) * 60000.0).floor();
        let date = if gi != 0.0 && !gi.is_nan() { (now / gi).ceil() * gi } else { now };
        let message = utf16_prefix(&live::to_js_string(&field("message")), 500);
        // users.notifications.update({$and: [{user}, {message}, {date}, {type: 'msg'}]}, {$inc: {count: 1}}, {upsert})
        let key = |d: &Obj| {
            d.get(atom!("user")).and_then(Val::as_str) == Some(&*uid)
                && d.get(atom!("message")).and_then(Val::as_str) == Some(&*message)
                && d.get(atom!("date")) == Some(&num_val(date))
                && d.get(atom!("type")).and_then(Val::as_str) == Some("msg")
        };
        let mut item = Obj::new();
        item.set(atom!("user"), Val::Str(uid.clone()));
        item.set(atom!("message"), Val::Str(message.clone()));
        item.set(atom!("date"), num_val(date));
        item.set(atom!("type"), Val::Str("msg".into()));
        upsert_notification(user_mut(w, u), key, item, None);
    }
}

/// index.js:371-410 sendConsoleError's `users.notifications` upsert (real clock, CLK-NOTIFY).
fn error_notification(w: &mut World, u: UserIdx, error: &str, now: f64) {
    let doc = &w.users[u as usize].doc;
    let prefs = doc.get(atom!("notifyPrefs")).and_then(Val::as_obj).and_then(|p| p.get(atom!("errorsInterval")));
    let interval = if truthy(prefs) { num(prefs) * 60.0 * 1000.0 } else { 30.0 * 60.0 * 1000.0 };
    let date = (now / interval).ceil() * interval;
    let uid: Arc<str> = w.users[u as usize].id.as_str().into();
    let key = |d: &Obj| {
        d.get(atom!("user")).and_then(Val::as_str) == Some(&*uid)
            && d.get(atom!("message")).and_then(Val::as_str) == Some(error)
            && d.get(atom!("type")).and_then(Val::as_str) == Some("error")
            && loki_cmp(d.get(atom!("date")), date, true, false)
    };
    let mut item = Obj::new();
    item.set(atom!("user"), Val::Str(uid.clone()));
    item.set(atom!("message"), Val::Str(error.into()));
    item.set(atom!("type"), Val::Str("error".into()));
    item.set(atom!("date"), num_val(date));
    upsert_notification(user_mut(w, u), key, item.clone(), Some(item));
}

/// storage db.js dbUpdate with `upsert`: every match gets `$set` then `$inc {count: 1}`; no match inserts the
/// query fields, `$set`, `count: 1`.
// ponytail: no `_id` on inserted notifications: an IdGen draw here would make world ids depend on the real
// clock (CLK-NOTIFY buckets); whoever first needs a notification id (email cron, API) assigns it.
fn upsert_notification(user: &mut UserState, key: impl Fn(&Obj) -> bool, item: Obj, set: Option<Obj>) {
    let inbox = &mut user.inbox;
    if Arc::get_mut(inbox).is_none() {
        *inbox = Arc::new(Inbox { notifications: inbox.notifications.clone(), messages: inbox.messages.clone() });
    }
    let list = &mut Arc::get_mut(inbox).unwrap().notifications;
    let mut found = false;
    for d in list.iter_mut().filter(|d| key(d)) {
        let d = Arc::make_mut(d);
        for (k, x) in set.iter().flat_map(|s| s.iter()) {
            d.set(k, x.clone());
        }
        let c = js_add(&or(get(d, "count"), V::Num(0.0)), &V::Num(1.0));
        put(d, atom!("count"), &c);
        found = true;
    }
    if !found {
        let mut item = item;
        item.set(atom!("count"), Val::Num(1.0));
        list.push(Arc::new(item));
    }
}

// ---- pre-run checks ----

enum PreRun {
    Penalty,
    Negative,
    /// make.js `userData.cpu` (Infinity when `user.cpu` is falsy).
    Run(V),
}

/// make.js:58-91 getUserData.
fn pre_run(d: &Obj, max: f64) -> PreRun {
    let ucpu = get(d, "cpu");
    if !live::truthy(&ucpu) {
        return PreRun::Run(V::Num(f64::INFINITY));
    }
    let avail = get(d, "cpuAvailable");
    let cpu = or(avail.clone(), V::Num(0.0));
    if live::to_number(&get(d, "skipTicksPenalty")) > 0.0 {
        return PreRun::Penalty;
    }
    if live::to_number(&avail) < 0.0 {
        return PreRun::Negative;
    }
    let cpu = js_add(&cpu, &ucpu);
    PreRun::Run(if live::to_number(&cpu) > max { V::Num(max) } else { cpu })
}

/// data.js:151-165 (CPU-LIMIT): (tickLimit, bucket) for `Game.cpu`.
fn cpu_limit(d: &Obj, max: f64) -> (f64, f64) {
    let ucpu = get(d, "cpu");
    if !live::truthy(&ucpu) {
        return (f64::INFINITY, f64::INFINITY);
    }
    let mut bucket = or(get(d, "cpuAvailable"), V::Num(0.0));
    if live::to_number(&bucket) < 0.0 {
        bucket = V::Num(0.0);
    }
    let cpu = live::to_number(&js_add(&bucket, &ucpu));
    (if cpu > max { max } else { cpu }, live::to_number(&bucket))
}

/// data.js:103-109: `rooms.objects.find({user})` is empty.
fn has_objects(w: &World, u: UserIdx) -> bool {
    w.index.presence.get(u as usize).is_some_and(|p| p.values().any(|x| x.objects > 0))
}

fn output(user: UserIdx, status: RunStatus, error: Option<&str>) -> RunOutput {
    RunOutput {
        user,
        status,
        error: error.map(str::to_string),
        intents: None,
        memory: None,
        segments: Vec::new(),
        seg_settings: None,
        inter_shard: None,
        console_log: Vec::new(),
        console_results: Vec::new(),
        visuals: Vec::new(),
        used_time: 0,
        used_clean_ms: 0.0,
        used_dirty_ms: 0,
    }
}

// ---- Loki operators (lokijs 1.5.9, src/lokijs.js) ----

/// `$ne: 0`: strict `a !== 0` (lokijs.js:473-481).
fn loki_ne_zero(a: Option<&Val>) -> bool {
    !matches!(a, Some(Val::Num(n)) if *n == 0.0)
}

/// `gtHelper(a, b, equal)` (`$gt`, `$gte`) or `ltHelper` (`$lt`, `$lte`) with a number on the query side
/// (lokijs.js:222-339). Also used by the tick loop's TICK-ACCESSIBLE queries.
pub fn loki_cmp(a: Option<&Val>, b: f64, equal: bool, gt: bool) -> bool {
    let rank = |x: &V| match x {
        V::Undef | V::Null => 1,
        V::Bool(false) => 3,
        V::Bool(true) => 4,
        V::Str(s) if s.is_empty() => 5,
        V::Num(n) if n.is_nan() => 0,
        _ => 9,
    };
    let (a, bv) = (v(a), V::Num(b));
    let a_nan = matches!(a, V::Num(n) if n.is_nan());
    if !live::truthy(&a) || !live::truthy(&bv) || matches!(a, V::Bool(true)) || a_nan || b.is_nan() {
        let (t1, t2) = (rank(&a), rank(&bv));
        if t1 != 9 || t2 != 9 {
            return if t1 == t2 { equal } else if gt { t1 > t2 } else { t1 < t2 };
        }
    }
    let cv1 = live::to_number(&a);
    match (cv1.is_nan(), b.is_nan()) {
        (false, false) => {
            if cv1 == b {
                equal
            } else {
                (cv1 > b) == gt
            }
        }
        (false, true) => !gt,
        (true, false) => gt,
        (true, true) => false, // b is always a number here
    }
}

// ---- JS value helpers ----

fn v(x: Option<&Val>) -> V {
    x.map_or(V::Undef, live::from_val)
}

fn get(d: &Obj, k: &str) -> V {
    v(d.get(Atom::intern(k)))
}

fn num(x: Option<&Val>) -> f64 {
    live::to_number(&v(x))
}

fn truthy(x: Option<&Val>) -> bool {
    live::truthy(&v(x))
}

/// `a || b`.
fn or(a: V, b: V) -> V {
    if live::truthy(&a) {
        a
    } else {
        b
    }
}

/// JS binary `+` (ToPrimitive, then string concatenation if either side is a string).
fn js_add(a: &V, b: &V) -> V {
    let prim = |x: &V| match x {
        V::Obj(_) | V::Arr(_) => V::Str(live::to_js_string(x)),
        _ => x.clone(),
    };
    let (a, b) = (prim(a), prim(b));
    if matches!(a, V::Str(_)) || matches!(b, V::Str(_)) {
        V::Str(format!("{}{}", live::to_js_string(&a), live::to_js_string(&b)).into())
    } else {
        V::Num(live::to_number(&a) + live::to_number(&b))
    }
}

/// `a == b` for the primitives a JSON transport carries (objects compare by identity, so never equal here).
fn loose_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Undef | V::Null, V::Undef | V::Null) => true,
        (V::Undef | V::Null, _) | (_, V::Undef | V::Null) => false,
        (V::Obj(_) | V::Arr(_), _) | (_, V::Obj(_) | V::Arr(_)) => false,
        (V::Str(x), V::Str(y)) => x == y,
        _ => live::to_number(a) == live::to_number(b),
    }
}

/// `$set {k: v}` through the JSON transport (NaN → null, undefined dropped).
fn put(d: &mut Obj, k: Atom, x: &V) {
    match live::to_val(x) {
        Some(x) => d.set(k, x),
        None => {
            d.remove(k);
        }
    }
}

fn num_val(x: f64) -> Val {
    if x.is_finite() {
        Val::Num(x)
    } else {
        Val::Null
    }
}

/// lodash 3 `_.merge(a, b)` of two documents.
fn merge(a: &Obj, b: &Obj) -> Obj {
    let dst = live::from_val(&Val::Obj(a.clone()));
    live::merge(&dst, &live::from_val(&Val::Obj(b.clone())));
    match live::to_val(&dst) {
        Some(Val::Obj(o)) => o,
        _ => a.clone(),
    }
}

/// `s.substring(0, n)` in UTF-16 units; a cut surrogate pair keeps its lone half as U+FFFD (J6).
fn utf16_prefix(s: &str, n: usize) -> Arc<str> {
    let mut out = String::new();
    let mut len = 0;
    for c in s.chars() {
        let l = c.len_utf16();
        if len + l > n {
            if len < n {
                out.push('\u{FFFD}');
            }
            break;
        }
        out.push(c);
        len += l;
    }
    out.into()
}

fn user_entry(b: &mut BTreeMap<RoomIdx, RoomIntents>, room: RoomIdx, user: UserIdx) -> &mut UserRoomIntents {
    let users = &mut b.entry(room).or_insert_with(|| RoomIntents { users: Vec::new() }).users;
    match users.iter().position(|e| e.user == user) {
        Some(i) => &mut users[i],
        None => {
            users.push(UserRoomIntents { user, objects: Vec::new(), manual: Vec::new() });
            users.last_mut().unwrap()
        }
    }
}

/// Copy-on-write access to one user (shared with the published view).
fn user_mut(w: &mut World, u: UserIdx) -> &mut UserState {
    let slot = &mut w.users[u as usize];
    if Arc::get_mut(slot).is_none() {
        let o = &**slot;
        *slot = Arc::new(UserState {
            id: o.id,
            seq: o.seq,
            doc: o.doc.clone(),
            auth: o.auth.clone(),
            code: o.code.clone(),
            memory: o.memory.clone(),
            memory_version: o.memory_version,
            segments: o.segments.clone(),
            public_segments: o.public_segments.clone(),
            inter_shard: o.inter_shard.clone(),
            console_queue: o.console_queue.clone(),
            power_creeps: o.power_creeps.clone(),
            inbox: o.inbox.clone(),
        });
    }
    Arc::get_mut(slot).unwrap()
}

/// Replays tests/fixtures/make/cases.ndjson: the official runner path (runner.js saveResult, make.js, data.js,
/// index.js over the real storage db.js and lokijs) for one user per case (`node22 .../make/gen.js`).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Id;
    use crate::intents::UserIntents;
    use crate::policy::{Compat, Profile, VClock};
    use crate::room_name::RoomXY;
    use crate::world::*;
    use std::collections::HashMap;
    use util::jsstr::JsStr;
    use util::json::Json;

    const CASES: &str = include_str!("../../tests/fixtures/make/cases.ndjson");

    /// Undoes gen.js's `{"$repeat": [char, n]}` compaction.
    fn expand(j: &Json) -> Json {
        match j {
            Json::Obj(m) if m.len() == 1 && m[0].0 == "$repeat" => {
                let (c, n) = (m[0].1.at(0).and_then(Json::as_str).unwrap(), m[0].1.at(1).and_then(Json::as_f64).unwrap());
                Json::Str(c.repeat(n as usize))
            }
            Json::Obj(m) => Json::Obj(m.iter().map(|(k, v)| (k.clone(), expand(v))).collect()),
            Json::Arr(a) => Json::Arr(a.iter().map(expand).collect()),
            _ => j.clone(),
        }
    }

    fn obj(j: &Json) -> Obj {
        match Val::from_json(j) {
            Val::Obj(o) => o,
            v => panic!("not an object: {v:?}"),
        }
    }

    fn user(doc: &Json, i: usize, memory: &str) -> UserState {
        UserState {
            id: Id::new(doc.get("_id").and_then(Json::as_str).unwrap()).unwrap(),
            seq: i as u64,
            doc: Arc::new(obj(doc)),
            auth: Arc::new(Obj::new()),
            code: Arc::new(CodeState { branches: Vec::new(), active_world: None, active_sim: None, version: 0 }),
            memory: JsStr::from_str(memory),
            memory_version: 0,
            segments: Arc::default(),
            public_segments: "".into(),
            inter_shard: None,
            console_queue: Arc::from(Vec::new()),
            power_creeps: Arc::from(Vec::new()),
            inbox: Arc::new(Inbox { notifications: Vec::new(), messages: Vec::new() }),
        }
    }

    fn world(users: &[Json], rooms: &[String], cfg: Option<&Json>) -> World {
        let n = |k: &str, d: f64| cfg.and_then(|c| c.get(k)).and_then(Json::as_f64).unwrap_or(d) as u32;
        let meta = RoomsMeta {
            x0: 0,
            y0: 0,
            w: 0,
            h: 0,
            grid: Vec::new(),
            rooms: rooms.iter().map(|r| RoomMeta { name: r.as_str().into(), xy: RoomXY::parse_engine(r).unwrap(), removed: false }).collect(),
            world_size: 0,
        };
        let users = users.iter().enumerate().map(|(i, d)| Arc::new(user(d, i, ""))).collect::<Vec<_>>();
        let clock = VClock { base_ms: 0, base_tick: 0, nominal_tick_ms: 1000 };
        let mut w = World::new(1, clock, Compat::new(Profile::Private), meta, TerrainStore::new(Vec::new()), Vec::new(), users);
        w.config = Arc::new(EngineConfig {
            main_loop_min_duration_ms: 1000,
            main_loop_reset_interval_ms: 5000,
            cpu_max_per_tick: n("cpuMaxPerTick", 500.0),
            cpu_bucket_size: n("cpuBucketSize", 10000.0),
            history_chunk_size: 20,
            socket_update_throttle_ms: 200,
            history: false,
            disabled_cronjobs: Vec::new(),
        });
        w.access = Arc::new(AccessSnapshot { version: 0, accessible: Vec::new(), status: HashMap::new() });
        // usernameLower is not in the fixture docs: index by the lowercased username as before.
        w.usernames = Arc::new(w.users.iter().enumerate().filter_map(|(i, u)| Some((u.doc.get(crate::atom!("username"))?.as_str()?.to_lowercase().into(), i as UserIdx))).collect());
        w.index.presence = (0..w.users.len()).map(|_| Arc::new(BTreeMap::from([(0, Presence { objects: 1, ..Default::default() })]))).collect();
        w.next_seq = 0;
        w
    }

    fn lines() -> impl Iterator<Item = Json> {
        CASES.lines().map(|l| expand(&Json::parse(l).unwrap()))
    }

    /// TICK-GET-USERS over lokijs 1.5.9 `$ne`/`$gt` with every value kind a JSON doc can hold; the LPT job order
    /// is getAllUsers' sort.
    #[test]
    fn tick_get_users_loki_operators() {
        let case = lines().find(|j| j.get("kind").and_then(Json::as_str) == Some("getUsers")).unwrap();
        let users = case.get("users").and_then(Json::as_array).unwrap();
        let w = world(users, &[], None);
        let (jobs, skipped) = users_to_run(&w, 1);
        let id = |u: UserIdx| w.users[u as usize].id.as_str().to_string();
        let mut picked: Vec<String> = jobs.iter().map(|j| id(j.user)).chain(skipped.iter().map(|o| id(o.user))).collect();
        let order: Vec<String> = case.get("order").and_then(Json::as_array).unwrap().iter().map(|j| j.as_str().unwrap().to_string()).collect();
        let job_ids: Vec<String> = jobs.iter().map(|j| id(j.user)).collect();
        let expected_jobs: Vec<String> = order.iter().filter(|o| job_ids.contains(o)).cloned().collect();
        assert_eq!(job_ids, expected_jobs, "LPT order = getAllUsers sort");
        picked.sort();
        let mut want = order.clone();
        want.sort();
        assert_eq!(picked, want);
        assert!(order.len() > 40 && order.len() < users.len(), "the matrix selects some, not all");
    }

    /// The vanilla `UserIntents` a run produced: storeIntents' object grouped by room name.
    fn user_intents(stored: &Json, count: u32, rooms: &[String]) -> UserIntents {
        let mut ui = UserIntents { rooms: Vec::new(), global: None, notify: Vec::new(), count };
        for (k, v) in stored.as_object().into_iter().flatten() {
            match k.as_str() {
                "notify" => ui.notify = v.as_array().unwrap().iter().map(Val::from_json).collect(),
                "global" => ui.global = Some(obj(v)),
                room => {
                    let r = rooms.iter().position(|n| n == room).unwrap() as RoomIdx;
                    ui.rooms.push((r, v.as_object().unwrap().iter().map(|(id, o)| (id.as_str().into(), obj(o))).collect()));
                }
            }
        }
        ui
    }

    fn output(u: UserIdx, run: &Json, intents: Option<UserIntents>) -> RunOutput {
        let f = |k: &str| run.get(k).and_then(Json::as_f64).unwrap_or(0.0);
        let mut seg = Obj::new();
        for k in ["activeSegments", "defaultPublicSegment", "activeForeignSegment", "publicSegments"] {
            if let Some(v) = run.get(k) {
                seg.set(Atom::intern(k), Val::from_json(v));
            }
        }
        let strs = |j: Option<&Json>, field: Option<&str>| -> Vec<String> {
            j.and_then(Json::as_array).into_iter().flatten().map(|x| field.map_or(x, |f| x.get(f).unwrap()).as_str().unwrap().to_string()).collect()
        };
        let console = run.get("console");
        RunOutput {
            user: u,
            status: if run.get("type").and_then(Json::as_str) == Some("error") { RunStatus::PartialError } else { RunStatus::Done },
            error: run.get("error").and_then(Json::as_str).map(str::to_string),
            intents,
            memory: run.get("memory").and_then(|m| m.get("data")).and_then(Json::as_str).map(JsStr::from_str),
            segments: run.get("memorySegments").and_then(Json::as_object).into_iter().flatten().map(|(k, v)| (k.parse().unwrap(), JsStr::from_str(v.as_str().unwrap()))).collect(),
            seg_settings: Some(Val::Obj(seg)),
            inter_shard: None,
            console_log: strs(console.and_then(|c| c.get("log")), Some("message")),
            console_results: strs(console.and_then(|c| c.get("results")), None),
            visuals: run.get("visual").and_then(Json::as_object).into_iter().flatten().map(|(k, v)| (k.as_str().into(), v.as_str().unwrap().into())).collect(),
            used_time: f("usedTime") as u32,
            used_clean_ms: f("usedCleanTime"),
            used_dirty_ms: f("usedDirtyTime") as u32,
        }
    }

    fn strip_id(o: &Obj) -> Json {
        let mut o = o.clone();
        o.remove(Atom::intern("_id"));
        Val::Obj(o).to_json()
    }

    /// RUN-SKIP, CPU-NEGATIVE-SKIP, RUN-DEACTIVATE, CPU-LIMIT, CPU-BUCKET-UPDATE, segment settings, visuals,
    /// cpu stats, RUN-MEMORY-LIMIT, INT-SAVE, INT-NOTIFY and console errors, against the official code.
    #[test]
    fn runner_path_matches_official() {
        let mut n = 0;
        for case in lines().filter(|j| j.get("kind").and_then(Json::as_str) == Some("run")) {
            n += 1;
            let name = case.get("name").and_then(Json::as_str).unwrap().to_string();
            let s = |k: &str| case.get(k).unwrap();
            let users = s("users").as_array().unwrap();
            let expect = s("expect");
            let uid = s("uid").as_str().unwrap();
            let mut rooms: Vec<String> = Vec::new();
            let manual = s("manual").as_array().unwrap();
            let names = s("intents").as_object().into_iter().flatten().map(|(k, _)| k.as_str()).chain(manual.iter().map(|m| m.at(0).and_then(Json::as_str).unwrap()));
            for k in names {
                if k != "notify" && k != "global" && !rooms.iter().any(|r| r == k) {
                    rooms.push(k.to_string());
                }
            }
            let mut w = world(users, &rooms, case.get("config"));
            let uidx = |id: &str| users.iter().position(|d| d.get("_id").and_then(Json::as_str) == Some(id)).unwrap() as UserIdx;
            for m in manual {
                let st = |i: usize| m.at(i).and_then(Json::as_str).unwrap();
                let room = rooms.iter().position(|r| r == st(0)).unwrap() as RoomIdx;
                w.manual_intents.push((room, uidx(st(1)), st(2).into(), obj(m.at(3).unwrap())));
            }
            let u = users.iter().position(|d| d.get("_id").and_then(Json::as_str) == Some(uid)).unwrap() as UserIdx;
            let objects = s("objects").as_f64().unwrap() as u32;
            w.index.presence[u as usize] = Arc::new(if objects == 0 { BTreeMap::new() } else { BTreeMap::from([(0, Presence { objects, ..Default::default() })]) });
            if let Some(m) = s("memory").as_str() {
                user_mut(&mut w, u).memory = JsStr::from_str(m);
            }
            for nd in s("notifications").as_array().unwrap() {
                let inbox = Arc::get_mut(&mut user_mut(&mut w, u).inbox).unwrap();
                inbox.notifications.push(Arc::new(obj(nd)));
            }
            let commands: Vec<Arc<Obj>> = (0..2).map(|_| Arc::new(Obj::new())).collect();
            user_mut(&mut w, u).console_queue = commands.into();
            let memory_before = w.users[u as usize].memory.clone();

            let time = s("time").as_f64().unwrap() as u64;
            let (jobs, skipped) = users_to_run(&w, time);
            let outcome = expect.get("outcome").and_then(Json::as_str).unwrap();
            let outs = match skipped.into_iter().find(|o| o.user == u) {
                Some(o) => {
                    match outcome {
                        "deactivated" => assert!(matches!(o.status, RunStatus::Deactivated), "{name}"),
                        _ => assert_eq!(o.error.as_deref(), expect.get("error").and_then(Json::as_str), "{name}"),
                    }
                    vec![o]
                }
                None => {
                    let job = jobs.iter().find(|j| j.user == u).unwrap_or_else(|| panic!("{name}: user did not run"));
                    // data.js can produce strings ("0" + "100"): RunJob carries their ToNumber
                    let lim = |k: &str| match expect.get(k).unwrap() {
                        Json::Str(s) => util::num::string_to_number(s),
                        j => j.as_f64().unwrap_or_else(|| panic!("{name}: {k} = {j}")),
                    };
                    assert_eq!((job.tick_limit, job.bucket), (lim("cpu"), lim("cpuBucket")), "{name}: CPU-LIMIT");
                    match case.get("run").filter(|r| !r.is_null()) {
                        Some(run) => {
                            let count = s("count").as_f64().unwrap() as u32;
                            let ui = case.get("intents").filter(|i| !i.is_null()).map(|i| user_intents(i, count, &rooms)).or(Some(UserIntents { rooms: Vec::new(), global: None, notify: Vec::new(), count }));
                            vec![output(u, run, ui)]
                        }
                        None => {
                            let mut o = output(u, &Json::Obj(Vec::new()), None);
                            o.status = RunStatus::TotalError;
                            o.error = expect.get("error").and_then(Json::as_str).map(str::to_string);
                            // what the isolate logged before the failure: vanilla never publishes it
                            o.console_log = vec!["lost".into()];
                            o.console_results = vec!["lost".into()];
                            vec![o]
                        }
                    }
                }
            };
            let saved = save_run_at(&mut w, outs, s("now").as_f64().unwrap());

            // users collection
            let docs: Vec<Json> = w.users.iter().map(|x| Val::Obj((*x.doc).clone()).to_json()).collect();
            assert_eq!(&Json::Arr(docs), expect.get("users").unwrap(), "{name}: users");
            // env
            let env = expect.get("env").unwrap();
            let user = &w.users[u as usize];
            let mem = env.get(&format!("memory:{uid}")).and_then(Json::as_str).map(JsStr::from_str).unwrap_or(memory_before);
            assert!(user.memory == mem, "{name}: memory");
            let segs: Vec<(String, String)> = env.get(&format!("memorySegments:{uid}")).and_then(Json::as_object).into_iter().flatten().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
            let ours: Vec<(String, String)> = user.segments.iter().map(|(k, v)| (k.to_string(), v.to_string_lossy())).collect();
            assert_eq!(ours, segs, "{name}: segments");
            assert_eq!(&*user.public_segments, env.get(&format!("publicMemorySegments:{uid}")).and_then(Json::as_str).unwrap_or(""), "{name}: public segments");
            let visuals: Vec<(String, String)> = env.as_object().unwrap().iter().filter_map(|(k, v)| Some((k.strip_prefix(&format!("roomVisual:{uid},"))?.strip_suffix(&format!(",{time}"))?.to_string(), v.as_str()?.to_string()))).collect();
            let ours: Vec<(String, String)> = saved.visuals.iter().map(|(_, r, d)| (r.to_string(), d.to_string())).collect();
            assert_eq!(ours, visuals, "{name}: visuals");
            // a set: the API's own activations (INT-MANUAL) plus the runner's
            let mut active: Vec<&str> = env.get("activeRooms").and_then(Json::as_array).into_iter().flatten().map(|r| r.as_str().unwrap()).collect();
            let mut ours: Vec<&str> = saved.buckets.keys().map(|&r| rooms[r as usize].as_str()).collect();
            active.sort_unstable();
            ours.sort_unstable();
            assert_eq!(ours, active, "{name}: activeRooms");
            // rooms.intents, users.intents, users.notifications
            let expected_rooms = expect.get("roomsIntents").and_then(Json::as_array).unwrap();
            assert_eq!(expected_rooms.len(), saved.buckets.len(), "{name}: rooms.intents docs");
            for d in expected_rooms {
                let room = d.get("room").and_then(Json::as_str).unwrap();
                let b = &saved.buckets[&(rooms.iter().position(|r| r == room).unwrap() as RoomIdx)];
                let list = |l: &[(Box<str>, Obj)]| Json::Obj(l.iter().map(|(id, o)| (id.to_string(), Val::Obj(o.clone()).to_json())).collect());
                let ours = Json::Obj(
                    b.users
                        .iter()
                        .map(|e| {
                            let mut v = Vec::new();
                            if !e.manual.is_empty() {
                                v.push(("objectsManual".to_string(), list(&e.manual)));
                            }
                            if !e.objects.is_empty() {
                                v.push(("objects".to_string(), list(&e.objects)));
                            }
                            (w.users[e.user as usize].id.as_str().to_string(), Json::Obj(v))
                        })
                        .collect(),
                );
                assert_eq!(Some(&ours), d.get("users"), "{name}: rooms.intents {room}");
            }
            let left = expect.get("consoleLeft").and_then(Json::as_f64).unwrap() as usize;
            assert_eq!(w.users[u as usize].console_queue.len(), left, "{name}: console commands");
            let globals: Vec<Json> = w.global_intents.iter().map(|g| Json::Obj(vec![("user".into(), Json::Str(w.users[g.user as usize].id.as_str().into())), ("intents".into(), Val::Obj(g.intents.clone()).to_json())])).collect();
            assert_eq!(&Json::Arr(globals), expect.get("usersIntents").unwrap(), "{name}: users.intents");
            let notes: Vec<Json> = user.inbox.notifications.iter().map(|o| strip_id(o)).collect();
            assert_eq!(&Json::Arr(notes), expect.get("notifications").unwrap(), "{name}: notifications");
            // pubsub
            let mut pubs = Vec::new();
            for &(_, used, mem) in &saved.cpu {
                let cpu = if used == CPU_ERROR { "\"error\"".to_string() } else { used.to_string() };
                pubs.push(Json::Arr(vec![Json::Str(format!("user:{uid}/cpu")), Json::Str(format!("{{\"cpu\":{cpu},\"memory\":{mem}}}"))]));
            }
            for (_, log, results, error) in &saved.console {
                let ch = Json::Str(format!("user:{uid}/console"));
                if !(log.is_empty() && results.is_empty() && error.is_some() && !matches!(outcome, "run")) {
                    let s = |v: &[String]| Json::Arr(v.iter().map(|x| Json::Str(x.clone())).collect());
                    let m = Json::Obj(vec![("messages".into(), Json::Obj(vec![("log".into(), s(log)), ("results".into(), s(results))])), ("userId".into(), Json::Str(uid.into()))]);
                    pubs.push(Json::Arr(vec![ch.clone(), Json::Str(m.stringify())]));
                }
                if let Some(e) = error {
                    pubs.push(Json::Arr(vec![ch, Json::Str(Json::Obj(vec![("userId".into(), Json::Str(uid.into())), ("error".into(), Json::Str(e.clone()))]).stringify())]));
                }
            }
            assert_eq!(&Json::Arr(pubs), expect.get("pubsub").unwrap(), "{name}: pubsub");
        }
        assert!(n > 250, "{n} cases");
    }
}

