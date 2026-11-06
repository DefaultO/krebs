//! lodash/JS-semantics fuzz for engine::live (CHITIN_ORACLE=1): tools/oracle/gen/lodash-fixture.js (the real
//! lodash 3.10.1 and node values) runs again with other PRNG seeds and every case is compared with
//! engine::live. Seeds: CHITIN_ORACLE_SEED (default 1) and the next CHITIN_ORACLE_LODASH_RUNS-1 (default 2;
//! ~3000 cases each). Failing cases go to fixtures/oracle/fuzz/lodash-seed-<s>.ndjson, replayed every run.

mod graph;

use engine::live::{self, V};
use graph::{decode, encode, items, tuple};
use std::path::PathBuf;
use util::json::Json;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

// ponytail: `run` and `check` copy tests/lodash.rs (a test crate root's private items are unreachable);
// drop the copy once tests/lodash.rs exports them (interface request).
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

/// One fixture line: Ok, or our output.
fn check(line: &str) -> Result<(), String> {
    let c = Json::parse(line).unwrap();
    let op = c.get("op").and_then(Json::as_str).unwrap();
    let mut want = c.get("out").unwrap().clone();
    let input = items(&decode(c.get("in").unwrap()));
    // JSON.stringify threw on a cycle: compare the rest.
    if op == "js" && matches!(want.at(2).and_then(|x| x.at(4)), Some(Json::Arr(a)) if a.len() == 3) {
        let v = &input[0];
        let got = tuple(vec![V::Bool(live::truthy(v)), V::Num(live::to_number(v)), V::Str(live::to_js_string(v)), V::Num(live::parse_int(v))]);
        if let Json::Arr(root) = &mut want {
            if let Some(Json::Arr(list)) = root.get_mut(2) {
                list.pop();
            }
        }
        return if encode(&got) == want { Ok(()) } else { Err(encode(&got).stringify()) };
    }
    let got = encode(&run(op, &input));
    if got == want { Ok(()) } else { Err(got.stringify()) }
}

#[test]
fn lodash_fuzz_regressions() {
    let dir = manifest().join("tests/fixtures/oracle/fuzz");
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with("lodash-seed-") {
            for line in std::fs::read_to_string(e.path()).unwrap().lines().filter(|l| !l.is_empty()) {
                check(line).unwrap_or_else(|ours| panic!("{}: {line}\n  ours: {ours}", e.path().display()));
            }
        }
    }
}

const RESEED: &str = r#"
const path = require('path');
const common = require(path.resolve('tools/oracle/lib/common.js'));
const prng = common.prng;
common.prng = () => prng(Number(process.env.CHITIN_LODASH_SEED));
common.writeFixture = (rel, lines) => { for (const l of lines) process.stdout.write(JSON.stringify(l) + '\n'); };
console.log = (...a) => process.stderr.write(a.join(' ') + '\n');
require(path.resolve('tools/oracle/gen/lodash-fixture.js'));
"#;

#[test]
fn lodash_fuzz() {
    if !std::env::var("CHITIN_ORACLE").is_ok_and(|v| !v.is_empty() && v != "0") {
        return;
    }
    let seed: u64 = std::env::var("CHITIN_ORACLE_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let runs: u64 = std::env::var("CHITIN_ORACLE_LODASH_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
    let node = std::env::var("CHITIN_NODE").unwrap_or_else(|_| "node".into());
    let (mut cases, mut failed) = (0, 0);
    for s in seed..seed + runs {
        let out = std::process::Command::new(&node)
            .current_dir(manifest().join("../.."))
            .args(["-e", RESEED])
            .env("CHITIN_LODASH_SEED", (0x10da5 + s * 7919).to_string())
            .stderr(std::process::Stdio::inherit())
            .output()
            .expect("spawn node (set CHITIN_NODE)");
        assert!(out.status.success(), "lodash generator failed for seed {s}");
        let mut bad = Vec::new();
        for line in String::from_utf8(out.stdout).unwrap().lines() {
            cases += 1;
            if let Err(ours) = check(line) {
                println!("FAIL seed {s}: {line}\n  ours: {ours}");
                bad.push(line.to_string());
            }
        }
        if !bad.is_empty() {
            failed += bad.len();
            let dir = manifest().join("tests/fixtures/oracle/fuzz");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("lodash-seed-{s}.ndjson")), bad.join("\n") + "\n").unwrap();
        }
    }
    println!("lodash fuzz: {cases} cases, {failed} failed");
    assert!(failed == 0, "{failed} lodash case(s) differ");
}
