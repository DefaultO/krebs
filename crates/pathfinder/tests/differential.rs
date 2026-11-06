//! Replays queries recorded from the official native PathFinder (tools/pf-gen-fixture.js) and
//! requires identical results: path, ops, cost, incomplete, errors, and the exact sequence of rooms
//! the room callback was called for. Also checks the JS option clamping against the arguments the
//! official glue passed to the addon.
//!
//! Benchmark (compare with `node tools/pf-bench.js`):
//!   cargo test -p pathfinder --release --test differential bench -- --ignored --nocapture
//! Real bots' searches, recorded by the runtime with CHITIN_PF_RECORD=<dir> (runtime/src/pf_record.rs), replayed
//! the same way and timed:
//!   CHITIN_PF_REPLAY=<dir> cargo test -p pathfinder --release --test differential replay -- --ignored --nocapture

use pathfinder::{Goal, PathFinder, Pos, RoomCost, RoomXY, SearchError, SearchOpts, SearchResult};
use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

struct World {
    terrain: HashMap<RoomXY, [u8; 2500]>,
    matrices: Vec<Vec<Cell<u8>>>,
    profiles: Vec<Profile>,
}

struct Profile {
    has_callback: bool,
    // Every matrix goes through one scratch buffer the callback rewrites (see the generator).
    shared: bool,
    rooms: HashMap<RoomXY, Code>,
}

#[derive(Clone, Copy)]
enum Code {
    Matrix(usize),
    Blocked,
    Throw,
    WrongLength,
}

struct Case {
    line: usize,
    origin: Pos,
    goals: Vec<Goal>,
    js: (i32, i32, Option<f64>, i32, i32, i32, bool),
    opts: SearchOpts,
    profile: usize,
    calls: Vec<RoomXY>,
    expect: Expect,
}

#[derive(Debug, PartialEq)]
enum Expect {
    Ok { ops: u32, cost: u32, incomplete: bool, dirs: String },
    Err(String),
}

fn room(name: &str) -> RoomXY {
    let b = name.as_bytes();
    let split = name[1..].find(|c| c == 'N' || c == 'S').unwrap() + 1;
    let n = |s: &str| s.parse::<i32>().unwrap();
    let x = n(&name[1..split]);
    let y = n(&name[split + 1..]);
    RoomXY { x: if b[0] == b'W' { -x - 1 } else { x }, y: if b[split] == b'N' { -y - 1 } else { y } }
}

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

fn load_world(dir: &Path) -> World {
    let text = std::fs::read_to_string(dir.join("world.txt")).unwrap();
    let mut w = World { terrain: HashMap::new(), matrices: Vec::new(), profiles: Vec::new() };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut t = line.split(' ');
        match t.next().unwrap() {
            "room" => {
                let r = room(t.next().unwrap());
                let mut tiles = [0u8; 2500];
                for (i, c) in t.next().unwrap().bytes().enumerate() {
                    tiles[i] = c - b'0';
                }
                w.terrain.insert(r, tiles);
            }
            "matrix" => {
                assert_eq!(t.next().unwrap().parse::<usize>().unwrap(), w.matrices.len());
                let hex = t.next().unwrap().as_bytes();
                let m = (0..2500)
                    .map(|i| Cell::new(u8::from_str_radix(std::str::from_utf8(&hex[i * 2..i * 2 + 2]).unwrap(), 16).unwrap()))
                    .collect();
                w.matrices.push(m);
            }
            "profile" => {
                assert_eq!(t.next().unwrap().parse::<usize>().unwrap(), w.profiles.len());
                let kind = t.next().unwrap();
                let (has_callback, shared) = (kind != "none", kind == "shared");
                let mut rooms = HashMap::new();
                for entry in t.filter(|s| !s.is_empty()) {
                    let (r, c) = entry.split_once('=').unwrap();
                    let code = match c {
                        "F" => Code::Blocked,
                        "T" => Code::Throw,
                        "X" => Code::WrongLength,
                        m => Code::Matrix(m[1..].parse().unwrap()),
                    };
                    rooms.insert(room(r), code);
                }
                w.profiles.push(Profile { has_callback, shared, rooms });
            }
            other => panic!("unknown world line {other}"),
        }
    }
    w
}

fn load_cases(dir: &Path) -> Vec<Case> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.file_name().unwrap().to_str().unwrap().starts_with("cases-"))
        .collect();
    files.sort();
    let mut cases = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for (i, line) in text.lines().enumerate() {
            cases.push(parse_case(i + 1, line));
        }
    }
    cases
}

