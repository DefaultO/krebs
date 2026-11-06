//! Live heap (ARCHITECTURE §3.2): one JS-like object graph per room tick or global
//! stage, `!Send` by design. JS identity is `Rc` identity. The lodash 3.10.1 functions
//! the processor relies on are ported statement by statement for JSON-shaped data
//! (plain objects, arrays, primitives); there are no functions, Dates, typed arrays,
//! `arguments` objects or non-index properties on arrays (only `length` is modelled).

use crate::atom;
use crate::doc::{js_insert_pos, Atom, DocRef, Obj, Val};
use std::borrow::Cow;
use std::cell::{OnceCell, Ref, RefCell, RefMut};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub enum V {
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(Arc<str>),
    Obj(O),
    Arr(A),
}

/// Plain object in JS own-property order (array-index keys first, ascending).
#[derive(Clone, Default)]
pub struct O(Rc<Node>);

#[derive(Default)]
struct Node {
    props: RefCell<Props>,
    /// Set once a `RoomObjects` has this object in its tile index, which then hears of writes to `x` and `y`.
    watch: OnceCell<Watch>,
}

struct Watch {
    moved: Rc<RefCell<Vec<u32>>>,
    slot: u32,
}

/// A room object starts as its stored doc (`from_doc`): primitive reads come straight off it. What is written, and
/// each nested object or array when first read (which gives it its identity), goes to the list beside the doc, in
/// the order it happened. Only what needs all the keys (`delete` of one, `keys`, JSON) copies the object into live
/// values, once, inside the same `Rc`. Most docs of a room tick (roads, walls, ramparts, sources) are only ever
/// read, and a creep's tick touches a few of its keys.
enum Props {
    Doc(DocRef, Vec<(Atom, V)>),
    /// An object inside a stored doc, read the same way: `doc.obj[key]`, or element `item` of the array there. The
    /// copy of a doc makes these for its objects, and so for every part of a creep's body.
    Sub(DocRef, u32, u32),
    /// A stored object outside the room's docs (a user, the room's info, a flag).
    Shared(Arc<Obj>),
    Live(Vec<(Atom, V)>),
}

impl Default for Props {
    fn default() -> Props {
        Props::Live(Vec::new())
    }
}

impl Props {
    /// What the list beside a doc has for `k`.
    #[inline(always)]
    fn beside(&self, k: Atom) -> Option<&V> {
        match self {
            Props::Doc(_, beside) if !beside.is_empty() => find(beside, k),
            _ => None,
        }
    }

    /// Key number `i` and its value, when that is a primitive; `Doc` only with nothing beside it.
    fn item(&self, i: usize) -> Option<(Atom, V)> {
        match self {
            Props::Live(m) => m.get(i).filter(|(_, v)| !is_object(v)).map(|(k, v)| (*k, v.clone())),
            stored => stored.stored().0.get(i).and_then(|(k, v)| Some((*k, primitive(Some(v))?))),
        }
    }

    /// The stored object of a `Doc` or `Sub`.
    #[inline(always)]
    fn stored(&self) -> &Obj {
        match self {
            Props::Doc(d, _) => &d.obj,
            other => other.stored_sub(),
        }
    }

    fn stored_sub(&self) -> &Obj {
        let (d, key, item) = match self {
            Props::Sub(d, key, item) => (d, key, item),
            Props::Shared(o) => return o,
            _ => unreachable!("only stored objects"),
        };
        match &d.obj.0[*key as usize].1 {
            Val::Arr(a) => match &a[*item as usize] {
                Val::Obj(o) => o,
                _ => unreachable!("made for an object"),
            },
            Val::Obj(o) => o,
            _ => unreachable!("made for an object"),
        }
    }
}

/// A stored value as a live one, when that makes no object (primitives and absence: `None` for an object or array).
#[inline]
fn primitive(v: Option<&Val>) -> Option<V> {
    Some(match v {
        None => V::Undef,
        Some(Val::Null) => V::Null,
        Some(Val::Bool(b)) => V::Bool(*b),
        Some(Val::Num(n)) => V::Num(*n),
        Some(Val::Str(s)) => V::Str(s.clone()),
        Some(Val::Arr(_) | Val::Obj(_)) => return None,
    })
}

#[derive(Clone, Default)]
pub struct A(Rc<RefCell<Vec<V>>>);

