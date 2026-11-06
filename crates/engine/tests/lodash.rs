//! engine::live against the real lodash 3.10.1 and node's value semantics
//! (fixture: node tools/oracle/gen/lodash-fixture.js).

mod graph;

use engine::live::{self, V};
use graph::{decode, encode, items, tuple};
use util::json::Json;

const FIXTURE: &str = include_str!("fixtures/lodash/cases.ndjson");

fn run(op: &str, input: &[V]) -> V {
    match op {
        "merge" => {
            let ret = live::merge(&input[0], &input[1]);
            tuple(vec![input[0].clone(), input[1].clone(), ret])
        }
        "cloneDeep" => tuple(vec![input[0].clone(), live::clone_deep(&input[0])]),
        "isEqual" => tuple(vec![V::Bool(live::is_equal(&input[0], &input[1]))]),
        "assign" => {
            let (V::Obj(dst), V::Obj(src)) = (&input[0], &input[1]) else { panic!("assign takes objects") };
            live::assign(dst, src);
            tuple(vec![input[0].clone(), input[1].clone(), input[0].clone()])
        }
        "forEach" => {
            let V::Obj(o) = &input[0] else { panic!() };
            let actions = items(&input[1]);
            let mut visits = Vec::new();
            for (i, k) in o.keys().into_iter().enumerate() {
                visits.push(tuple(vec![V::str(k.as_str()), o.get(k)]));
                for a in items(&actions[i]) {
                    let a = items(&a);
                    let (V::Str(what), V::Str(key)) = (&a[0], &a[1]) else { panic!() };
                    let key = engine::doc::Atom::intern(key);
                    match &**what {
                        "del" => _ = o.delete(key),
                        "add" => o.set(key, V::str("added")),
                        _ => o.set(key, V::Num(42.0)),
                    }
                }
            }
            tuple(vec![input[0].clone(), tuple(visits)])
        }
        "js" => {
            let v = &input[0];
            let json = match live::to_val(v) {
                None => V::Undef,
                Some(_) => V::str(&live::stringify(v)),
            };
            tuple(vec![V::Bool(live::truthy(v)), V::Num(live::to_number(v)), V::Str(live::to_js_string(v)), V::Num(live::parse_int(v)), json])
        }
        _ => panic!("unknown op {op}"),
    }
}

#[test]
fn matches_lodash() {
    let mut failures = Vec::new();
    let mut cases = 0;
    for line in FIXTURE.lines() {
        let c = Json::parse(line).unwrap();
        let op = c.get("op").and_then(Json::as_str).unwrap();
        let mut want = c.get("out").unwrap().clone();
        let input = items(&decode(c.get("in").unwrap()));
        // JSON.stringify threw on a cycle: compare the rest.
        let throws = op == "js" && matches!(want.at(2).and_then(|x| x.at(4)), Some(Json::Arr(a)) if a.len() == 3);
        if throws {
            let v = &input[0];
            let got = tuple(vec![V::Bool(live::truthy(v)), V::Num(live::to_number(v)), V::Str(live::to_js_string(v)), V::Num(live::parse_int(v))]);
            if let Json::Arr(root) = &mut want {
                if let Some(Json::Arr(list)) = root.get_mut(2) {
                    list.pop();
                }
            }
            if encode(&got) != want {
                failures.push(format!("{op} {line}\n  ours: {}", encode(&got).stringify()));
            }
            cases += 1;
            continue;
        }
        let got = encode(&run(op, &input));
        if got != want {
            failures.push(format!("{op} {line}\n  ours: {}", got.stringify()));
        }
        cases += 1;
    }
    assert!(cases > 3000, "{cases} cases");
    if !failures.is_empty() {
        panic!("{} of {cases} cases differ:\n{}", failures.len(), failures[..failures.len().min(8)].join("\n"));
    }
}
