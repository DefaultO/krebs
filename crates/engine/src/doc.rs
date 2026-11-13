//! Persisted values (ARCHITECTURE §3.1): vanilla DB documents verbatim, as ordered
//! objects with interned property names.

use std::cell::RefCell;
use std::collections::hash_map::Entry as MapEntry;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use util::json::Json;

/// Interned property name (process-local, never persisted). Only for property names:
/// never intern ids or user-controlled map keys. `atom!("store")` caches the Atom per
/// call site.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Atom(u32);

struct Entry {
    s: &'static str,
    index: Option<u32>,
    /// `"name":` as a document's JSON has it, made the first time a document with this key is written.
    key: OnceLock<Option<&'static str>>,
}

/// Longer names are written out each time: a name from outside can be `OUTSIDE_NAME_BYTES` long, and this second copy
/// is never freed either.
const KEY_BYTES: usize = 48;

/// Append-only arena: chunk k holds 64 << k entries, so slots never move and
/// `as_str` reads them without a lock.
const CHUNKS: usize = 26;
static ARENA: [OnceLock<Box<[OnceLock<Entry>]>>; CHUNKS] = [const { OnceLock::new() }; CHUNKS];
static NAMES: Mutex<Option<HashMap<&'static str, u32>>> = Mutex::new(None);

/// What `Val::from_json` may add to the table, which is never freed. Its input can be a request body or a bot's output,
/// so the names in it are the sender's to choose, and without a limit one client could grow the table for as long as it
/// liked. The most this leaves behind is a few tens of MB. Names already in the table are not counted.
const OUTSIDE_NAMES: u32 = 1 << 17;
pub const OUTSIDE_NAME_BYTES: usize = 256;
static OUTSIDE_ADDED: AtomicU32 = AtomicU32::new(0);

/// How many of those limits' names have been used.
pub fn outside_names_added() -> u32 {
    OUTSIDE_ADDED.load(Relaxed)
}

thread_local! {
    /// Lock-free hit path for `intern`: each thread remembers the atoms it has seen (FxHash: property names only).
    static SEEN: RefCell<util::hash::FxHashMap<&'static str, Atom>> = RefCell::new(HashMap::default());
}

fn slot(i: u32) -> (usize, usize) {
    let k = 31 - (i / 64 + 1).leading_zeros();
    (k as usize, (i - 64 * ((1 << k) - 1)) as usize)
}

impl Atom {
    pub fn intern(s: &str) -> Atom {
        Atom::get(s, false).expect("only an outside name is refused")
    }

    /// `intern` for a name from outside the server: None when it is new and past the limits above.
    fn intern_outside(s: &str) -> Option<Atom> {
        Atom::get(s, true)
    }

    fn get(s: &str, outside: bool) -> Option<Atom> {
        if let Some(a) = SEEN.with(|m| m.borrow().get(s).copied()) {
            return Some(a);
        }
        let a = {
            let mut g = NAMES.lock().unwrap_or_else(|e| e.into_inner());
            let names = g.get_or_insert_with(HashMap::new);
            match names.get(s) {
                Some(&i) => Atom(i),
                None => {
                    if outside {
                        // Read and written under the lock.
                        let added = OUTSIDE_ADDED.load(Relaxed);
                        if s.len() > OUTSIDE_NAME_BYTES || added >= OUTSIDE_NAMES {
                            return None;
                        }
                        OUTSIDE_ADDED.store(added + 1, Relaxed);
                    }
                    let i = names.len() as u32;
                    let leaked: &'static str = Box::leak(s.into());
                    let (k, off) = slot(i);
                    let chunk = ARENA[k].get_or_init(|| (0..64usize << k).map(|_| OnceLock::new()).collect());
                    let _ = chunk[off].set(Entry { s: leaked, index: array_index(leaked), key: OnceLock::new() });
                    names.insert(leaked, i);
                    Atom(i)
                }
            }
        };
        SEEN.with(|m| m.borrow_mut().insert(a.as_str(), a));
        Some(a)
    }

