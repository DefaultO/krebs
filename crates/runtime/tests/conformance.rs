//! Host conformance: the V8 host produces the same `outMessage` as the node-hosted run for the same bundle
//! and inputs (ARCHITECTURE.md §7.4). Owner: WP08.
//!
//! The game bundle is written by other packages, so these cases run a fixture bundle
//! (tests/fixtures/host/*.js: a vanilla-shaped miniature of runtime.js/runtime-driver.js and __store)
//! through the real host: snapshots, isolates, natives, deadlines, marshalling, system isolate.

use engine::intents::UserIntents;
use engine::runner::{CodeBundle, Module, ResetReason, RunInput, RunOutput, RunStatus};
use engine::doc::{Doc, Obj, Val};
use engine::world::{MarketState, RoomIdx, UserIdx};
use runtime::{Bundle, Meter, Runtime, RuntimeConfig, WorldAccess};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use util::json::Json;
use util::jsstr::JsStr;

static FILES: &[(&str, &str)] = &[
    ("vendor/lodash.js", include_str!("../js/vendor/lodash.js")),
    ("host/prelude.js", include_str!("fixtures/host/prelude.js")),
    ("driver/runtime.js", include_str!("fixtures/host/runtime.js")),
    ("host/system.js", include_str!("fixtures/host/system.js")),
    ("engine/utils.js", include_str!("fixtures/host/bundle/utils.js")),
    ("engine/game/rooms.js", include_str!("fixtures/host/bundle/rooms.js")),
    ("driver/runtime-driver.js", include_str!("fixtures/host/bundle/runtime-driver.js")),
    ("driver/path-finder.js", include_str!("fixtures/host/bundle/path-finder.js")),
    ("common/system.js", include_str!("fixtures/host/bundle/system.js")),
];

// ---- a fixture world ----

struct Fake {
    users: Vec<String>,
    terrain: [u8; 2500],
    stores: Mutex<Vec<(UserIdx, String)>>,
    market: Arc<MarketState>,
}

fn doc(json: &str) -> Obj {
    match Val::from_json(&Json::parse(json).unwrap()) {
        Val::Obj(o) => o,
        _ => unreachable!(),
    }
}