/// A vanilla `throw` in room/global processing.
#[derive(Clone, Debug, PartialEq)]
pub struct Crash(pub Cow<'static, str>);

impl fmt::Display for Crash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl O {
    pub fn new() -> O {
        O::default()
    }

    /// `{k1: null, k2: null, …}`; the keys are different and none is an array index.
    pub fn nulls(keys: &[Atom]) -> O {
        O(Rc::new(Node { props: RefCell::new(Props::Live(keys.iter().map(|&k| (k, V::Null)).collect())), watch: OnceCell::new() }))
    }

    pub fn same(&self, other: &O) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// A stored doc's properties copied into live values (once).
    fn thaw(&self) {
        if matches!(&*self.0.props.borrow(), Props::Live(_)) {
            return;
        }
        let mut p = self.0.props.borrow_mut();
        let live = match &mut *p {
            Props::Doc(d, beside) => {
                let mut m = doc_pairs(d, beside);
                for (k, v) in std::mem::take(beside) {
                    set_pair(&mut m, k, v);
                }
                m
            }
            stored => pairs(stored.stored()),
        };
        *p = Props::Live(live);
    }

    fn props(&self) -> Ref<'_, Vec<(Atom, V)>> {
        self.thaw();
        Ref::map(self.0.props.borrow(), |p| match p {
            Props::Live(m) => m,
            _ => unreachable!("thawed"),
        })
    }

    fn props_mut(&self) -> RefMut<'_, Vec<(Atom, V)>> {
        self.thaw();
        RefMut::map(self.0.props.borrow_mut(), |p| match p {
            Props::Live(m) => m,
            _ => unreachable!("thawed"),
        })
    }

    /// `o.k == "s"` (`processor::eq_str` of `o.k`) without copying a string value.
    pub fn eq_str(&self, k: Atom, s: &str) -> bool {
        match &*self.0.props.borrow() {
            Props::Live(m) => return find(m, k).is_some_and(|v| crate::processor::eq_str(v, s)),
            stored => {
                if let Some(v) = stored.beside(k) {
                    return crate::processor::eq_str(v, s);
                }
                match stored.stored().get(k) {
                    None | Some(Val::Null) => return false,
                    Some(Val::Str(x)) => return &**x == s,
                    Some(_) => {}
                }
            }
        }
        crate::processor::eq_str(&self.get(k), s)
    }

    /// `o.k === "s"` without copying a string value.
    pub fn is_str(&self, k: Atom, s: &str) -> bool {
        match &*self.0.props.borrow() {
            Props::Live(m) => matches!(find(m, k), Some(V::Str(x)) if &**x == s),
            stored => match stored.beside(k) {
                Some(v) => matches!(v, V::Str(x) if &**x == s),
                None => matches!(stored.stored().get(k), Some(Val::Str(x)) if &**x == s),
            },
        }
    }

    /// `f` of the stored doc, for an object that is still all stored.
    pub fn with_stored<R>(&self, f: impl FnOnce(&Obj) -> R) -> Option<R> {
        match &*self.0.props.borrow() {
            Props::Live(_) => None,
            Props::Doc(_, beside) if !beside.is_empty() => None,
            stored => Some(f(stored.stored())),
        }
    }

    /// `o.k`; Undef when absent.
    pub fn get(&self, k: Atom) -> V {
        match &*self.0.props.borrow() {
            Props::Live(m) => return find(m, k).map_or(V::Undef, V::clone),
            stored => {
                if let Some(v) = stored.beside(k) {
                    return v.clone();
                }
                if let Some(v) = primitive(stored.stored().get(k)) {
                    return v;
                }
            }
        }
        self.nested(k)
    }

    /// `o.k` for a stored object or array, the first time it is read.
    fn nested(&self, k: Atom) -> V {
        if let Props::Doc(d, beside) = &mut *self.0.props.borrow_mut() {
            let at = d.obj.0.iter().position(|(a, _)| *a == k).expect("the doc has this key");
            let v = doc_value(d, at);
            beside.push((k, v.clone()));
            return v;
        }
        find(&self.props(), k).map_or(V::Undef, V::clone)
    }

    pub fn has(&self, k: Atom) -> bool {
        match &*self.0.props.borrow() {
            Props::Live(m) => find(m, k).is_some(),
            stored => stored.beside(k).is_some() || stored.stored().get(k).is_some(),
        }
    }

    /// `o.k = v` (Undef keeps the key, like JS).
    pub fn set(&self, k: Atom, v: V) {
        match &mut *self.0.props.borrow_mut() {
            Props::Live(m) => set_pair(m, k, v),
            Props::Doc(_, beside) => match beside.iter_mut().find(|(a, _)| *a == k) {
                Some(slot) => slot.1 = v,
                None => beside.push((k, v)),
            },
            sub => {
                let mut m = pairs(sub.stored());
                set_pair(&mut m, k, v);
                *sub = Props::Live(m);
            }
        }
        self.wrote(k);
    }

    /// `delete o.k`; true when the key existed.
    pub fn delete(&self, k: Atom) -> bool {
        if !self.has(k) {
            return false;
        }
        let mut m = self.props_mut();
        let Some(i) = m.iter().position(|(a, _)| *a == k) else { return false };
        m.remove(i);
        self.wrote(k);
        true
    }

    /// Every change of a key passes here: a room object's new position reaches its tile index.
    #[inline]
    fn wrote(&self, k: Atom) {
        if let Some(w) = self.0.watch.get() {
            if k == atom!("x") || k == atom!("y") {
                w.moved.borrow_mut().push(w.slot);
            }
        }
    }

    /// Key snapshot, as lodash `_.forEach` takes it before iterating.
    pub fn keys(&self) -> Vec<Atom> {
        self.thaw_written();
        match &*self.0.props.borrow() {
            Props::Live(m) => m.iter().map(|(a, _)| *a).collect(),
            stored => stored.stored().iter().map(|(a, _)| a).collect(),
        }
    }

    /// Makes `stored()` the object's own keys, when the list beside the doc may add to them.
    fn thaw_written(&self) {
        if matches!(&*self.0.props.borrow(), Props::Doc(_, beside) if !beside.is_empty()) {
            self.thaw();
        }
    }

    /// `ToNumber(o.k)`; Undef → NaN.
    pub fn num(&self, k: Atom) -> f64 {
        to_number(&self.get(k))
    }

    pub fn str(&self, k: Atom) -> Option<Arc<str>> {
        match self.get(k) {
            V::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        self.thaw_written();
        match &*self.0.props.borrow() {
            Props::Live(m) => m.len(),
            stored => stored.stored().len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl A {
    pub fn new() -> A {
        A::default()
    }

    pub fn from_vec(v: Vec<V>) -> A {
        A(Rc::new(RefCell::new(v)))
    }

    pub fn same(&self, other: &A) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    /// `a[i]`; Undef past the end.
    pub fn get(&self, i: usize) -> V {
        self.0.borrow().get(i).cloned().unwrap_or(V::Undef)
    }

    /// `a[i] = v`; writing past the end fills the gap with Undef (JS holes).
    pub fn set(&self, i: usize, v: V) {
        let mut a = self.0.borrow_mut();
        if i >= a.len() {
            a.resize(i + 1, V::Undef);
        }
        a[i] = v;
    }

    pub fn push(&self, v: V) {
        self.0.borrow_mut().push(v);
    }

    /// Element snapshot.
    pub fn to_vec(&self) -> Vec<V> {
        self.0.borrow().clone()
    }

    /// The elements, for reading them without a snapshot; nothing in `f` may change this array.
    pub fn with_items<R>(&self, f: impl FnOnce(&[V]) -> R) -> R {
        f(&self.0.borrow())
    }
}

impl V {
    pub fn str(s: &str) -> V {
        V::Str(s.into())
    }
}

/// A stored object as a fresh live object. Stored keys are unique and in JS order already.
pub fn from_obj(o: &Obj) -> O {
    O(Rc::new(Node { props: RefCell::new(Props::Live(pairs(o))), watch: OnceCell::new() }))
}

/// A stored object as a fresh live one, copied only when needed; reads as `from_obj` would.
pub fn from_shared(o: &Arc<Obj>) -> O {
    O(Rc::new(Node { props: RefCell::new(Props::Shared(o.clone())), watch: OnceCell::new() }))
}

/// A stored doc as a fresh live object, copied only when needed (`Props`); reads as `from_obj(&d.obj)` would.
pub fn from_doc(d: &DocRef) -> O {
    O(Rc::new(Node { props: RefCell::new(Props::Doc(d.clone(), Vec::new())), watch: OnceCell::new() }))
}

fn pairs(o: &Obj) -> Vec<(Atom, V)> {
    o.iter().map(|(k, v)| (k, from_val(v))).collect()
}

/// `o[k] = v` in a live object's keys.
fn set_pair(m: &mut Vec<(Atom, V)>, k: Atom, v: V) {
    match m.iter_mut().find(|(a, _)| *a == k) {
        Some(slot) => slot.1 = v,
        None => {
            let at = js_insert_pos(m, k);
            m.insert(at, (k, v));
        }
    }
}

fn find(m: &[(Atom, V)], k: Atom) -> Option<&V> {
    m.iter().find(|(a, _)| *a == k).map(|(_, v)| v)
}

/// Key number `key` of a doc as a live value: its objects, and those in its arrays, stay stored until written.
fn doc_value(d: &DocRef, key: usize) -> V {
    let sub = |item: usize| V::Obj(O(Rc::new(Node { props: RefCell::new(Props::Sub(d.clone(), key as u32, item as u32)), watch: OnceCell::new() })));
    match &d.obj.0[key].1 {
        Val::Obj(_) => sub(0),
        Val::Arr(a) => V::Arr(A::from_vec(a.iter().enumerate().map(|(i, x)| if matches!(x, Val::Obj(_)) { sub(i) } else { from_val(x) }).collect())),
        v => from_val(v),
    }
}

/// `pairs` of a doc, but for the keys in `beside`, which are about to replace these values.
fn doc_pairs(d: &DocRef, beside: &[(Atom, V)]) -> Vec<(Atom, V)> {
    let value = |(key, (k, _)): (usize, &(Atom, Val))| (*k, if find(beside, *k).is_some() { V::Undef } else { doc_value(d, key) });
    d.obj.0.iter().enumerate().map(value).collect()
}

/// Fresh graph, like a DB read.
pub fn from_val(v: &Val) -> V {
    match v {
        Val::Null => V::Null,
        Val::Bool(b) => V::Bool(*b),
        Val::Num(n) => V::Num(*n),
        Val::Str(s) => V::Str(s.clone()),
        Val::Arr(a) => V::Arr(A::from_vec(a.iter().map(from_val).collect())),
        Val::Obj(o) => V::Obj(from_obj(o)),
    }
}

/// JSON transport: Undef keys dropped, Undef in arrays → null, NaN/±Inf → null, -0 → 0.
/// None for a top-level Undef.
pub fn to_val(v: &V) -> Option<Val> {
    to_val_at(v, &mut Vec::new())
}

/// `path`: the objects being serialized, outermost first (JSON.stringify's stack).
fn to_val_at(v: &V, path: &mut Vec<V>) -> Option<Val> {
    if is_object(v) {
        // ponytail: JSON.stringify throws on a cycle (the room would crash); here it is a
        // panic, which process_room's catch_unwind turns into a logged crash.
        assert!(!path.iter().any(|p| same_ref(p, v)), "Converting circular structure to JSON");
        path.push(v.clone());
    }
    let out = match v {
        V::Undef => None,
        V::Null => Some(Val::Null),
        V::Bool(b) => Some(Val::Bool(*b)),
        V::Num(n) if n.is_finite() => Some(Val::Num(if *n == 0.0 { 0.0 } else { *n })),
        V::Num(_) => Some(Val::Null),
        V::Str(s) => Some(Val::Str(s.clone())),
        V::Arr(a) => Some(Val::Arr(a.to_vec().iter().map(|x| to_val_at(x, path).unwrap_or(Val::Null)).collect())),
        V::Obj(o) => {
            let pairs = o.props().clone();
            Some(Val::Obj(Obj(pairs.iter().filter_map(|(k, x)| Some((*k, to_val_at(x, path)?))).collect())))
        }
    };
    if is_object(v) {
        path.pop();
    }
    out
}

// ---- property access by lodash key (name or index) ----

#[derive(Clone, Copy)]
enum K {
    Name(Atom),
    Index(usize),
}

fn atom_of(k: K) -> Atom {
    match k {
        K::Name(a) => a,
        K::Index(i) => Atom::intern(&i.to_string()),
    }
}

fn index_of(k: K) -> Option<usize> {
    match k {
        K::Index(i) => Some(i),
        K::Name(a) => a.array_index().map(|i| i as usize),
    }
}

fn is_length(k: K) -> bool {
    matches!(k, K::Name(a) if a == atom!("length"))
}

fn get_k(obj: &V, k: K) -> V {
    match obj {
        V::Obj(o) => o.get(atom_of(k)),
        V::Arr(a) if is_length(k) => V::Num(a.len() as f64),
        V::Arr(a) => index_of(k).map_or(V::Undef, |i| a.get(i)),
        V::Str(s) => index_of(k).and_then(|i| s.encode_utf16().nth(i)).map_or(V::Undef, |u| V::Str(String::from_utf16_lossy(&[u]).into())),
        _ => V::Undef,
    }
}

fn set_k(obj: &V, k: K, v: V) {
    match obj {
        V::Obj(o) => o.set(atom_of(k), v),
        // `arr.length = n` truncates, or pads with holes (Undef here). ponytail: an invalid
        // length (JS throws a RangeError) and lengths past 2^24 are ignored.
        V::Arr(a) if is_length(k) => {
            let n = to_number(&v);
            if n >= 0.0 && n.fract() == 0.0 && n < (1 << 24) as f64 {
                a.0.borrow_mut().resize(n as usize, V::Undef);
            }
        }
        // ponytail: other named properties on arrays are not modelled (JSON drops them).
        V::Arr(a) => {
            if let Some(i) = index_of(k) {
                a.set(i, v)
            }
        }
        _ => {} // assignment to a primitive is ignored (sloppy mode)
    }
}

fn has_k(obj: &V, k: K) -> bool {
    match obj {
        V::Obj(o) => o.has(atom_of(k)),
        V::Arr(a) => index_of(k).is_some_and(|i| i < a.len()),
        _ => false,
    }
}

/// lodash `keys` (and the array/string index walk of `arrayEach`), as a snapshot.
fn own_keys(v: &V) -> Vec<K> {
    match v {
        V::Obj(o) => o.keys().into_iter().map(K::Name).collect(),
        V::Arr(a) => (0..a.len()).map(K::Index).collect(),
        V::Str(s) => (0..s.encode_utf16().count()).map(K::Index).collect(),
        _ => Vec::new(),
    }
}

fn is_object(v: &V) -> bool {
    matches!(v, V::Obj(_) | V::Arr(_))
}

pub(crate) fn same_ref(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Obj(x), V::Obj(y)) => x.same(y),
        (V::Arr(x), V::Arr(y)) => x.same(y),
        _ => false,
    }
}

/// `===`.
pub fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Undef, V::Undef) | (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Num(x), V::Num(y)) => x == y,
        (V::Str(x), V::Str(y)) => x == y,
        _ => same_ref(a, b),
    }
}