fn parse_case(line: usize, s: &str) -> Case {
    let mut t = s.split(' ');
    let mut next = || t.next().unwrap();
    fn tag(got: &str, want: &str) {
        assert_eq!(got, want);
    }
    let pos = |r: &str, x: &str, y: &str| Pos { room: room(r), x: x.parse().unwrap(), y: y.parse().unwrap() };
    tag(next(), "O");
    let origin = pos(next(), next(), next());
    tag(next(), "G");
    let n: usize = next().parse().unwrap();
    let goals = (0..n)
        .map(|_| {
            let range = next().parse().unwrap();
            Goal { range, pos: pos(next(), next(), next()) }
        })
        .collect();
    tag(next(), "J");
    let i = |s: &str| s.parse::<i32>().unwrap();
    let weight = |s: &str| if s == "-" { None } else { Some(s.parse().unwrap()) };
    let js = (i(next()), i(next()), weight(next()), i(next()), i(next()), i(next()), next() == "1");
    tag(next(), "N");
    let u = |s: &str| s.parse::<u32>().unwrap();
    let (plain_cost, swamp_cost, max_rooms, max_ops, max_cost) = (u(next()), u(next()), u(next()), u(next()), u(next()));
    let flee = next() == "1";
    let opts = SearchOpts {
        plain_cost: plain_cost.try_into().unwrap(),
        swamp_cost: swamp_cost.try_into().unwrap(),
        max_rooms: max_rooms.try_into().unwrap(),
        max_ops,
        max_cost,
        flee,
        heuristic_weight: next().parse().unwrap(),
    };
    tag(next(), "P");
    let profile = next().parse().unwrap();
    tag(next(), "C");
    let n: usize = next().parse().unwrap();
    let calls = (0..n).map(|_| room(next())).collect();
    tag(next(), "=");
    let expect = match next() {
        "ok" => Expect::Ok {
            ops: u(next()),
            cost: u(next()),
            incomplete: next() == "1",
            dirs: match next() {
                "-" => String::new(),
                d => d.to_string(),
            },
        },
        _ => Expect::Err(next().to_string()),
    };
    Case { line, origin, goals, js, opts, profile, calls, expect }
}

// Path as directions 1..8 (TOP..TOP_LEFT) from the origin, the fixture's encoding.
fn dirs(origin: Pos, path: &[Pos]) -> String {
    let world = |p: Pos| (p.room.x as i64 * 50 + p.x as i64, p.room.y as i64 * 50 + p.y as i64);
    let mut prev = world(origin);
    let mut out = String::new();
    for &p in path {
        let w = world(p);
        let d = match (w.0 - prev.0, w.1 - prev.1) {
            (0, -1) => '1',
            (1, -1) => '2',
            (1, 0) => '3',
            (1, 1) => '4',
            (0, 1) => '5',
            (-1, 1) => '6',
            (-1, 0) => '7',
            (-1, -1) => '8',
            _ => '?',
        };
        out.push(d);
        prev = w;
    }
    out
}

// Runs a case with every room moved by `shift`; terrain, callback and recorded calls use the
// fixture's own room coordinates.
fn run(
    pf: &mut PathFinder,
    w: &World,
    c: &Case,
    calls: &mut Vec<RoomXY>,
    shift: RoomXY,
) -> Result<SearchResult, SearchError> {
    let profile = &w.profiles[c.profile];
    let moved = |p: Pos| Pos { room: RoomXY { x: p.room.x + shift.x, y: p.room.y + shift.y }, ..p };
    let back = |r: RoomXY| RoomXY { x: r.x - shift.x, y: r.y - shift.y };
    let goals: Vec<Goal> = c.goals.iter().map(|g| Goal { pos: moved(g.pos), ..*g }).collect();
    let scratch: Vec<Cell<u8>> = (0..2500).map(|_| Cell::new(0)).collect();
    let wrong_length: Vec<Cell<u8>> = (0..100).map(|_| Cell::new(0)).collect();
    let (scratch, wrong_length) = (&scratch[..], &wrong_length[..]);
    pf.search(
        moved(c.origin),
        &goals,
        &c.opts,
        |r| w.terrain.get(&back(r)),
        |r| {
            let r = back(r);
            calls.push(r);
            if !profile.has_callback {
                return RoomCost::Terrain;
            }
            match profile.rooms.get(&r) {
                None => RoomCost::Terrain,
                Some(Code::WrongLength) => RoomCost::Matrix(wrong_length),
                Some(Code::Blocked) => RoomCost::Blocked,
                Some(Code::Throw) => RoomCost::Abort,
                Some(&Code::Matrix(i)) if profile.shared => {
                    for (s, m) in scratch.iter().zip(&w.matrices[i]) {
                        s.set(m.get());
                    }
                    RoomCost::Matrix(scratch)
                }
                Some(&Code::Matrix(i)) => RoomCost::Matrix(&w.matrices[i]),
            }
        },
    )
}

