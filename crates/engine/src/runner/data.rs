//! mirror: reference/driver/lib/runtime/data.js
//! parity: DIVERGENT(ORDER-MISC, ENV-BINARY-BUFFER)
//!
//! DB half of data.js `get(userId)`: the per-user meta JSON (ARCHITECTURE.md §6.6), the room blobs, event logs,
//! visible rooms, Memory, segments and the code bundle. `runtime/js/host/prelude.js` assembles runtimeData from
//! them (foreign docs of the vision rooms in `rooms` order, then own docs of every blob merged by `seq`; Game.rooms
//! from the vision entries in RoomIdx order). Elsewhere: market orders/history, accessibleRooms and
//! roomStatusData are natives (§6.7); the users.console removal (data.js:149) and RUN-DEACTIVATE (data.js:106-108)
//! belong to make.rs.
//!
//! DIVERGENT(ORDER-MISC): `rooms.flags.find({user})` (data.js:140) lists the user's per-room flag docs in creation
//! order; `RoomSnapshot.flags` carries no seq, so they come in RoomIdx order, then per-room order (interface
//! request: flag docs as `DocRef`). Fixtures: tests/fixtures/world/data.ndjson (official data.js over Loki).
//!
//! DIVERGENT(ENV-BINARY-BUFFER): a binary module whose value makes the runtime's `Buffer.from` throw at `require`
//! (a number, null, a boolean, a plain object) or an array-like object with a numeric `length` arrive as an empty
//! buffer, since `Module` has no throwing variant (interface request). Strings, arrays and JSON-ified Buffers are exact.
//!
//! Meta encoding: `cpu`/`cpuBucket` are data.js's JS values (a string after `cpuAvailable + cpu` concatenates);
//! Infinity (a user with `cpu` 0) is written `1e999`, which JSON.parse reads as Infinity. `codeTimestamp`
//! (data.js:204, `module.timestamp`) is an additive field. `segments.foreign` is
//! `{username, id}` whenever vanilla sets `foreignMemorySegment`; `RunInput.foreign_segment` is `None` when that
//! segment holds no data (vanilla: `data: undefined`).

use super::{CodeBundle, Module, RunInput, RunJob};
use crate::atom;
use crate::doc::{array_index, Atom, DocRef, Id, Obj, Val};
use crate::live;
use crate::world::{quote_ascii, write_obj_ascii, CodeState, RoomIdx, UserIdx, World};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use util::jsstr::JsStr;