/// `getLength(v)` when it is a valid length (lodash `isArrayLike`).
fn array_like_len(v: &V) -> Option<usize> {
    let n = match v {
        V::Arr(a) => return Some(a.len()),
        V::Str(s) => return Some(s.encode_utf16().count()),
        V::Obj(o) => match o.get(atom!("length")) {
            V::Num(n) => n,
            _ => return None,
        },
        _ => return None,
    };
    // isLength; ponytail: lengths past 2^24 would allocate absurdly and are rejected.
    (n >= 0.0 && n.fract() == 0.0 && n <= 9007199254740991.0 && n < (1 << 24) as f64).then_some(n as usize)
}

/// Traversed source objects and their counterparts (lodash `stackA`/`stackB`).
#[derive(Default)]
struct Stacks {
    a: Vec<V>,
    b: Vec<V>,
}

impl Stacks {
    fn find(&self, v: &V) -> Option<V> {
        self.a.iter().rposition(|x| same_ref(x, v)).map(|i| self.b[i].clone())
    }
}

/// lodash 3.10.1 `_.merge(dst, src)`: deep merge into `dst`, which is returned.
pub fn merge(dst: &V, src: &V) -> V {
    // createAssigner: a null/undefined object is returned as is; falsy sources are skipped.
    if !matches!(dst, V::Undef | V::Null) && truthy(src) {
        match (dst, src) {
            // baseMerge of a flat object: its defined values are assigned
            (V::Obj(d), V::Obj(s)) if !d.same(s) && !s.props().iter().any(|(_, x)| is_object(x)) => {
                for (k, x) in s.props().iter().filter(|(_, x)| !matches!(x, V::Undef)) {
                    d.set(*k, x.clone());
                }
            }
            _ => _ = base_merge(dst, src, &mut Stacks::default()),
        }
    }
    dst.clone()
}

// lodash.js:2351 baseMerge
fn base_merge(object: &V, source: &V, st: &mut Stacks) -> V {
    if !is_object(object) {
        return object.clone();
    }
    let is_src_arr = matches!(source, V::Arr(_));
    for key in own_keys(source) {
        let src_value = get_k(source, key);
        if is_object(&src_value) {
            base_merge_deep(object, source, key, st);
        } else if !matches!(src_value, V::Undef) || (is_src_arr && !has_k(object, key)) {
            set_k(object, key, src_value);
        }
    }
    object.clone()
}

// lodash.js:2400 baseMergeDeep
fn base_merge_deep(object: &V, source: &V, key: K, st: &mut Stacks) {
    let src_value = get_k(source, key);
    if let Some(merged) = st.find(&src_value) {
        set_k(object, key, merged);
        return;
    }
    let value = get_k(object, key);
    let result = if let V::Arr(_) = src_value {
        match &value {
            V::Arr(_) => value.clone(),
            _ => match array_like_len(&value) {
                // arrayCopy
                Some(n) => V::Arr(A::from_vec((0..n).map(|i| get_k(&value, K::Index(i))).collect())),
                None => V::Arr(A::new()),
            },
        }
    } else {
        match &value {
            V::Obj(_) => value.clone(), // isPlainObject
            _ => V::Obj(O::new()),
        }
    };
    st.a.push(src_value.clone());
    st.b.push(result.clone());
    let merged = base_merge(&result, &src_value, st);
    set_k(object, key, merged);
}

/// lodash 3.10.1 `_.cloneDeep` (shared and cyclic references keep their shape).
pub fn clone_deep(v: &V) -> V {
    // Most payloads of `bulk.update` are flat.
    if let V::Obj(o) = v {
        let m = o.props();
        if !m.iter().any(|(_, x)| is_object(x)) {
            return V::Obj(O(Rc::new(Node { props: RefCell::new(Props::Live(m.clone())), watch: OnceCell::new() })));
        }
    }
    base_clone(v, &mut Stacks::default())
}

// lodash.js:1686 baseClone (isDeep)
fn base_clone(v: &V, st: &mut Stacks) -> V {
    if !is_object(v) {
        return v.clone();
    }
    if let Some(c) = st.find(v) {
        return c;
    }
    let result = match v {
        V::Arr(a) => V::Arr(A::from_vec(vec![V::Undef; a.len()])),
        _ => V::Obj(O::new()),
    };
    st.a.push(v.clone());
    st.b.push(result.clone());
    for key in own_keys(v) {
        let sub = get_k(v, key);
        set_k(&result, key, base_clone(&sub, st));
    }
    result
}

