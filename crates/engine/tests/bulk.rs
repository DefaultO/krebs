//! engine::bulk against the real driver/lib/bulk.js and the storage update rules
//! (fixture: node tools/oracle/gen/bulk-fixture.js). Replays every op on the live heap,
//! compares the live room state after each op, then the documents after dbBulk (or the
//! error execute() threw on a cyclic payload).

mod graph;

use engine::atom;
use engine::bulk::{apply, Bulk, DocStore, Target};
use engine::doc::{Atom, Id, Obj, Val};
use engine::live::{to_js_string, to_val, truthy, RoomObjects, O, V};
use graph::{decode, encode_entries};
use util::json::Json;

const FIXTURE: &str = include_str!("fixtures/bulk/cases.ndjson");

/// A Loki collection: data[] order, unique `_id`.
struct Store(Vec<Obj>);

impl Store {
    fn find(&self, id: &Id) -> Option<usize> {
        self.0.iter().position(|d| d.get(atom!("_id")).and_then(Val::as_str) == Some(id.as_str()))
    }
}

impl DocStore for Store {
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

fn state(ro: &RoomObjects) -> Json {
    let entries: Vec<(String, V)> = ro.keys().iter().map(|k| (k.to_string(), V::Obj(ro.get(k).unwrap()))).collect();
    encode_entries(&entries)
}

fn walk(ro: &RoomObjects, path: &[Json]) -> V {
    let mut v = V::Obj(ro.get(path[0].as_str().unwrap()).expect("root object"));
    for k in &path[1..] {
        v = match (&v, k) {
            (V::Obj(o), Json::Str(s)) => o.get(Atom::intern(s)),
            (V::Arr(a), Json::Num(i)) => a.get(*i as usize),
            _ => panic!("bad path {path:?}"),
        };
    }
    v
}

fn set(ro: &RoomObjects, path: &[Json], value: V) {
    let (key, parent) = path.split_last().unwrap();
    match (walk(ro, parent), key) {
        (V::Obj(o), Json::Str(s)) => o.set(Atom::intern(s), value),
        (V::Arr(a), Json::Num(i)) => a.set(*i as usize, value),
        _ => panic!("bad path {path:?}"),
    }
}

fn replay(case: &Json) -> Result<(), String> {
    let V::Obj(root) = decode(case.get("objects").unwrap()) else { panic!() };
    let mut ro = RoomObjects::new();
    let mut store = Store(Vec::new());
    for k in root.keys() {
        let v = root.get(k);
        // The objects are JSON-shaped: the DB holds exactly their JSON.
        store.0.push(to_val(&v).unwrap().as_obj().unwrap().clone());
        let V::Obj(o) = v else { panic!() };
        ro.insert(k.as_str().into(), o);
    }
    let mut bulk = Bulk::new();
    let mut want_state = case.get("objects").unwrap().clone();
    let steps = case.get("steps").unwrap().as_array().unwrap();
    for (i, (op, step)) in case.get("ops").unwrap().as_array().unwrap().iter().zip(steps).enumerate() {
        let op = op.as_array().unwrap();
        let name = op[0].as_str().unwrap();
        let obj = |t: &Json| -> Option<O> { (t.at(0)?.as_str()? == "o").then(|| ro.get(t.at(1).unwrap().as_str().unwrap()).unwrap()) };
        let target_of = |t: &Json, o: &Option<O>| -> String {
            if o.is_some() {
                String::new()
            } else {
                t.at(1).unwrap().as_str().unwrap().to_string()
            }
        };
        let mut ret = Json::Null;
        let mut crash = Json::Null;
        match name {
            "update" | "inc" | "addToSet" | "pull" => {
                let o = obj(&op[1]);
                let s = target_of(&op[1], &o);
                let t = match &o {
                    Some(o) => Target::Obj(o),
                    None => Target::Id(&s),
                };
                match name {
                    "update" => {
                        if let Err(e) = bulk.update(t, &decode(&op[2])) {
                            crash = Json::Str(e.0.to_string());
                        }
                    }
                    "inc" => {
                        let V::Num(n) = decode(&op[3]) else { panic!() };
                        bulk.inc(t, &[Atom::intern(op[2].as_str().unwrap())], n);
                    }
                    "addToSet" => bulk.add_to_set(t, Atom::intern(op[2].as_str().unwrap()), &decode(&op[3])),
                    _ => bulk.pull(t, Atom::intern(op[2].as_str().unwrap()), &decode(&op[3])),
                }
            }
            "insert" => {
                let data = decode(&op[1]);
                let n = if op[3].as_str() == Some("tmp") {
                    // _create-energy: `obj._id = "" + bulk.insert(obj); roomObjects[obj._id] = obj`
                    let V::Obj(o) = &data else { panic!() };
                    let n = bulk.insert(&data, None);
                    o.set(atom!("_id"), V::str(&n.to_string()));
                    ro.insert(n.to_string().into(), o.clone());
                    n
                } else {
                    bulk.insert(&data, op[2].as_str().filter(|s| !s.is_empty()))
                };
                ret = Json::Num(n as f64);
            }
            "remove" => {
                match obj(&op[1]) {
                    Some(o) => {
                        // vanilla call site: bulk.remove(object._id), a no-op for a falsy id
                        let id = o.get(atom!("_id"));
                        if truthy(&id) {
                            bulk.remove(&to_js_string(&id));
                        }
                        if op[2].as_bool() == Some(true) {
                            ro.remove(op[1].at(1).unwrap().as_str().unwrap());
                        }
                    }
                    None => bulk.remove(op[1].at(1).unwrap().as_str().unwrap()),
                }
            }
            "set" => set(&ro, op[1].as_array().unwrap(), decode(&op[2])),
            "alias" => set(&ro, op[1].as_array().unwrap(), walk(&ro, op[2].as_array().unwrap())),
            "del" => {
                let path = op[1].as_array().unwrap();
                let (key, parent) = path.split_last().unwrap();
                let V::Obj(o) = walk(&ro, parent) else { panic!() };
                o.delete(Atom::intern(key.as_str().unwrap()));
            }
            _ => panic!("unknown op {name}"),
        }
        let at = || format!("step {i} {}", Json::Arr(op.clone()).stringify());
        if ret != *step.get("ret").unwrap() || crash != *step.get("crash").unwrap() {
            return Err(format!("{}: returned {ret} / threw {crash}, bulk.js {} / {}", at(), step.get("ret").unwrap(), step.get("crash").unwrap()));
        }
        if let Some(s) = step.get("state") {
            want_state = s.clone();
        }
        let got = state(&ro);
        if got != want_state {
            return Err(format!("{}: live state\n  ours:    {}\n  bulk.js: {}", at(), got.stringify(), want_state.stringify()));
        }
    }
    if let Some(want) = case.get("executeError").and_then(Json::as_str) {
        // JSON.stringify threw on a cyclic payload; ours panics with the same text.
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bulk.execute())).err().ok_or("execute did not throw")?;
        let got = err.downcast_ref::<&str>().copied().unwrap_or("?");
        return if got == want { Ok(()) } else { Err(format!("execute threw {got:?}, bulk.js {want:?}")) };
    }
    apply(&mut store, bulk.execute());
    let got = Json::Arr(store.0.iter().map(|d| Val::Obj(d.clone()).to_json()).collect());
    if got != *case.get("docs").unwrap() {
        return Err(format!("persisted docs\n  ours:    {}\n  bulk.js: {}", got.stringify(), case.get("docs").unwrap().stringify()));
    }
    Ok(())
}

#[test]
fn matches_bulk_js() {
    let mut failures = Vec::new();
    let mut cases = 0;
    let mut ops = 0;
    for line in FIXTURE.lines() {
        let case = Json::parse(line).unwrap();
        ops += case.get("ops").unwrap().as_array().unwrap().len();
        cases += 1;
        if let Err(e) = replay(&case) {
            failures.push(format!("case {cases}: {e}"));
        }
    }
    assert!(cases >= 300 && ops > 2000, "{cases} cases, {ops} ops");
    if !failures.is_empty() {
        panic!("{} of {cases} scenarios differ:\n{}", failures.len(), failures[..failures.len().min(5)].join("\n"));
    }
}