#[test]
fn matches_official_native_pathfinder() {
    check(fixtures(), RoomXY { x: 0, y: 0 }, true);
}

// The official world stops at W127/E127; ours must give the same answers anywhere.
#[test]
fn matches_official_far_outside_official_world() {
    check(fixtures(), RoomXY { x: 9_000_000, y: -16_000_000 }, true);
}

// Recorded cases carry the addon's arguments only (their J fields are placeholders), so `js` is not compared.
fn check(dir: &Path, shift: RoomXY, js: bool) {
    let world = load_world(dir);
    let cases = load_cases(dir);
    assert!(cases.len() >= 1000, "expected thousands of fixture cases, found {}", cases.len());
    // One instance for all cases, like the addon's thread_local path finders.
    let mut pf = PathFinder::new();
    let mut failures = Vec::new();
    let mut calls = Vec::new();
    for c in &cases {
        let (p, s, w, ops, cost, rooms, flee) = c.js;
        let opts = SearchOpts::from_js(p, s, w, ops, cost, rooms, flee);
        // Debug, not ==, so a NaN weight compares equal to itself.
        if js && format!("{opts:?}") != format!("{:?}", c.opts) {
            failures.push(format!("case {}: options {:?} != native {:?}", c.line, opts, c.opts));
        }

        calls.clear();
        let origin = Pos { room: RoomXY { x: c.origin.room.x + shift.x, y: c.origin.room.y + shift.y }, ..c.origin };
        let got = match run(&mut pf, &world, c, &mut calls, shift) {
            Ok(r) => Expect::Ok { ops: r.ops, cost: r.cost, incomplete: r.incomplete, dirs: dirs(origin, &r.path) },
            Err(SearchError::NoTerrain(_)) => Expect::Err("terrain".into()),
            Err(SearchError::Aborted) => Expect::Err("throw".into()),
            Err(e) => Expect::Err(format!("{e:?}")),
        };
        if got != c.expect {
            failures.push(format!("case {}: got {:?}\n  expected {:?}", c.line, got, c.expect));
        }
        if world.profiles[c.profile].has_callback && calls != c.calls {
            failures.push(format!("case {}: callback rooms {:?}\n  expected {:?}", c.line, calls, c.calls));
        }
    }
    for f in failures.iter().take(10) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} mismatches over {} cases", failures.len(), cases.len());
}

#[test]
#[ignore]
fn replay_recorded() {
    let dir = std::env::var_os("CHITIN_PF_REPLAY").expect("CHITIN_PF_REPLAY=<directory written by CHITIN_PF_RECORD>");
    check(Path::new(&dir), RoomXY { x: 0, y: 0 }, false);
    bench(Path::new(&dir));
}

#[test]
#[ignore]
fn bench_vs_native() {
    bench(fixtures());
}

fn bench(dir: &Path) {
    let world = load_world(dir);
    let cases: Vec<_> = load_cases(dir).into_iter().filter(|c| matches!(c.expect, Expect::Ok { .. })).collect();
    // Same subsets as tools/pf-bench.js: "typical" = maxOps <= 2000 (the default), "long" = the rest.
    let typical: Vec<&Case> = cases.iter().filter(|c| c.opts.max_ops <= 2000).collect();
    let long: Vec<&Case> = cases.iter().filter(|c| c.opts.max_ops > 2000).collect();
    let all: Vec<&Case> = cases.iter().collect();
    let mut pf = PathFinder::new();
    let mut calls = Vec::new();
    for (name, list) in [("all", &all), ("typical", &typical), ("long", &long)] {
        let rounds = 20;
        let mut ops = 0u64;
        for _ in 0..2 {
            for c in list {
                run(&mut pf, &world, c, &mut calls, RoomXY { x: 0, y: 0 }).unwrap();
            }
        }
        let start = Instant::now();
        for _ in 0..rounds {
            for c in list {
                calls.clear();
                ops += run(&mut pf, &world, c, &mut calls, RoomXY { x: 0, y: 0 }).unwrap().ops as u64;
            }
        }
        let el = start.elapsed().as_secs_f64();
        let n = rounds * list.len();
        println!(
            "rust {name}: {n} searches, {:.3} us/search, {:.1} ns/op ({ops} ops)",
            el * 1e6 / n as f64,
            el * 1e9 / ops as f64
        );
    }
}