pub fn build_input(w: &World, job: &RunJob) -> RunInput {
    let user = &w.users[job.user as usize];
    let me = user.id.as_str();
    let doc = &*user.doc;
    let no_presence = BTreeMap::new();
    let presence = w.index.presence.get(job.user as usize).map_or(&no_presence, |p| &**p);
    let name = |r: RoomIdx| &*w.meta.rooms[r as usize].name;

    // data.js:113-127: roomIds = first appearance over the user's objects in creation order, flags and sites
    // skipped, an observer adding `observeRoom` right after its own room; `Object.keys` puts array indices first.
    let mut firsts: Vec<(u64, u8, &str)> = Vec::new();
    for (&r, p) in presence {
        if p.vision > 0 {
            firsts.push((p.first_vision_seq, 0, name(r)));
        }
        if let Some((seq, target)) = &p.observe {
            firsts.push((*seq, 1, target));
        }
    }
    firsts.sort_unstable();
    let mut listed = HashSet::new();
    // `roomIdsHash['__proto__'] = true` (an observeRoom sent through add-object-intent) sets the prototype instead
    // of a key.
    let mut room_ids: Vec<&str> = firsts.into_iter().filter_map(|(_, _, n)| (n != "__proto__" && listed.insert(n)).then_some(n)).collect();
    room_ids.sort_by_key(|n| array_index(n).map_or((1, 0), |i| (0, i)));

    // [name, vision, RoomIdx]: the vision rooms (`rooms.find({_id: {$in: roomIds}})` keeps existing ones only),
    // then the rooms holding only the user's sites/legacy flags, whose own docs still belong in userObjects.
    let mut rooms: Vec<(&str, bool, Option<RoomIdx>)> = room_ids.iter().map(|&n| (n, true, w.meta.by_name(n))).collect();
    for (&r, p) in presence {
        if p.objects > 0 && p.vision == 0 && !listed.contains(name(r)) {
            rooms.push((name(r), false, Some(r)));
        }
    }
    let vision = || rooms.iter().filter(|e| e.1);
    let mut visible: Vec<RoomIdx> = vision().filter_map(|e| e.2).collect();
    visible.sort_unstable();

    // data.js:138-139: findEx({sender|recipient: me}, {sort: {time: -1}, limit: 100}).
    let outgoing: Vec<&DocRef> = w.market.transactions_of(me, false).collect();
    let incoming: Vec<&DocRef> = w.market.transactions_of(me, true).collect();

    // userIdsHash (data.js:101,123-125,180-198) → users.find({_id: {$in}}): a full scan, `users` order.
    let mut user_set = BTreeSet::from([job.user]);
    for r in &visible {
        user_set.extend(w.rooms[*r as usize].user_refs.iter().copied());
    }
    let counterparty = |list: &[&DocRef], k: Atom| -> Vec<UserIdx> {
        list.iter().filter_map(|t| str_field(&t.obj, k).and_then(Id::new).and_then(|id| w.user_ids.get(&id).copied())).collect()
    };
    user_set.extend(counterparty(&outgoing, atom!("recipient")));
    user_set.extend(counterparty(&incoming, atom!("sender")));

    // data.js:151-165 (CPU-LIMIT) on the stored values: `cpuAvailable || 0` keeps a truthy value as it is, and
    // `cpuBucket + cpu` concatenates when either side is a string, array or object.
    let (cpu, cpu_bucket) = match doc.get(atom!("cpu")) {
        Some(stored) if truthy(Some(stored)) => {
            let mut bucket = doc.get(atom!("cpuAvailable")).filter(|v| truthy(Some(v))).cloned().unwrap_or(Val::Num(0.0));
            if number(Some(&bucket)) < 0.0 {
                bucket = Val::Num(0.0);
            }
            let cpu = js_add(&bucket, stored);
            let max = w.config.cpu_max_per_tick as f64;
            (if number(Some(&cpu)) > max { Val::Num(max) } else { cpu }, bucket)
        }
        _ => (Val::Num(f64::INFINITY), Val::Num(f64::INFINITY)),
    };
    let code = code_bundle(&user.code);
    let code_timestamp = user.code.active_world.and_then(|i| user.code.branches.get(i)).and_then(|b| b.get(atom!("timestamp"))).filter(|v| truthy(Some(v)));

    // ---- meta ----
    let mut m = String::with_capacity(4096);
    m.push_str("{\"time\":");
    number_json(&mut m, w.time as f64);
    m.push_str(",\"user\":");
    write_obj_ascii(&mut m, doc);
    m.push_str(",\"cpu\":");
    value_json(&mut m, &cpu);
    m.push_str(",\"cpuBucket\":");
    value_json(&mut m, &cpu_bucket);
    m.push_str(",\"codeVersion\":");
    number_json(&mut m, code.version as f64);
    m.push_str(",\"codeTimestamp\":");
    crate::doc::write_val(&mut m, code_timestamp.unwrap_or(&Val::Num(0.0)), true);
    m.push_str(",\"rooms\":[");
    for (i, (n, vis, r)) in rooms.iter().enumerate() {
        m.push_str(if i > 0 { ",[" } else { "[" });
        quote_ascii(&mut m, n);
        m.push_str(if *vis { ",1," } else { ",0," });
        number_json(&mut m, r.map_or(-1.0, |r| r as f64));
        m.push(']');
    }
    // data.js:140 rooms.flags.find({user}).
    m.push_str("],\"flags\":[");
    let flags = w.index.flag_rooms(job.user).flat_map(|r| w.rooms[r as usize].flags.iter()).filter(|f| str_field(f, atom!("user")) == Some(me));
    list(&mut m, flags, |m, f| write_obj_ascii(m, f));
    // data.js:142,220 _.indexBy(power creeps, '_id'): a repeated id keeps its first position and takes the last
    // doc, which JSON.parse does too.
    m.push_str("],\"powerCreeps\":{");
    list(&mut m, user.power_creeps.iter(), |m, pc| {
        quote_ascii(m, &id_key(&pc.obj));
        m.push(':');
        m.push_str(pc.json());
    });
    m.push_str("},\"users\":{");
    list(&mut m, user_set.iter().map(|&u| &w.users[u as usize]), |m, u| {
        quote_ascii(m, u.id.as_str());
        m.push(':');
        let mut projected = Obj::new();
        for k in [atom!("_id"), atom!("username"), atom!("badge")] {
            if let Some(v) = u.doc.get(k) {
                projected.set(k, v.clone());
            }
        }
        write_obj_ascii(m, &projected);
    });
    m.push_str("},\"transactions\":{\"outgoing\":[");
    list(&mut m, outgoing.iter(), |m, t| m.push_str(t.json()));
    m.push_str("],\"incoming\":[");
    list(&mut m, incoming.iter(), |m, t| m.push_str(t.json()));
    m.push_str("]},\"consoleCommands\":[");
    list(&mut m, user.console_queue.iter(), |m, c| write_obj_ascii(m, c));
    // data.js:227 market.orders.find({user}): no index, creation order.
    m.push_str("],\"myOrders\":[");
    list(&mut m, w.market.orders_of(me), |m, o| m.push_str(o.json()));

    // data.js:228-260 segments.
    let mut segments = Vec::new();
    let active = doc.get(atom!("activeSegments"));
    if let Some(Val::Arr(ids)) = active {
        for id in ids {
            if let Some(id) = segment_id(Some(id)) {
                segments.push((id, user.segments.get(&id).cloned().unwrap_or_else(|| JsStr::from_str(""))));
            }
        }
    }
    let mut foreign_meta = None;
    let mut foreign_segment = None;
    if let Some(Val::Obj(afs)) = doc.get(atom!("activeForeignSegment")) {
        let raw_id = afs.get(atom!("id"));
        // ID-FOREIGN-SEG0: vanilla tests `id` for truthiness, so segment 0 is never fetched.
        let id_ok = if w.compat.fix("ID-FOREIGN-SEG0") { !matches!(raw_id, None | Some(Val::Null)) } else { truthy(raw_id) };
        let other = afs.get(atom!("user_id")).filter(|v| truthy(Some(v))).and_then(|v| Id::new(&js_string(v))).and_then(|id| w.user_ids.get(&id)).map(|&u| &w.users[u as usize]);
        if let (true, Some(other)) = (id_ok, other) {
            let id_str = js_string(raw_id.unwrap_or(&Val::Null));
            if !other.public_segments.is_empty() && other.public_segments.split(',').any(|s| *s == *id_str) {
                let data = segment_id(raw_id).and_then(|id| other.segments.get(&id).cloned());
                foreign_meta = Some((afs.get(atom!("username")), raw_id));
                if let (Some(data), Some(id)) = (data, segment_id(raw_id)) {
                    let username = afs.get(atom!("username")).map_or_else(String::new, |v| js_string(v).to_string());
                    foreign_segment = Some((username.into_boxed_str(), id, data));
                }
            }
        }
    }
    m.push_str("],\"segments\":{\"activeSegments\":");
    crate::doc::write_val(&mut m, active.unwrap_or(&Val::Null), true);
    m.push_str(",\"foreign\":");
    match foreign_meta {
        Some((username, id)) => {
            m.push_str("{\"username\":");
            crate::doc::write_val(&mut m, username.unwrap_or(&Val::Null), true);
            m.push_str(",\"id\":");
            crate::doc::write_val(&mut m, id.unwrap_or(&Val::Null), true);
            m.push('}');
        }
        None => m.push_str("null"),
    }
    m.push_str(",\"publicSegments\":");
    quote_ascii(&mut m, &user.public_segments);
    m.push_str("},\"shard\":\"shard0\"}");

    RunInput {
        user: job.user,
        meta: m,
        blobs: rooms.iter().map(|e| e.2.map(|r| w.rooms[r as usize].blob().clone())).collect(),
        blob_rooms: rooms.iter().map(|e| e.2).collect(),
        event_logs: vision().map(|e| e.2.map_or_else(|| Arc::from(""), |r| w.rooms[r as usize].event_log.clone())).collect(),
        visible,
        memory: user.memory.clone(),
        memory_version: user.memory_version,
        segments,
        foreign_segment,
        code,
        vnow: w.clock.at(w.time),
        nominal_tick_ms: w.clock.nominal_tick_ms,
    }
}

