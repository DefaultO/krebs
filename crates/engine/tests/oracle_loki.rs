//! Loki migration fixtures (`node tools/oracle/loki.js gen …`: the official storage db.js `loadDb` + `upgradeDb`
//! over random legacy DBs): `engine::loki::upgrade` must leave the same env, version and collections, and crash
//! where vanilla's startup would.

use engine::loki::{upgrade, Db};
use util::json::Json;

#[test]
fn loki_upgrade_oracle() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/loki");
    let mut n = 0;
    for f in std::fs::read_dir(&dir).unwrap() {
        let text = std::fs::read_to_string(f.unwrap().path()).unwrap();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let case = Json::parse(line).unwrap();
            let name = case.get("name").and_then(Json::as_str).unwrap();
            let s = case.get("scenario").unwrap();
            let r = case.get("result").unwrap();
            let mut db = Db::parse(&s.get("db").unwrap().stringify()).unwrap();
            let got = upgrade(&mut db, s.get("nowMs").and_then(Json::as_f64).unwrap() as i64);
            let crashed = !matches!(r.get("crash"), None | Some(Json::Null));
            assert_eq!(got.is_err(), crashed, "{name}: crash {got:?} vs {:?}", r.get("crash"));
            n += 1;
            if crashed {
                continue;
            }
            assert_eq!(db.version, r.get("version").and_then(Json::as_f64), "{name}: version");
            let env = db.env.as_ref().map(|e| engine::doc::Val::Obj(e.clone()).to_json());
            assert_eq!(env.map(|e| e.stringify()), r.get("env").map(Json::stringify), "{name}: env");
            let Some(Json::Obj(colls)) = r.get("collections") else { panic!("{name}: no collections") };
            for (cname, docs) in colls {
                let ours: Vec<Json> = db.colls.iter().find(|(n, _)| n == cname).map_or(Vec::new(), |(_, d)| d.iter().map(|(_, o)| engine::doc::Val::Obj(o.clone()).to_json()).collect());
                assert_eq!(Json::Arr(ours).stringify(), docs.stringify(), "{name}: {cname}");
            }
        }
    }
    assert!(n > 0, "no fixtures in {}", dir.display());
}

use engine::doc::{Atom, Doc, Obj, Val};
use engine::hooks::EngineHooks;
use engine::loki::{export, import, ImportOpts};
use engine::mapgen::{generate_world, WorldSpec};
use engine::policy::Profile;
use engine::world::{CodeState, Inbox, MarketState, UserState};
use std::sync::Arc;
use util::jsstr::JsStr;

fn obj(text: &str) -> Obj {
    match Val::from_json(&Json::parse(text).unwrap()) {
        Val::Obj(o) => o,
        _ => panic!("not an object: {text}"),
    }
}

fn opts() -> ImportOpts {
    ImportOpts { seed: 999, profile: Profile::Mmo, now_ms: 1, nominal_tick_ms: 1000 }
}