    fn entry(self) -> &'static Entry {
        let (k, off) = slot(self.0);
        ARENA[k].get().and_then(|c| c[off].get()).expect("atom from this process")
    }

    pub fn as_str(self) -> &'static str {
        self.entry().s
    }

    /// Canonical array index ⇒ enumerated first (JS key order).
    pub fn array_index(self) -> Option<u32> {
        self.entry().index
    }

    /// `"name":`, the key as a document's JSON has it with its colon, for a short name that needs no escape. Kept per
    /// atom: a document's keys are the same few names over and over, and quoting them each time was a good part of
    /// writing it.
    fn json_key(self) -> Option<&'static str> {
        let e = self.entry();
        *e.key.get_or_init(|| {
            (e.s.len() <= KEY_BYTES && plain(e.s.as_bytes())).then(|| {
                let mut k = String::with_capacity(e.s.len() + 3);
                k.push('"');
                k.push_str(e.s);
                k.push_str("\":");
                &*Box::leak(k.into_boxed_str())
            })
        })
    }
}

impl fmt::Debug for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[macro_export]
macro_rules! atom {
    ($s:literal) => {{
        static A: std::sync::OnceLock<$crate::doc::Atom> = std::sync::OnceLock::new();
        *A.get_or_init(|| $crate::doc::Atom::intern($s))
    }};
}

/// Canonical array index ("0".."4294967294"), ECMA-262 6.1.7.
pub fn array_index(k: &str) -> Option<u32> {
    let b = k.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) || (b[0] == b'0' && b.len() > 1) {
        return None;
    }
    let v: u64 = k.parse().ok()?;
    (v < u32::MAX as u64).then_some(v as u32)
}

/// Where a new key goes in JS own-property order: array-index keys ascending first,
/// then everything else in insertion order.
pub(crate) fn js_insert_pos<T>(v: &[(Atom, T)], k: Atom) -> usize {
    match k.array_index() {
        None => v.len(),
        Some(i) => v.iter().position(|(a, _)| a.array_index().is_none_or(|j| j > i)).unwrap_or(v.len()),
    }
}

/// Object / user / order id: ASCII, ≤ 24 bytes, inline, Copy.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id {
    len: u8,
    b: [u8; 24],
}

impl Id {
    pub const EMPTY: Id = Id { len: 0, b: [0; 24] };

    pub fn new(s: &str) -> Option<Id> {
        if s.len() > 24 || !s.is_ascii() {
            return None;
        }
        let mut b = [0; 24];
        b[..s.len()].copy_from_slice(s.as_bytes());
        Some(Id { len: s.len() as u8, b })
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.b[..self.len as usize]).expect("ids are ASCII")
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the Loki JSON transport can carry. NaN/±Inf never appear (they persist as null).
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Num(f64),
    Str(Arc<str>),
    Arr(Vec<Val>),
    Obj(Obj),
}

/// Plain object in JS own-property order: array-index keys ascending, then insertion order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Obj(pub(crate) Vec<(Atom, Val)>);

impl Obj {
    pub fn new() -> Obj {
        Obj(Vec::new())
    }

    pub fn get(&self, k: Atom) -> Option<&Val> {
        self.0.iter().find(|(a, _)| *a == k).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, k: Atom) -> Option<&mut Val> {
        self.0.iter_mut().find(|(a, _)| *a == k).map(|(_, v)| v)
    }

    /// `o[k] = v`: an existing key keeps its position.
    pub fn set(&mut self, k: Atom, v: Val) {
        match self.0.iter_mut().find(|(a, _)| *a == k) {
            Some(slot) => slot.1 = v,
            None => {
                let at = js_insert_pos(&self.0, k);
                self.0.insert(at, (k, v));
            }
        }
    }