/// Comma-separated items.
fn list<T>(m: &mut String, items: impl IntoIterator<Item = T>, mut write: impl FnMut(&mut String, T)) {
    for (i, x) in items.into_iter().enumerate() {
        if i > 0 {
            m.push(',');
        }
        write(m, x);
    }
}

/// A JSON number; ±Infinity as `±1e999` (JSON.parse reads Infinity), NaN as null.
fn number_json(out: &mut String, x: f64) {
    match x {
        x if x.is_finite() => util::num::write_js_string(out, x),
        x if x.is_nan() => out.push_str("null"),
        x => out.push_str(if x > 0.0 { "1e999" } else { "-1e999" }),
    }
}

/// A JS value as JSON: numbers through `number_json` (Infinity survives), anything else as stored.
fn value_json(out: &mut String, v: &Val) {
    match v {
        Val::Num(n) => number_json(out, *n),
        v => crate::doc::write_val(out, v, true),
    }
}

/// JS `a + b` on stored values: concatenation when either side's ToPrimitive is a string (strings, arrays,
/// objects), else numeric addition.
fn js_add(a: &Val, b: &Val) -> Val {
    let stringy = |v: &Val| matches!(v, Val::Str(_) | Val::Arr(_) | Val::Obj(_));
    if stringy(a) || stringy(b) {
        Val::Str(format!("{}{}", js_string(a), js_string(b)).into())
    } else {
        Val::Num(number(Some(a)) + number(Some(b)))
    }
}

/// `o[k]` when it is a string (Loki `$eq` against a string id is strict).
fn str_field(o: &Obj, k: Atom) -> Option<&str> {
    o.get(k).and_then(Val::as_str)
}

fn truthy(v: Option<&Val>) -> bool {
    v.is_some_and(|v| live::truthy(&live::from_val(v)))
}

/// ToNumber; the CPU fields are numbers in every vanilla writer.
fn number(v: Option<&Val>) -> f64 {
    v.map_or(f64::NAN, |v| live::to_number(&live::from_val(v)))
}