/// lodash 3.10.1 `_.isEqual`.
pub fn is_equal(a: &V, b: &V) -> bool {
    if let (V::Obj(x), V::Obj(y)) = (a, b) {
        if let Some(equal) = flat_equal(x, y) {
            return equal;
        }
    }
    base_is_equal(a, b, &mut Stacks::default())
}

/// `is_equal` of objects read side by side, while their keys line up and hold primitives; `None` for the rest.
fn flat_equal(a: &O, b: &O) -> Option<bool> {
    let n = a.len();
    if n != b.len() {
        return Some(false);
    }
    let (pa, pb) = (a.0.props.borrow(), b.0.props.borrow());
    for i in 0..n {
        let ((ka, x), (kb, y)) = (pa.item(i)?, pb.item(i)?);
        if ka != kb {
            return None;
        }
        if !strict_eq(&x, &y) && !(is_nan(&x) && is_nan(&y)) {
            return Some(false);
        }
    }
    Some(true)
}

fn is_nan(v: &V) -> bool {
    matches!(v, V::Num(n) if n.is_nan())
}

// lodash.js:2125 baseIsEqual
fn base_is_equal(a: &V, b: &V, st: &mut Stacks) -> bool {
    if strict_eq(a, b) {
        return true;
    }
    if matches!(a, V::Undef | V::Null) || matches!(b, V::Undef | V::Null) || (!is_object(a) && !is_object(b)) {
        return is_nan(a) && is_nan(b);
    }
    // baseIsEqualDeep: only array/array and object/object share a tag here.
    if !matches!((a, b), (V::Arr(_), V::Arr(_)) | (V::Obj(_), V::Obj(_))) {
        return false;
    }
    if let Some(i) = st.a.iter().rposition(|x| same_ref(x, a)) {
        return same_ref(&st.b[i], b);
    }
    st.a.push(a.clone());
    st.b.push(b.clone());
    let result = match (a, b) {
        (V::Arr(x), V::Arr(y)) => equal_arrays(x, y, st),
        (V::Obj(x), V::Obj(y)) => equal_objects(x, y, st),
        _ => unreachable!(),
    };
    st.a.pop();
    st.b.pop();
    result
}

// lodash.js:3711 equalArrays
fn equal_arrays(a: &A, b: &A, st: &mut Stacks) -> bool {
    let n = a.len();
    if n != b.len() {
        return false;
    }
    (0..n).all(|i| {
        let (x, y) = (a.get(i), b.get(i));
        strict_eq(&x, &y) || base_is_equal(&x, &y, st)
    })
}

// lodash.js:3798 equalObjects
fn equal_objects(a: &O, b: &O, st: &mut Stacks) -> bool {
    let keys = a.keys();
    if keys.len() != b.len() || !keys.iter().all(|&k| b.has(k)) {
        return false;
    }
    keys.into_iter().all(|k| base_is_equal(&a.get(k), &b.get(k), st))
}

/// `_.assign` / `_.extend`: copies every own key of `src`, Undef values included.
pub fn assign(dst: &O, src: &O) {
    for k in src.keys() {
        dst.set(k, src.get(k));
    }
}

/// JS ToBoolean.
pub fn truthy(v: &V) -> bool {
    match v {
        V::Undef | V::Null => false,
        V::Bool(b) => *b,
        V::Num(n) => *n != 0.0 && !n.is_nan(),
        V::Str(s) => !s.is_empty(),
        V::Obj(_) | V::Arr(_) => true,
    }
}

/// JS ToNumber.
pub fn to_number(v: &V) -> f64 {
    match v {
        V::Undef => f64::NAN,
        V::Null => 0.0,
        V::Bool(b) => *b as u8 as f64,
        V::Num(n) => *n,
        V::Str(s) => util::num::string_to_number(s),
        V::Obj(_) => f64::NAN, // "[object Object]"
        V::Arr(_) => util::num::string_to_number(&to_js_string(v)),
    }
}

/// `"" + v`.
pub fn to_js_string(v: &V) -> Arc<str> {
    match v {
        V::Str(s) => s.clone(),
        _ => {
            let mut out = String::new();
            write_js_string(&mut out, v, &mut Vec::new());
            out.into()
        }
    }
}

fn write_js_string(out: &mut String, v: &V, joining: &mut Vec<A>) {
    match v {
        V::Undef => out.push_str("undefined"),
        V::Null => out.push_str("null"),
        V::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        V::Num(n) => util::num::write_js_string(out, *n),
        V::Str(s) => out.push_str(s),
        V::Obj(_) => out.push_str("[object Object]"),
        V::Arr(a) => {
            // Array.prototype.join; V8 joins an array already being joined as "".
            if joining.iter().any(|x| x.same(a)) {
                return;
            }
            joining.push(a.clone());
            for (i, x) in a.to_vec().iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !matches!(x, V::Undef | V::Null) {
                    write_js_string(out, x, joining);
                }
            }
            joining.pop();
        }
    }
}

/// `parseInt(v)` (no radix).
pub fn parse_int(v: &V) -> f64 {
    let s = to_js_string(v);
    let t = s.trim_start_matches(util::num::is_js_whitespace);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let digits = |t: &str, hex: bool| t.bytes().take_while(|b| if hex { b.is_ascii_hexdigit() } else { b.is_ascii_digit() }).count();
    let n = if t.starts_with("0x") || t.starts_with("0X") {
        let d = digits(&t[2..], true);
        if d == 0 {
            return f64::NAN;
        }
        util::num::string_to_number(&t[..2 + d])
    } else {
        let d = digits(t, false);
        if d == 0 {
            return f64::NAN;
        }
        t[..d].parse::<f64>().unwrap_or(f64::NAN)
    };
    if neg {
        -n
    } else {
        n
    }
}

/// `JSON.stringify(v)` (raw non-ASCII; keeps `_` keys). A top-level Undef gives
/// "undefined", where JS returns the value undefined.
pub fn stringify(v: &V) -> String {
    let mut out = String::new();
    write_json(&mut out, v, false);
    out
}

/// What `to_val` then `Val` serialization write (non-ASCII escaped when `ascii`), straight from the graph.
pub fn write_json(out: &mut String, v: &V, ascii: bool) {
    if !write_json_at(out, v, ascii, &mut Vec::new()) {
        out.push_str("undefined");
    }
}

/// An array element of `write_json`: Undef writes null.
pub fn write_json_item(out: &mut String, v: &V, ascii: bool) {
    if !write_json_at(out, v, ascii, &mut Vec::new()) {
        out.push_str("null");
    }
}

/// False for Undef, which writes nothing (`to_val` gives None).
fn write_json_at(out: &mut String, v: &V, ascii: bool, path: &mut Vec<V>) -> bool {
    match v {
        V::Undef => return false,
        V::Null => out.push_str("null"),
        V::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        // -0 prints as 0, as `to_val` stores it
        V::Num(n) if n.is_finite() => util::num::write_js_string(out, *n),
        V::Num(_) => out.push_str("null"),
        V::Str(s) => crate::doc::write_str(out, s, ascii),
        V::Arr(_) | V::Obj(_) => {
            assert!(!path.iter().any(|p| same_ref(p, v)), "Converting circular structure to JSON");
            path.push(v.clone());
            match v {
                V::Arr(a) => {
                    out.push('[');
                    for (i, x) in a.0.borrow().iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        if !write_json_at(out, x, ascii, path) {
                            out.push_str("null");
                        }
                    }
                    out.push(']');
                }
                V::Obj(o) => {
                    out.push('{');
                    let mut first = true;
                    for (k, x) in o.props().iter().filter(|(_, x)| !matches!(x, V::Undef)) {
                        if !first {
                            out.push(',');
                        }
                        first = false;
                        crate::doc::write_str(out, k.as_str(), ascii);
                        out.push(':');
                        write_json_at(out, x, ascii, path);
                    }
                    out.push('}');
                }
                _ => unreachable!(),
            }
            path.pop();
        }
    }
    true
}

