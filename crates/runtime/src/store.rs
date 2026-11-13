//! not a mirror: the system isolate's `__store` (host/system.js: make.js:192 storeIntents + intentsCount, §4.2)
//! in Rust for the outMessages nearly every run produces, without a second isolate or a deserialization.
//!
//! The run's outMessage arrives as ValueSerializer bytes (marshal::capture). This reads them directly, but only
//! as far as they are JSON-shaped: undefined, null, booleans, numbers, strings, plain objects and dense arrays,
//! each object reached once. Everything else returns `None` and the run goes through host/system.js as before,
//! which is the reference: Dates, Maps, boxed values, BigInts, holes, shared or cyclic references, array-index
//! or `__proto__` keys, lone surrogates; `undefined` where it is not just a missing intent field; names that
//! reach Object.prototype (vanilla's prototype pollution); and every throw (the JS error text and stack).
//! In the subset both paths give the same Json (tests::matches_the_system_isolate).

use engine::system::{sanitize_intent, sanitize_user_intents, INTENT_TYPES};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use util::json::Json;

/// A deserialized value of the subset. ASCII strings, which nearly all are (names, ids, a tick's visuals), stay in the
/// bytes they were read from.
#[derive(Debug)]
enum Js<'a> {
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(Cow<'a, str>),
    Arr(Vec<Js<'a>>),
    Obj(Vec<(Cow<'a, str>, Js<'a>)>),
}

impl Js<'_> {
    fn get(&self, key: &str) -> &Self {
        match self {
            Js::Obj(props) => props.iter().find(|(k, _)| k == key).map_or(&Js::Undef, |(_, v)| v),
            _ => &Js::Undef,
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Js::Undef | Js::Null => false,
            Js::Bool(b) => *b,
            Js::Num(n) => *n != 0.0 && !n.is_nan(),
            Js::Str(s) => !s.is_empty(),
            Js::Arr(_) | Js::Obj(_) => true,
        }
    }
}

/// Keys a plain object inherits from Object.prototype (`({})[k]` is truthy for these).
const OBJECT_PROTOTYPE: &[&str] = &[
    "__proto__", "__defineGetter__", "__defineSetter__", "__lookupGetter__", "__lookupSetter__", "constructor",
    "hasOwnProperty", "isPrototypeOf", "propertyIsEnumerable", "toLocaleString", "toString", "valueOf",
];

/// Up to here a scan beats hashing, and nearly every object is this small. Past it nothing in this file may scan: it runs
/// on every bot's output after the CPU meter has stopped and outside the hard timeout, so a bot that passed one object of
/// 100000 keys held every tick for 26 s and paid 0.06 CPU for it.
const SCAN: usize = 16;

/// `__store(out, userId)` for a subset outMessage: the Json host/system.js returns (after JSON.parse), or `None`
/// for the system isolate. `object_room` is the objectRoom native.
pub(crate) fn store(bytes: &[u8], object_room: impl Fn(&str) -> Option<String>) -> Option<Json> {
    let out = Wire::new(bytes)?.value(0)?;
    if !matches!(out, Js::Obj(_)) {
        return None;
    }
    let list = out.get("intentsList");
    let intents = store_intents(list, &object_room)?;
    let count = intents_count(list);
    // JSON.stringify of host/system.js's literal: undefined members are left out
    let mut o = Vec::new();
    let mut put = |k: &str, v: Option<Json>| {
        if let Some(v) = v {
            o.push((k.to_string(), v));
        }
    };
    put("type", json(out.get("type")));
    put("error", if matches!(out.get("error"), Js::Undef) { Some(Json::Null) } else { json(out.get("error")) });
    for k in ["usedTime", "usedCleanTime", "usedDirtyTime"] {
        put(k, json(out.get(k)));
    }
    put("intents", Some(intents));
    put("count", Some(Json::Num(count as f64)));
    for k in ["console", "visual", "activeSegments", "activeForeignSegment", "defaultPublicSegment", "publicSegments"] {
        put(k, json(out.get(k)));
    }
    Some(Json::Obj(o))
}

/// A key of `intents` under construction, in insertion order (no array-index keys reach it).
enum Node {
    /// `intents.notify = undefined`: holds the key's place, left out of the JSON.
    Undef,
    Json(Json),
    Map(Map),
    List(Vec<Json>),
}

