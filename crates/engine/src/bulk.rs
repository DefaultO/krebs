//! mirror: reference/driver/lib/bulk.js, reference/storage/lib/db.js
//! parity: DIVERGENT(BUG-INSERT-COUNTER, BUG-DOTKEY-INC)
//!
//! The per-collection write buffer of room and global processing (ARCHITECTURE §3.3);
//! from db.js only updateDocument and dbBulk.
//! Payloads hold live references and are serialized at `execute`, so whether an
//! object-valued key persists its value at the update or at room end follows from
//! identity, as in vanilla. Deliberate differences:
//! * BUG-INSERT-COUNTER (F): the temp counter is never decremented, so it is never reused.
//! * BUG-DOTKEY-INC (F): `inc` takes a key path and increments the nested value.
//! * An op whose target has no `_id` targets the id "undefined" (a no-op); vanilla's Loki
//!   `by(undefined)` instead aborts the rest of the dbBulk. Engine code never does this.
//!   Loki also aborts on an insert whose `_id` exists and on `$addToSet`/`$pull` of a
//!   truthy non-array; `apply` carries on (the serial id pass owns `_id` uniqueness).

use crate::atom;
use crate::doc::{array_index, Atom, Id, Obj, Val};
use crate::live::{self, assign, clone_deep, merge, same_ref, to_js_string, to_val, truthy, Crash, O, V};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub enum Target<'a> {
    /// A live object: updates merge into it (BULK-UPDATE, VIS-INMEM).
    Obj(&'a O),
    /// A bare id: memory is untouched (VIS-USERS-STALE).
    Id(&'a str),
}

enum Queued {
    Remove(Arc<str>),
    Inc(Arc<str>, Vec<Atom>, f64),
    AddToSet(Arc<str>, Atom, V),
    Pull(Arc<str>, Atom, V),
}

pub struct Bulk {
    queued: Vec<Queued>,
    /// `$set` payloads by id, in first-touch order.
    updates: Vec<(Arc<str>, O)>,
    /// Pending inserts by counter, ascending.
    inserts: Vec<(u32, O)>,
    insert_cnt: u32,
    ops_cnt: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DbOp {
    Remove(Id),
    Inc(Id, Vec<Atom>, f64),
    AddToSet(Id, Atom, Val),
    Pull(Id, Atom, Val),
    /// Shallow `_.extend(doc, $set)`.
    Set(Id, Obj),
    /// `_id` is assigned in the serial id pass unless given.
    Insert(Obj),
    /// The whole document (a storage `update` computed outside: CLI, mods); keeps its place and seq.
    Replace(Id, Obj),
}

pub trait DocStore {
    /// Copy-on-write access to the stored document.
    fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj>;
    fn remove(&mut self, id: &Id);
    /// Staged; seq and `_id` come later.
    fn insert(&mut self, doc: Obj);
}

impl Default for Bulk {
    fn default() -> Bulk {
        Bulk::new()
    }
}

impl Bulk {
    pub fn new() -> Bulk {
        Bulk { queued: Vec::new(), updates: Vec::new(), inserts: Vec::new(), insert_cnt: 0, ops_cnt: 0 }
    }

    /// bulk.js:33. The `Err` (an object-valued key with a bare id) leaves everything untouched.
    pub fn update(&mut self, t: Target, data: &V) -> Result<(), Crash> {
        if let Target::Id("") = t {
            return Ok(());
        }
        self.ops_cnt += 1;
        let data = clone_deep(data);
        // ponytail: array payloads, and objects with a numeric `length` key (which lodash's
        // forEach walks as array-likes), are not modelled; engine updates pass plain objects.
        if let V::Obj(d) = &data {
            for key in d.keys() {
                let value = d.get(key);
                if !matches!(value, V::Obj(_) | V::Arr(_)) {
                    continue;
                }
                let Target::Obj(o) = t else {
                    return Err(Crash(format!("can not update an object diff property '{key}' without object reference").into()));
                };
                // A truthy live object is merged in place and becomes the payload (alias);
                // otherwise a fresh object holds this update's value (snapshot).
                let orig = o.get(key);
                let orig = if truthy(&orig) { orig } else { V::Obj(O::new()) };
                merge(&orig, &value);
                d.set(key, orig);
            }
        }
        let id = match t {
            Target::Obj(o) => {
                merge(&V::Obj(o.clone()), &data);
                to_js_string(&o.get(atom!("_id")))
            }
            Target::Id(s) => s.into(),
        };
        remove_hidden(&data, &mut Vec::new())?;
        let payload = match self.pending_insert(&id) {
            Some(i) => self.inserts[i].1.clone(),
            None => match self.updates.iter().find(|(k, _)| *k == id) {
                Some((_, p)) => p.clone(),
                None => {
                    self.updates.push((id, O::new()));
                    self.updates.last().unwrap().1.clone()
                }
            },
        };
        if let V::Obj(d) = &data {
            assign(&payload, d);
        }
        Ok(())
    }

    /// bulk.js:64. Returns the temp counter (never reused, BUG-INSERT-COUNTER).
    pub fn insert(&mut self, data: &V, id: Option<&str>) -> u32 {
        let data = clone_deep(data);
        // ponytail: vanilla throws here on a null/undefined array element (removeHidden reads
        // `$loki` of it); engine inserts never hold one, so that is a panic, not a Crash.
        if let Err(Crash(e)) = remove_hidden(&data, &mut Vec::new()) {
            panic!("bulk.insert: {e}");
        }
        let V::Obj(o) = data else { panic!("bulk.insert of a non-object") };
        if let Some(id) = id.filter(|s| !s.is_empty()) {
            o.set(atom!("_id"), V::str(id));
        }
        self.ops_cnt += 1;
        self.insert_cnt += 1;
        self.inserts.push((self.insert_cnt, o));
        self.insert_cnt
    }

    /// bulk.js:78. Removing a pending insert cancels it.
    pub fn remove(&mut self, id: &str) {
        if id.is_empty() {
            return;
        }
        self.ops_cnt += 1;
        match self.pending_insert(id) {
            Some(i) => _ = self.inserts.remove(i),
            None => self.queued.push(Queued::Remove(id.into())),
        }
    }

    /// bulk.js:90 `inc`: for a live object also `o.k = (o.k || 0) + n`. A key path
    /// (vanilla's dotted key) increments the nested value (BUG-DOTKEY-INC, F).
    pub fn inc(&mut self, t: Target, path: &[Atom], n: f64) {
        let id = match t {
            Target::Id("") => return,
            Target::Id(s) => s.into(),
            Target::Obj(o) => {
                inc_live(o, path, n);
                to_js_string(&o.get(atom!("_id")))
            }
        };
        self.ops_cnt += 1;
        self.queued.push(Queued::Inc(id, path.to_vec(), n));
    }

    /// bulk.js:101; no live mutation.
    pub fn add_to_set(&mut self, t: Target, key: Atom, v: &V) {
        if let Some(id) = self.queued_id(t) {
            self.queued.push(Queued::AddToSet(id, key, v.clone()));
        }
    }

    /// bulk.js:111; no live mutation.
    pub fn pull(&mut self, t: Target, key: Atom, v: &V) {
        if let Some(id) = self.queued_id(t) {
            self.queued.push(Queued::Pull(id, key, v.clone()));
        }
    }

    fn queued_id(&mut self, t: Target) -> Option<Arc<str>> {
        let id = match t {
            Target::Id("") => return None,
            Target::Id(s) => s.into(),
            Target::Obj(o) => to_js_string(&o.get(atom!("_id"))),
        };
        self.ops_cnt += 1;
        Some(id)
    }

    /// `inserts[id]`: the pending insert whose counter is the property key `id`.
    fn pending_insert(&self, id: &str) -> Option<usize> {
        let n = array_index(id)?;
        self.inserts.iter().position(|(c, _)| *c == n)
    }

    /// bulk.js:121. Payloads are serialized now: queued ops in call order, then `$set`
    /// per id in `updates` key order, then inserts in counter order.
    pub fn execute(self) -> Vec<DbOp> {
        if self.ops_cnt == 0 {
            return Vec::new();
        }
        let mut out = Vec::new();
        for q in self.queued {
            let op = match q {
                Queued::Remove(id) => Id::new(&id).map(DbOp::Remove),
                Queued::Inc(id, path, n) => Id::new(&id).map(|id| DbOp::Inc(id, path, n)),
                // `{$addToSet: {k: undefined}}` loses the key in transport: nothing to do.
                Queued::AddToSet(id, k, v) => Id::new(&id).zip(to_val(&v)).map(|(id, v)| DbOp::AddToSet(id, k, v)),
                Queued::Pull(id, k, v) => Id::new(&id).zip(to_val(&v)).map(|(id, v)| DbOp::Pull(id, k, v)),
            };
            out.extend(op);
        }
        // `for (id in updates)`: integer-like ids first, ascending (stable sort).
        let mut updates = self.updates;
        updates.sort_by_key(|(id, _)| array_index(id).map_or(u64::MAX, u64::from));
        for (id, payload) in updates {
            if let (Some(id), Some(Val::Obj(o))) = (Id::new(&id), to_val(&V::Obj(payload))) {
                out.push(DbOp::Set(id, o));
            }
        }
        for (_, payload) in self.inserts {
            if let Some(Val::Obj(o)) = to_val(&V::Obj(payload)) {
                out.push(DbOp::Insert(o));
            }
        }
        out
    }
}

/// bulk.js:6 removeHidden: deletes `_` keys recursively and a truthy `$loki`. `path` holds
/// the objects being walked. On a cycle vanilla recurses until the stack overflows; walking
/// a node again repeats only finished (idempotent) work, so throwing at the first re-entry
/// leaves the same state, without risking Rust's own stack.
fn remove_hidden(obj: &V, path: &mut Vec<V>) -> Result<(), Crash> {
    if path.iter().any(|p| same_ref(p, obj)) {
        return Err(Crash("Maximum call stack size exceeded".into()));
    }
    path.push(obj.clone());
    // `for (var i in obj)`, in order, so a throw leaves later `_` keys in place: own keys of
    // an object, indices of an array (never `_`); a string's characters need nothing.
    let walked = match obj {
        V::Obj(o) => o.keys().into_iter().try_for_each(|k| {
            if k.as_str().starts_with('_') {
                o.delete(k);
                Ok(())
            } else {
                visit_hidden(&o.get(k), path)
            }
        }),
        V::Arr(a) => a.to_vec().iter().try_for_each(|x| visit_hidden(x, path)),
        _ => Ok(()),
    };
    path.pop();
    walked?;
    match obj {
        V::Undef => Err(Crash("Cannot read properties of undefined (reading '$loki')".into())),
        V::Null => Err(Crash("Cannot read properties of null (reading '$loki')".into())),
        V::Obj(o) => {
            if truthy(&o.get(atom!("$loki"))) {
                o.delete(atom!("$loki"));
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// removeHidden's loop body for one property value: arrays recurse per element (a null
/// element throws), objects recurse. Every cycle passes through `remove_hidden` of one of
/// its nodes, so the path check there catches it.
fn visit_hidden(x: &V, path: &mut Vec<V>) -> Result<(), Crash> {
    match x {
        V::Arr(a) => a.to_vec().iter().try_for_each(|e| remove_hidden(e, path)),
        V::Obj(_) => remove_hidden(x, path),
        _ => Ok(()),
    }
}

fn inc_live(o: &O, path: &[Atom], n: f64) {
    let (last, parents) = path.split_last().expect("inc needs a key");
    let mut cur = o.clone();
    for &k in parents {
        cur = match cur.get(k) {
            V::Obj(c) => c,
            _ => {
                let c = O::new();
                cur.set(k, V::Obj(c.clone()));
                c
            }
        };
    }
    let x = cur.get(*last);
    cur.set(*last, js_add(if truthy(&x) { x } else { V::Num(0.0) }, n));
}

/// `x + n` for a truthy `x` (or 0).
fn js_add(x: V, n: f64) -> V {
    match x {
        V::Num(a) => V::Num(a + n),
        V::Bool(b) => V::Num(b as u8 as f64 + n),
        other => V::Str(format!("{}{}", to_js_string(&other), util::num::to_js_string(n)).into()),
    }
}

/// dbBulk (db.js:480): ops in order; an op on a missing id is a no-op; per document
/// updateDocument (db.js:257). Values come out of JSON, so NaN/±Inf results persist as null.
pub fn apply(store: &mut impl DocStore, ops: Vec<DbOp>) {
    for op in ops {
        match op {
            DbOp::Remove(id) => store.remove(&id),
            DbOp::Insert(doc) => store.insert(doc),
            DbOp::Replace(id, doc) => {
                if let Some(d) = store.doc_mut(&id) {
                    *d = doc;
                }
            }
            DbOp::Set(id, set) => {
                if let Some(doc) = store.doc_mut(&id) {
                    for (k, v) in set.0 {
                        doc.set(k, v);
                    }
                }
            }
            DbOp::Inc(id, path, n) => {
                if let Some(doc) = store.doc_mut(&id) {
                    inc_doc(doc, &path, n);
                }
            }
            DbOp::AddToSet(id, k, v) => {
                let Some(doc) = store.doc_mut(&id) else { continue };
                if !doc.get(k).is_some_and(val_truthy) {
                    doc.set(k, Val::Arr(Vec::new()));
                }
                // ponytail: a truthy non-array value would make Loki throw (no `push`); skipped.
                if let Some(Val::Arr(a)) = doc.get_mut(k) {
                    if !a.iter().any(|x| val_strict_eq(x, &v)) {
                        a.push(v);
                    }
                }
            }
            DbOp::Pull(id, k, v) => {
                if let Some(Some(Val::Arr(a))) = store.doc_mut(&id).map(|d| d.get_mut(k)) {
                    if let Some(i) = a.iter().position(|x| val_strict_eq(x, &v)) {
                        a.remove(i);
                    }
                }
            }
        }
    }
}

/// `doc[k] = (doc[k] || 0) + n`, nested along the path. `n` crossed JSON, so a
/// non-finite amount arrives as null (`x + null`).
fn inc_doc(doc: &mut Obj, path: &[Atom], n: f64) {
    let (last, parents) = path.split_last().expect("inc needs a key");
    let mut cur = doc;
    for &k in parents {
        if !matches!(cur.get(k), Some(Val::Obj(_))) {
            cur.set(k, Val::Obj(Obj::new()));
        }
        let Some(Val::Obj(next)) = cur.get_mut(k) else { unreachable!() };
        cur = next;
    }
    let x = cur.get(*last).filter(|x| val_truthy(x)).cloned().unwrap_or(Val::Num(0.0));
    let rhs = if n.is_finite() { V::Num(if n == 0.0 { 0.0 } else { n }) } else { V::Null };
    let sum = match (live::from_val(&x), rhs) {
        (V::Num(a), V::Num(b)) => V::Num(a + b),
        (V::Num(a), _) => V::Num(a),
        (V::Bool(b), r) => V::Num(b as u8 as f64 + live::to_number(&r)),
        (other, r) => V::Str(format!("{}{}", to_js_string(&other), to_js_string(&r)).into()),
    };
    cur.set(*last, to_val(&sum).unwrap());
}

fn val_truthy(v: &Val) -> bool {
    match v {
        Val::Null => false,
        Val::Bool(b) => *b,
        Val::Num(n) => *n != 0.0 && !n.is_nan(),
        Val::Str(s) => !s.is_empty(),
        Val::Arr(_) | Val::Obj(_) => true,
    }
}

/// `indexOf`'s `===` between a stored value and one that crossed JSON (a fresh object).
fn val_strict_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Null, Val::Null) => true,
        (Val::Bool(x), Val::Bool(y)) => x == y,
        (Val::Num(x), Val::Num(y)) => x == y,
        (Val::Str(x), Val::Str(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::{from_val, stringify};

    fn v(json: &str) -> V {
        from_val(&Val::from_json(&util::json::Json::parse(json).unwrap()))
    }

    fn obj(json: &str) -> O {
        match v(json) {
            V::Obj(o) => o,
            _ => panic!(),
        }
    }

    struct Store(Vec<Obj>, Vec<Obj>);

    impl DocStore for Store {
        fn doc_mut(&mut self, id: &Id) -> Option<&mut Obj> {
            self.0.iter_mut().find(|d| d.get(atom!("_id")).and_then(Val::as_str) == Some(id.as_str()))
        }
        fn remove(&mut self, id: &Id) {
            self.0.retain(|d| d.get(atom!("_id")).and_then(Val::as_str) != Some(id.as_str()));
        }
        fn insert(&mut self, doc: Obj) {
            self.1.push(doc);
        }
    }

    fn json(o: &Obj) -> String {
        let mut s = String::new();
        Val::Obj(o.clone()).write_json(&mut s);
        s
    }

    #[test]
    fn alias_snapshot_and_truthy_scalar() {
        let creep = obj(r#"{"_id":"c1","store":{"energy":5},"x":5}"#);
        let mut store = Store(
            vec![match to_val(&V::Obj(creep.clone())) {
                Some(Val::Obj(o)) => o,
                _ => panic!(),
            }],
            vec![],
        );
        let mut b = Bulk::new();
        b.update(Target::Obj(&creep), &v(r#"{"store":{"energy":3},"effects":[{"e":1}],"x":{"a":1}}"#)).unwrap();
        // An absent key's merge base is `{}`, so an array value lands as {"0": …} (vanilla
        // usePower reads effects with Object.values for this reason).
        // Later unrecorded mutations: the aliased store persists them, the fresh effects do not.
        let V::Obj(store_live) = creep.get(atom!("store")) else { panic!() };
        store_live.set(atom!("energy"), V::Num(1.0));
        let V::Obj(effects) = creep.get(atom!("effects")) else { panic!() };
        effects.set(atom!("1"), V::Num(9.0));
        assert_eq!(stringify(&V::Obj(creep.clone())), r#"{"_id":"c1","store":{"energy":1},"x":5,"effects":{"0":{"e":1},"1":9}}"#);
        apply(&mut store, b.execute());
        assert_eq!(json(&store.0[0]), r#"{"_id":"c1","store":{"energy":1},"x":5,"effects":{"0":{"e":1}}}"#);
    }

    #[test]
    fn hidden_inserts_and_order() {
        let mut b = Bulk::new();
        let drop = obj(r#"{"_id":null,"type":"energy","_x":1,"energy":10}"#);
        let n = b.insert(&V::Obj(drop.clone()), None);
        drop.set(atom!("_id"), V::str(&n.to_string()));
        b.update(Target::Obj(&drop), &v(r#"{"energy":7,"_tmp":{"a":1}}"#)).unwrap();
        assert_eq!(b.insert(&v(r#"{"type":"tombstone"}"#), Some("given")), 2);
        b.remove("1");
        assert_eq!(b.insert(&v(r#"{"type":"ruin"}"#), None), 3, "counters are never reused");
        b.update(Target::Id("9"), &v(r#"{"a":1}"#)).unwrap();
        b.update(Target::Id("zz"), &v(r#"{"a":1}"#)).unwrap();
        b.update(Target::Id("2"), &v(r#"{"b":1}"#)).unwrap();
        assert_eq!(b.update(Target::Id("zz"), &v(r#"{"o":{}}"#)), Err(Crash("can not update an object diff property 'o' without object reference".into())));
        b.inc(Target::Id("zz"), &[atom!("n")], 2.0);
        let ops = b.execute();
        let names: Vec<String> = ops.iter().map(|o| format!("{o:?}")).collect();
        assert_eq!(names.len(), 5, "{names:?}");
        assert!(matches!(&ops[0], DbOp::Inc(id, _, n) if id.as_str() == "zz" && *n == 2.0));
        assert!(matches!(&ops[1], DbOp::Set(id, _) if id.as_str() == "9"), "integer-like ids first");
        assert!(matches!(&ops[2], DbOp::Set(id, _) if id.as_str() == "zz"));
        assert!(matches!(&ops[3], DbOp::Insert(o) if json(o) == r#"{"type":"tombstone","_id":"given","b":1}"#));
        assert!(matches!(&ops[4], DbOp::Insert(o) if json(o) == r#"{"type":"ruin"}"#));
    }

    #[test]
    fn inc_add_pull_and_dotted_paths() {
        let user = obj(r#"{"_id":"u","money":"5","resources":{"pixel":1}}"#);
        let mut store = Store(
            vec![match to_val(&V::Obj(user.clone())) {
                Some(Val::Obj(o)) => o,
                _ => panic!(),
            }],
            vec![],
        );
        let mut b = Bulk::new();
        b.inc(Target::Obj(&user), &[atom!("money")], 3.0);
        b.inc(Target::Obj(&user), &[atom!("resources"), atom!("pixel")], 2.0);
        b.inc(Target::Obj(&user), &[atom!("gcl")], f64::NAN);
        b.add_to_set(Target::Id("u"), atom!("rooms"), &V::str("W1N1"));
        b.add_to_set(Target::Id("u"), atom!("rooms"), &V::str("W1N1"));
        b.add_to_set(Target::Id("u"), atom!("rooms"), &V::str("W2N2"));
        b.pull(Target::Id("u"), atom!("rooms"), &V::str("W1N1"));
        b.add_to_set(Target::Id("u"), atom!("skip"), &V::Undef);
        assert_eq!(stringify(&V::Obj(user.clone())), r#"{"_id":"u","money":"53","resources":{"pixel":3},"gcl":null}"#);
        apply(&mut store, b.execute());
        assert_eq!(json(&store.0[0]), r#"{"_id":"u","money":"53","resources":{"pixel":3},"gcl":0,"rooms":["W2N2"]}"#);
    }

    #[test]
    fn remove_hidden_crashes_like_vanilla() {
        // A live array is the merge base (aliased), so its null element reaches removeHidden.
        let o = obj(r#"{"_id":"a","path":[]}"#);
        let mut b = Bulk::new();
        let r = b.update(Target::Obj(&o), &v(r#"{"path":[1,null],"k":2}"#));
        assert_eq!(r, Err(Crash("Cannot read properties of null (reading '$loki')".into())));
        assert_eq!(stringify(&V::Obj(o)), r#"{"_id":"a","path":[1,null],"k":2}"#, "the live merge happened first");
        assert!(b.execute().is_empty());
        // The throw stops the key walk: `_` keys after it stay on the aliased live object.
        let o = obj(r#"{"_id":"a","store":{"a":[],"_x":1,"b":{"_y":2}}}"#);
        assert!(Bulk::new().update(Target::Obj(&o), &v(r#"{"store":{"a":[null]}}"#)).is_err());
        assert_eq!(stringify(&V::Obj(o)), r#"{"_id":"a","store":{"a":[null],"_x":1,"b":{"_y":2}}}"#);
    }

    #[test]
    fn cyclic_update_crashes_without_deep_recursion() {
        // Vanilla's removeHidden overflows the JS stack on a cycle; a depth limit used to
        // overflow Rust's (an uncatchable abort in debug builds). 64 KiB proves no deep recursion.
        std::thread::Builder::new()
            .stack_size(64 << 10)
            .spawn(|| {
                let o = obj(r#"{"_id":"a","store":{"_x":1}}"#);
                let data = v(r#"{"store":{"k":1},"list":[]}"#);
                let V::Obj(d) = &data else { panic!() };
                let V::Obj(store) = d.get(atom!("store")) else { panic!() };
                store.set(atom!("self"), V::Obj(store.clone()));
                let mut b = Bulk::new();
                assert_eq!(b.update(Target::Obj(&o), &data), Err(Crash("Maximum call stack size exceeded".into())));
                // The `_x` before the cycle went, as in vanilla; nothing was buffered.
                let V::Obj(live) = o.get(atom!("store")) else { panic!() };
                assert!(!live.has(atom!("_x")) && live.has(atom!("self")));
                assert!(b.execute().is_empty());
                // A cycle through an array element, and a shared acyclic sub-object (fine).
                let l = live::A::new();
                l.push(V::Arr(l.clone()));
                let data = V::Obj(O::new());
                let V::Obj(d) = &data else { panic!() };
                d.set(atom!("list"), V::Arr(l));
                assert!(Bulk::new().update(Target::Obj(&obj(r#"{"_id":"b"}"#)), &data).is_err());
                let shared = v(r#"{"e":1,"_h":2}"#);
                d.set(atom!("list"), V::Arr(live::A::from_vec(vec![shared.clone(), shared])));
                let target = obj(r#"{"_id":"c"}"#);
                let mut b = Bulk::new();
                assert!(b.update(Target::Obj(&target), &data).is_ok());
                // node: the live copy keeps `_h`, the $set payload does not.
                assert_eq!(stringify(&V::Obj(target)), r#"{"_id":"c","list":{"0":{"e":1,"_h":2},"1":{"e":1,"_h":2}}}"#);
                let DbOp::Set(_, set) = &b.execute()[0] else { panic!() };
                assert_eq!(stringify(&from_val(&Val::Obj(set.clone()))), r#"{"list":{"0":{"e":1},"1":{"e":1}}}"#);
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