impl fmt::Debug for V {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn go(f: &mut fmt::Formatter<'_>, v: &V, depth: usize) -> fmt::Result {
            if depth > 8 {
                return f.write_str("…");
            }
            match v {
                V::Undef => f.write_str("undefined"),
                V::Str(s) => write!(f, "{s:?}"),
                V::Arr(a) => {
                    f.write_str("[")?;
                    for (i, x) in a.to_vec().iter().enumerate() {
                        if i > 0 {
                            f.write_str(",")?;
                        }
                        go(f, x, depth + 1)?;
                    }
                    f.write_str("]")
                }
                V::Obj(o) => {
                    f.write_str("{")?;
                    for (i, k) in o.keys().into_iter().enumerate() {
                        if i > 0 {
                            f.write_str(",")?;
                        }
                        write!(f, "{:?}:", k.as_str())?;
                        go(f, &o.get(k), depth + 1)?;
                    }
                    f.write_str("}")
                }
                _ => f.write_str(&to_js_string(v)),
            }
        }
        go(f, self, 0)
    }
}

impl fmt::Debug for O {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&V::Obj(self.clone()), f)
    }
}

impl fmt::Debug for A {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&V::Arr(self.clone()), f)
    }
}

/// processor.js `roomObjects`: id-keyed, JS key order (integer-like temp keys first),
/// snapshot iteration (PROC-ITER).
#[derive(Default)]
pub struct RoomObjects {
    index: Vec<(u32, usize)>, // canonical array-index keys, ascending → slot
    /// Every key ever inserted, in insertion order; `None` once deleted (a re-insert takes a new slot, as the
    /// key moves to the end in JS). Lives for one room tick, so the holes never pile up.
    slots: Vec<(Arc<str>, Option<O>)>,
    map: HashMap<Arc<str>, usize, util::hash::FxBuild>,
    tiles: RefCell<Option<Tiles>>,
    /// Slots of the keys deleted so far.
    removed: Vec<u32>,
    /// How many `insert`s there have been.
    inserts: u32,
    /// Slots taken when the room's docs had been loaded.
    pub loaded: usize,
}

/// Slots per tile, for `RoomObjects::near`. Built by the first lookup of a room tick and kept exact from then on:
/// `insert` and `remove` update it, and `O::set`/`O::delete` of `x` or `y` on an object in it leave the slot in
/// `moved`, which the next lookup works off.
struct Tiles {
    /// Tile (x * 50 + y) → its first slot in key order.
    first: Box<[u32; 2500]>,
    /// Slot → the next slot of its tile in key order.
    next: Vec<u32>,
    /// Slot → where it is: a tile, `NOWHERE` or `ODD`.
    tile: Vec<u16>,
    /// Slots at `ODD`: while there are any, every lookup scans (a loose match can then be anywhere).
    odd: usize,
    moved: Rc<RefCell<Vec<u32>>>,
    /// An object is in two slots, so `moved` cannot name them: every lookup scans.
    off: bool,
}

const NO_SLOT: u32 = u32::MAX;
/// x or y is a value no integer tile coordinate equals, not even loosely: absent, null, a fraction, off the map.
const NOWHERE: u16 = u16::MAX;
/// x or y is a string, a boolean, an array or an object, which `==` converts.
const ODD: u16 = u16::MAX - 1;
/// A lookup that would return more than this scans instead.
const NEAR_MAX: usize = 8;
/// Rooms this small scan: it is about as quick and needs no table.
const NEAR_MIN_OBJECTS: usize = 16;

/// A number value that is an integer 0–49 (`-0` too).
pub(crate) fn tile_coord(v: &V) -> Option<u16> {
    match v {
        V::Num(n) if n.fract() == 0.0 && (0.0..=49.0).contains(n) => Some(*n as u16),
        _ => None,
    }
}

/// Where an object goes in `Tiles`.
fn tile_of(o: &O) -> u16 {
    let (x, y) = (o.get(atom!("x")), o.get(atom!("y")));
    if let (Some(x), Some(y)) = (tile_coord(&x), tile_coord(&y)) {
        return x * 50 + y;
    }
    let never = |v: &V| matches!(v, V::Undef | V::Null | V::Num(_)) && tile_coord(v).is_none();
    if never(&x) || never(&y) {
        NOWHERE
    } else {
        ODD
    }
}

impl Tiles {
    fn new(objects: &RoomObjects) -> Tiles {
        let n = objects.slots.len();
        let mut t = Tiles { first: Box::new([NO_SLOT; 2500]), next: vec![NO_SLOT; n], tile: vec![NOWHERE; n], odd: 0, moved: Rc::default(), off: false };
        // Backwards through the key order, each at the front of its tile.
        let named = (0..n).rev().filter(|&s| crate::doc::array_index(&objects.slots[s].0).is_none());
        for s in named.chain(objects.index.iter().rev().map(|&(_, s)| s)) {
            let Some(o) = &objects.slots[s].1 else { continue };
            t.watch(o, s as u32);
            let at = tile_of(o);
            t.tile[s] = at;
            match at {
                NOWHERE => {}
                ODD => t.odd += 1,
                at => {
                    t.next[s] = t.first[at as usize];
                    t.first[at as usize] = s as u32;
                }
            }
        }
        t
    }

    fn watch(&mut self, o: &O, slot: u32) {
        self.off |= o.0.watch.set(Watch { moved: self.moved.clone(), slot }).is_err();
    }

    /// Takes `slot` off its tile.
    fn unlink(&mut self, slot: u32) {
        match self.tile[slot as usize] {
            NOWHERE => {}
            ODD => self.odd -= 1,
            at => {
                let after = self.next[slot as usize];
                let mut s = self.first[at as usize];
                if s == slot {
                    self.first[at as usize] = after;
                } else {
                    while self.next[s as usize] != slot {
                        s = self.next[s as usize];
                    }
                    self.next[s as usize] = after;
                }
            }
        }
        self.tile[slot as usize] = NOWHERE;
    }

    /// Puts `slot` where its object is now, in key order among the slots there.
    fn relink(&mut self, slot: u32, slots: &[(Arc<str>, Option<O>)]) {
        self.unlink(slot);
        let (key, Some(o)) = &slots[slot as usize] else { return };
        let at = tile_of(o);
        self.tile[slot as usize] = at;
        match at {
            NOWHERE => {}
            ODD => self.odd += 1,
            at => {
                let rank = |key: &str, s: u32| crate::doc::array_index(key).map_or(1 << 32 | s as u64, |i| i as u64);
                let mine = rank(key, slot);
                let (mut before, mut s) = (NO_SLOT, self.first[at as usize]);
                while s != NO_SLOT && rank(&slots[s as usize].0, s) < mine {
                    (before, s) = (s, self.next[s as usize]);
                }
                self.next[slot as usize] = s;
                if before == NO_SLOT {
                    self.first[at as usize] = slot;
                } else {
                    self.next[before as usize] = slot;
                }
            }
        }
    }
}

/// What `RoomObjects::near` walks: the slots of a tile, or everything.
enum Near<A, B> {
    All(A),
    Tile(B),
}

impl<'a, A: Iterator<Item = &'a O>, B: Iterator<Item = &'a O>> Iterator for Near<A, B> {
    type Item = &'a O;

    #[inline]
    fn next(&mut self) -> Option<&'a O> {
        match self {
            Near::All(i) => i.next(),
            Near::Tile(i) => i.next(),
        }
    }
}

impl RoomObjects {
    pub fn new() -> RoomObjects {
        RoomObjects::default()
    }

    pub fn with_capacity(n: usize) -> RoomObjects {
        RoomObjects { index: Vec::new(), slots: Vec::with_capacity(n), map: HashMap::with_capacity_and_hasher(n, Default::default()), tiles: RefCell::default(), removed: Vec::new(), inserts: 0, loaded: 0 }
    }

