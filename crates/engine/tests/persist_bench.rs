//! Checkpoint cost on a big world (ignored; run with `--release -- --ignored --nocapture`):
//! CHITIN_PB_SECTORS (default 20) square sectors, CHITIN_PB_BOTS (300) players with code, ~20 KB Memory, a
//! spawn and 10 creeps each. Prints view, save, size and load for the store and for a db.json export.

use engine::coll::{self, Write};
use engine::doc::{Atom, Obj, Val};
use engine::hooks::EngineHooks;
use engine::loki::{self, ImportOpts};
use engine::mapgen::{generate_world, WorldSpec};
use engine::persist;
use engine::policy::Profile;
use engine::world::{RoomIdx, World};
use std::time::Instant;
use util::json::Json;

fn obj(text: &str) -> Obj {
    match Val::from_json(&Json::parse(text).unwrap()) {
        Val::Obj(o) => o,
        _ => panic!("not an object"),
    }
}

fn env(k: &str, d: usize) -> usize {
    std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn world(sectors: i32, bots: usize) -> World {
    let lo = -sectors / 2;
    let spec = WorldSpec { sectors_x: lo..lo + sectors, sectors_y: lo..lo + sectors, seed: 7, wide_coords: false, open: true, portals: None };
    let mut w = generate_world(&spec, &util::pool::Pool::new(8), &EngineHooks::default());
    coll::add_default_users(&mut w);
    let rooms: Vec<RoomIdx> = (0..w.rooms.len() as RoomIdx).filter(|&r| w.rooms[r as usize].info.get(Atom::intern("status")).and_then(Val::as_str) == Some("normal")).collect();
    let step = rooms.len() / bots.max(1);
    let code = Json::Str(include_str!("../../../tools/bots/m1/main.js").into()).stringify();
    let users = (0..bots).map(|i| Write::Insert(obj(&format!(r#"{{"username":"B{i}","usernameLower":"b{i}","cpu":100,"gcl":0,"bot":"bench","active":10000}}"#)))).collect();
    let ids: Vec<String> = coll::write(&mut w, "users", users).unwrap().iter().map(|u| u.get(Atom::intern("_id")).and_then(Val::as_str).unwrap().to_string()).collect();
    let code_docs = ids.iter().map(|id| Write::Insert(obj(&format!(r#"{{"user":"{id}","modules":{{"main":{code}}},"branch":"default","activeWorld":true,"activeSim":true}}"#)))).collect();
    coll::write(&mut w, "users.code", code_docs).unwrap();
    let mut objs = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let memory: String = format!(r#"{{"creeps":{{{}}},"rooms":{{}},"stats":[{}]}}"#, (0..10).map(|c| format!(r#""c{c}":{{"role":"harvester","src":"abcdef0123456789abcdef01","path":"{}"}}"#, "1234".repeat(100))).collect::<Vec<_>>().join(","), (0..1500).map(|n| n.to_string()).collect::<Vec<_>>().join(","));
        coll::env_set(&mut w, &format!("memory:{id}"), &Val::Str(memory.as_str().into())).unwrap();
        let room = w.meta.rooms[rooms[i * step] as usize].name.to_string();
        objs.push(Write::Insert(obj(&format!(r#"{{"type":"spawn","room":"{room}","x":25,"y":25,"name":"Spawn1","user":"{id}","store":{{"energy":300}},"storeCapacityResource":{{"energy":300}},"hits":5000,"hitsMax":5000,"spawning":null,"notifyWhenAttacked":false}}"#))));
        for c in 0..10 {
            let body = (0..12).map(|b| format!(r#"{{"type":"{}","hits":100}}"#, ["work", "carry", "move"][b % 3])).collect::<Vec<_>>().join(",");
            objs.push(Write::Insert(obj(&format!(r#"{{"type":"creep","room":"{room}","x":{},"y":{},"name":"c{c}","user":"{id}","body":[{body}],"hits":1200,"hitsMax":1200,"fatigue":0,"ageTime":1500,"store":{{"energy":50}},"storeCapacity":200,"spawning":false,"notifyWhenAttacked":true,"actionLog":{{}}}}"#, 10 + c, 10 + c))));
        }
    }
    coll::write(&mut w, "rooms.objects", objs).unwrap();
    w.time = 123456;
    w
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore]
fn checkpoint_cost() {
    let (sectors, bots) = (env("CHITIN_PB_SECTORS", 20) as i32, env("CHITIN_PB_BOTS", 300));
    let t = Instant::now();
    let w = world(sectors, bots);
    let objects: usize = w.rooms.iter().map(|r| r.objects.len()).sum();
    println!("world: {sectors}x{sectors} sectors, {} rooms, {objects} objects, {} users, built in {:.0} ms", w.rooms.len(), w.users.len(), ms(t));
    let dir = std::env::temp_dir().join(format!("chitin-pb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let opts = ImportOpts { seed: 7, profile: Profile::Private, now_ms: 1, nominal_tick_ms: 1000 };

    let t = Instant::now();
    let view = w.view();
    println!("view (tick-loop stall): {:.3} ms", ms(t));

    // db.json (the old checkpoint, now `chitin export`)
    for pass in ["cold", "warm"] {
        let t = Instant::now();
        let text = loki::export(&view, Profile::Private);
        let ser = ms(t);
        let t = Instant::now();
        persist::write_atomic(&dir.join("db.json"), text.as_bytes()).unwrap();
        println!("db.json {pass}: serialize {ser:.0} ms, write {:.0} ms, {:.1} MB", ms(t), text.len() as f64 / 1048576.0);
    }
    let t = Instant::now();
    let text = std::fs::read_to_string(dir.join("db.json")).unwrap();
    let (w2, _) = loki::import(&text, &opts).unwrap();
    println!("db.json load: {:.0} ms", ms(t));
    drop((text, w2));

    // the native store
    let t = Instant::now();
    persist::save(&dir, &view, Profile::Private).unwrap();
    println!("store save: {:.0} ms, {:.1} MB", ms(t), std::fs::metadata(persist::path(&dir)).unwrap().len() as f64 / 1048576.0);
    let t = Instant::now();
    let (w3, _) = persist::load(&dir, &opts).unwrap().unwrap();
    println!("store load: {:.0} ms", ms(t));
    assert_eq!(w3.time, w.time);
    let _ = std::fs::remove_dir_all(&dir);
}