fn js_string(v: &Val) -> Arc<str> {
    live::to_js_string(&live::from_val(v))
}

fn id_key(o: &Obj) -> Arc<str> {
    o.get(atom!("_id")).map_or_else(|| Arc::from("undefined"), js_string)
}

/// A segment id as the runtime stores it (an integer 0..99, RawMemory.setActiveSegments).
fn segment_id(v: Option<&Val>) -> Option<u8> {
    match v {
        Some(Val::Num(n)) if n.fract() == 0.0 && (0.0..=255.0).contains(n) => Some(*n as u8),
        _ => None,
    }
}

// ---- code (data.js:167-176, 203-204) ----

/// The active world branch's modules, names unescaped. Cached per `CodeState` allocation: the bundle is rebuilt
/// only when P0 installs a new `Arc<CodeState>`. The `Weak` keeps the allocation (not the code) alive, so an
/// address in the cache always names that same `CodeState`.
fn code_bundle(code: &Arc<CodeState>) -> Arc<CodeBundle> {
    type Cache = HashMap<usize, (Weak<CodeState>, Arc<CodeBundle>)>;
    static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);
    let key = Arc::as_ptr(code) as usize;
    if let Some((_, b)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return b.clone();
    }
    let bundle = Arc::new(make_bundle(code));
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.retain(|_, (weak, _)| weak.strong_count() > 0);
    cache.insert(key, (Arc::downgrade(code), bundle.clone()));
    bundle
}

fn make_bundle(code: &CodeState) -> CodeBundle {
    // `result[1] && result[1].modules || {}`
    let branch = code.active_world.and_then(|i| code.branches.get(i));
    let modules: Vec<(String, Val)> = match branch.and_then(|b| b.get(atom!("modules"))) {
        Some(Val::Obj(o)) => o.iter().map(|(k, v)| (k.as_str().to_string(), v.clone())).collect(),
        _ => Vec::new(),
    };
    let modules = rename_modules(modules)
        .into_iter()
        .map(|(name, v)| {
            let module = match &v {
                Val::Str(s) => Module::Text(s.clone()),
                // game.js:561 `_.isObject(m) && m.binary !== undefined` → driver.bufferFromBase64(m.binary).
                Val::Obj(o) if o.get(atom!("binary")).is_some() => Module::Binary(buffer_from(o.get(atom!("binary")).unwrap()).into()),
                // Anything else is evaluated as its string form (`vm.Script` coerces).
                other => Module::Text(js_string(other)),
            };
            (name.into_boxed_str(), module)
        })
        .collect();
    CodeBundle { version: code.version, modules }
}

/// `for (key in modules) { … modules[newKey] = modules[key]; delete modules[key]; }`: a renamed module moves to the end,
/// or overwrites a module already named `newKey` in place. One pass: the tick waits for this, and a small upload can hold
/// hundreds of thousands of dotted names.
fn rename_modules(modules: Vec<(String, Val)>) -> Vec<(String, Val)> {
    if !modules.iter().any(|(k, _)| k.contains('$')) {
        return modules;
    }
    let mut slots: HashMap<String, usize> = modules.iter().enumerate().map(|(i, (k, _))| (k.clone(), i)).collect();
    let mut out: Vec<Option<(String, Val)>> = modules.into_iter().map(Some).collect();
    for i in 0..out.len() {
        let Some((key, value)) = out[i].take() else { continue };
        let new_key = key.replace("$DOT$", ".").replace("$SLASH$", "/").replace("$BACKSLASH$", "\\");
        if new_key == key {
            out[i] = Some((key, value));
            continue;
        }
        slots.remove(&key);
        match slots.get(&new_key) {
            Some(&j) => out[j].as_mut().expect("a slot in the map is filled").1 = value,
            None => {
                slots.insert(new_key.clone(), out.len());
                out.push(Some((new_key, value)));
            }
        }
    }
    out.into_iter().flatten().collect()
}

