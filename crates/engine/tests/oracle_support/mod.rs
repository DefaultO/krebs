//! Shared by the oracle replays (oracle_processor.rs, oracle_global.rs): fixture discovery, the mirror
//! parity map that decides which cases are enforced, the report, and the CHITIN_ORACLE=1 fuzz loop.
//!
//! A case is *enforced* once every reference file it ran (the fixture's `reached`, from V8 coverage) has a
//! Rust mirror whose `//! parity:` is FAITHFUL or DIVERGENT(…): a FAITHFUL file must pass its cases.
//! Other cases are *pending*: not run, only counted. Cases that reached an F row of spec 01 §19A
//! (`fRows`) are skipped. Environment:
//!   CHITIN_ORACLE_STRICT=1      enforce every case (what a porter runs while porting)
//!   CHITIN_ORACLE_CASE=<text>   only cases whose name contains <text>; runs and prints them all
//!   CHITIN_ORACLE=1             also run the fuzz tests: node generates random scenarios; enforced (with
//!                               STRICT: all) failing ones are written to fixtures/oracle/fuzz/
//!   CHITIN_ORACLE_SEED, CHITIN_ORACLE_N   fuzz seed (default 1) and case count (default 200)
//!   CHITIN_NODE                 node 22 (default E:\screeps\node22\node-v22.22.3-win-x64\node.exe)
#![allow(dead_code)]

use engine::scenario::first_diff;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use util::json::Json;

pub fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

/// reference/… path → (Rust file, parity) from every `//! mirror:` header of the engine crate. Only engine
/// code runs in these replays: a mirror elsewhere (backend/src/png.rs claims backend/lib/utils.js for its
/// map helpers) must not hold engine cases (the cron jobs' utils.js calls) pending.
pub fn mirrors() -> HashMap<String, (String, String)> {
    let mut rs = Vec::new();
    files(&manifest().join("src"), "rs", &mut rs);
    let mut map = HashMap::new();
    for f in rs {
        let p = f.to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        let header = |tag: &str| text.lines().take_while(|l| l.starts_with("//!")).find_map(|l| l.strip_prefix(tag).map(str::trim));
        if let (Some(m), Some(parity)) = (header("//! mirror:"), header("//! parity:")) {
            let short = p[p.find("crates/").unwrap_or(0)..].to_string();
            for path in m.split(',') {
                map.insert(path.trim().to_string(), (short.clone(), parity.to_string()));
            }
        }
    }
    map
}

fn ported(parity: &str) -> bool {
    parity.starts_with("FAITHFUL") || parity.starts_with("DIVERGENT")
}

pub struct Case {
    pub file: String,
    pub name: String,
    pub scenario: Json,
    pub result: Json,
}

/// Every `{name, scenario, result}` line of kind `kind` under tests/fixtures (porters keep theirs in their
/// own fixture areas).
pub fn cases(kind: &str) -> Vec<Case> {
    let mut paths = Vec::new();
    files(&manifest().join("tests/fixtures"), "ndjson", &mut paths);
    let mut out = Vec::new();
    for p in paths {
        let rel = p.strip_prefix(manifest()).unwrap_or(&p).to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(&p).unwrap();
        if !text.starts_with("{\"name\":") || !text.lines().next().is_some_and(|l| l.contains("\"scenario\":{")) {
            continue;
        }
        for (i, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
            let j = Json::parse(line).unwrap_or_else(|e| panic!("{rel}:{}: {e}", i + 1));
            if let Some(c) = case(&rel, &j).filter(|c| c.scenario.get("kind").and_then(Json::as_str) == Some(kind)) {
                out.push(c);
            }
        }
    }
    out
}

fn case(file: &str, j: &Json) -> Option<Case> {
    Some(Case { file: file.into(), name: j.get("name").and_then(Json::as_str).unwrap_or("?").into(), scenario: j.get("scenario")?.clone(), result: j.get("result")?.clone() })
}