    pub fn get(&self, key: &str) -> Option<O> {
        self.map.get(key).and_then(|&s| self.slots[s].1.clone())
    }

    /// `roomObjects[key] = o`: an existing key keeps its position.
    pub fn insert(&mut self, key: Arc<str>, o: O) {
        self.inserts += 1;
        let s = self.slots.len();
        match self.map.entry(key) {
            Entry::Occupied(e) => {
                let s = *e.get();
                let slot = &mut self.slots[s];
                if slot.1.as_ref().is_some_and(|x| !x.same(&o)) {
                    slot.1 = Some(o);
                    if let Some(t) = self.tiles.get_mut() {
                        t.watch(slot.1.as_ref().expect("just set"), s as u32);
                        t.relink(s as u32, &self.slots);
                    }
                }
            }
            Entry::Vacant(e) => {
                let key = e.key().clone();
                e.insert(s);
                if let Some(i) = crate::doc::array_index(&key) {
                    let at = self.index.partition_point(|(j, _)| *j < i);
                    self.index.insert(at, (i, s));
                }
                if let Some(t) = self.tiles.get_mut() {
                    t.next.push(NO_SLOT);
                    t.tile.push(NOWHERE);
                    t.watch(&o, s as u32);
                }
                self.slots.push((key, Some(o)));
                if let Some(t) = self.tiles.get_mut() {
                    t.relink(s as u32, &self.slots);
                }
            }
        }
    }

    /// `delete roomObjects[key]`.
    pub fn remove(&mut self, key: &str) {
        if let Some(s) = self.map.remove(key) {
            self.slots[s].1 = None;
            self.removed.push(s as u32);
            self.index.retain(|&(_, x)| x != s);
            if let Some(t) = self.tiles.get_mut() {
                t.unlink(s as u32);
            }
        }
    }

    /// Slots in key order: array-index keys ascending, then the rest in insertion order.
    fn order(&self) -> impl Iterator<Item = (usize, &Arc<str>, &O)> {
        let at = |s: usize| {
            let (k, o) = &self.slots[s];
            o.as_ref().map(|o| (s, k, o))
        };
        let plain = self.index.is_empty();
        let named = (0..self.slots.len()).filter(move |&s| plain || crate::doc::array_index(&self.slots[s].0).is_none());
        self.index.iter().map(|&(_, s)| s).chain(named).filter_map(at)
    }

    /// Key snapshot for `_.forEach`.
    pub fn keys(&self) -> Vec<Arc<str>> {
        let mut keys = Vec::with_capacity(self.map.len());
        keys.extend(self.order().map(|(_, k, _)| k.clone()));
        keys
    }

    /// The key snapshot of `_.forEach` as slots, for `at`: the same walk as `keys` and `get` without the hashing.
    pub fn key_order(&self) -> Vec<u32> {
        let mut slots = Vec::with_capacity(self.map.len());
        slots.extend(self.order().map(|(s, _, _)| s as u32));
        slots
    }

    /// `roomObjects[key]` for the key of a `key_order` slot.
    #[inline]
    pub fn at(&self, slot: u32) -> Option<O> {
        match &self.slots[slot as usize] {
            (_, Some(o)) => Some(o.clone()),
            (key, None) => self.get(key),
        }
    }

    /// The object of a `key_order` slot while its key has not been deleted.
    #[inline]
    pub fn peek(&self, slot: u32) -> Option<&O> {
        self.slots[slot as usize].1.as_ref()
    }

    /// The number of `insert`s so far: the same number means the same keys, in the same order, with the same
    /// objects, but for deleted ones.
    pub fn inserts(&self) -> u32 {
        self.inserts
    }

    /// The slot `key` was given among the first `n` inserts, deleted since or not.
    pub fn first_slot(&self, key: &str, n: usize) -> Option<usize> {
        match self.map.get(key) {
            Some(&s) if s < n => Some(s),
            _ => self.removed.iter().map(|&s| s as usize).find(|&s| s < n && &*self.slots[s].0 == key),
        }
    }

    /// The number of keys ever given a slot.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// The same number means the same objects under the same keys.
    pub fn version(&self) -> u32 {
        self.inserts + self.removed.len() as u32
    }

    /// The objects in key order, for scans that do not change `roomObjects` (the borrow guarantees it), which
    /// then see what a key snapshot read live would.
    pub fn iter(&self) -> impl Iterator<Item = &O> {
        self.order().map(|(_, _, o)| o)
    }