/// An object under construction, in insertion order. Its keys are the bot's to pick through `roomName`, so past `SCAN`
/// they are found through an index.
#[derive(Default)]
struct Map {
    items: Vec<(String, Node)>,
    index: HashMap<String, usize>,
}

impl Map {
    fn position(&mut self, k: &str) -> Option<usize> {
        if self.items.len() <= SCAN {
            return self.items.iter().position(|(key, _)| key == k);
        }
        if self.index.is_empty() {
            self.index = self.items.iter().enumerate().map(|(i, (key, _))| (key.clone(), i)).collect();
        }
        self.index.get(k).copied()
    }

    fn push(&mut self, k: &str, v: Node) -> usize {
        if !self.index.is_empty() {
            self.index.insert(k.to_string(), self.items.len());
        }
        self.items.push((k.to_string(), v));
        self.items.len() - 1
    }
}

/// `m[k] = m[k] || {}` for a key that never holds a falsy value (only maps are stored under these keys).
fn map_at<'a>(m: &'a mut Map, k: &str) -> Option<&'a mut Map> {
    let i = match m.position(k) {
        Some(i) => i,
        None => m.push(k, Node::Map(Map::default())),
    };
    match &mut m.items[i].1 {
        Node::Map(inner) => Some(inner),
        _ => None,
    }
}

/// `m[k] = v`.
fn assign(m: &mut Map, k: &str, v: Node) {
    match m.position(k) {
        Some(i) => m.items[i].1 = v,
        None => drop(m.push(k, v)),
    }
}

fn node_json(n: Node) -> Option<Json> {
    match n {
        Node::Undef => None,
        Node::Json(j) => Some(j),
        Node::Map(m) => Some(Json::Obj(m.items.into_iter().filter_map(|(k, v)| Some((k, node_json(v)?))).collect())),
        Node::List(l) => Some(Json::Arr(l)),
    }
}

// vanilla: utils.js:405 storeIntents (host/system.js)
fn store_intents(user_intents: &Js, object_room: &impl Fn(&str) -> Option<String>) -> Option<Json> {
    let mut intents = Map::default();

    let entries = match user_intents {
        // for-in over undefined / null visits nothing
        Js::Undef | Js::Null => &[][..],
        Js::Obj(props) => props,
        _ => return None,
    };
    for (i, value) in entries {
        // an undefined entry would reach the sanitizers as `input` and throw
        if matches!(value, Js::Undef) {
            return None;
        }

        let i: &str = i;
        if i == "notify" {
            let input = Json::Obj(vec![("notify".into(), input_json(value)?)]);
            let notify = sanitize_user_intents(&input, &[]).ok()?.get(engine::doc::Atom::intern("notify")).map(|v| v.to_json());
            assign(&mut intents, "notify", notify.map_or(Node::Undef, Node::Json));
            continue;
        }

        if i == "room" {
            sanitize_user_room_intents(value, &mut intents)?;
            continue;
        }

        if i == "global" {
            let global = sanitize_user_intents(&input_json(value)?, &[]).ok()?;
            assign(&mut intents, "global", Node::Json(engine::doc::Val::Obj(global).to_json()));
            continue;
        }

        let room = match object_room(i) {
            Some(room) => room,
            // `({})[i]`: inherited members are truthy objects without a room
            None if OBJECT_PROTOTYPE.contains(&i) => return None,
            None => continue,
        };

        let sanitized = sanitize_user_intents(&input_json(value)?, &[]).ok()?;
        assign(map_at(&mut intents, &room)?, i, Node::Json(engine::doc::Val::Obj(sanitized).to_json()));
    }

    node_json(Node::Map(intents))
}