    pub fn remove(&mut self, k: Atom) -> Option<Val> {
        let i = self.0.iter().position(|(a, _)| *a == k)?;
        Some(self.0.remove(i).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Atom, &Val)> {
        self.0.iter().map(|(a, v)| (*a, v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Linear in the keys: `set` scans the object, which a body of a million keys turns into minutes on the tick thread.
    fn from_json(m: &[(String, Json)], name: fn(&str) -> Option<Atom>) -> Obj {
        let mut o = Obj(Vec::with_capacity(m.len()));
        let named = m.iter().filter_map(|(k, v)| Some((name(k)?, v)));
        // The usual document: scanning a few keys beats hashing them, and this runs for every intent.
        if m.len() <= 16 {
            for (k, v) in named {
                o.set(k, Val::convert(v, name));
            }
            return o;
        }
        let mut slots: HashMap<Atom, usize> = HashMap::with_capacity(m.len());
        for (k, v) in named {
            match slots.entry(k) {
                MapEntry::Occupied(slot) => o.0[*slot.get()].1 = Val::convert(v, name),
                MapEntry::Vacant(slot) => {
                    slot.insert(o.0.len());
                    o.0.push((k, Val::convert(v, name)));
                }
            }
        }
        // Stable: array-index keys ascending, then the rest in insertion order, as `set` leaves them.
        o.0.sort_by_key(|(a, _)| a.array_index().map_or(u64::MAX, u64::from));
        o
    }
}

/// JS truthiness of a JSON value, for a request field that is only tested: no `Val`, so nothing is interned.
pub fn json_truthy(j: &Json) -> bool {
    match j {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Num(n) => *n != 0.0 && !n.is_nan(),
        Json::Str(s) => !s.is_empty(),
        Json::Arr(_) | Json::Obj(_) => true,
    }
}

impl Val {
    /// Duplicate keys and array-index keys follow JS order. Keys are interned, but this reads request bodies and bot
    /// output too, so past the limits on outside names a new key is left out. A caller that must keep every key
    /// (module names) compares the sizes and refuses.
    pub fn from_json(j: &Json) -> Val {
        Val::convert(j, Atom::intern_outside)
    }

    /// `from_json` for text whose names the server or its host chose: a file it loads, its own constants, the host's
    /// console, intents once sanitized. Those keep every key, however many names an outsider has spent.
    pub fn from_trusted_json(j: &Json) -> Val {
        Val::convert(j, |k| Some(Atom::intern(k)))
    }

    fn convert(j: &Json, name: fn(&str) -> Option<Atom>) -> Val {
        match j {
            Json::Null => Val::Null,
            Json::Bool(b) => Val::Bool(*b),
            Json::Num(n) => Val::Num(*n),
            Json::Str(s) => Val::Str(s.as_str().into()),
            Json::Arr(a) => Val::Arr(a.iter().map(|v| Val::convert(v, name)).collect()),
            Json::Obj(m) => Val::Obj(Obj::from_json(m, name)),
        }
    }

    pub fn to_json(&self) -> Json {
        match self {
            Val::Null => Json::Null,
            Val::Bool(b) => Json::Bool(*b),
            Val::Num(n) => Json::Num(*n),
            Val::Str(s) => Json::Str(s.to_string()),
            Val::Arr(a) => Json::Arr(a.iter().map(Val::to_json).collect()),
            Val::Obj(o) => Json::Obj(o.iter().map(|(k, v)| (k.as_str().to_string(), v.to_json())).collect()),
        }
    }

    /// JSON.stringify-exact (numbers via util::num), but with non-ASCII escaped as
    /// `\uXXXX` so the text is ASCII (a one-byte V8 string).
    pub fn write_json(&self, out: &mut String) {
        write_val(out, self, true);
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Val::Num(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_obj(&self) -> Option<&Obj> {
        match self {
            Val::Obj(o) => Some(o),
            _ => None,
        }
    }
}

pub(crate) fn write_val(out: &mut String, v: &Val, ascii: bool) {
    match v {
        Val::Null => out.push_str("null"),
        Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Val::Num(n) if n.is_finite() => util::num::write_js_string(out, *n),
        Val::Num(_) => out.push_str("null"),
        Val::Str(s) => write_str(out, s, ascii),
        Val::Arr(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_val(out, x, ascii);
            }
            out.push(']');
        }
        Val::Obj(o) => write_obj(out, o, ascii),
    }
}

fn write_obj(out: &mut String, o: &Obj, ascii: bool) {
    out.push('{');
    for (i, (k, x)) in o.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match k.json_key() {
            Some(key) => out.push_str(key),
            None => {
                write_str(out, k.as_str(), ascii);
                out.push(':');
            }
        }
        write_val(out, x, ascii);
    }
    out.push('}');
}

/// Printable ASCII with no quote or backslash: JSON quotes it as it stands, in either mode. Ids, names and types all are.
fn plain(b: &[u8]) -> bool {
    b.iter().all(|&c| (0x20..0x80).contains(&c) && c != b'"' && c != b'\\')
}

pub(crate) fn write_str(out: &mut String, s: &str, ascii: bool) {
    if plain(s.as_bytes()) {
        out.reserve(s.len() + 2);
        out.push('"');
        out.push_str(s);
        out.push('"');
        return;
    }
    if !ascii || s.is_ascii() {
        return util::json::write_quoted(out, s);
    }
    // write_quoted only adds ASCII escapes, so the non-ASCII left over is the input's.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut q = String::new();
    util::json::write_quoted(&mut q, s);
    for c in q.chars() {
        if c.is_ascii() {
            out.push(c);
            continue;
        }
        for u in c.encode_utf16(&mut [0u16; 2]) {
            out.push_str("\\u");
            for shift in [12, 8, 4, 0] {
                out.push(HEX[(*u >> shift) as usize & 15] as char);
            }
        }
    }
}

/// One persisted document (rooms.objects, users.power_creeps, market.orders, …).
/// Immutable once shared; mutate an unshared one through `obj_mut`.
pub struct Doc {
    pub seq: u64,
    pub obj: Obj,
    json: OnceLock<Box<str>>,
}

pub type DocRef = Arc<Doc>;

impl Doc {
    pub fn new(seq: u64, obj: Obj) -> DocRef {
        Arc::new(Doc { seq, obj, json: OnceLock::new() })
    }

    /// Cached ASCII `write_json` of `obj`. Written into a buffer the thread keeps, so the text is allocated once, at its
    /// size: a creep's 700 bytes used to grow a new string ten times over.
    pub fn json(&self) -> &str {
        thread_local!(static TEXT: RefCell<String> = const { RefCell::new(String::new()) });
        self.json.get_or_init(|| {
            TEXT.with(|t| {
                let mut s = t.borrow_mut();
                s.clear();
                write_obj(&mut s, &self.obj, true);
                let text = Box::from(s.as_str());
                if s.capacity() > 1 << 20 {
                    *s = String::new();
                }
                text
            })
        })
    }

    /// `obj._id` (empty when missing or not a valid id).
    pub fn id(&self) -> Id {
        self.obj.get(crate::atom!("_id")).and_then(Val::as_str).and_then(Id::new).unwrap_or(Id::EMPTY)
    }

    /// `obj.type` ("" when missing).
    pub fn kind(&self) -> &str {
        self.obj.get(crate::atom!("type")).and_then(Val::as_str).unwrap_or("")
    }

    /// Mutable access for copy-on-write (`Arc::make_mut`); drops the cached JSON.
    pub fn obj_mut(&mut self) -> &mut Obj {
        self.json = OnceLock::new();
        &mut self.obj
    }
}

impl Clone for Doc {
    fn clone(&self) -> Doc {
        Doc { seq: self.seq, obj: self.obj.clone(), json: OnceLock::new() }
    }
}

impl fmt::Debug for Doc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Doc#{} {}", self.seq, self.json())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(s: &str) -> Val {
        Val::from_json(&Json::parse(s).unwrap())
    }

    fn json(v: &Val) -> String {
        let mut s = String::new();
        v.write_json(&mut s);
        s
    }

    #[test]
    fn atoms() {
        let a = Atom::intern("store");
        assert_eq!(a, Atom::intern("store"));
        assert_eq!(a, atom!("store"));
        assert_ne!(a, Atom::intern("Store"));
        assert_eq!(a.as_str(), "store");
        assert_eq!(Atom::intern("7").array_index(), Some(7));
        assert_eq!(Atom::intern("07").array_index(), None);
        assert_eq!(Atom::intern("4294967294").array_index(), Some(4294967294));
        assert_eq!(Atom::intern("4294967295").array_index(), None);
        // Many atoms across threads: chunk boundaries and the cross-thread table.
        let names: Vec<String> = (0..5000).map(|i| format!("k{i}")).collect();
        let ids: Vec<Atom> = names.iter().map(|n| Atom::intern(n)).collect();
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for (n, &a) in names.iter().zip(&ids) {
                        assert_eq!(Atom::intern(n), a);
                        assert_eq!(a.as_str(), n);
                    }
                });
            }
        });
    }

    #[test]
    fn ids() {
        let id = Id::new("5f3a0c1d2e4b6a7").unwrap();
        assert_eq!(id.as_str(), "5f3a0c1d2e4b6a7");
        assert_eq!(format!("{id:?}"), "\"5f3a0c1d2e4b6a7\"");
        assert!(Id::new(&"a".repeat(24)).is_some());
        assert!(Id::new(&"a".repeat(25)).is_none());
        assert!(Id::new("é").is_none());
        assert_eq!(Id::new("").unwrap(), Id::EMPTY);
    }

    #[test]
    fn js_key_order() {
        let v = val(r#"{"b":1,"2":2,"a":3,"1":4,"b":5}"#);
        assert_eq!(json(&v), r#"{"1":4,"2":2,"b":5,"a":3}"#);
        let Val::Obj(mut o) = v else { panic!() };
        o.set(Atom::intern("0"), Val::Null);
        o.set(Atom::intern("10"), Val::Null);
        o.set(Atom::intern("c"), Val::Null);
        o.set(Atom::intern("b"), Val::Bool(true));
        assert_eq!(json(&Val::Obj(o.clone())), r#"{"0":null,"1":4,"2":2,"10":null,"b":true,"a":3,"c":null}"#);
        assert_eq!(o.remove(Atom::intern("1")), Some(Val::Num(4.0)));
        assert_eq!(o.len(), 6);
    }

    /// A large object keeps `set`'s order and its last value per key, and takes linear time: 200 000 keys took minutes
    /// through `set`. The keys are interned first: tests share the limit on outside names (tests/outside_names.rs has that).
    /// The bound is a minute, which a debug build on a busy build machine keeps and the scans did not.
    #[test]
    fn large_objects_are_built_in_linear_time() {
        let mut m: Vec<(String, Json)> = (0..200_000).map(|i| (format!("big{i}"), Json::Num(i as f64))).collect();
        for (k, _) in &m {
            Atom::intern(k);
        }
        m.push(("big7".into(), Json::Str("last".into())));
        m.push(("3".into(), Json::Bool(true)));
        m.push(("1".into(), Json::Null));
        Atom::intern("3");
        Atom::intern("1");
        let start = std::time::Instant::now();
        let Val::Obj(o) = Val::from_json(&Json::Obj(m)) else { panic!() };
        assert!(start.elapsed().as_secs() < 60, "{:?}", start.elapsed());
        assert_eq!(o.len(), 200_002);
        let keys: Vec<&str> = o.iter().take(3).map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["1", "3", "big0"]);
        assert_eq!(o.get(atom!("big7")).and_then(Val::as_str), Some("last"));
    }

    #[test]
    fn long_keys_are_not_interned() {
        let long = "k".repeat(OUTSIDE_NAME_BYTES + 1);
        let Val::Obj(o) = Val::from_json(&Json::Obj(vec![(long.clone(), Json::Null), ("x".into(), Json::Null)])) else { panic!() };
        assert_eq!(o.len(), 1);
        assert!(NAMES.lock().unwrap().as_ref().unwrap().get(long.as_str()).is_none());
    }

    #[test]
    fn json_truthiness() {
        for (j, want) in [("null", false), ("0", false), ("\"\"", false), ("false", false), ("1", true), ("\"0\"", true), ("[]", true), ("{}", true)] {
            assert_eq!(json_truthy(&Json::parse(j).unwrap()), want, "{j}");
        }
    }

    #[test]
    fn write_json_ascii() {
        // node: JSON.stringify gives the raw text; ours escapes non-ASCII per UTF-16 unit.
        let v = val(r#"{"név":"é€😀\u0001\"\\\n","n":[1.5,-0,1e21,1e-7,123456789012345680000]}"#);
        let escaped =
            ["{\"n", "u00e9v\":\"", "u00e9", "u20ac", "ud83d", "ude00", "u0001\\\"\\\\\\n\",\"n\":[1.5,0,1e+21,1e-7,123456789012345680000]}"].join("\\");
        assert_eq!(json(&v), escaped);
        let mut raw = String::new();
        write_val(&mut raw, &v, false);
        assert_eq!(raw, r#"{"név":"é€😀\u0001\"\\\n","n":[1.5,0,1e+21,1e-7,123456789012345680000]}"#);
        assert_eq!(Val::from_json(&Json::parse(&json(&v)).unwrap()), v);
        assert_eq!(v.to_json().stringify(), raw);
    }

    #[test]
    fn docs() {
        let Val::Obj(o) = val(r#"{"_id":"abc","type":"creep","x":1}"#) else { panic!() };
        let mut d = Doc::new(7, o);
        assert_eq!(d.json(), r#"{"_id":"abc","type":"creep","x":1}"#);
        assert_eq!(d.id().as_str(), "abc");
        assert_eq!(d.kind(), "creep");
        let shared = d.clone();
        Arc::make_mut(&mut d).obj_mut().set(atom!("x"), Val::Num(2.0));
        assert_eq!(d.json(), r#"{"_id":"abc","type":"creep","x":2}"#);
        assert_eq!(shared.json(), r#"{"_id":"abc","type":"creep","x":1}"#);
        assert_eq!(Doc::new(0, Obj::new()).id(), Id::EMPTY);
    }
}