/// A generated world with a user carrying every per-user piece, flags, event logs, market docs and an active set:
/// export → import → export gives the same text (a checkpoint resumes exactly).
#[test]
fn export_import_round_trip() {
    let spec = WorldSpec { sectors_x: -1..0, sectors_y: -1..0, seed: 5, wide_coords: false, open: true, portals: None };
    let mut w = generate_world(&spec, &util::pool::Pool::new(4), &EngineHooks::default());
    w.time = 4321;
    let seq = w.next_seq;
    w.next_seq += 3;
    let mut segments = std::collections::BTreeMap::new();
    segments.insert(3u8, JsStr::from_str("seg \"three\" ü"));
    w.users.push(Arc::new(UserState {
        id: engine::doc::Id::new("u1").unwrap(),
        seq,
        doc: Arc::new(obj(r#"{"_id":"u1","username":"Alice","usernameLower":"alice","cpu":100,"gcl":5,"rooms":["W5N5"]}"#)),
        auth: Arc::new(obj(r#"{"password":"ab12","salt":"cd34"}"#)),
        code: Arc::new(CodeState { branches: vec![Arc::new(obj(r#"{"_id":"c1","user":"u1","branch":"default","modules":{"main":"x"},"activeWorld":true,"activeSim":true}"#))], active_world: Some(0), active_sim: Some(0), version: 1 }),
        memory: JsStr::from_str(r#"{"a":"ü\n€"}"#),
        memory_version: 1,
        segments: Arc::new(segments),
        public_segments: "3".into(),
        inter_shard: None,
        console_queue: vec![Arc::new(obj(r#"{"_id":"k2","user":"u1","expression":"2"}"#)), Arc::new(obj(r#"{"_id":"k1","user":"u1","expression":"1"}"#))].into(),
        power_creeps: vec![Doc::new(seq + 1, obj(r#"{"_id":"p1","user":"u1","name":"op","className":"operator","level":0}"#))].into(),
        inbox: Arc::new(Inbox { notifications: vec![Arc::new(obj(r#"{"_id":"n1","user":"u1","message":"hi","date":5,"type":"msg","count":1}"#))], messages: vec![] }),
    }));
    w.reindex();
    w.market = Arc::new(MarketState::new(
        vec![Doc::new(seq + 2, obj(r#"{"_id":"o1","type":"sell","resourceType":"H","price":1000,"roomName":"W5N5"}"#))].into(),
        vec![Arc::new(obj(r#"{"_id":"s1","resourceType":"H","date":"2026-10-01"}"#))].into(),
        vec![].into(),
        vec![Arc::new(obj(r#"{"_id":"mo1","user":"u1","change":1}"#))].into(),
        vec![].into(),
    ));
    w.active.insert(3);
    w.active.insert(1);
    let a = export(&w.view(), Profile::Private);
    let (w2, report) = import(&a, &opts()).unwrap();
    assert_eq!((report.orphan_objects, report.pending_intents), (0, 0));
    assert_eq!((w2.time, w2.seed, w2.next_seq, w2.users.len()), (4321, 5, w.next_seq, 1));
    assert_eq!(w2.users[0].auth.get(Atom::intern("salt")).and_then(Val::as_str), Some("cd34"));
    assert!(w2.users[0].doc.get(Atom::intern("password")).is_none());
    let b = export(&w2.view(), Profile::Private);
    assert_eq!(a, b);

    // the native store resumes the same world
    let dir = std::env::temp_dir().join(format!("chitin-store-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    engine::persist::save(&dir, &w.view(), Profile::Private).unwrap();
    let (w3, _) = engine::persist::load(&dir, &opts()).unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!((w3.next_seq, w3.ids.counter()), (w.next_seq, w.ids.counter()));
    assert_eq!(export(&w3.view(), Profile::Private), a);

    if std::env::var("CHITIN_ORACLE").is_ok() {
        // the official storage loads it (Loki + upgradeDb) and can query and update it
        let path = std::env::temp_dir().join(format!("chitin-export-{}.json", std::process::id()));
        std::fs::write(&path, &a).unwrap();
        let node = std::env::var("CHITIN_NODE").unwrap_or_else(|_| "E:/screeps/node22/node-v22.22.3-win-x64/node.exe".into());
        let out = std::process::Command::new(node)
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .args(["tools/oracle/loki.js", "check", path.to_str().unwrap()])
            .output()
            .unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let r = Json::parse(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
        assert_eq!(r.get("gameTime").and_then(Json::as_f64), Some(4321.0));
        assert_eq!(r.get("users").and_then(Json::as_f64), Some(1.0));
    }
}

/// The launcher's shipped db.json (version 7) imports through the migrations: 121 rooms, 339 objects, legacy
/// stores converted, the four rooms marked `active` in the active set. Skipped without reference/.
#[test]
fn import_shipped_db() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reference/launcher/init_dist/db.json");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let (w, _) = import(&text, &opts()).unwrap();
    assert_eq!((w.rooms.len(), w.rooms.iter().map(|r| r.objects.len()).sum::<usize>(), w.users.len(), w.time), (121, 339, 7, 1));
    let spawn = w.rooms.iter().flat_map(|r| r.objects.iter()).find(|d| d.obj.get(Atom::intern("type")).and_then(Val::as_str) == Some("spawn")).unwrap();
    assert!(spawn.json().contains(r#""store":{"energy":300},"storeCapacityResource":{"energy":300}"#), "{}", spawn.json());
    let mut active: Vec<&str> = w.active.list().iter().map(|&r| &*w.meta.rooms[r as usize].name).collect();
    active.sort_unstable();
    assert_eq!(active, ["W1N1", "W1N9", "W9N1", "W9N9"]);
}