/// `Buffer.from(value, 'base64')` with the runtime bundle's Buffer, buffer@4.9 (ENV-BINARY-BUFFER, runtime-driver.js:13):
/// a string is base64 (`buffer_from_base64`), an array gives `x & 255` per element, a JSON-ified Buffer
/// `{type: 'Buffer', data: [..]}` its data, `_isBuffer` or a non-numeric `length` an empty buffer. Every other value
/// throws there (an array-like object with a numeric `length` reads its index keys); `Module` cannot say so, so
/// those load as an empty buffer (DIVERGENT(ENV-BINARY-BUFFER), interface request).
fn buffer_from(v: &Val) -> Vec<u8> {
    let bytes = |a: &[Val]| a.iter().map(|x| to_uint8(number(Some(x)))).collect();
    match v {
        Val::Str(s) => buffer_from_base64(s),
        Val::Arr(a) => bytes(a),
        Val::Obj(o) if !truthy(o.get(atom!("_isBuffer"))) && o.get(atom!("length")).is_none() => match (o.get(atom!("type")).and_then(Val::as_str), o.get(atom!("data"))) {
            (Some("Buffer"), Some(Val::Arr(a))) => bytes(a),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// `x & 255`: ToInt32, low byte.
fn to_uint8(n: f64) -> u8 {
    if n.is_finite() {
        n.trunc().rem_euclid(256.0) as u8
    } else {
        0
    }
}

/// buffer@4.9 `base64ToBytes` as bundled in the runtime: `base64clean` drops every character outside
/// `[+/0-9A-Za-z-_]` (`=` included, so padding inside the string does not end the data), then base64-js decodes and
/// a trailing partial byte is dropped. Node's own Buffer stops at the first `=` instead. The `base64ToBuffer`
/// native decodes the same way.
pub fn buffer_from_base64(s: &str) -> Vec<u8> {
    let (mut out, mut acc, mut bits) = (Vec::with_capacity(s.len() * 3 / 4), 0u32, 0);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;
    use crate::policy::{Compat, Profile, VClock};
    use crate::terrain::RoomTerrain;
    use crate::world::{Inbox, MarketState, RoomMeta, RoomSnapshot, RoomsMeta, TerrainStore, UserState};
    use util::json::Json;

    fn val_obj(j: &Json) -> Obj {
        match Val::from_json(j) {
            Val::Obj(o) => o,
            _ => panic!("not an object"),
        }
    }

    fn s(j: Option<&Json>) -> Option<&str> {
        j.and_then(Json::as_str)
    }

    /// The scenario's collections (creation order) and env as a World, the way loaders lay them out.
    fn world_from(sc: &Json) -> World {
        let col = |name: &str| sc.get("collections").and_then(|c| c.get(name)).and_then(Json::as_array).cloned().unwrap_or_default();
        let env = sc.get("env").unwrap();
        let rooms = col("rooms");
        let names: Vec<&str> = rooms.iter().map(|r| s(r.get("_id")).unwrap()).collect();
        let meta = RoomsMeta::new(names.iter().map(|n| RoomMeta { name: (*n).into(), xy: n.parse().unwrap(), removed: false }).collect());
        let users = col("users");
        let user_ids: crate::world::UserIds = users.iter().enumerate().map(|(i, u)| (Id::new(s(u.get("_id")).unwrap()).unwrap(), i as UserIdx)).collect();
        let objects: Vec<DocRef> = col("rooms.objects").iter().enumerate().map(|(i, d)| Doc::new(i as u64 + 1, val_obj(d))).collect();
        let flags = col("rooms.flags");
        let logs = env.get("roomEventLog:");
        let snapshots = rooms
            .iter()
            .zip(&names)
            .map(|(info, name)| {
                let docs = objects.iter().filter(|d| str_field(&d.obj, atom!("room")) == Some(name)).cloned().collect();
                let fl: Vec<Arc<Obj>> = flags.iter().filter(|f| s(f.get("room")) == Some(name)).map(|f| Arc::new(val_obj(f))).collect();
                let log = s(logs.and_then(|l| l.get(name))).unwrap_or("");
                Arc::new(RoomSnapshot::from_docs(Arc::new(val_obj(info)), docs, fl.into(), log.into(), "".into(), 0, &user_ids))
            })
            .collect();
        let of_user = |c: &str, id: &str| -> Vec<Json> { col(c).into_iter().filter(|d| s(d.get("user")) == Some(id)).collect() };
        let users = users
            .iter()
            .enumerate()
            .map(|(i, u)| {
                let id = s(u.get("_id")).unwrap();
                let branches: Vec<Arc<Obj>> = of_user("users.code", id).iter().map(|b| Arc::new(val_obj(b))).collect();
                let active_world = branches.iter().position(|b| b.get(atom!("activeWorld")) == Some(&Val::Bool(true)));
                let segments = env.get(&format!("memorySegments:{id}")).and_then(Json::as_object).map_or_else(BTreeMap::new, |h| {
                    h.iter().map(|(k, v)| (k.parse::<u8>().unwrap(), JsStr::from_str(v.as_str().unwrap()))).collect()
                });
                // users.console / users.power_creeps are adaptive: freshly inserted docs list newest first.
                let console: Vec<Arc<Obj>> = of_user("users.console", id).iter().rev().map(|c| Arc::new(val_obj(c))).collect();
                let pcs: Vec<DocRef> = of_user("users.power_creeps", id).iter().rev().map(|p| Doc::new(0, val_obj(p))).collect();
                Arc::new(UserState {
                    id: Id::new(id).unwrap(),
                    seq: i as u64,
                    doc: Arc::new(val_obj(u)),
                    auth: Arc::new(Obj::new()),
                    code: Arc::new(CodeState { branches, active_world, active_sim: None, version: 3 }),
                    memory: JsStr::from_str(s(env.get(&format!("memory:{id}"))).unwrap_or("")),
                    memory_version: 1,
                    segments: Arc::new(segments),
                    public_segments: s(env.get(&format!("publicMemorySegments:{id}"))).unwrap_or("").into(),
                    inter_shard: None,
                    console_queue: console.into(),
                    power_creeps: pcs.into(),
                    inbox: Arc::new(Inbox::default()),
                })
            })
            .collect();
        let terrain = TerrainStore::new(vec![RoomTerrain::default(); names.len()]);
        let clock = VClock { base_ms: 1_000_000, base_tick: 0, nominal_tick_ms: 1000 };
        let mut w = World::new(1, clock, Compat::new(Profile::Private), meta, terrain, snapshots, users);
        w.time = env.get("gameTime").and_then(Json::as_f64).unwrap() as u64;
        let docs = |c: &str| -> Arc<[DocRef]> { col(c).iter().enumerate().map(|(i, d)| Doc::new(i as u64, val_obj(d))).collect() };
        let stats: Arc<[Arc<Obj>]> = col("market.stats").iter().map(|d| Arc::new(val_obj(d))).collect();
        w.market = Arc::new(MarketState::new(docs("market.orders"), stats, docs("transactions"), Arc::new([]), Arc::new([])));
        w
    }

    fn strs(j: Option<&Json>) -> Vec<String> {
        j.and_then(Json::as_array).unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
    }

    fn ids(j: Option<&Json>) -> Vec<String> {
        j.and_then(Json::as_array).unwrap().iter().map(|d| s(d.get("_id")).unwrap().to_string()).collect()
    }

    fn keys(j: Option<&Json>) -> Vec<String> {
        j.and_then(Json::as_object).unwrap().iter().map(|(k, _)| k.clone()).collect()
    }

    /// JS own-key order of keys inserted in this order.
    fn js_order(mut keys: Vec<String>) -> Vec<String> {
        keys.sort_by_key(|k| array_index(k).map_or((1, 0), |i| (0, i)));
        keys
    }

    /// Official data.js over Loki vs build_input + the documented JS assembly (§6.6): every order the runtime
    /// builds from runtimeData (roomIds, roomObjects, rooms, users, flags, transactions, console, power creeps,
    /// orders), CPU, Memory, segments, code and event logs.
    #[test]
    fn data_fixture() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/world/data.ndjson")).unwrap();
        let mut checked = 0;
        for line in text.lines() {
            let sc = Json::parse(line).unwrap();
            let w = world_from(&sc);
            let me = s(sc.get("me")).unwrap();
            let user = w.user_ids[&Id::new(me).unwrap()];
            let out = sc.get("out").unwrap();
            if out.get("rejected").is_some() {
                // RUN-DEACTIVATE: data.js finds no objects of the user; make.rs reads that off the presence.
                assert!(w.index.presence[user as usize].values().all(|p| p.objects == 0), "{line}");
                continue;
            }
            let input = build_input(&w, &RunJob { user, expected_ms: 0.0, limit: 0.0, tick_limit: 0.0, bucket: 0.0 });
            assert!(input.meta.is_ascii());
            let meta = Json::parse(&input.meta).unwrap();
            let ctx = |what: &str| format!("{what} in {line}");
            let entries: Vec<(String, bool, f64)> = meta
                .get("rooms")
                .and_then(Json::as_array)
                .unwrap()
                .iter()
                .map(|e| (e.at(0).unwrap().as_str().unwrap().to_string(), e.at(1).unwrap().as_f64() == Some(1.0), e.at(2).unwrap().as_f64().unwrap()))
                .collect();
            let vision: Vec<&(String, bool, f64)> = entries.iter().filter(|e| e.1).collect();
            assert_eq!(vision.iter().map(|e| e.0.clone()).collect::<Vec<_>>(), strs(out.get("roomIds")), "{}", ctx("roomIds"));

            // roomObjects = _.extend(mapById(foreign of the vision rooms, roomIds order), own docs by creation order)
            assert_eq!(input.blobs.len(), entries.len());
            let (mut foreign, mut own) = (Vec::new(), Vec::new());
            for (e, blob) in entries.iter().zip(&input.blobs) {
                let Some(blob) = blob else {
                    assert_eq!(e.2, -1.0);
                    continue;
                };
                let b = Json::parse(std::str::from_utf8(blob.text()).unwrap()).unwrap();
                let seqs = b.get("seq").and_then(Json::as_array).unwrap();
                for (sq, d) in seqs.iter().zip(b.get("objects").and_then(Json::as_array).unwrap()) {
                    let id = s(d.get("_id")).unwrap().to_string();
                    if s(d.get("user")) == Some(me) {
                        own.push((sq.as_f64().unwrap() as u64, id));
                    } else if e.1 {
                        foreign.push(id);
                    }
                }
            }
            own.sort_by_key(|(sq, _)| *sq);
            foreign.extend(own.into_iter().map(|(_, id)| id));
            assert_eq!(js_order(foreign), strs(out.get("roomObjects")), "{}", ctx("roomObjects"));

            let mut game_rooms: Vec<(f64, String)> = vision.iter().filter(|e| e.2 >= 0.0).map(|e| (e.2, e.0.clone())).collect();
            game_rooms.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert_eq!(game_rooms.iter().map(|r| r.1.clone()).collect::<Vec<_>>(), strs(out.get("rooms")), "{}", ctx("rooms"));
            let mut visible: Vec<RoomIdx> = game_rooms.iter().map(|r| r.0 as RoomIdx).collect();
            visible.sort_unstable();
            assert_eq!(input.visible, visible);
            assert_eq!(keys(meta.get("users")), strs(out.get("users")), "{}", ctx("users"));

            // Flags: vanilla's cross-room creation order is DIVERGENT(ORDER-MISC); within a room it matches.
            let flag_room = |id: &str| {
                let all = sc.get("collections").and_then(|c| c.get("rooms.flags")).and_then(Json::as_array).unwrap();
                let f = all.iter().find(|f| s(f.get("_id")) == Some(id)).unwrap();
                w.meta.by_name(s(f.get("room")).unwrap()).unwrap()
            };
            let mut want_flags = strs(out.get("flags"));
            want_flags.sort_by_key(|id| flag_room(id));
            assert_eq!(ids(meta.get("flags")), want_flags, "{}", ctx("flags"));

            let tx = meta.get("transactions").unwrap();
            assert_eq!(ids(tx.get("outgoing")), strs(out.get("outgoing")), "{}", ctx("outgoing"));
            assert_eq!(ids(tx.get("incoming")), strs(out.get("incoming")), "{}", ctx("incoming"));
            assert_eq!(ids(meta.get("consoleCommands")), strs(out.get("consoleCommands")), "{}", ctx("console"));
            assert_eq!(keys(meta.get("powerCreeps")), strs(out.get("powerCreeps")), "{}", ctx("powerCreeps"));
            assert_eq!(ids(meta.get("myOrders")), strs(out.get("myOrders")), "{}", ctx("myOrders"));
            // Any JSON value; a non-finite number travels as {number: "Infinity"}.
            let value = |k: &str| match out.get(k).unwrap() {
                j if j.get("number").is_some() => Json::Num(util::num::string_to_number(s(j.get("number")).unwrap())),
                j => j.clone(),
            };
            assert_eq!(meta.get("cpu"), Some(&value("cpu")), "{}", ctx("cpu"));
            assert_eq!(meta.get("cpuBucket"), Some(&value("cpuBucket")), "{}", ctx("cpuBucket"));
            assert_eq!(meta.get("time"), out.get("time"));
            assert_eq!(meta.get("codeTimestamp"), out.get("codeTimestamp"), "{}", ctx("codeTimestamp"));
            assert_eq!(input.memory.to_string_lossy(), s(out.get("memory")).unwrap());

            match out.get("memorySegments").unwrap() {
                Json::Null => assert!(input.segments.is_empty(), "{}", ctx("segments")),
                m => {
                    let got: BTreeMap<String, String> = input.segments.iter().map(|(id, v)| (id.to_string(), v.to_string_lossy())).collect();
                    let want: BTreeMap<String, String> = m.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
                    assert_eq!(got, want, "{}", ctx("segments"));
                }
            }
            let foreign_meta = meta.get("segments").and_then(|x| x.get("foreign")).unwrap();
            match out.get("foreign").unwrap() {
                Json::Null => assert!(foreign_meta.is_null() && input.foreign_segment.is_none(), "{}", ctx("foreign")),
                f => {
                    assert_eq!((foreign_meta.get("username"), foreign_meta.get("id")), (f.get("username"), f.get("id")), "{}", ctx("foreign"));
                    let data = input.foreign_segment.as_ref().map(|(u, id, d)| {
                        assert_eq!((Some(&**u), Some(*id as f64)), (s(f.get("username")), f.get("id").and_then(Json::as_f64)));
                        d.to_string_lossy()
                    });
                    assert_eq!(data.as_deref(), s(f.get("data")), "{}", ctx("foreign data"));
                }
            }

            let want_code = out.get("code").and_then(Json::as_array).unwrap();
            assert_eq!(input.code.modules.len(), want_code.len(), "{}", ctx("code"));
            for ((name, module), want) in input.code.modules.iter().zip(want_code) {
                assert_eq!(Some(&**name), want.at(0).unwrap().as_str(), "{}", ctx("code names"));
                let want = want.at(1).unwrap();
                match module {
                    Module::Text(t) => assert_eq!(Some(&**t), want.as_str().or(s(want.get("coerced"))), "{}", ctx("module text")),
                    Module::Binary(b) => {
                        // DIVERGENT(ENV-BINARY-BUFFER): where buffer@4.9 throws at require, the module is empty.
                        let bytes: Vec<u8> = want.get("binary").and_then(Json::as_array).map_or_else(Vec::new, |a| a.iter().map(|x| x.as_f64().unwrap() as u8).collect());
                        assert!(want.get("binary").is_some() || want.get("throws").is_some(), "{}", ctx("binary module"));
                        assert_eq!(&**b, &bytes[..], "{}", ctx("binary module"));
                    }
                }
            }

            // The market natives' JSON (data.js:31-75), twice to go through the per-version cache.
            for (k, want) in out.get("marketOrders").and_then(Json::as_object).unwrap() {
                for _ in 0..2 {
                    assert_eq!(w.market.orders_json(k).as_deref(), want.as_str(), "{}", ctx(&format!("orders {k}")));
                }
                let all = Json::parse(&w.market.orders_json("all").unwrap()).unwrap();
                for (id, order) in all.as_object().unwrap() {
                    assert_eq!(Json::parse(&w.market.order_json(id).unwrap()).unwrap(), *order);
                }
                assert_eq!(w.market.order_json("no such order"), None);
            }
            for (k, want) in out.get("marketHistory").and_then(Json::as_object).unwrap() {
                for _ in 0..2 {
                    assert_eq!(w.market.history_json(k).as_deref(), want.as_str(), "{}", ctx(&format!("history {k}")));
                }
            }

            let logs = out.get("eventLogs").and_then(Json::as_array).unwrap();
            assert_eq!(input.event_logs.len(), logs.len());
            for (got, want) in input.event_logs.iter().zip(logs) {
                assert_eq!((!got.is_empty()).then_some(&**got), want.as_str(), "{}", ctx("eventLogs"));
            }
            checked += 1;
        }
        assert!(checked >= 100, "only {checked} scenarios");
    }

    /// The runtime bundle's buffer@4.9 (not node's Buffer, which stops at the first `=`); the data fixture pins
    /// the same strings against the real package.
    #[test]
    fn base64_like_the_runtime_buffer() {
        for (s, want) in [
            ("QU JD\n", &b"ABC"[..]),
            ("QUJD=QUJD", b"ABCABC"),
            ("QUJ-_w", &[65, 66, 126, 255]),
            ("QUJ", b"AB"),
            ("Q", b""),
            ("QQ==QQ", &[65, 4, 16]),
            ("!!QUJD", b"ABC"),
            ("QUJDRA=", b"ABCD"),
            ("QUJDR", b"ABC"),
        ] {
            assert_eq!(buffer_from_base64(s), want, "{s:?}");
        }
        let v = |s: &str| Val::from_json(&Json::parse(s).unwrap());
        assert_eq!(buffer_from(&v(r#"[1, 256, -1, "7", 2.9, null, [5], {}]"#)), [1, 0, 255, 7, 2, 0, 5, 0]);
        assert_eq!(buffer_from(&v(r#"{"type": "Buffer", "data": [104, 105]}"#)), b"hi");
        assert_eq!(buffer_from(&v(r#"{"type": "Buffer", "data": [104], "_isBuffer": 1}"#)), b"");
    }

    #[test]
    fn module_names_rename_like_the_for_in_loop() {
        let renamed = |names: &[(&str, f64)]| {
            let list = names.iter().map(|(k, n)| (k.to_string(), Val::Num(*n))).collect();
            rename_modules(list).into_iter().map(|(k, v)| (k, v.as_f64().unwrap())).collect::<Vec<_>>()
        };
        let want = |names: &[(&str, f64)]| names.iter().map(|(k, n)| (k.to_string(), *n)).collect::<Vec<_>>();
        assert_eq!(renamed(&[("main", 1.0), ("lib", 2.0)]), want(&[("main", 1.0), ("lib", 2.0)]));
        // A renamed module moves to the end, in the order the names are visited.
        assert_eq!(renamed(&[("a$DOT$b", 1.0), ("main", 2.0), ("c$SLASH$d", 3.0)]), want(&[("main", 2.0), ("a.b", 1.0), ("c/d", 3.0)]));
        assert_eq!(renamed(&[("a$BACKSLASH$b", 1.0)]), want(&[("a\\b", 1.0)]));
        // A name that is taken keeps its place and takes the value.
        assert_eq!(renamed(&[("a.b", 1.0), ("a$DOT$b", 2.0), ("main", 3.0)]), want(&[("a.b", 2.0), ("main", 3.0)]));
        assert_eq!(renamed(&[("a$DOT$b", 1.0), ("a.b", 2.0), ("main", 3.0)]), want(&[("a.b", 1.0), ("main", 3.0)]));
    }

    /// 100 000 dotted names stalled ticks for 53 s through the scans this replaced, in a release build, and three times
    /// as many would take them nine times as long. So the bound can be wide and still tell the two apart: a debug build
    /// on a build machine in the cloud took 5.6 s for this.
    #[test]
    fn many_dotted_module_names_take_linear_time() {
        let list: Vec<(String, Val)> = (0..300_000).map(|i| (format!("m{i}$DOT$js"), Val::Null)).collect();
        let start = std::time::Instant::now();
        let out = rename_modules(list);
        assert!(start.elapsed().as_secs() < 60, "{:?}", start.elapsed());
        assert_eq!((out.len(), out[299_999].0.as_str()), (300_000, "m299999.js"));
    }
}
