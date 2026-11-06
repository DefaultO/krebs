//! The graph codec of tools/oracle/lib/common.js: live values with identity, cycles,
//! undefined and NaN/±Infinity/-0 as JSON.

#![allow(dead_code)]

use engine::doc::Atom;
use engine::live::{A, O, V};
use util::json::Json;

pub fn decode(j: &Json) -> V {
    dec(j, &mut Vec::new())
}

fn dec(j: &Json, nodes: &mut Vec<V>) -> V {
    match j {
        Json::Null => V::Null,
        Json::Bool(b) => V::Bool(*b),
        Json::Num(n) => V::Num(*n),
        Json::Str(s) => V::str(s),
        Json::Arr(a) => match a[0].as_str().unwrap() {
            "u" => V::Undef,
            "n" => V::Num(match a[1].as_str().unwrap() {
                "NaN" => f64::NAN,
                "Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                "-0" => -0.0,
                x => panic!("bad number tag {x}"),
            }),
            "r" => nodes[a[1].as_f64().unwrap() as usize].clone(),
            "o" => {
                let o = O::new();
                assert_eq!(a[1].as_f64().unwrap() as usize, nodes.len());
                nodes.push(V::Obj(o.clone()));
                for kv in a[2].as_array().unwrap() {
                    let kv = kv.as_array().unwrap();
                    let v = dec(&kv[1], nodes);
                    o.set(Atom::intern(kv[0].as_str().unwrap()), v);
                }
                V::Obj(o)
            }
            "a" => {
                let arr = A::new();
                assert_eq!(a[1].as_f64().unwrap() as usize, nodes.len());
                nodes.push(V::Arr(arr.clone()));
                for x in a[2].as_array().unwrap() {
                    let v = dec(x, nodes);
                    arr.push(v);
                }
                V::Arr(arr)
            }
            t => panic!("bad tag {t}"),
        },
        Json::Obj(_) => panic!("plain JSON object in a graph"),
    }
}

pub fn encode(v: &V) -> Json {
    enc(v, &mut Vec::new())
}

fn tag(t: &str, rest: Vec<Json>) -> Json {
    let mut a = vec![Json::Str(t.into())];
    a.extend(rest);
    Json::Arr(a)
}

fn same(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Obj(x), V::Obj(y)) => x.same(y),
        (V::Arr(x), V::Arr(y)) => x.same(y),
        _ => false,
    }
}

fn enc(v: &V, seen: &mut Vec<V>) -> Json {
    match v {
        V::Undef => tag("u", vec![]),
        V::Null => Json::Null,
        V::Bool(b) => Json::Bool(*b),
        V::Num(n) if n.is_nan() => tag("n", vec![Json::Str("NaN".into())]),
        V::Num(n) if n.is_infinite() => tag("n", vec![Json::Str(if *n > 0.0 { "Infinity" } else { "-Infinity" }.into())]),
        V::Num(n) if *n == 0.0 && n.is_sign_negative() => tag("n", vec![Json::Str("-0".into())]),
        V::Num(n) => Json::Num(*n),
        V::Str(s) => Json::Str(s.to_string()),
        _ => {
            if let Some(id) = seen.iter().position(|x| same(x, v)) {
                return tag("r", vec![Json::Num(id as f64)]);
            }
            let id = Json::Num(seen.len() as f64);
            seen.push(v.clone());
            match v {
                V::Obj(o) => {
                    let pairs = o.keys().into_iter().map(|k| Json::Arr(vec![Json::Str(k.as_str().into()), enc(&o.get(k), seen)])).collect();
                    tag("o", vec![id, Json::Arr(pairs)])
                }
                V::Arr(a) => {
                    let items = a.to_vec().iter().map(|x| enc(x, seen)).collect();
                    tag("a", vec![id, Json::Arr(items)])
                }
                _ => unreachable!(),
            }
        }
    }
}

/// An object node (id 0) with exactly these entries in this order, sharing one id space.
pub fn encode_entries(entries: &[(String, V)]) -> Json {
    let mut seen = vec![V::Undef]; // id 0 is the root; Undef never matches a node
    let pairs = entries.iter().map(|(k, v)| Json::Arr(vec![Json::Str(k.clone()), enc(v, &mut seen)])).collect();
    tag("o", vec![Json::Num(0.0), Json::Arr(pairs)])
}

/// Items of a tuple root (`["a", id, [...]]`).
pub fn items(v: &V) -> Vec<V> {
    match v {
        V::Arr(a) => a.to_vec(),
        _ => panic!("tuple expected"),
    }
}

pub fn tuple(items: Vec<V>) -> V {
    V::Arr(A::from_vec(items))
}
