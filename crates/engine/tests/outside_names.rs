//! `Val::from_json` stops adding names to the atom table at its limit. A file of its own, so the process this runs in
//! holds nothing else that needs a new name afterwards.

use engine::doc::{Atom, Val};
use util::json::Json;

fn object(names: impl Iterator<Item = String>) -> Json {
    Json::Obj(names.map(|k| (k, Json::Null)).collect())
}

#[test]
fn names_from_outside_stop_at_the_limit() {
    let known = Atom::intern("known");
    let Val::Obj(first) = Val::from_json(&object((0..1 << 18).map(|i| format!("name{i}")))) else { panic!() };
    let limit = first.len();
    assert!((1 << 16..1 << 18).contains(&limit), "{limit}");
    // Spent: a new name is left out, a known one still goes in, and so does anything the server interns itself.
    let Val::Obj(next) = Val::from_json(&object(["fresh".to_string(), "known".to_string()].into_iter())) else { panic!() };
    assert_eq!(next.len(), 1);
    assert!(next.get(known).is_some());
    let Val::Obj(again) = Val::from_json(&object((0..limit).map(|i| format!("name{i}")))) else { panic!() };
    assert_eq!(again.len(), limit, "names already in the table are not counted");
    assert_eq!(Atom::intern("fresh").as_str(), "fresh");
}
