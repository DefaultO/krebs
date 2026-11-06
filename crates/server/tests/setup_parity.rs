//! Game setup's match path (`runner.setupList`: prelude.js objectList, game.js addRoomObject, store.js) on the real host,
//! where room docs are reused between ticks. `same_with_the_option_on_and_off` holds the hand-made copy of vanilla's object
//! loop: tests/fixtures/setup/dump_all.js reads everything Game setup builds on a world with every object type, and both
//! ways round must print the same. The other bots reach into it on purpose. Their expected lines are what each printed on
//! the engine before the path, the same with the option off and on but for what ARCHITECTURE.md "What differs in a
//! match" lists, each pinned here; tools/oracle/api/cases game-setup-accessors.js and store.js check the same reaches
//! against vanilla, except proto_setter_in_setup.js: chitin's Game setup already kept `register._objects[i]` in a local and
//! vanilla's read back shows to that setter, so there it holds the older engine.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A directory of its own, gone when the test ends.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("chitin-setup-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The console lines of `chitin bench <world>` that hold `] probe `, without that word, with the option as given.
fn bench(dir: &Path, on: bool, ticks: u32, world: &[&str]) -> Vec<String> {
    let config = dir.join(format!("config-{on}.json"));
    std::fs::write(&config, format!(r#"{{"runner":{{"setupList":{on}}}}}"#)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_krebs"))
        .current_dir(dir)
        .args(["bench", "--config", config.to_str().unwrap(), "--warmup", "0", "--console", "--ticks", &ticks.to_string()])
        .args(world)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}\n{}", String::from_utf8_lossy(&out.stderr));
    text.lines().filter_map(|l| l.split_once("] probe ").map(|(_, rest)| rest.to_string())).collect()
}

/// `bench` over `ticks` ticks of one bot on a small generated world.
fn probes(name: &str, bot: &str, ticks: u32, on: bool) -> Vec<String> {
    let dir = Dir::new(&format!("{name}-{on}"));
    let main = dir.0.join("main.js");
    std::fs::write(&main, bot).unwrap();
    bench(&dir.0, on, ticks, &["--bots", "1", "--world", "2x2", "--bot", main.to_str().unwrap()])
}

/// The same lines both ways round.
fn probes_both(name: &str, bot: &str, ticks: u32) -> Vec<String> {
    let (off, on) = (probes(name, bot, ticks, false), probes(name, bot, ticks, true));
    assert_eq!(off, on);
    off
}

/// Doc bodies of the world below by room, each given an `_id` in this order, which is the order `roomObjects` holds them
/// in. u1 is the bot, u2 a player whose code does nothing. Ramparts lie over structures of every kind, before and after
/// them in key order, W3N1 holds only things its owner has no vision by, and W4N1 only what the bot may not see.
const OWN: &str = r#""user":"u1","hits":5000,"hitsMax":5000,"notifyWhenAttacked":true"#;
const FOE: &str = r#""user":"u2","hits":5000,"hitsMax":5000,"notifyWhenAttacked":true"#;
const WALL: &str = r#""hits":5000,"hitsMax":300000000"#;

fn creep(owner: &str, name: &str, x: u32, y: u32, more: &str) -> String {
    let body = r#"[{"type":"work","hits":100},{"type":"carry","hits":100},{"type":"move","hits":100}]"#;
    format!(r#"{{"type":"creep","name":"{name}","x":{x},"y":{y},"user":"{owner}","body":{body},"hits":300,"hitsMax":300,"store":{{"energy":10}},"storeCapacity":50,"fatigue":0,"ageTime":1000000,"notifyWhenAttacked":true{more}}}"#)
}

fn at(x: u32, y: u32, kind: &str, rest: &str) -> String {
    format!(r#"{{"type":"{kind}","x":{x},"y":{y},{rest}}}"#)
}

fn world_docs() -> Vec<(&'static str, Vec<String>)> {
    let energy = |n: u32, cap: u32| format!(r#""store":{{"energy":{n}}},"storeCapacity":{cap},"storeCapacityResource":{{"energy":{cap}}}"#);
    let spawn = |name: &str, more: &str| format!(r#"{OWN},"name":"{name}","spawning":null,{}{more}"#, energy(200, 300));
    let rampart = |who: &str| format!(r#"{who},"nextDecayTime":100000,"isPublic":false"#);
    let goods = r#""store":{"energy":1000},"storeCapacity":1000000,"storeCapacityResource":null"#;
    let home = vec![
        r#"{"type":"controller","x":1,"y":1,"user":"u1","level":8,"progress":0,"downgradeTime":1000000,"safeModeAvailable":1}"#.to_string(),
        at(10, 10, "spawn", &spawn("Spawn1", "")),
        at(10, 10, "rampart", &rampart(OWN)),
        at(12, 10, "rampart", &rampart(OWN)),
        at(12, 10, "tower", &format!("{OWN},{}", energy(100, 1000))),
        at(14, 10, "extension", &format!("{OWN},{}", energy(40, 200))),
        at(15, 10, "extension", &format!(r#"{OWN},{},"off":true"#, energy(40, 200))),
        at(16, 10, "spawn", &spawn("Spawn2", r#","off":true"#)),
        at(18, 10, "road", r#""hits":5000,"hitsMax":5000,"nextDecayTime":100000"#),
        at(18, 10, "rampart", &rampart(OWN)),
        at(20, 10, "rampart", &rampart(OWN)),
        at(20, 10, "container", r#""hits":250000,"hitsMax":250000,"nextDecayTime":100000,"store":{"energy":50},"storeCapacity":2000,"storeCapacityResource":null"#),
        at(22, 10, "constructedWall", WALL),
        at(23, 12, "link", &format!(r#"{OWN},{},"cooldown":0"#, energy(0, 800))),
        at(24, 12, "storage", &format!("{OWN},{goods}")),
        at(24, 12, "rampart", &rampart(OWN)),
        at(25, 12, "observer", &format!(r#"{OWN},"observeRoom":null"#)),
        at(26, 12, "powerSpawn", &format!(r#"{OWN},"store":{{"energy":0,"power":0}},"storeCapacity":5100,"storeCapacityResource":{{"energy":5000,"power":100}}"#)),
        at(27, 12, "lab", &format!(r#"{OWN},"store":{{"energy":0}},"storeCapacity":5000,"storeCapacityResource":{{"energy":2000}},"cooldown":0,"mineralAmount":0"#)),
        at(28, 12, "terminal", &format!(r#"{OWN},{goods},"cooldownTime":0"#)),
        at(29, 12, "nuker", &format!(r#"{OWN},"store":{{"energy":0,"G":0}},"storeCapacity":305000,"storeCapacityResource":{{"energy":300000,"G":5000}},"cooldownTime":0"#)),
        at(30, 12, "factory", &format!(r#"{OWN},{goods},"cooldown":0,"level":1"#)),
        at(31, 12, "powerBank", r#""hits":2000000,"hitsMax":2000000,"store":{"power":100},"decayTime":100000"#),
        at(33, 12, "portal", r#""destination":{"x":1,"y":1,"room":"W1N1"},"decayTime":100000"#),
        at(34, 12, "keeperLair", r#""nextSpawnTime":100000"#),
        at(35, 12, "invaderCore", r#""user":"2","hits":100000,"hitsMax":100000,"level":1,"decayTime":100000,"store":{},"strongholdId":"s1","nextExpandTime":100000,"depositType":"silicon""#),
        at(30, 30, "source", r#""energy":3000,"energyCapacity":3000,"ticksToRegeneration":100,"nextRegenerationTime":100000,"invaderHarvested":0"#),
        at(32, 32, "mineral", r#""mineralType":"H","mineralAmount":1000,"density":3"#),
        at(32, 32, "extractor", &format!(r#"{OWN},"cooldown":0"#)),
        at(32, 32, "rampart", &rampart(OWN)),
        at(34, 34, "deposit", r#""depositType":"silicon","harvested":0,"cooldownTime":0,"decayTime":100000"#),
        at(35, 35, "energy", r#""energy":500,"resourceType":"energy""#),
        at(36, 35, "tombstone", r#""user":"u1","deathTime":990,"decayTime":100000,"store":{"energy":5},"creepId":"c0","creepName":"gone","creepTicksToLive":0,"creepBody":["work"],"creepSaying":null"#),
        at(37, 35, "ruin", r#""user":"u1","destroyTime":990,"decayTime":100000,"store":{"energy":5},"structure":{"id":"s0","type":"tower","hits":0,"hitsMax":3000,"user":"u1"}"#),
        at(38, 35, "nuke", r#""launchRoomName":"W2N1","landTime":100000"#),
        at(39, 35, "constructionSite", r#""user":"u1","name":"","structureType":"extension","progress":0,"progressTotal":3000"#),
        at(40, 35, "constructionSite", r#""user":"u2","name":"","structureType":"road","progress":0,"progressTotal":300"#),
        at(41, 35, "energy", r#""energy":9,"resourceType":"energy","temp":true"#),
        at(41, 37, "constructedWall", &format!(r#"{WALL},"temp":true"#)),
        at(42, 35, "flag", r#""user":"u1","name":"Home","color":1,"secondaryColor":2"#),
        creep("u1", "Mine", 18, 10, ""),
        creep("u1", "Born", 11, 11, r#","spawning":true"#),
        creep("u1", "Twin", 19, 19, ""),
        creep("u1", "Twin", 19, 20, ""),
        creep("u2", "Theirs", 40, 40, ""),
        at(41, 40, "powerCreep", r#""user":"u2","name":"Pc2","hits":1000,"hitsMax":1000,"store":{},"storeCapacity":100,"powers":{},"level":1,"className":"operator","ageTime":1000000"#),
        at(42, 40, "powerCreep", r#""user":"u1","name":"Pc1","hits":1000,"hitsMax":1000,"store":{},"storeCapacity":100,"powers":{},"level":1,"className":"operator","ageTime":1000000"#),
    ];
    let foreign = vec![
        r#"{"type":"controller","x":1,"y":1,"user":"u2","level":8,"progress":0,"downgradeTime":1000000}"#.to_string(),
        at(10, 10, "spawn", &format!(r#"{FOE},"name":"Their","spawning":null,{}"#, energy(200, 300))),
        at(10, 10, "rampart", &rampart(FOE)),
        at(12, 10, "tower", &format!("{FOE},{}", energy(100, 1000))),
        at(12, 10, "rampart", &rampart(FOE)),
        at(14, 10, "extension", &format!("{FOE},{}", energy(40, 200))),
        creep("u1", "Scout", 20, 20, ""),
    ];
    let unseen = vec![
        at(25, 26, "constructionSite", r#""user":"u1","name":"","structureType":"road","progress":0,"progressTotal":300"#),
        at(25, 27, "flag", r#""user":"u1","name":"Away","color":3,"secondaryColor":4"#),
        at(10, 10, "tower", &format!("{FOE},{}", energy(100, 1000))),
    ];
    let hidden = vec![r#"{"type":"controller","x":1,"y":1,"user":"u2","level":3,"progress":0,"downgradeTime":1000000}"#.to_string()];
    vec![("W1N1", home), ("W2N1", foreign), ("W3N1", unseen), ("W4N1", hidden)]
}

/// A scenario file of the world in `dir`, with the bot as u1's code and every doc given `id(n)`, the extra docs of `more`
/// added to the first room.
fn scenario(dir: &Path, bot: &str, more: &[(&str, &str)]) -> String {
    let (mine, idle) = (dir.join("mine"), dir.join("idle"));
    for (path, code) in [(&mine, bot), (&idle, "module.exports.loop = function () {};")] {
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("main.js"), code).unwrap();
    }
    let mut n = 0;
    let mut rooms = Vec::new();
    for (name, docs) in world_docs() {
        let mut objects: Vec<String> = docs
            .iter()
            .map(|doc| {
                n += 1;
                format!(r#"{{"_id":"5f00000000000000{n:08x}",{}"#, &doc[1..])
            })
            .collect();
        if name == "W1N1" {
            objects.extend(more.iter().map(|(id, rest)| format!(r#"{{"_id":"{id}",{rest}}}"#)));
        }
        rooms.push(format!(r#"{{"name":"{name}","terrain":"{}","objects":[{}]}}"#, "0".repeat(2500), objects.join(",")));
    }
    let player = |id: &str, code: &str| format!(r#"{{"id":"{id}","username":"{id}","cpu":100,"bucket":10000,"gcl":1000000,"power":0,"code":"{code}"}}"#);
    let text = format!(r#"{{"gameTime":1000,"rooms":[{}],"players":[{},{}]}}"#, rooms.join(","), player("u1", "mine"), player("u2", "idle"));
    let file = dir.join("scenario.json");
    std::fs::write(&file, text).unwrap();
    file.to_str().unwrap().to_string()
}

fn on_the_world(name: &str, bot: &str, more: &[(&str, &str)], ticks: u32) -> (Vec<String>, Vec<String>) {
    let dir = Dir::new(name);
    let file = scenario(&dir.0, bot, more);
    (bench(&dir.0, false, ticks, &["--scenario", &file]), bench(&dir.0, true, ticks, &["--scenario", &file]))
}

/// First line that differs, with its neighbour's tag, instead of two walls of text.
fn assert_same(off: &[String], on: &[String]) {
    for (a, b) in off.iter().zip(on) {
        assert_eq!(a, b);
    }
    assert_eq!(off.len(), on.len());
}

const DUMP: &str = include_str!("fixtures/setup/dump_all.js");

#[test]
fn same_with_the_option_on_and_off() {
    let (off, on) = on_the_world("dump", DUMP, &[], 4);
    assert_same(&off, &on);
    let has = |what: &str| off.iter().any(|l| l.contains(what));
    assert!(off.len() > 700, "{}", off.len());
    // the dump read what it is there for, and nothing threw
    assert!(!has("threw"), "{:?}", off.iter().find(|l| l.contains("threw")));
    for class in ["StructureSpawn", "StructureRampart", "StructureRoad", "StructureTower", "StructureExtension", "StructureContainer", "StructureWall", "StructureLink", "StructureStorage", "StructureObserver", "StructurePowerSpawn", "StructureLab", "StructureTerminal", "StructureNuker", "StructureFactory", "StructurePowerBank", "StructurePortal", "StructureKeeperLair", "StructureInvaderCore", "StructureController", "StructureExtractor", "Source", "Mineral", "Deposit", "Resource", "Tombstone", "Ruin", "Nuke", "ConstructionSite", "Flag", "Creep", "PowerCreep"] {
        assert!(has(&format!(" object {class}+")) || has(&format!(" object {class} ")), "no {class}");
    }
    assert!(has("W1N1 tile 10,10"), "no rampart over the spawn");
    assert!(has("W2N1 tile 12,10"), "no foreign rampart over a tower");
    let temp = |l: &&String| l.starts_with("1000 object ") && (l.contains("W1N1:41,35") || l.contains("W1N1:41,37"));
    assert!(!off.iter().any(|l| temp(&l)), "a temp doc became an object");
    assert!(off.iter().any(|l| l.contains("keys flags") && l.contains("Away")));
    assert!(off.iter().any(|l| l.contains("no vision 2: undefined | 1")), "the site with no vision: {:?}", off.iter().find(|l| l.contains("no vision")));
}

/// Ids that can sort as array indexes or are short take the tick off the list; the same must come out.
#[test]
fn same_on_a_world_with_short_ids() {
    let more = [("1234", r#""type":"road","x":3,"y":44,"hits":5000,"hitsMax":5000,"nextDecayTime":100000"#), ("short1", r#""type":"road","x":4,"y":44,"hits":5000,"hitsMax":5000,"nextDecayTime":100000"#), ("0042", r#""type":"energy","x":5,"y":44,"energy":100,"resourceType":"energy""#)];
    let (off, on) = on_the_world("short", DUMP, &more, 3);
    assert_same(&off, &on);
    assert!(off.iter().any(|l| l.contains("short ids") && l.contains("1234:") && l.contains("W1N1:3,44")));
}

/// A rampart over another owned structure, ARCHITECTURE.md "What differs in a match" 3: the one place Game setup does less
/// that the bot's `push` can count. W1N1 has four tiles with two owned structures, over a spawn, a tower, a storage and an
/// extractor, and W2N1 two. It is also the proof the list path runs at all.
#[test]
fn a_rampart_over_an_owned_structure_is_one_push_fewer_in_a_match() {
    let (off, on) = on_the_world("push", include_str!("fixtures/setup/owned_push.js"), &[], 4);
    assert_eq!(off.len(), on.len());
    let total = |l: &String| l.split(' ').next().unwrap().parse::<i64>().unwrap();
    assert!(total(&off[1]) > 0, "{off:?}");
    // the module first runs in the first tick's loop, after Game setup: the patch counts from the second
    for (a, b) in off.iter().zip(&on).skip(1) {
        assert_eq!(total(a) - total(b), 6, "{a}\n{b}");
    }
}

#[test]
fn a_store_handler_is_shared_only_in_a_match() {
    let same = |on: bool| probes("store", include_str!("fixtures/setup/store_handler.js"), 3, on);
    assert!(same(false).iter().all(|l| l.ends_with("same handler false")));
    assert!(same(true).iter().all(|l| l.ends_with("same handler true")));
}

#[test]
fn a_table_read_in_setup_sees_the_whole_room() {
    let lines = probes_both("read", include_str!("fixtures/setup/table_read_in_setup.js"), 8);
    assert!(lines.len() >= 3, "{lines:?}");
    for line in &lines {
        assert_eq!(line, "-7 -7 true", "{lines:?}");
    }
}

#[test]
fn a_setter_added_in_the_loop_never_sees_a_raw_doc() {
    let lines = probes_both("loop", include_str!("fixtures/setup/proto_setter_in_loop.js"), 9);
    assert_eq!(lines, PROTO_SETTER_IN_LOOP, "{lines:?}");
}

#[test]
fn a_replaced_proxy_gets_a_handler_per_store() {
    let lines = probes_both("proxy", include_str!("fixtures/setup/proxy_replaced.js"), 9);
    assert_eq!(lines, PROXY_REPLACED, "{lines:?}");
}

#[test]
fn a_setter_added_in_setup_sees_the_table_writes() {
    let lines = probes_both("setter", include_str!("fixtures/setup/proto_setter_in_setup.js"), 9);
    assert_eq!(lines, PROTO_SETTER_IN_SETUP, "{lines:?}");
}

#[test]
fn getters_added_in_setup_see_every_doc_read() {
    let lines = probes_both("getters", include_str!("fixtures/setup/proto_getters_in_setup.js"), 9);
    assert_eq!(lines, PROTO_GETTERS_IN_SETUP, "{lines:?}");
}

/// An index setter on every array sees no raw doc: `__tick` builds the list only on a tick the check of the builtins passed.
#[test]
fn a_setter_at_index_0_sees_no_raw_doc() {
    let lines = probes_both("index", include_str!("fixtures/setup/proto_index_setter.js"), 5);
    assert_eq!(lines.len(), 5, "{lines:?}");
    for line in &lines[1..] {
        assert!(line.contains(r#"raw docs seen "" source energy 1500"#), "{lines:?}");
    }
}

/// A stack overflow in a room's first table read, caught by the bot, must leave the tables whole for the next read.
#[test]
fn a_stack_overflow_in_a_first_read_loses_no_room() {
    let lines = probes_both("stack", include_str!("fixtures/setup/stack_overflow_in_first_read.js"), 4);
    assert_eq!(lines.len(), 4, "{lines:?}");
    for line in &lines {
        assert!(line.ends_with("container on the spawn tile -7 path around the spawn 0,1 1,0"), "{lines:?}");
    }
}

/// A room's first read calls nothing the bot patched in the loop, and a table has its usual prototype. In a match the table
/// a getter reaches holds only the rooms read so far (ARCHITECTURE.md "What differs in a match" 5), not whole as here.
#[test]
fn a_first_read_uses_no_builtin_patched_since_setup() {
    let lines = probes_both("builtins", include_str!("fixtures/setup/builtins_patched_in_loop.js"), 4);
    assert_eq!(lines.len(), 4, "{lines:?}");
    for (n, line) in lines.iter().enumerate() {
        assert!(line.starts_with(&format!("{} ", n + 1)) && line.ends_with(" true true"), "{lines:?}");
    }
}

/// `pristine()` counts a flipped enumerable bit as a change: the flipped builtin shows in for-in, the walk over roomObjects
/// included, so the tick cannot take the list, and doc reuse stops.
#[test]
fn a_flipped_enumerable_bit_leaves_the_list() {
    let lines = probes_both("flip", include_str!("fixtures/setup/enumerable_flip.js"), 5);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[1].contains("sources 2") && lines[1].contains("toLocaleString@5,5:7"), "{lines:?}");
}

const PROTO_SETTER_IN_LOOP: [&str; 6] = ["2 3000", "0 0 false", "3 3000", "4 3000", "5 3000", "6 3000"];
const PROXY_REPLACED: [&str; 5] = ["1 1 1", "2 2 1", "3 3 1", "4 4 1", "5 5 1"];
const PROTO_SETTER_IN_SETUP: [&str; 9] = ["1 undefined 1", "2 0 1", "3 7 1", "4 0 1", "5 7 1", "6 0 1", "7 7 1", "8 0 1", "9 7 1"];
const PROTO_GETTERS_IN_SETUP: [&str; 9] = ["1 undefined undefined", "2 1 2", "3 1 2", "4 1 2", "5 1 2", "6 1 2", "7 1 2", "8 1 2", "9 1 2"];