// vanilla: system.js:116 sanitizeUserRoomIntents (built-in names; customIntentTypes is always `{}` here)
fn sanitize_user_room_intents(input: &Js, result: &mut Map) -> Option<()> {
    match input {
        // `input[name]` throws
        Js::Undef | Js::Null => return None,
        Js::Obj(_) => {}
        // primitives and arrays have no own intent names; inherited ones are not intent names either
        _ => return Some(()),
    }
    for &(name, _) in INTENT_TYPES {
        let list = input.get(name);
        if !list.truthy() {
            continue;
        }
        // `for(let intent of input[name])`: only arrays here (strings iterate, other values throw)
        let Js::Arr(items) = list else { return None };
        for intent in items {
            let sanitized = sanitize_intent(name, &input_json(intent)?, &[]).ok()?;
            // `"" + roomName` for types with a roomName field, `undefined` (the key "undefined") without one
            let grouping_value = match sanitized.get(engine::doc::Atom::intern("roomName")) {
                Some(engine::doc::Val::Str(s)) => s.to_string(),
                Some(_) => return None,
                None => "undefined".to_string(),
            };
            if OBJECT_PROTOTYPE.contains(&grouping_value.as_str())
                || grouping_value == "notify"
                || grouping_value == "global"
                || engine::doc::array_index(&grouping_value).is_some()
            {
                return None;
            }
            let room_name_result = map_at(result, &grouping_value)?;
            let room_result = map_at(room_name_result, "room")?;
            let at = match room_result.position(name) {
                Some(at) => at,
                None => room_result.push(name, Node::List(Vec::new())),
            };
            let Node::List(l) = &mut room_result.items[at].1 else { return None };
            l.push(engine::doc::Val::Obj(sanitized).to_json());
        }
    }
    Some(())
}

// vanilla: make.js:98 intentsCount, counted over the raw list: free say/pull, arrays per element
fn intents_count(intents_list: &Js) -> usize {
    // `freeMethods[intentName]`: own say/pull, or anything the literal inherits
    let free = |name: &str| name == "say" || name == "pull" || OBJECT_PROTOTYPE.contains(&name);
    let per_value = |v: &Js| match v {
        Js::Arr(a) => a.len(),
        _ => 1,
    };
    let Js::Obj(entities) = intents_list else { return 0 };
    let mut count = 0;
    for (_, entity) in entities {
        count += match entity {
            Js::Obj(names) => names.iter().filter(|(name, _)| !free(name)).map(|(_, v)| per_value(v)).sum(),
            // for-in over an array visits its indices; "0", "1", … are never free
            Js::Arr(items) => items.iter().map(per_value).sum(),
            // over a string its UTF-16 indices, each a one-unit string
            Js::Str(s) => s.encode_utf16().count(),
            _ => 0,
        };
    }
    count
}

/// A value handed to the sanitizers (engine::system reads Json): an undefined object member is a missing one,
/// which `intent[field]` cannot tell apart; undefined anywhere else is not handled.
fn input_json(v: &Js) -> Option<Json> {
    Some(match v {
        Js::Undef => return None,
        Js::Null => Json::Null,
        Js::Bool(b) => Json::Bool(*b),
        Js::Num(n) => Json::Num(*n),
        Js::Str(s) => Json::Str(s.to_string()),
        Js::Arr(items) => Json::Arr(items.iter().map(input_json).collect::<Option<_>>()?),
        Js::Obj(props) => Json::Obj(
            props.iter().filter(|(_, v)| !matches!(v, Js::Undef)).map(|(k, v)| Some((k.to_string(), input_json(v)?))).collect::<Option<_>>()?,
        ),
    })
}

/// JSON.stringify then JSON.parse of a subset value: `None` is undefined (left out of objects, `null` in arrays),
/// NaN and ±Infinity become null, -0 becomes 0.
fn json(v: &Js) -> Option<Json> {
    Some(match v {
        Js::Undef => return None,
        Js::Null => Json::Null,
        Js::Bool(b) => Json::Bool(*b),
        Js::Num(n) if n.is_finite() => Json::Num(if *n == 0.0 { 0.0 } else { *n }),
        Js::Num(_) => Json::Null,
        Js::Str(s) => Json::Str(s.to_string()),
        Js::Arr(items) => Json::Arr(items.iter().map(|x| json(x).unwrap_or(Json::Null)).collect()),
        Js::Obj(props) => Json::Obj(props.iter().filter_map(|(k, v)| Some((k.to_string(), json(v)?))).collect()),
    })
}

fn has_repeats(props: &[(Cow<str>, Js)]) -> bool {
    if props.len() <= SCAN {
        return props.iter().enumerate().any(|(i, (k, _))| props[..i].iter().any(|(earlier, _)| earlier == k));
    }
    let mut seen = HashSet::with_capacity(props.len());
    !props.iter().all(|(k, _)| seen.insert(&**k))
}