pub fn strs(j: Option<&Json>) -> Vec<String> {
    j.and_then(Json::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

pub enum Verdict {
    Pass,
    Skip(String),
    Fail(String),
}

/// Skip on F rows, else compare `actual` (or its error) with the fixture's outcome. BUG-DOTKEY-INC alone is
/// not skipped: the oracle's literal `"a.b"` keys are folded into the nested value chitin's fix produces.
pub fn verdict(c: &Case, actual: impl FnOnce() -> Result<Json, String>) -> Verdict {
    let f_rows = strs(c.result.get("fRows"));
    let mut expected = c.result.get("outcome").cloned().unwrap_or(Json::Null);
    if f_rows == ["BUG-DOTKEY-INC"] {
        fold_dotted_incs(&mut expected);
    } else if !f_rows.is_empty() {
        return Verdict::Skip(format!("F rows {f_rows:?}"));
    }
    match actual() {
        Err(e) => Verdict::Fail(e),
        Ok(actual) => match first_diff(&expected, &actual) {
            None => Verdict::Pass,
            Some(d) => Verdict::Fail(match c.result.get("crash").and_then(Json::as_str) {
                Some(crash) => format!("{d}\n      (vanilla crashed: {crash})"),
                None => d,
            }),
        },
    }
}

/// The Rust mirrors of the case's reached reference files that are not ported yet.
fn pending(c: &Case, mirrors: &HashMap<String, (String, String)>) -> Vec<String> {
    strs(c.result.get("reached"))
        .into_iter()
        .filter_map(|r| mirrors.get(&r).filter(|(_, parity)| !ported(parity)).map(|(f, parity)| format!("{f} ({parity})")))
        .collect()
}

pub fn env_flag(k: &str) -> bool {
    std::env::var(k).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Replays every fixture of `kind`; fails on enforced differences.
pub fn check_all(kind: &str, replay: impl Fn(&Case) -> Verdict) {
    let strict = env_flag("CHITIN_ORACLE_STRICT");
    let filter = std::env::var("CHITIN_ORACLE_CASE").ok().filter(|f| !f.is_empty());
    let mirrors = mirrors();
    let (mut pass, mut skip, mut pending_n, mut failed) = (0, 0, 0, Vec::new());
    for c in cases(kind) {
        if filter.as_ref().is_some_and(|f| !c.name.contains(f.as_str())) {
            continue;
        }
        let pending = pending(&c, &mirrors);
        let enforced = strict || pending.is_empty();
        if !enforced && filter.is_none() {
            pending_n += 1; // not run: its unported files are stubs
            continue;
        }
        let tags = strs(c.scenario.get("tags")).join(", ");
        match replay(&c) {
            Verdict::Pass => pass += 1,
            Verdict::Skip(why) => {
                skip += 1;
                if filter.is_some() {
                    println!("SKIP {} :: {} — {why}", c.file, c.name);
                }
            }
            Verdict::Fail(d) if enforced => failed.push(format!("FAIL {} :: {} [{tags}]\n      {d}", c.file, c.name)),
            Verdict::Fail(d) => {
                pending_n += 1;
                println!("PENDING {} :: {} [{tags}]\n      {d}\n      not ported: {}", c.file, c.name, pending.join(", "));
            }
        }
    }
    println!("{kind} oracle: {pass} passed, {} failed, {pending_n} pending (unported files), {skip} skipped", failed.len());
    for f in &failed {
        println!("{f}");
    }
    assert!(failed.is_empty(), "{} enforced {kind} oracle case(s) differ from the official engine", failed.len());
}

fn node() -> String {
    std::env::var("CHITIN_NODE").unwrap_or_else(|_| {
        let n22 = r"E:\screeps\node22\node-v22.22.3-win-x64\node.exe";
        if Path::new(n22).exists() { n22.into() } else { "node".into() }
    })
}

/// CHITIN_ORACLE=1: `node tools/oracle/<script> fuzz <seed> <n> <generator>` (room, recorded, global);
/// enforced failures become fixtures.
pub fn fuzz(kind: &str, script: &str, replay: impl Fn(&Case) -> Verdict) {
    if !env_flag("CHITIN_ORACLE") {
        return;
    }
    let strict = env_flag("CHITIN_ORACLE_STRICT");
    let seed = std::env::var("CHITIN_ORACLE_SEED").unwrap_or_else(|_| "1".into());
    let n = std::env::var("CHITIN_ORACLE_N").unwrap_or_else(|_| "200".into());
    let out = std::process::Command::new(node())
        .current_dir(manifest().join("../.."))
        .args([&format!("tools/oracle/{script}"), "fuzz", &seed, &n, kind])
        .stderr(std::process::Stdio::inherit())
        .output()
        .expect("spawn node (set CHITIN_NODE to node 22)");
    assert!(out.status.success(), "oracle fuzz run failed");
    let mirrors = mirrors();
    let (mut pass, mut pending_n, mut written) = (0, 0, Vec::new());
    let mut skipped: std::collections::BTreeMap<String, usize> = Default::default();
    for line in String::from_utf8(out.stdout).unwrap().lines() {
        let c = case("fuzz", &Json::parse(line).unwrap()).expect("fuzz line without scenario/result");
        if !strict && !pending(&c, &mirrors).is_empty() {
            pending_n += 1;
            continue;
        }
        match replay(&c) {
            Verdict::Pass => pass += 1,
            Verdict::Skip(why) => *skipped.entry(why.chars().take(120).collect()).or_default() += 1,
            Verdict::Fail(d) => {
                println!("FAIL {} :: {d}", c.name);
                written.push(line.to_string());
            }
        }
    }
    if !written.is_empty() {
        let dir = manifest().join("tests/fixtures/oracle/fuzz");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(format!("{kind}-seed-{seed}.ndjson"));
        std::fs::write(&file, written.join("\n") + "\n").unwrap();
        println!("wrote {} failing case(s) to {}", written.len(), file.display());
    }
    println!("{kind} fuzz: {pass} passed, {} failed, {pending_n} pending, {} skipped", written.len(), skipped.values().sum::<usize>());
    for (why, n) in &skipped {
        println!("  skipped {n}: {why}");
    }
    assert!(written.is_empty(), "fuzz found {} enforced {kind} case(s) that differ from the official engine", written.len());
}

/// BUG-DOTKEY-INC: Loki's `$inc: {"a.b": n}` leaves a literal `"a.b"` key holding the increments; the fix adds
/// them to the nested `a.b` instead. Applied to every top-level doc key of every outcome collection.
fn fold_dotted_incs(outcome: &mut Json) {
    let Json::Obj(colls) = outcome else { return };
    for (_, docs) in colls.iter_mut() {
        let Json::Arr(docs) = docs else { continue };
        for doc in docs.iter_mut() {
            let Json::Obj(fields) = doc else { continue };
            let dotted: Vec<(String, f64)> = fields.iter().filter(|(k, _)| k.contains('.')).filter_map(|(k, v)| v.as_f64().map(|n| (k.clone(), n))).collect();
            fields.retain(|(k, _)| !k.contains('.'));
            for (key, n) in dotted {
                let mut cur = &mut *fields;
                let parts: Vec<&str> = key.split('.').collect();
                for p in &parts[..parts.len() - 1] {
                    if !cur.iter().any(|(k, v)| k == p && matches!(v, Json::Obj(_))) {
                        cur.retain(|(k, _)| k != p);
                        cur.push((p.to_string(), Json::Obj(Vec::new())));
                    }
                    let i = cur.iter().position(|(k, _)| k == p).unwrap();
                    let Json::Obj(next) = &mut cur[i].1 else { unreachable!() };
                    cur = next;
                }
                let last = parts[parts.len() - 1];
                match cur.iter_mut().find(|(k, _)| k == last) {
                    Some((_, v)) => *v = Json::Num(v.as_f64().unwrap_or(0.0) + n),
                    None => cur.push((last.to_string(), Json::Num(n))),
                }
            }
        }
    }
}