    /// The objects to test for `o.x == x && o.y == y` or `===`, in the order `iter` gives them. Every object that
    /// matches is among them, and usually nothing else, so a scan looking for a place only needs this.
    pub fn near<'a>(&'a self, x: &V, y: &V) -> impl Iterator<Item = &'a O> + 'a {
        let mut list = [0; NEAR_MAX];
        match self.tile_slots(x, y, &mut list) {
            Some(n) => Near::Tile(list.into_iter().take(n).map(|s| self.slots[s as usize].1.as_ref().expect("slots of a tile are in use"))),
            None => Near::All(self.iter()),
        }
    }

    /// Fills `list` with the slots of tile (x, y), unless the caller has to scan.
    fn tile_slots(&self, x: &V, y: &V, list: &mut [u32; NEAR_MAX]) -> Option<usize> {
        let tile = tile_coord(x)? * 50 + tile_coord(y)?;
        if self.slots.len() < NEAR_MIN_OBJECTS {
            return None;
        }
        let mut tiles = self.tiles.borrow_mut();
        let t = tiles.get_or_insert_with(|| Tiles::new(self));
        loop {
            let Some(s) = t.moved.borrow_mut().pop() else { break };
            t.relink(s, &self.slots);
        }
        if t.off || t.odd > 0 {
            return None;
        }
        let (mut n, mut s) = (0, t.first[tile as usize]);
        while s != NO_SLOT {
            *list.get_mut(n)? = s;
            n += 1;
            s = t.next[s as usize];
        }
        #[cfg(any(debug_assertions, test))]
        self.check_near(x, y, &list[..n]);
        Some(n)
    }

    /// The slots of a tile against the scan they stand for: the objects loosely at (x, y), in `iter` order.
    #[cfg(any(debug_assertions, test))]
    fn check_near(&self, x: &V, y: &V, list: &[u32]) {
        let eq = crate::processor::loose_eq;
        let at = |o: &O| eq(&o.get(atom!("x")), x) && eq(&o.get(atom!("y")), y);
        let scan: Vec<&O> = self.iter().filter(|o| at(o)).collect();
        let tile: Vec<&O> = list.iter().filter_map(|&s| self.slots[s as usize].1.as_ref()).filter(|o| at(o)).collect();
        assert!(scan.len() == tile.len() && scan.iter().zip(&tile).all(|(a, b)| a.same(b)), "tile index out of step at {x:?},{y:?}");
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(json: &str) -> V {
        from_val(&Val::from_json(&util::json::Json::parse(json).unwrap()))
    }

    fn js(x: &V) -> String {
        stringify(x)
    }

    #[test]
    fn merge_basics() {
        let dst = v(r#"{"a":[1,2,3],"b":{"x":1},"c":5,"d":"keep"}"#);
        let src = v(r#"{"a":[9],"b":{"y":2},"c":{"z":1},"d":null,"e":[1]}"#);
        let d0 = match &dst {
            V::Obj(o) => o.clone(),
            _ => unreachable!(),
        };
        let b_before = d0.get(atom!("b"));
        merge(&dst, &src);
        assert_eq!(js(&dst), r#"{"a":[9,2,3],"b":{"x":1,"y":2},"c":{"z":1},"d":null,"e":[1]}"#);
        assert!(same_ref(&b_before, &d0.get(atom!("b"))), "existing plain objects are merged in place");
        // undefined in a source object is skipped; in a source array it fills absent slots.
        let dst = v(r#"{"a":1,"arr":[1]}"#);
        let src = V::Obj(O::new());
        if let V::Obj(s) = &src {
            s.set(atom!("a"), V::Undef);
            s.set(atom!("arr"), V::Arr(A::from_vec(vec![V::Undef, V::Undef])));
        }
        merge(&dst, &src);
        assert_eq!(format!("{dst:?}"), r#"{"a":1,"arr":[1,undefined]}"#);
        // A primitive or nullish destination is returned unchanged.
        assert!(matches!(merge(&V::Num(5.0), &v(r#"{"a":1}"#)), V::Num(n) if n == 5.0));
        assert!(matches!(merge(&V::Null, &v(r#"{"a":1}"#)), V::Null));
    }

    #[test]
    fn merge_shared_and_cyclic_sources() {
        // The same source object reached twice maps to one merged object.
        let shared = v(r#"{"k":1}"#);
        let src = V::Obj(O::new());
        if let V::Obj(s) = &src {
            s.set(atom!("p"), shared.clone());
            s.set(atom!("q"), shared.clone());
            s.set(atom!("self"), src.clone());
        }
        let dst = V::Obj(O::new());
        merge(&dst, &src);
        let V::Obj(d) = &dst else { unreachable!() };
        assert!(same_ref(&d.get(atom!("p")), &d.get(atom!("q"))));
        assert!(!same_ref(&d.get(atom!("p")), &shared));
        let V::Obj(inner) = d.get(atom!("self")) else { panic!() };
        assert!(same_ref(&inner.get(atom!("p")), &d.get(atom!("p"))));
    }

    #[test]
    fn array_length_writes_and_json_cycles() {
        // node: _.merge([1,2,3], {length: 1}) → [1]; {length: 4} pads with holes.
        let a = v("[1,2,3]");
        merge(&a, &v(r#"{"length":1}"#));
        assert_eq!(js(&a), "[1]");
        merge(&a, &v(r#"{"length":3,"0":"z"}"#));
        assert_eq!(format!("{a:?}"), r#"["z",undefined,undefined]"#);
        // JSON.stringify finds a cycle by its ancestor stack: shared is fine, cyclic panics
        // at once (64 KiB of stack is plenty).
        std::thread::Builder::new()
            .stack_size(64 << 10)
            .spawn(|| {
                let shared = v(r#"{"k":1}"#);
                let o = O::new();
                o.set(atom!("a"), shared.clone());
                o.set(atom!("b"), V::Arr(A::from_vec(vec![shared.clone(), shared])));
                assert_eq!(js(&V::Obj(o.clone())), r#"{"a":{"k":1},"b":[{"k":1},{"k":1}]}"#);
                o.set(atom!("me"), V::Arr(A::from_vec(vec![V::Obj(o.clone())])));
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| to_val(&V::Obj(o))));
                assert_eq!(*r.unwrap_err().downcast::<&str>().unwrap(), "Converting circular structure to JSON");
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn write_json_matches_to_val() {
        let o = v(r#"{"a":[1,null,{"b":"é "}],"n":-0,"s":"x\"y","e":{},"z":[]}"#);
        let V::Obj(inner) = &o else { unreachable!() };
        inner.set(atom!("u"), V::Undef);
        inner.set(atom!("nan"), V::Num(f64::NAN));
        let shared = v(r#"{"k":[1,2]}"#);
        inner.set(atom!("arr"), V::Arr(A::from_vec(vec![V::Undef, V::Num(f64::INFINITY), shared.clone(), shared])));
        for ascii in [false, true] {
            let mut want = String::new();
            crate::doc::write_val(&mut want, &to_val(&o).unwrap(), ascii);
            let mut got = String::new();
            write_json(&mut got, &o, ascii);
            assert_eq!(got, want);
        }
        let mut s = String::new();
        write_json(&mut s, &V::Undef, true);
        assert_eq!(s, "undefined");
    }

    #[test]
    fn clone_and_equal() {
        let a = v(r#"{"x":[1,{"y":null}],"z":"s","n":-0}"#);
        let b = clone_deep(&a);
        assert!(!same_ref(&a, &b));
        assert!(is_equal(&a, &b));
        assert_eq!(js(&b), r#"{"x":[1,{"y":null}],"z":"s","n":0}"#);
        assert!(is_equal(&V::Num(f64::NAN), &V::Num(f64::NAN)));
        assert!(is_equal(&V::Num(0.0), &V::Num(-0.0)));
        assert!(!is_equal(&V::Num(1.0), &V::str("1")));
        assert!(!is_equal(&V::Undef, &V::Null));
        assert!(!is_equal(&v("{}"), &v("[]")));
        let with_undef = v("{}");
        if let V::Obj(o) = &with_undef {
            o.set(atom!("a"), V::Undef);
        }
        assert!(!is_equal(&with_undef, &v("{}")), "an undefined-valued key still counts");
        assert!(is_equal(&v(r#"{"a":1,"b":2}"#), &v(r#"{"b":2,"a":1}"#)));
    }

    #[test]
    fn js_values() {
        assert_eq!(&*to_js_string(&v(r#"[1,[2,null],{},"x",true]"#)), "1,2,,[object Object],x,true");
        assert_eq!(to_number(&v("[]")), 0.0);
        assert_eq!(to_number(&v("[\" 7 \"]")), 7.0);
        assert!(to_number(&v("{}")).is_nan());
        assert_eq!(parse_int(&V::str("  -0x1A9z")), -425.0);
        assert_eq!(parse_int(&V::Num(1e21)), 1.0);
        assert_eq!(parse_int(&V::Num(0.0000005)), 5.0);
        assert!(parse_int(&V::str("Infinity")).is_nan());
        assert!(parse_int(&V::str("-0")).is_sign_negative());
        assert!(!truthy(&V::Num(f64::NAN)) && !truthy(&V::str("")) && truthy(&v("[]")));
        assert_eq!(stringify(&V::Undef), "undefined");
    }

    #[test]
    fn room_objects_order() {
        let mut r = RoomObjects::new();
        for k in ["abc", "createdStructure1", "3", "1", "_createdConstructionSite2", "10"] {
            r.insert(k.into(), O::new());
        }
        r.insert("abc".into(), O::new());
        assert_eq!(r.keys().iter().map(|k| &**k).collect::<Vec<_>>(), ["1", "3", "10", "abc", "createdStructure1", "_createdConstructionSite2"]);
        r.remove("3");
        r.remove("abc");
        r.insert("abc".into(), O::new());
        assert_eq!(r.keys().iter().map(|k| &**k).collect::<Vec<_>>(), ["1", "10", "createdStructure1", "_createdConstructionSite2", "abc"]);
        assert_eq!(r.len(), 5);
    }

    /// `near` against the scan it stands for, through every kind of change, odd coordinates included.
    #[test]
    fn near_follows_every_change() {
        let at = |o: &O, x: &V, y: &V| crate::processor::loose_eq(&o.get(atom!("x")), x) && crate::processor::loose_eq(&o.get(atom!("y")), y);
        let check = |r: &RoomObjects| {
            for (x, y) in (0..6).flat_map(|x| (0..6).map(move |y| (V::Num(x as f64), V::Num(y as f64)))) {
                let near: Vec<&O> = r.near(&x, &y).filter(|o| at(o, &x, &y)).collect();
                let scan: Vec<&O> = r.iter().filter(|o| at(o, &x, &y)).collect();
                assert!(near.len() == scan.len() && near.iter().zip(&scan).all(|(a, b)| a.same(b)), "{x:?},{y:?}");
            }
        };
        let doc = |x: i32, y: i32| {
            let Val::Obj(o) = Val::from_json(&util::json::Json::parse(&format!(r#"{{"type":"road","x":{x},"y":{y}}}"#)).unwrap()) else { unreachable!() };
            from_doc(&crate::doc::Doc::new(1, o))
        };
        let mut r = RoomObjects::new();
        for i in 0..24 {
            r.insert(format!("id{i}").into(), doc(i % 6, i / 6));
        }
        let mut seed = 12345u32;
        let mut next = |n: u32| {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            (seed >> 16) % n
        };
        let key = |k: u32| -> Arc<str> { if k % 4 == 0 { format!("{}", k / 4).into() } else { format!("id{k}").into() } };
        let values = [V::Num(2.0), V::Num(4.0), V::Num(0.0), V::Num(-0.0), V::Num(2.5), V::Num(50.0), V::Null, V::Undef];
        for step in 0..3000 {
            match next(8) {
                0 => r.insert(key(next(40)), doc(next(6) as i32, next(6) as i32)),
                1 => r.remove(&key(next(40))),
                6 if step % 50 == 0 => {
                    // odd values only now and then: a room with one scans until it is gone
                    if let Some(o) = r.get(&key(next(40))) {
                        o.set(atom!("x"), V::str("3"));
                        check(&r);
                        o.set(atom!("x"), V::Num(3.0));
                    }
                }
                7 => {
                    if let Some(o) = r.get(&key(next(40))) {
                        o.delete(atom!("y"));
                    }
                }
                _ => {
                    if let Some(o) = r.get(&key(next(40))) {
                        o.set(if next(2) == 0 { atom!("x") } else { atom!("y") }, values[next(8) as usize].clone());
                    }
                }
            }
            check(&r);
        }
        assert!(r.tiles.borrow().is_some(), "the walk went through the index");
    }

    #[test]
    fn doc_objects_read_as_copies() {
        let json = r#"{"_id":"a1","type":"road","x":3,"hits":-0,"store":{"energy":5},"body":[1,2]}"#;
        let V::Obj(copy) = v(json) else { unreachable!() };
        let doc = || {
            let Val::Obj(o) = Val::from_json(&util::json::Json::parse(json).unwrap()) else { unreachable!() };
            from_doc(&crate::doc::Doc::new(1, o))
        };
        let o = doc();
        assert_eq!((o.keys(), o.len(), o.has(atom!("x")), o.has(atom!("y"))), (copy.keys(), copy.len(), true, false));
        assert!(matches!(o.get(atom!("x")), V::Num(n) if n == 3.0) && matches!(o.get(atom!("y")), V::Undef));
        assert!(o.eq_str(atom!("type"), "road") && !o.eq_str(atom!("type"), "wall") && !o.eq_str(atom!("y"), "undefined"));
        assert!(matches!(&*o.0.props.borrow(), Props::Doc(_, b) if b.is_empty()), "primitive reads keep the doc");
        // a nested value is copied out once and keeps its identity
        let (V::Obj(s1), V::Obj(s2)) = (o.get(atom!("store")), o.get(atom!("store"))) else { panic!() };
        assert!(s1.same(&s2) && matches!(&*o.0.props.borrow(), Props::Doc(_, b) if b.len() == 1));
        // reads and writes stay beside the doc until something needs all the keys, and the keys keep JS order
        let w = doc();
        let plain = || { let V::Obj(c) = v(json) else { unreachable!() }; c };
        let c = plain();
        let (V::Obj(ws), V::Obj(cs)) = (w.get(atom!("store")), c.get(atom!("store"))) else { panic!() };
        for x in [&ws, &cs] {
            x.set(atom!("energy"), V::Num(9.0));
        }
        for x in [&w, &c] {
            x.set(atom!("_new"), V::Num(1.0));
            x.set(atom!("x"), V::str("moved"));
            x.set(atom!("7"), V::Undef);
            x.set(atom!("3"), V::Bool(true));
        }
        assert!(w.has(atom!("_new")) && w.has(atom!("7")) && matches!(w.get(atom!("7")), V::Undef) && w.is_str(atom!("x"), "moved") && w.eq_str(atom!("x"), "moved"));
        assert!(matches!(&*w.0.props.borrow(), Props::Doc(..)) && w.with_stored(|_| ()).is_none());
        assert!(!w.delete(atom!("nope")) && matches!(&*w.0.props.borrow(), Props::Doc(..)));
        assert_eq!((w.keys(), w.len()), (c.keys(), c.len()));
        assert!(matches!(&*w.0.props.borrow(), Props::Live(_)));
        assert_eq!(js(&V::Obj(w.clone())), js(&V::Obj(c.clone())));
        assert!(w.delete(atom!("3")) && c.delete(atom!("3")));
        let V::Obj(after) = w.get(atom!("store")) else { panic!() };
        assert!(after.same(&ws), "a value read before the copy keeps its identity");
        assert_eq!(js(&V::Obj(w)), js(&V::Obj(c)));
        assert_eq!(js(&V::Obj(o)), js(&V::Obj(copy.clone())));
        // so do the objects of the copy: read in place, copied on a write, each with its own identity
        let json_sub = r#"{"store":{"energy":5},"body":[{"type":"move","hits":100},7,[{"a":1}],{"type":"work"}],"e":{}}"#;
        let V::Obj(plain) = v(json_sub) else { unreachable!() };
        let Val::Obj(d) = Val::from_json(&util::json::Json::parse(json_sub).unwrap()) else { unreachable!() };
        let o = from_doc(&crate::doc::Doc::new(1, d));
        let (V::Arr(body), V::Obj(store), V::Obj(e)) = (o.get(atom!("body")), o.get(atom!("store")), o.get(atom!("e"))) else { panic!() };
        let V::Obj(part) = body.get(0) else { panic!() };
        assert!(matches!(&*part.0.props.borrow(), Props::Sub(..)) && matches!(&*store.0.props.borrow(), Props::Sub(..)));
        assert!(part.same(match &body.get(0) { V::Obj(p) => p, _ => panic!() }) && !e.same(&store) && e.is_empty());
        assert!(part.eq_str(atom!("type"), "move") && part.is_str(atom!("type"), "move") && part.has(atom!("hits")) && !part.has(atom!("x")));
        assert_eq!((part.keys(), part.len()), (vec![atom!("type"), atom!("hits")], 2));
        assert!(matches!(part.get(atom!("hits")), V::Num(n) if n == 100.0) && matches!(store.get(atom!("energy")), V::Num(n) if n == 5.0));
        assert!(matches!(&*part.0.props.borrow(), Props::Sub(..)), "reads keep the doc");
        part.set(atom!("hits"), V::Num(50.0));
        store.set(atom!("energy"), V::Num(1.0));
        let (V::Arr(b2), V::Obj(s2)) = (plain.get(atom!("body")), plain.get(atom!("store"))) else { panic!() };
        let V::Obj(p2) = b2.get(0) else { panic!() };
        p2.set(atom!("hits"), V::Num(50.0));
        s2.set(atom!("energy"), V::Num(1.0));
        assert_eq!(js(&V::Obj(o)), js(&V::Obj(plain)));
        let o = doc();
        o.set(atom!("hits"), V::Num(7.0));
        copy.set(atom!("hits"), V::Num(7.0));
        assert_eq!(js(&V::Obj(o.clone())), js(&V::Obj(copy)));
        assert!(o.delete(atom!("x")) && !o.has(atom!("x")));
    }

    #[test]
    fn match_str_is_lodash_match() {
        use crate::processor::{lodash_match, lodash_match_str};
        let json = r#"{"type":"road","n":5,"s5":"5","u":null,"o":{"type":"road"},"a":["road"]}"#;
        let Val::Obj(d) = Val::from_json(&util::json::Json::parse(json).unwrap()) else { unreachable!() };
        let values = [V::Obj(from_doc(&crate::doc::Doc::new(1, d))), v(json), v(r#"["road","5"]"#), V::str("road"), V::Num(5.0), V::Undef, V::Null];
        let keys = [atom!("type"), atom!("n"), atom!("s5"), atom!("u"), atom!("o"), atom!("a"), atom!("y"), atom!("0"), atom!("1"), atom!("length")];
        for o in &values {
            for k in keys {
                for s in ["road", "5", "r", "", "undefined", "null"] {
                    assert_eq!(lodash_match_str(o, k, s), lodash_match(o, k, &V::str(s)), "{} {} {s}", js(o), k.as_str());
                }
            }
        }
    }

}