fn market() -> MarketState {
    let order = |seq, json| Doc::new(seq, doc(json));
    let orders: Arc<[engine::doc::DocRef]> = Arc::from(vec![
        order(1, r#"{"_id":"o1","created":10,"active":true,"type":"sell","resourceType":"energy","roomName":"W1N1","amount":5,"remainingAmount":5,"totalAmount":5,"price":1500}"#),
        order(2, r#"{"_id":"o2","active":false,"type":"buy","resourceType":"energy","price":1}"#),
        order(3, r#"{"_id":"o3","active":true,"type":"buy","resourceType":"H","price":20,"roomName":"Wé"}"#),
    ]);
    let stats: Arc<[Arc<Obj>]> = Arc::from(vec![
        Arc::new(doc(r#"{"_id":"s1","resourceType":"H","date":"2026-10-01","transactions":1,"volume":2,"avgPrice":3,"stddevPrice":0}"#)),
    ]);
    MarketState::new(orders, stats, Arc::from(Vec::new()), Arc::from(Vec::new()), Arc::from(Vec::new()))
}

const ROOMS: [&str; 2] = ["W1N1", "W2N1"];

impl WorldAccess for Fake {
    fn room_by_name(&self, name: &str) -> Option<RoomIdx> {
        ROOMS.iter().position(|r| r.eq_ignore_ascii_case(name)).map(|i| i as RoomIdx)
    }
    fn room_by_xy(&self, x: i32, y: i32) -> Option<RoomIdx> {
        match (x, y) {
            (-2, -2) => Some(0),
            (-3, -2) => Some(1),
            _ => None,
        }
    }
    fn room_name(&self, r: RoomIdx) -> &str {
        ROOMS[r as usize]
    }
    fn terrain(&self, _r: RoomIdx) -> &[u8; 2500] {
        &self.terrain
    }
    fn exit_counts(&self, r: RoomIdx) -> [u8; 4] {
        [r as u8 * 3, 0, 48, 1]
    }
    fn accessible(&self, r: RoomIdx) -> bool {
        r == 0
    }
    fn status(&self, r: RoomIdx) -> Option<(&'static str, Option<i64>)> {
        (r == 1).then_some(("novice", Some(1234)))
    }
    fn world_size(&self) -> u32 {
        4
    }
    /// Two rooms: vanilla's `staticTerrainDataSize / 1024 / 1024`.
    fn terrain_heap_mb(&self) -> f64 {
        5000.0 / 1048576.0
    }
    fn user_id(&self, u: UserIdx) -> &str {
        &self.users[u as usize]
    }
    /// `mine` is the user's (room 1, never visible); `seen` lies in room 0; `hidden` in room 1.
    fn object_room(&self, id: &str, _user: UserIdx, visible: &[RoomIdx]) -> Option<RoomIdx> {
        match id {
            "mine" => Some(1),
            "seen" if visible.contains(&0) => Some(0),
            "hidden" if visible.contains(&1) => Some(1),
            _ => None,
        }
    }
    fn parse_intents(&self, store: &Json) -> UserIntents {
        let user = store.get("user").and_then(Json::as_str).unwrap().trim_start_matches('u').parse().unwrap();
        self.stores.lock().unwrap().push((user, store.stringify()));
        let count = store.get("count").and_then(Json::as_f64).unwrap_or(0.0) as u32;
        UserIntents { rooms: Vec::new(), global: None, notify: Vec::new(), count }
    }
    fn market(&self) -> &Arc<MarketState> {
        &self.market
    }
}

fn world() -> &'static Fake {
    static W: OnceLock<Fake> = OnceLock::new();
    W.get_or_init(|| {
        let mut terrain = [0u8; 2500];
        terrain[0] = 1;
        terrain[2499] = 2;
        Fake { users: (0..2100).map(|i| format!("u{i}")).collect(), terrain, stores: Mutex::new(Vec::new()), market: Arc::new(market()) }
    })
}

fn store_json(user: UserIdx) -> Json {
    let stores = world().stores.lock().unwrap();
    let (_, s) = stores.iter().rev().find(|(u, _)| *u == user).expect("store output");
    Json::parse(s).unwrap()
}

// ---- runtimes ----

fn config(heap_mb: usize, hard_timeout_ms: u64, meter: Meter) -> RuntimeConfig {
    RuntimeConfig {
        runner_threads: 4,
        heap_mb,
        semi_space_mb: 0,
        setup_list: false,
        heap_budget_mb: 0,
        meter,
        hard_timeout_ms,
        idle_evict_s: 180,
        watchdog_poll_us: 500,
        profile: engine::policy::Profile::Private,
        shard_name: "shard0".into(),
    }
}

fn new_runtime(cfg: RuntimeConfig) -> Runtime {
    runtime::init_v8();
    Runtime::new(cfg, Bundle { files: FILES, mod_game_js: Vec::new() })
}

/// The shared runtime; every test uses its own user indexes.
fn rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| new_runtime(config(256, 5000, Meter::Thread)))
}

/// Held by the deadline-accuracy case and the 1000-isolate stress case: next to the harness's own test
/// threads the stress pool oversubscribes the CPUs, and a descheduled watchdog then overshoots by
/// several ms (the gate is about the watchdog, not about an oversubscribed machine).
fn exclusive() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct Run {
    user: UserIdx,
    version: u64,
    main: String,
    meta: String,
    memory: (u64, JsStr),
    visible: Vec<RoomIdx>,
    blobs: Vec<Option<engine::world::RoomBlob>>,
    segments: Vec<(u8, JsStr)>,
    vnow: i64,
}

impl Run {
    fn new(user: UserIdx, main: &str) -> Run {
        Run {
            user,
            version: 1,
            main: main.into(),
            meta: r#"{"cpu":50}"#.into(),
            memory: (0, JsStr::from_str("")),
            visible: vec![0],
            blobs: Vec::new(),
            segments: Vec::new(),
            vnow: 1_000_000,
        }
    }
    fn input(&self) -> RunInput {
        RunInput {
            user: self.user,
            meta: self.meta.clone(),
            blobs: self.blobs.clone(),
            blob_rooms: vec![None; self.blobs.len()],
            event_logs: vec![Arc::from("[{\"event\":1}]")],
            visible: self.visible.clone(),
            memory: self.memory.1.clone(),
            memory_version: self.memory.0,
            segments: self.segments.clone(),
            foreign_segment: None,
            code: Arc::new(CodeBundle { version: self.version, modules: vec![("main".into(), Module::Text(Arc::from(&*self.main)))] }),
            vnow: self.vnow,
            nominal_tick_ms: 1000,
        }
    }
    fn on(&self, rt: &Runtime) -> RunOutput {
        rt.run_input(world(), self.input())
    }
}

fn memory(out: &RunOutput) -> String {
    out.memory.as_ref().map(JsStr::to_string_lossy).unwrap_or_default()
}

fn status(out: &RunOutput) -> String {
    format!("{:?} {:?}", std::mem::discriminant(&out.status), out.error)
}

fn is(out: &RunOutput, want: RunStatus) -> bool {
    match (&out.status, &want) {
        (RunStatus::Reset(a), RunStatus::Reset(b)) => a == b,
        (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

// ---- cases ----

#[test]
fn out_message_round_trip() {
    let mut run = Run::new(1, r#"
        module.exports.loop = function () {
            H.intents.set('mine', 'move', {direction: 3});
            H.intents.set('seen', 'say', {message: 'hi'});
            H.intents.set('hidden', 'harvest', {id: 'x'});
            H.log('log ' + H.blobs.length + ' ' + H.eventLogs[0]);
            Memory.a = (Memory.a || 0) + 1;
            H.raw.segments[3] = 'segé';
            H.raw.segments[4] = 'w€';
        };"#);
    run.meta = r#"{"cpu":50,"consoleCommands":["1 + 1","typeof module"],"activeSegments":[1,2]}"#.into();
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(out.console_log, ["log 0 [{\"event\":1}]"]);
    assert_eq!(out.console_results, ["2", "object"]);
    assert_eq!(memory(&out), r#"{"a":1}"#);
    assert_eq!(out.segments.len(), 2);
    assert_eq!(out.segments[0], (3, JsStr::Latin1(Arc::from(&b"seg\xe9"[..]))));
    assert_eq!(out.segments[1], (4, JsStr::Utf16(Arc::from(&[0x77u16, 0x20ac][..]))));
    assert_eq!(out.intents.as_ref().unwrap().count, 2, "say is free; hidden is still counted");
    let j = store_json(1);
    assert_eq!(
        j.get("intents").unwrap().stringify(),
        r#"{"W2N1":{"mine":{"move":{"direction":3}}},"W1N1":{"seen":{"say":{"message":"hi"}}}}"#
    );
    assert_eq!(j.get("isArray").and_then(Json::as_bool), Some(true), "system bundle has lodash");
    assert!(out.seg_settings.is_some());
    assert!(out.used_clean_ms >= 0.0 && out.used_clean_ms < 1000.0);
}

#[test]
fn memory_stays_resident_and_unchanged_memory_is_not_returned() {
    let mut run = Run::new(2, "module.exports.loop = function () { H.log(H.memoryPassed + ' ' + JSON.stringify(Memory)); };");
    run.memory = (7, JsStr::from_str(r#"{"x":1}"#));
    let out = run.on(rt());
    assert_eq!(out.console_log, [r#"true {"x":1}"#]);
    assert!(out.memory.is_none(), "unchanged Memory is not copied out");
    let out = run.on(rt());
    assert_eq!(out.console_log, [r#"false {"x":1}"#], "same memory_version: not passed again");
    run.memory = (8, JsStr::from_str(r#"{"x":2}"#));
    let out = run.on(rt());
    assert_eq!(out.console_log, [r#"true {"x":2}"#]);
}

#[test]
fn lone_surrogates_survive_memory_and_segments() {
    let mut run = Run::new(3, r"module.exports.loop = function () { H.raw.segments[5] = Memory.s; Memory.t = Memory.s + '\udc00'; };");
    let units: Vec<u16> = r#"{"s":"a"#.encode_utf16().chain([0xd800]).chain(r#""}"#.encode_utf16()).collect();
    run.memory = (1, JsStr::Utf16(units.into()));
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(out.segments, [(5, JsStr::Utf16(Arc::from(&[0x61u16, 0xd800][..])))]);
    // JSON.stringify escapes the lone surrogate and keeps the completed pair.
    let JsStr::Utf16(u) = out.memory.unwrap() else { panic!("expected UTF-16") };
    assert_eq!(String::from_utf16(&u).unwrap(), "{\"s\":\"a\\ud800\",\"t\":\"a\u{10000}\"}");
}

#[test]
fn hot_code_sees_every_memory_replacement() {
    // The pointer-compression build compiles `Memory` reads to the global IC instead of the property cell
    // (tools/v8-ptrcomp.patch); optimized code must still see vanilla's per-tick accessor, a memhack's data
    // property and a plain reassignment.
    let run = Run::new(32, r#"
        function read() { return Memory.v; }
        function hot() { var r; for (var i = 0; i < 100000; i++) r = read(); return r; }
        module.exports.loop = function () {
            Memory.v = 'accessor';
            var a = hot();
            delete global.Memory;
            global.Memory = {v: 'memhack'};
            var b = hot();
            global.Memory = {v: 'assigned'};
            H.log(a + ' ' + b + ' ' + hot());
        };"#);
    for _ in 0..3 {
        let out = run.on(rt());
        assert!(is(&out, RunStatus::Done), "{}", status(&out));
        assert_eq!(out.console_log, ["accessor memhack assigned"]);
    }
}

#[test]
fn timeout_is_uncatchable_and_keeps_earlier_intents_and_memory() {
    let run = Run::new(4, r#"
        module.exports.loop = function () {
            H.intents.set('mine', 'move', {direction: 1});
            Memory.before = 1;
            try { while (true) {} } catch (e) { Memory.caught = 1; } finally { Memory.fin = 1; }
        };"#);
    let out = run.on(rt());
    assert!(is(&out, RunStatus::PartialError), "{}", status(&out));
    let err = out.error.as_deref().unwrap();
    assert!(err.starts_with("Script execution timed out.\n    at ") && err.contains("(main:5:"), "{err}");
    assert_eq!(memory(&out), r#"{"before":1}"#);
    assert_eq!(out.intents.as_ref().unwrap().count, 1);
}

/// Deadline accuracy (WP08 gate: dies at the deadline +- 2 ms of thread CPU): the loop deadline is
/// max(30, cpu + 5) = 55 ms; the bot records the meter inside the timed script, so compiling the loop
/// call and the error path outside it do not count, and reads the record back next tick.
#[test]
fn infinite_loop_dies_at_the_deadline() {
    let _exclusive = exclusive();
    let run = Run::new(31, r#"module.exports.loop = function () {
        if (global.t0) { H.log(global.last - global.t0); return; }
        global.t0 = H.natives.cpuTime();
        while (true) global.last = H.natives.cpuTime();
    };"#);
    let out = run.on(rt());
    assert!(is(&out, RunStatus::PartialError), "{}", status(&out));
    let elapsed: f64 = run.on(rt()).console_log[0].parse().unwrap();
    eprintln!("loop deadline 55 ms, terminated after {elapsed:.3} ms of thread CPU");
    assert!((54.9..=57.0).contains(&elapsed), "terminated after {elapsed} ms, want 55 +- 2");
}

#[test]
fn nested_require_timeout() {
    // The loop's own deadline (armed first) fires while `slow` runs: termination passes the inner
    // boundary (require's catch does not run, so the -1 marker stays) and surfaces at the loop.
    let mut run = Run::new(5, "module.exports.loop = function () { Memory.n = 1; require('slow'); };");
    run.version = 1;
    let mut input = run.input();
    Arc::get_mut(&mut input.code).unwrap().modules.push(("slow".into(), Module::Text(Arc::from("function spin() { while (true) {} }\nspin();"))));
    let out = rt().run_input(world(), input);
    let err = out.error.clone().unwrap();
    assert!(err.starts_with("Script execution timed out.\n    at spin (slow:1:"), "{err}");
    assert_eq!(memory(&out).as_str(), r#"{"n":1}"#);
    let out = rt().run_input(world(), run.input());
    assert!(out.error.unwrap().starts_with("Error: Circular reference to module 'slow'"));

    // An inner deadline shorter than the outer one throws a catchable error at its own boundary.
    let run = Run::new(6, r#"
        module.exports.loop = function () {
            try { H.natives.compileAndRun('function inner() { while (true) {} } inner()', 'inner', 30, 'module'); }
            catch (e) { H.log(e.constructor.name + '|' + e.message + '|' + e.stack); }
            H.log('after');
        };"#);
    let mut run = run;
    run.meta = r#"{"cpu":500}"#.into();
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(
        out.console_log[..2],
        ["Error|Script execution timed out.|Script execution timed out.\n    at inner (inner:1:20)\n    at inner:1:38", "after"]
    );
}

#[test]
fn console_commands_have_no_deadline_but_hit_the_hard_timeout() {
    let rt = new_runtime(config(256, 300, Meter::Thread));
    let mut run = Run::new(7, "module.exports.loop = function () { global.n = (global.n || 0) + 1; H.log(global.n); };");
    assert_eq!(run.on(&rt).console_log, ["1"]);
    run.meta = r#"{"cpu":50,"consoleCommands":["while (true) {}"]}"#.into();
    let out = run.on(&rt);
    assert!(is(&out, RunStatus::Reset(ResetReason::HardTimeout)), "{}", status(&out));
    assert_eq!(out.error.as_deref(), Some("Script execution timed out ungracefully, restarting virtual machine"));
    assert!(out.intents.is_none() && out.memory.is_none());
    run.meta = r#"{"cpu":50}"#.into();
    assert_eq!(run.on(&rt).console_log, ["1"], "a fresh VM after the reset");
}

#[test]
fn halt_resets_the_vm() {
    let mut run = Run::new(8, "module.exports.loop = function () { global.n = (global.n || 0) + 1; H.log(global.n); if (Memory.halt) H.natives.halt(); Memory.halt = 1; };");
    assert_eq!(run.on(rt()).console_log, ["1"]);
    run.memory = (1, JsStr::from_str(r#"{"halt":1}"#));
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Reset(ResetReason::Halt)), "{}", status(&out));
    assert_eq!(out.error.as_deref(), Some("CPU halted"));
    run.memory = (2, JsStr::from_str("{}"));
    assert_eq!(run.on(rt()).console_log, ["1"]);
}

#[test]
fn heap_blow_up_resets_the_vm() {
    let rt = new_runtime(config(64, 5000, Meter::Thread));
    let mut run = Run::new(9, "module.exports.loop = function () { var a = []; while (true) a.push(new Array(100000).fill(a.length)); };");
    run.meta = r#"{"cpu":5000}"#.into();
    let out = run.on(&rt);
    assert!(is(&out, RunStatus::Reset(ResetReason::Heap)), "{}", status(&out));
    assert_eq!(out.error.as_deref(), Some("Isolate has exhausted v8 heap space"));
    let run = Run::new(9, "module.exports.loop = function () { H.log(H.natives.heapStats().heap_size_limit > 60e6); };");
    assert_eq!(run.on(&rt).console_log, ["true"]);
    // ENV-CODEGEN: eval/Function sources over memory limit / 8 (8 MiB here) are refused.
    let run = Run::new(23, r#"module.exports.loop = function () {
        H.log(eval(' '.repeat(8e6) + '1') + new Function('return 2')());
        try { eval(' '.repeat(8.5e6) + '1'); } catch (e) { H.log(e.name + ': ' + e.message); }
        H.log(eval({a: 1}).a);
    };"#);
    assert_eq!(run.on(&rt).console_log, ["3", "EvalError: Code generation from strings disallowed for this context", "1"]);
}

#[test]
fn clone_failure_is_a_total_error() {
    let run = Run::new(10, "module.exports.loop = function () { Memory.x = 1; H.intents.set('mine', 'move', {f: function hello(){ return 1 }}); };");
    let out = run.on(rt());
    assert!(is(&out, RunStatus::TotalError), "{}", status(&out));
    assert_eq!(out.error.as_deref(), Some("TypeError: function hello(){ return 1 } could not be cloned."));
    assert!(out.memory.is_none() && out.intents.is_none());
}

#[test]
fn unhandled_rejection_fails_the_run_after_output_capture() {
    let run = Run::new(11, "module.exports.loop = function () { Promise.reject(new Error('boom')); };");
    let out = run.on(rt());
    assert!(is(&out, RunStatus::TotalError), "{}", status(&out));
    assert!(out.error.as_deref().unwrap().starts_with("Error: boom\n    at module.exports.loop (main:1:88)"), "{:?}", out.error);
    // Handled in the same tick, and continuations after output capture cannot change this tick.
    let run = Run::new(12, r#"module.exports.loop = function () {
        Promise.reject(1).catch(function () {});
        Promise.resolve().then(function () { H.log('late'); Memory.late = 1; });
    };"#);
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert!(out.console_log.is_empty() && out.memory.is_none());
}

#[test]
fn same_user_runs_on_two_threads() {
    let run = Run::new(13, "module.exports.loop = function () { global.n = (global.n || 0) + 1; H.log(global.n); };");
    let before = rt().stats().migrations;
    for want in ["1", "2", "3"] {
        let out = std::thread::scope(|s| s.spawn(|| run.on(rt())).join().unwrap());
        assert_eq!(out.console_log, [want]);
    }
    assert!(rt().stats().migrations >= before + 2);
}

#[test]
fn code_version_change_resets() {
    let mut run = Run::new(14, "module.exports.loop = function () { global.n = (global.n || 0) + 1; H.log(global.n); };");
    assert_eq!(run.on(rt()).console_log, ["1"]);
    assert_eq!(run.on(rt()).console_log, ["2"]);
    run.version = 2;
    assert_eq!(run.on(rt()).console_log, ["1"]);
}

#[test]
fn natives_core_and_world() {
    let mut run = Run::new(15, r#"
        module.exports.loop = function () {
            var n = H.natives;
            H.log([n.now(), n.shardName, n.profile, n.worldSize(), n.nativeMod.version].join());
            var t = n.terrain('W1N1');
            H.log([t.length, t[0], t[2499], n.terrain('w1n1'), n.terrain('W9N9'), t === n.terrain('W1N1')].join());
            H.log([n.isAccessible('W1N1'), n.isAccessible('W2N1'), n.isAccessible('w1n1')].join());
            H.log(JSON.stringify([n.roomStatus('W1N1'), n.roomStatus('W2N1')]));
            H.log(JSON.stringify([n.gridData(-2, -2), n.gridData(-3, -2), n.gridData(-2.5, -2)]));
            var b = n.base64ToBuffer('aGVsbG8', Object.create(Uint8Array.prototype, {tag: {value: 1}}));
            H.log([b.length, b[0], b instanceof Uint8Array, b.tag, Object.prototype.toString.call(b)].join());
            H.log(Object.keys(n.heapStats()).join());
            H.log(typeof n.cpuTime() + ' ' + (n.cpuTime() <= n.cpuTime()));
            H.log(n.cpuTime.toString());
        };"#);
    run.vnow = 123_456;
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    let now: i64 = out.console_log[0].split(',').next().unwrap().parse().unwrap();
    assert!((123_456..123_456 + 1000).contains(&now), "{now}");
    assert!(out.console_log[0].ends_with(",shard0,private,4,11"));
    assert_eq!(out.console_log[1], "2500,1,2,,,false");
    assert_eq!(out.console_log[2], "true,false,false");
    assert_eq!(out.console_log[3], r#"[null,{"status":"novice","timestamp":1234}]"#);
    assert_eq!(out.console_log[4], r#"[458752,null,null]"#); // b 48 << 12 | l 1 << 18
    assert_eq!(out.console_log[5], "5,104,true,1,[object Uint8Array]");
    assert_eq!(
        out.console_log[6],
        "total_heap_size,total_heap_size_executable,total_physical_size,total_available_size,used_heap_size,heap_size_limit,malloced_memory,peak_malloced_memory,does_zap_garbage,externally_allocated_size"
    );
    assert_eq!(out.console_log[7], "number true");
    assert_eq!(out.console_log[8], "function cpuTime() { [native code] }");
}

#[test]
fn room_blobs_are_shared_external_strings() {
    let blob = engine::world::RoomBlob { json: Arc::from(&br#"{"room":{},"seq":[],"objects":[]}"#[..]), offsets: Arc::from(&[0u32][..]), info_end: 0, docs: Arc::from([]) };
    let before = Arc::strong_count(&blob.json);
    for user in [16, 17] {
        let mut run = Run::new(user, "module.exports.loop = function () { global.keep = H.blobs[0]; H.log(JSON.parse(H.blobs[0]).seq.length); };");
        run.blobs = vec![Some(blob.clone()), None];
        assert_eq!(run.on(rt()).console_log, ["0"]);
    }
    // Each VM holds one count on the same allocation: the bytes were not copied.
    assert_eq!(Arc::strong_count(&blob.json), before + 2);
    rt().reset_user(16, "test");
    rt().reset_user(17, "test");
    assert_eq!(Arc::strong_count(&blob.json), before, "disposal releases the external strings");
}

#[test]
fn bundle_loader_resolves_like_the_vanilla_tree() {
    let run = Run::new(24, r#"
        module.exports.loop = function () {
            var rooms = H.bundleRequire('@screeps/engine/src/game/rooms');
            H.log(JSON.stringify(rooms));
            var utils = H.bundleRequire('@screeps/engine/src/utils');
            H.log([utils.sloppy, utils.driver.system.OK, utils.driver.system.ERR_BUSY, rooms === H.bundleRequire('@screeps/engine/src/game/rooms')].join());
            H.log([typeof global, Object.getOwnPropertyDescriptor(globalThis, 'global').enumerable, typeof _init, typeof __tick].join());
        };"#);
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(
        out.console_log,
        [
            r#"{"strict":true,"self":true,"utils":"utils","driver":"runtime-driver","pathFinder":"path-finder","lodash":true,"constantsOK":0,"thisIsExports":true,"missing":"Cannot find module './nope' MODULE_NOT_FOUND","name":"rooms"}"#,
            "true,0,-4,true",
            "object,true,undefined,undefined",
        ]
    );
}

#[test]
fn market_natives() {
    let run = Run::new(22, r#"
        module.exports.loop = function () {
            var n = H.natives;
            H.log(n.marketOrders('energy'));
            H.log(Object.keys(JSON.parse(n.marketOrders('all'))).join());
            H.log([n.marketOrders('O'), n.marketOrder('o2'), JSON.parse(n.marketOrder('o3')).roomName].join('|'));
            H.log(n.marketHistory('H') + ' ' + n.marketHistory('energy') + ' ' + JSON.parse(n.marketHistory('all')).length);
        };"#);
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(
        out.console_log,
        [
            r#"{"o1":{"created":10,"active":true,"type":"sell","resourceType":"energy","roomName":"W1N1","amount":5,"remainingAmount":5,"totalAmount":5,"price":1500,"id":"o1"}}"#,
            "o1,o3",
            "||W\u{e9}",
            r#"[{"resourceType":"H","date":"2026-10-01","transactions":1,"volume":2,"avgPrice":3,"stddevPrice":0}] {} 1"#,
        ]
    );
}

#[test]
fn segment_validation_and_large_memory() {
    // SEG-SAVE-ERRORS: thrown after the user code, so the whole tick is lost (a string, no stack).
    let run = Run::new(19, "module.exports.loop = function () { for (var i = 0; i < 11; i++) H.raw.segments[i] = 'x'; Memory.x = 1; };");
    let out = run.on(rt());
    assert!(is(&out, RunStatus::TotalError), "{}", status(&out));
    assert_eq!(out.error.as_deref(), Some("Cannot save more than 10 memory segments on the same tick"));
    assert!(out.memory.is_none() && out.intents.is_none());
    // Memory over 2 MiB leaves the host intact (MEM-SIZE-LIMIT is applied when saving, §6.8) and is sent
    // again next tick because the save may not happen.
    let mut run = Run::new(20, "module.exports.loop = function () { H.log(H.memoryPassed); if (!Memory.big) Memory.big = new Array(1100000).join('ab'); };");
    let out = run.on(rt());
    assert_eq!(memory(&out).len(), 2_199_998 + 10);
    let out = run.on(rt());
    assert_eq!(out.console_log, ["true"], "the VM's Memory is not known to be saved");
    run.memory.0 = 1;
    let _ = run.on(rt());
}

#[test]
fn wall_meter_deadlines() {
    let rt = new_runtime(config(256, 5000, Meter::Wall));
    let run = Run::new(21, "module.exports.loop = function () { Memory.a = 1; while (true) {} };");
    let out = run.on(&rt);
    assert!(is(&out, RunStatus::PartialError), "{}", status(&out));
    assert!(out.error.as_deref().unwrap().starts_with("Script execution timed out.\n    at module.exports.loop (main:1:"));
    // Billed wall time: the loop deadline (55 ms) plus the first evaluation of main.
    assert!((55.0..200.0).contains(&out.used_clean_ms), "{}", out.used_clean_ms);
}

#[test]
fn fixed_meter_is_reproducible() {
    let rt = new_runtime(config(256, 5000, Meter::Fixed { ms_per_tick: 2.5 }));
    let run = Run::new(18, "module.exports.loop = function () { H.intents.set('mine', 'move', {}); H.log(H.natives.cpuTime() + ' ' + H.natives.cpuTime()); };");
    let a = run.on(&rt);
    let b = run.on(&rt);
    assert_eq!(a.used_clean_ms, 2.5);
    assert_eq!(a.used_time, 3); // ceil(2.5 + 0.2 * 1)
    assert_ne!(a.console_log, b.console_log, "the counter continues across ticks");
    let ms: Vec<f64> = a.console_log[0].split(' ').map(|s| s.parse().unwrap()).collect();
    assert!((ms[1] - ms[0] - 0.01).abs() < 1e-9);
}

/// ENV-HEAP-STATS: isolated-vm's constraints give its `heap_size_limit` (node 22 + isolated-vm 6.1.2,
/// `new ivm.Isolate({memoryLimit: 256}).getHeapStatisticsSync().heap_size_limit` = 274726912), and
/// `externally_allocated_size` counts array buffers only (the terrain copy among them): a room blob the
/// bot keeps is not external memory (vanilla copies runtime data into the heap).
#[test]
fn heap_stats_match_isolated_vm() {
    let mut run = Run::new(26, r#"module.exports.loop = function () {
        global.keep = H.blobs[0];
        global.t = H.natives.terrain('W1N1');
        var h = H.natives.heapStats();
        H.log(h.heap_size_limit + ' ' + h.externally_allocated_size);
        global.ta = new Uint8Array(10 << 20);
        H.log(H.natives.heapStats().externally_allocated_size - h.externally_allocated_size);
    };"#);
    let big = vec![b' '; 8 << 20];
    run.blobs = vec![Some(engine::world::RoomBlob { json: Arc::from(&big[..]), offsets: Arc::from(&[0u32][..]), info_end: 0, docs: Arc::from([]) })];
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    // The whole terrain (5000 bytes) is charged at creation, as vanilla's terrain copy; the room copy is
    // part of it.
    assert_eq!(out.console_log, ["274726912 5000", "10485760"]);
}

/// Array buffers count against the memory limit (isolated-vm's LimitedAllocator): refused with a
/// catchable `RangeError: Array buffer allocation failed` once heap in use plus buffers would pass the
/// reported `heap_size_limit`, and the VM lives on. Oracle, node 22 + isolated-vm 6.1.2 with
/// `memoryLimit: 64` in an empty context: the same two RangeErrors (the 17th 4 MB array; this bundle's
/// bigger heap stops earlier) and `externally_allocated_size` = the arrays (+ the terrain charge here).
/// (`heap_size_limit` is 70254592 there but 68681728 with V8 15.2's young-generation rounding; 256 MB
/// matches exactly.)
#[test]
fn array_buffers_are_limited_like_isolated_vm() {
    let rt = new_runtime(config(64, 5000, Meter::Thread));
    let mut run = Run::new(29, r#"module.exports.loop = function () {
        if (global.k) { H.log('kept ' + k.length); return; }
        var r = [];
        try { new ArrayBuffer(100 << 20); r.push('ok'); } catch (e) { r.push(e.name + ': ' + e.message); }
        try { global.k = []; for (var i = 0; i < 20; i++) k.push(new Uint8Array(4 << 20)); r.push('ok ' + i); }
        catch (e) { r.push(e.name + ': ' + e.message); }
        var h = H.natives.heapStats(), used = h.used_heap_size + h.externally_allocated_size;
        H.log(r.join(' | '));
        H.log([k.length > 10, h.externally_allocated_size === k.length * (4 << 20) + 5000, used <= h.heap_size_limit, used + (4 << 20) > h.heap_size_limit].join());
    };"#);
    run.meta = r#"{"cpu":500}"#.into();
    let out = run.on(&rt);
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(out.console_log, ["RangeError: Array buffer allocation failed | RangeError: Array buffer allocation failed", "true,true,true,true"]);
    let n = run.on(&rt).console_log;
    assert!(n[0].starts_with("kept "), "the VM survives: {n:?}");
}

/// make.js:289-292: a run that fails with an array buffer allocation failure (here an unhandled
/// rejection, ENV-MICROTASKS) clears the VM with its own message.
#[test]
fn allocation_failure_escaping_the_run_resets_the_vm() {
    let mut run = Run::new(30, "module.exports.loop = function () { global.n = (global.n || 0) + 1; H.log(global.n); };");
    assert_eq!(run.on(rt()).console_log, ["1"]);
    run.main = "module.exports.loop = function () { Promise.resolve().then(function () { new ArrayBuffer(4e9); }); };".into();
    run.version = 2;
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Reset(ResetReason::Heap)), "{}", status(&out));
    assert_eq!(out.error.as_deref(), Some("Script execution has been terminated: unable to allocate memory, restarting virtual machine"));
    run.main = "module.exports.loop = function () { global.n = (global.n || 0) + 1; H.log(global.n); };".into();
    assert_eq!(run.on(rt()).console_log, ["1"], "a fresh VM");
}

/// runSync reads `timeout` with `ReadOption<int32_t>` after the compile: null/undefined and 0 mean
/// none, a fraction is a TypeError (a syntax error still wins).
#[test]
fn compile_and_run_reads_timeout_like_run_sync() {
    let run = Run::new(27, r#"module.exports.loop = function () {
        var n = H.natives;
        try { n.compileAndRun('1', 'frac', 35.5, 'module'); } catch (e) { H.log(e.name + ': ' + e.message); }
        try { n.compileAndRun('(', 'bad', 35.5, 'module'); } catch (e) { H.log(e.name); }
        H.log(n.compileAndRun('2', 'none', null, 'module') + n.compileAndRun('3', 'zero', 0, 'module'));
    };"#);
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(out.console_log, ["TypeError: `timeout` must be a 32-bit number", "SyntaxError", "5"]);
}

/// The modules object has own data properties (vanilla's ExternalCopy of `userCode`), even `__proto__`.
#[test]
fn module_names_are_own_properties() {
    let run = Run::new(28, r#"module.exports.loop = function () {
        var m = H.modules, d = Object.getOwnPropertyDescriptor(m, '__proto__');
        H.log([Object.getPrototypeOf(m) === Object.prototype, Object.keys(m).join('|'), d && d.value].join());
    };"#);
    let mut input = run.input();
    Arc::get_mut(&mut input.code).unwrap().modules.push(("__proto__".into(), Module::Text(Arc::from("x"))));
    let out = rt().run_input(world(), input);
    assert_eq!(out.console_log, ["true,main|__proto__,x"]);
}

#[test]
fn thousand_isolates() {
    let _exclusive = exclusive();
    let n = 1000;
    // The runner default (D3): physical cores. Saturating every logical CPU also starves the watchdog
    // the deadline tests running alongside depend on.
    let threads = util::sys::physical_cores();
    let pool = util::pool::Pool::new(threads);
    let rt = new_runtime(config(256, 5000, Meter::Thread));
    let run = |user: usize| Run::new(1000 + user as UserIdx, "module.exports.loop = function () {};");
    // The process's own start: the first VMs cost about 33 MB more than the rest, for V8 and the threads. Counted here it
    // adds 0.03 MB to every VM, which took a build at 4.97 MB a VM over the limit on some runs.
    let warm = 100;
    assert!(pool.map(warm, |i| run(n + i).on(&rt)).iter().all(|o| is(o, RunStatus::Done)));
    let mem0 = private_bytes();
    let t = Instant::now();
    let outs = pool.map(n, |i| run(i).on(&rt));
    let elapsed = t.elapsed();
    assert!(outs.iter().all(|o| is(o, RunStatus::Done)));
    let per_vm_mb = (private_bytes() - mem0) as f64 / n as f64 / 1048576.0;
    let heap_mb = rt.stats().heap_used_mb / n as f64;
    eprintln!("{n} isolates (created + one tick) in {elapsed:?} on {threads} threads; {per_vm_mb:.2} MB committed and {heap_mb:.2} MB used heap per VM");
    assert!(elapsed.as_secs_f64() < 5.0, "{elapsed:?}");
    assert!(per_vm_mb <= 5.0, "{per_vm_mb} MB per VM");
}

/// Host overhead of one run of a bot doing nothing (fixture prelude, no objects), on a warm VM.
/// `cargo test --release -p runtime --test conformance -- --ignored --nocapture host_overhead`
#[test]
#[ignore]
fn host_overhead() {
    let run = Run::new(25, "module.exports.loop = function () {};");
    for _ in 0..100 {
        run.on(rt());
    }
    let n = 5000;
    let t = Instant::now();
    for _ in 0..n {
        run.on(rt());
    }
    eprintln!("host overhead: {:?} per run", t.elapsed() / n);
}

// ---- what a bot is not given ----

/// V8 allocates a resizable buffer outside the array buffer allocator, which is what holds a VM to its memory limit, so
/// none must exist in a bot: not from the options, not from `transfer`, not from the real constructor.
#[test]
fn a_bot_has_only_fixed_buffers() {
    let run = Run::new(41, r#"
        module.exports.loop = function () {
            var p = ArrayBuffer.prototype;
            H.log([typeof SharedArrayBuffer, typeof p.resize, typeof p.transfer, typeof p.transferToFixedLength, typeof Atomics.waitAsync].join());
            var a = new ArrayBuffer(8, {maxByteLength: 1 << 30});
            H.log([a.byteLength, a.resizable, a.maxByteLength, a instanceof ArrayBuffer, a.constructor === ArrayBuffer].join());
            class X extends ArrayBuffer { constructor() { super(16, {maxByteLength: 1 << 30}); this.mine = 1; } }
            var x = new X();
            H.log([x.byteLength, x.resizable, x instanceof X, x.mine, Object.prototype.toString.call(x)].join());
            try { new ArrayBuffer(1 << 30, {maxByteLength: 1 << 30}); H.log('made'); } catch (e) { H.log(e.message); }
            var r = Reflect.construct(ArrayBuffer, [4], Object);
            H.log([r.byteLength, Object.getPrototypeOf(r) === Object.prototype].join());
            var bad = [];
            [undefined, 1.9, '32', null, -0.5, NaN].forEach(function (n) { bad.push(new ArrayBuffer(n).byteLength); });
            H.log(bad.join());
            [-1, Infinity].forEach(function (n) { try { new ArrayBuffer(n); } catch (e) { H.log(e.name + ': ' + e.message); } });
            try { ArrayBuffer(1); } catch (e) { H.log(e.name); }
            try { new ArrayBuffer(1n); } catch (e) { H.log(e.name); }
            H.log([ArrayBuffer.isView(new Uint8Array(1)), ArrayBuffer[Symbol.species] === ArrayBuffer, ArrayBuffer.name, ArrayBuffer.length].join());
            var slice = new ArrayBuffer(8).slice(2);
            H.log([slice.byteLength, slice.constructor === ArrayBuffer, new Uint8Array(new ArrayBuffer(4)).length].join());
        };"#);
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(
        out.console_log,
        [
            "undefined,undefined,undefined,undefined,undefined",
            "8,false,8,true,true",
            "16,false,true,1,[object ArrayBuffer]",
            "Array buffer allocation failed",
            ",true",
            "0,1,32,0,0,0",
            "RangeError: Invalid array buffer length",
            "RangeError: Invalid array buffer length",
            "TypeError",
            "TypeError",
            "true,true,ArrayBuffer,1",
            "6,true,4",
        ]
    );
}

/// Every place a bot's code can look at the stack while a buffer is made, and every error V8 throws from under the real
/// constructor: a frame's `getFunction` must not give it back, or the options work again.
#[test]
fn a_bot_cannot_get_the_real_array_buffer_back() {
    let mut run = Run::new(42, r#"
        module.exports.loop = function () {
            var found = [], seen = 0;
            function look(sites) {
                if (!Array.isArray(sites)) return;
                sites.forEach(function (c) {
                    seen++;
                    [c.getFunction(), c.getThis()].forEach(function (x) {
                        if (typeof x === 'function' && x !== ArrayBuffer && /Buffer/.test(x.name)) found.push(c.toString());
                    });
                });
            }
            function here() {
                var o = {};
                Error.captureStackTrace(o);
                look(o.stack);
                sloppy();
            }
            function sloppy() {
                for (var f = sloppy.caller, n = 0; f && n < 50; f = f.caller, n++) {
                    var a = f.arguments;
                    for (var i = 0; a && i < a.length; i++) if (typeof a[i] === 'function' && a[i] !== ArrayBuffer && /Buffer/.test(a[i].name)) found.push('arguments');
                }
            }
            Error.prepareStackTrace = function (e, sites) { return sites; };
            var attempts = [
                function () { new ArrayBuffer(-1); },
                function () { new ArrayBuffer(1e30); },
                function () { new ArrayBuffer(1e12); },
                function () { new Uint8Array(1e12); },
                function () { new Uint8Array(-1); },
                function () { new Float64Array(1e15); },
                function () { new ArrayBuffer(8).slice({valueOf: function () { here(); return 0; }}); },
                function () { new ArrayBuffer({valueOf: function () { here(); return 1; }}); },
                function () { new ArrayBuffer(1, {get maxByteLength() { here(); return 8; }}); },
                function () { Reflect.construct(ArrayBuffer, [1], new Proxy(function () {}, {get: function (t, k) { here(); return t[k]; }})); },
                function () { class X extends ArrayBuffer { constructor() { here(); super({valueOf: function () { here(); return 1; }}); here(); } } new X(); },
                function () { var B = function () {}; B[Symbol.species] = function (n) { here(); return new ArrayBuffer(n); }; var a = new ArrayBuffer(8); a.constructor = B; a.slice(1); },
                function () { new Uint8Array({get length() { here(); return 1; }}); },
                function () { new Uint8Array(new ArrayBuffer(8), {valueOf: function () { here(); return 0; }}); }
            ];
            attempts.forEach(function (f) {
                try { f(); } catch (e) { look(e.stack); }
            });
            delete Error.prepareStackTrace;
            H.log(found.join('; ') || 'none');
            H.log(seen > 20 ? 'looked' : 'saw ' + seen);
            var shared = new WebAssembly.Memory({initial: 1, maximum: 1, shared: true}).buffer;
            var proto = Object.getPrototypeOf(shared);
            H.log([proto.constructor === Object, typeof proto.grow, Object.prototype.toString.call(shared)].join());
        };"#);
    run.meta = r#"{"cpu":2000}"#.into();
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(out.console_log, ["none", "looked", "true,undefined,[object SharedArrayBuffer]"]);

    // The audit's way in: with a plain handler, Object.prototype.get was a trap and got the real constructor as its first
    // argument. Every trap name goes on Object.prototype here, every operation that looks one up runs, and the things
    // the trap itself touches are replaced as well. Descriptors have no prototype, or `get` and `set` would be read from it.
    let mut run = Run::new(46, r#"
        module.exports.loop = function () {
            var names = ['get', 'set', 'has', 'deleteProperty', 'ownKeys', 'getOwnPropertyDescriptor', 'defineProperty',
                'getPrototypeOf', 'setPrototypeOf', 'isExtensible', 'preventExtensions', 'apply', 'construct'];
            var called = [], real = null, made = [];
            function keep(name) {
                return function (target) {
                    called.push(name);
                    if (typeof target === 'function') real = target;
                    return Reflect[name].apply(Reflect, arguments);
                };
            }
            function touch() {
                void ArrayBuffer.isView;
                ArrayBuffer.mine = 1;
                void ('mine' in ArrayBuffer);
                void Object.keys(ArrayBuffer);
                void Object.getOwnPropertyDescriptor(ArrayBuffer, 'prototype');
                Object.defineProperty(ArrayBuffer, 'mine2', {__proto__: null, value: 1, configurable: true});
                delete ArrayBuffer.mine;
                delete ArrayBuffer.mine2;
                void Object.getPrototypeOf(ArrayBuffer);
                Object.setPrototypeOf(ArrayBuffer, Function.prototype);
                void Object.isExtensible(ArrayBuffer);
                try { ArrayBuffer(1); } catch (e) { void e; }
                made.push(new ArrayBuffer(8).byteLength);
                void (new ArrayBuffer(4) instanceof ArrayBuffer);
                made.push(new ArrayBuffer(8).slice(2).byteLength);
            }
            names.forEach(function (n) { Object.prototype[n] = keep(n); });
            touch();
            names.forEach(function (n) { delete Object.prototype[n]; });
            H.log(called.join() || 'no trap');
            H.log(real ? 'real' : 'no constructor');
            H.log(made.join());

            // The bot's own function under `new ArrayBuffer()`, where the list of arguments is empty.
            var read = [];
            Object.defineProperty(Array.prototype, '0', {__proto__: null, configurable: true,
                get: function () { read.push('get'); return 1 << 30; }, set: function () { read.push('set'); }});
            var empty = new ArrayBuffer().byteLength, given = new ArrayBuffer(8).byteLength;
            delete Array.prototype[0];
            H.log([empty, given, read.join() || 'not read'].join());

            // Replacing what the trap calls does nothing: it holds its own copy and calls it directly.
            var saved = [Reflect.construct, Function.prototype.call, Function.prototype.apply], hit = [];
            Reflect.construct = function () { hit.push('Reflect.construct'); return saved[0].apply(Reflect, arguments); };
            Function.prototype.call = function () { hit.push('call'); };
            Function.prototype.apply = function () { hit.push('apply'); };
            var size = new ArrayBuffer(8).byteLength;
            Function.prototype.apply = saved[2];
            Function.prototype.call = saved[1];
            Reflect.construct = saved[0];
            H.log([size, hit.join() || 'not called'].join());
        };"#);
    run.meta = r#"{"cpu":2000}"#.into();
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(out.console_log, ["no trap", "no constructor", "8,6", "0,8,not read", "8,not called"]);
}

/// A bot used to sleep through its CPU limit on `Atomics.wait`, which held every tick for the 5 s hard timeout.
#[test]
fn a_bot_cannot_sleep() {
    let run = Run::new(43, r#"
        module.exports.loop = function () {
            var shared = new Int32Array(new WebAssembly.Memory({initial: 1, maximum: 1, shared: true}).buffer);
            try { H.log(String(Atomics.wait(shared, 0, 0, 2000))); } catch (e) { H.log(e.name + ': ' + e.message); }
            H.log(String(Atomics.add(new Int32Array(1), 0, 1)));
        };"#);
    let start = Instant::now();
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert!(start.elapsed().as_millis() < 1500, "{:?}", start.elapsed());
    assert!(out.console_log[0].starts_with("TypeError: "), "{:?}", out.console_log);
    assert_eq!(out.console_log[1], "0");
}

/// A cost matrix on a buffer that can be unmapped under the search is not read: this one crashed the process.
#[test]
fn a_cost_matrix_on_a_shared_buffer_is_ignored() {
    let run = Run::new(44, r#"
        module.exports.loop = function () {
            var n = H.natives, wall = new Uint8Array(new WebAssembly.Memory({initial: 1, maximum: 1, shared: true}).buffer, 0, 2500);
            wall.fill(255);
            var r = n.nativeMod.search({xx: 6325, yy: 6325}, [{range: 0, pos: {xx: 6328, yy: 6325}}], function () { return wall; }, 1, 5, 16, 2000, 1000, false, 1.2);
            H.log([r.incomplete, r.path.length].join());
        };"#);
    let out = run.on(rt());
    assert!(is(&out, RunStatus::Done), "{}", status(&out));
    assert_eq!(out.console_log, ["false,3"], "walls in the matrix would leave no path");
}

/// Committed private memory of this process.
fn private_bytes() -> usize {
    #[cfg(windows)]
    {
        #[repr(C)]
        #[derive(Default)]
        struct Counters {
            cb: u32,
            page_faults: u32,
            peak_ws: usize,
            ws: usize,
            peak_paged: usize,
            paged: usize,
            peak_nonpaged: usize,
            nonpaged: usize,
            pagefile: usize,
            peak_pagefile: usize,
            private: usize,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn K32GetProcessMemoryInfo(p: *mut std::ffi::c_void, c: *mut Counters, cb: u32) -> i32;
        }
        let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Default::default() };
        // SAFETY: valid out pointer of the declared size.
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        c.private
    }
    #[cfg(not(windows))]
    {
        let s = std::fs::read_to_string("/proc/self/statm").unwrap();
        s.split(' ').nth(1).unwrap().parse::<usize>().unwrap() * 4096
    }
}