/// ValueSerializer bytes (v8/src/objects/value-serializer.cc, format 13-16), the subset only.
struct Wire<'a> {
    b: &'a [u8],
    pos: usize,
}

/// Deeper nesting is left to the system isolate (and its stack check).
const MAX_DEPTH: usize = 256;

impl<'a> Wire<'a> {
    // value-serializer.cc ReadHeader: kVersion, version varint
    fn new(b: &'a [u8]) -> Option<Wire<'a>> {
        let mut w = Wire { b, pos: 0 };
        if w.b.first() == Some(&0xFF) {
            w.pos = 1;
            let version = w.varint()?;
            if !(13..=16).contains(&version) {
                return None;
            }
        }
        Some(w)
    }

    // ReadTag: kPadding ('\0') is skipped
    fn tag(&mut self) -> Option<u8> {
        loop {
            let t = *self.b.get(self.pos)?;
            self.pos += 1;
            if t != 0 {
                return Some(t);
            }
        }
    }

    fn peek_tag(&self) -> Option<u8> {
        self.b[self.pos.min(self.b.len())..].iter().copied().find(|&t| t != 0)
    }

    fn varint(&mut self) -> Option<u32> {
        let (mut v, mut shift) = (0u64, 0);
        loop {
            let byte = *self.b.get(self.pos)?;
            self.pos += 1;
            if shift < 35 {
                v |= ((byte & 0x7F) as u64) << shift;
            }
            shift += 7;
            if byte & 0x80 == 0 {
                return Some(v as u32);
            }
        }
    }

    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }

    // ReadObjectInternal for the subset tags
    fn value(&mut self, depth: usize) -> Option<Js<'a>> {
        if depth > MAX_DEPTH {
            return None;
        }
        Some(match self.tag()? {
            b'?' => {
                self.varint()?; // kVerifyObjectCount, ignored
                return self.value(depth);
            }
            b'_' => Js::Undef,
            b'0' => Js::Null,
            b'T' => Js::Bool(true),
            b'F' => Js::Bool(false),
            b'I' => {
                let z = self.varint()?;
                Js::Num(((z >> 1) as i32 ^ -((z & 1) as i32)) as f64)
            }
            b'U' => Js::Num(self.varint()? as f64),
            b'N' => Js::Num(f64::from_le_bytes(self.bytes(8)?.try_into().ok()?)),
            b'"' | b'c' => Js::Str(self.string_body()?),
            b'o' => {
                let mut props: Vec<(Cow<'a, str>, Js<'a>)> = Vec::new();
                while self.peek_tag()? != b'{' {
                    // string keys only: number keys and array-index strings keep JS key order out of this path
                    let Js::Str(key) = self.value(depth + 1)? else { return None };
                    if key == "__proto__" || engine::doc::array_index(&key).is_some() {
                        return None;
                    }
                    let v = self.value(depth + 1)?;
                    props.push((key, v));
                }
                self.tag()?;
                if self.varint()? as usize != props.len() || has_repeats(&props) {
                    return None;
                }
                Js::Obj(props)
            }
            b'A' => {
                let len = self.varint()? as usize;
                if len > self.b.len() - self.pos {
                    return None;
                }
                let mut items = Vec::with_capacity(len);
                for _ in 0..len {
                    // kTheHole and every other non-subset tag end here
                    items.push(self.value(depth + 1)?);
                }
                // no named properties on arrays
                if self.tag()? != b'$' || self.varint()? != 0 || self.varint()? as usize != len {
                    return None;
                }
                Js::Arr(items)
            }
            // a holey-kind array, written as index → value pairs (index keys are numbers, kKeepNumbers): in the
            // subset only when every index below its length is there, in order, and nothing else
            b'a' => {
                let len = self.varint()? as usize;
                let mut items = Vec::new();
                while self.peek_tag()? != b'@' {
                    let Js::Num(k) = self.value(depth + 1)? else { return None };
                    if k != items.len() as f64 {
                        return None;
                    }
                    items.push(self.value(depth + 1)?);
                }
                self.tag()?;
                if items.len() != len || self.varint()? as usize != len || self.varint()? as usize != len {
                    return None;
                }
                Js::Arr(items)
            }
            _ => return None,
        })
    }

    // ReadOneByteString / ReadTwoByteString (the tag already read); a lone surrogate is not handled
    fn string_body(&mut self) -> Option<Cow<'a, str>> {
        let tag = self.b[self.pos - 1];
        let n = self.varint()? as usize;
        let raw = self.bytes(n)?;
        if tag == b'"' {
            // Latin-1; visuals are long and ASCII, and ASCII is its own UTF-8
            return Some(if raw.is_ascii() { Cow::Borrowed(std::str::from_utf8(raw).ok()?) } else { Cow::Owned(raw.iter().map(|&c| c as char).collect()) });
        }
        if n % 2 != 0 {
            return None;
        }
        String::from_utf16(&raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).ok().map(Cow::Owned)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::RunMeter;
    use crate::Meter;
    use crate::natives::{RunCtx, RunGuard};
    use crate::WorldAccess;
    use engine::world::{RoomIdx, UserIdx};

    fn serialize(src: &str) -> Vec<u8> {
        crate::init_v8();
        let mut iso = v8::Isolate::new(Default::default());
        v8::scope!(let scope, &mut iso);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let code = v8::String::new(scope, src).unwrap();
        let value = v8::Script::compile(scope, code, None).unwrap().run(scope).unwrap();
        crate::marshal::serialize_for_tests(scope, value)
    }

    /// obj<n> is in room W<n>N1; nothing else exists.
    fn room_of(id: &str) -> Option<String> {
        id.strip_prefix("obj")?.parse::<u32>().ok().map(|n| format!("W{n}N1"))
    }

    fn native(src: &str) -> Option<String> {
        store(&serialize(src), room_of).map(|j| j.stringify())
    }

    /// The world the system isolate's objectRoom native sees, as room_of.
    struct Rooms(Vec<String>);

    impl WorldAccess for Rooms {
        fn room_by_name(&self, _: &str) -> Option<RoomIdx> {
            unimplemented!()
        }
        fn room_by_xy(&self, _: i32, _: i32) -> Option<RoomIdx> {
            unimplemented!()
        }
        fn room_name(&self, r: RoomIdx) -> &str {
            &self.0[r as usize]
        }
        fn terrain(&self, _: RoomIdx) -> &[u8; 2500] {
            unimplemented!()
        }
        fn exit_counts(&self, _: RoomIdx) -> [u8; 4] {
            unimplemented!()
        }
        fn accessible(&self, _: RoomIdx) -> bool {
            unimplemented!()
        }
        fn status(&self, _: RoomIdx) -> Option<(&'static str, Option<i64>)> {
            unimplemented!()
        }
        fn world_size(&self) -> u32 {
            unimplemented!()
        }
        fn terrain_heap_mb(&self) -> f64 {
            unimplemented!()
        }
        fn user_id(&self, _: UserIdx) -> &str {
            unimplemented!()
        }
        fn object_room(&self, id: &str, _: UserIdx, _: &[RoomIdx]) -> Option<RoomIdx> {
            id.strip_prefix("obj")?.parse::<RoomIdx>().ok().filter(|&n| (n as usize) < self.0.len())
        }
        fn parse_intents(&self, _: &Json) -> engine::intents::UserIntents {
            unimplemented!()
        }
        fn market(&self) -> &std::sync::Arc<engine::world::MarketState> {
            unimplemented!()
        }
    }

    /// host/system.js on the same bytes.
    fn system_store(rt: &crate::Runtime, bytes: &[u8]) -> Result<String, String> {
        let world = Rooms((0..64).map(|n| format!("W{n}N1")).collect());
        let guard = RunGuard::enter(RunCtx::new(&world, 0, Vec::new(), 0, 0, RunMeter::start(Meter::Thread, 0.0), Meter::Thread.deadline_clock()));
        let r = crate::host::store(rt, bytes, "user1").map(|s| Json::parse(&s).unwrap().stringify());
        drop(guard);
        r
    }

    fn runtime() -> crate::Runtime {
        crate::init_v8();
        let cfg = crate::RuntimeConfig {
            runner_threads: 1,
            heap_mb: 256,
            semi_space_mb: 0,
            setup_list: false,
            heap_budget_mb: 0,
            meter: Meter::Thread,
            hard_timeout_ms: 5000,
            idle_evict_s: 180,
            watchdog_poll_us: 500,
            profile: engine::policy::Profile::Private,
            shard_name: "shard0".into(),
        };
        crate::Runtime::new(cfg, crate::Bundle { files: crate::bundle::FILES, mod_game_js: Vec::new() })
    }

    #[test]
    fn reads_the_subset() {
        let j = native(
            r#"({type: 'done', error: undefined, usedTime: 3, usedCleanTime: 2.5, usedDirtyTime: 4,
                intentsList: {obj1: {move: {direction: 3}, say: {message: 'hi', isPublic: true}},
                              notify: [{message: 'x', groupInterval: 5}],
                              room: {createFlag: [{roomName: 'W2N2', x: 1, y: 2, name: 'f', color: 1, secondaryColor: 1}]},
                              nowhere: {move: {direction: 1}}},
                console: {log: ['a', 'é😀'], results: []}, visual: {}, activeSegments: [1, NaN, -0]})"#,
        )
        .unwrap();
        assert_eq!(
            j,
            r#"{"type":"done","error":null,"usedTime":3,"usedCleanTime":2.5,"usedDirtyTime":4,"intents":{"W1N1":{"obj1":{"move":{"id":"undefined","direction":3},"say":{"message":"hi","isPublic":true}}},"notify":[{"message":"x","groupInterval":5}],"W2N2":{"room":{"createFlag":[{"roomName":"W2N2","x":1,"y":2,"name":"f","color":1,"secondaryColor":1}]}}},"count":4,"console":{"log":["a","é😀"],"results":[]},"visual":{},"activeSegments":[1,null,0]}"#
        );
        // a holey-kind array without holes
        assert!(native("(() => { const a = new Array(2); a[0] = 'm'; a[1] = 'n'; return {intentsList: {}, console: {log: a}}; })()").is_some());
    }

    #[test]
    fn leaves_the_rest_to_the_system_isolate() {
        for src in [
            "({intentsList: {obj1: {move: {direction: new Date(0)}}}})",
            "({intentsList: {}, console: new Map()})",
            "(() => { const a = {}; return {intentsList: {obj1: {say: a}, obj2: {say: a}}}; })()",
            "({intentsList: {obj1: {say: {message: [1,,2]}}}})",
            "({intentsList: {obj1: {say: {message: 10n}}}})",
            "({intentsList: {obj1: undefined}})",
            "({intentsList: {constructor: {move: {}}}})",
            "({intentsList: {room: {createFlag: [{roomName: '__proto__'}]}}})",
            "({intentsList: {room: {createFlag: [{roomName: '7'}]}}})",
            "({intentsList: {obj1: null}})",
            "({intentsList: {}, visual: {'5': 'x'}})",
            "({intentsList: {}, visual: '\\ud800'})",
        ] {
            assert_eq!(native(src), None, "{src}");
        }
    }

    /// Everything a bot picks the size of goes through here after its CPU has stopped. Each of these took minutes at this
    /// size when the decoder or the room groups scanned, so a minute each tells the two apart on any machine.
    #[test]
    fn large_outputs_take_linear_time() {
        let n = 200_000;
        let object = format!("(() => {{ const o = {{}}; for (let i = 0; i < {n}; i++) o['k' + i] = 1; return {{intentsList: {{}}, activeForeignSegment: {{username: o}}}}; }})()");
        let visuals = format!("(() => {{ const o = {{}}; for (let i = 0; i < {n}; i++) o['W' + i + 'N1'] = 'x'; return {{intentsList: {{}}, visual: o}}; }})()");
        let groups = format!("({{intentsList: {{room: {{createFlag: Array.from({{length: {n}}}, (_, i) => ({{roomName: 'x' + i, x: 1, y: 1, name: 'f', color: 1, secondaryColor: 1}}))}}}}}})");
        for src in [object, visuals, groups] {
            let bytes = serialize(&src);
            let t = std::time::Instant::now();
            assert!(store(&bytes, room_of).is_some());
            assert!(t.elapsed().as_secs_f64() < 60.0, "{:?}", t.elapsed());
        }
    }

    /// Past `SCAN` the index must give what the scan gives, order and repeats included.
    #[test]
    fn large_maps_keep_their_order() {
        let mut m = Map::default();
        for i in 0..100 {
            assign(&mut m, &format!("k{i}"), Node::Json(Json::Num(i as f64)));
        }
        assign(&mut m, "k3", Node::Json(Json::Num(-1.0)));
        assign(map_at(&mut m, "inner").unwrap(), "a", Node::Json(Json::Bool(true)));
        assert!(map_at(&mut m, "k4").is_none(), "not a map");
        assign(map_at(&mut m, "inner").unwrap(), "b", Node::Json(Json::Bool(false)));
        let j = node_json(Node::Map(m)).unwrap();
        let o = j.as_object().unwrap();
        assert_eq!(o.len(), 101);
        assert_eq!(o[0].0, "k0");
        assert_eq!(j.get("k3").and_then(Json::as_f64), Some(-1.0));
        assert_eq!(o[100].0, "inner");
        assert_eq!(o[100].1.stringify(), r#"{"a":true,"b":false}"#);
    }

    #[test]
    fn repeated_keys_are_found_at_any_size() {
        let keys = |n: usize| (0..n).map(|i| (Cow::Owned(format!("k{i}")), Js::Null)).collect::<Vec<_>>();
        for n in [2, SCAN, SCAN + 1, 1000] {
            assert!(!has_repeats(&keys(n)), "{n}");
            let mut repeated = keys(n);
            repeated.push(("k1".into(), Js::Null));
            assert!(has_repeats(&repeated), "{n}");
        }
    }

    /// The same outMessages through host/system.js: every one the native path takes gives the same Json.
    #[test]
    fn matches_the_system_isolate() {
        let values = [
            "undefined", "null", "true", "false", "0", "-0", "1.5", "-7", "1e21", "5e-7", "NaN", "Infinity", "''",
            "'abc'", "' 42px'", "'0x1F'", "'W1N1'", "'move'", "'é😀'", "[]", "[1, '2', null]", "['move', 'work', 'x']",
            "{}", "{length: 2, 0: 'move', 1: 'carry'}", "[[1, 2], 'a']", "'x'.repeat(150)", "0.0005", "1.0005", "'-0.5e1'",
            "[undefined]", "{a: undefined}", "'\\u00a0 12'", "2.5", "-2.5", "123456789012345678901234",
        ];
        let mut cases = Vec::new();
        for (i, v) in values.iter().enumerate() {
            let w = values[(i * 7 + 3) % values.len()];
            let j = (i + 1) % 64;
            cases.push(format!(
                "({{type: 'done', usedTime: {i}, intentsList: {{obj{i}: {{move: {{direction: {v}}}, say: {{message: {v}, isPublic: {w}}}, \
                 transfer: {{id: {w}, amount: {v}, resourceType: {v}}}, createCreep: {{name: {v}, body: {v}, energyStructures: {w}, directions: {v}}}}}, \
                 notify: [{{message: {v}, groupInterval: {w}}}], global: {{createOrder: {{type: {v}, resourceType: {w}, price: {v}, totalAmount: {w}, roomName: {v}}}}}, \
                 room: {{createConstructionSite: [{{roomName: {v}, x: {w}, y: {v}, structureType: {w}, name: {v}}}], removeFlag: [{{roomName: 'W9N9', name: {v}}}]}}, \
                 missing{i}: {{say: {{}}}}, obj{j}: {{harvest: {{id: {v}}}}}}}, console: {{log: [{v}], results: [{w}]}}, activeSegments: {v}, publicSegments: {w}, \
                 defaultPublicSegment: {v}, visual: {{W1N1: {w}}}}})"
            ));
        }
        let rt = runtime();
        let mut native_taken = 0;
        for src in &cases {
            let bytes = serialize(src);
            let Some(n) = store(&bytes, room_of) else { continue };
            native_taken += 1;
            assert_eq!(Ok(n.stringify()), system_store(&rt, &bytes), "{src}");
        }
        assert!(native_taken * 3 > cases.len() * 2, "only {native_taken} of {} took the native path", cases.len());
    }
}
