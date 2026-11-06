//! M1 vertical slice (ARCHITECTURE.md §14.6): new world, register, place-spawn, deploy tools/bots/m1, 3000
//! ticks to RCL ≥ 2, SockJS room stream. Owner: WP11.
//!
//! `krebs new --world 2x2 --seed 1` → `krebs serve` (tick rate 0) → `register/submit` + `auth/signin` →
//! `place-spawn` → `POST /api/user/code` → 3000 ticks; then RCL ≥ 2 and ≥ 4 creeps, no runtime or engine
//! errors, and a raw-websocket `room:` diff stream that stays consistent with fresh snapshots.
//! Run: `cargo test -p server --release --test m1 -- --ignored --nocapture`.
//! Env: `CHITIN_M1_TICKS` (default 3000), `CHITIN_M1_KEEP=1` leaves the server running for the manual client
//! checks of §14.6.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use util::json::Json;

const BOT: &str = include_str!("../../../tools/bots/m1/main.js");
const ROOMS: &[&str] = &["W2N2", "W3N3", "E2N2", "W2S2", "E2S2", "W7N3"];

#[test]
#[ignore = "M1 integration: needs every wave-1 package landed; run with --ignored"]
fn m1() {
    let ticks: u64 = std::env::var("CHITIN_M1_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let data = std::env::temp_dir().join(format!("chitin-m1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let (port, cli_port) = (free_port(), free_port());
    let addr = format!("127.0.0.1:{port}");

    let out = chitin(&data).args(["new", "--world", "2x2", "--seed", "1"]).output().unwrap();
    assert!(out.status.success(), "chitin new: {}", String::from_utf8_lossy(&out.stderr));
    let server = Server::start(&data, port, cli_port);
    wait_for(Duration::from_secs(120), "server up", || http(&addr, "GET", "/api/version", None, None).is_ok());

    // FLOW-FIRST-LOGIN over the screepsmod-auth surface; extra players (CHITIN_M1_BOTS, default 2) run the same bot
    let bots: usize = std::env::var("CHITIN_M1_BOTS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let (token, uid, room) = join(&addr, "m1bot");
    for i in 1..bots {
        join(&addr, &format!("m1rival{i}"));
    }

    // a socket that follows the room from the start, and one that checks for player errors
    let mut stream = Socket::open(&addr, &token, &[format!("room:{room}"), format!("user:{uid}/console")]);
    let start = game_time(&addr);
    let started = Instant::now();
    let mut follower = RoomState::default();
    let mut errors = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(ticks / 2 + 1200);
    while game_time(&addr) < start + ticks {
        assert!(Instant::now() < deadline, "too slow: tick {} of {}", game_time(&addr) - start, ticks);
        stream.drain(|ch, msg| match ch {
            c if c.starts_with("room:") => follower.apply(msg),
            c if c.ends_with("/console") => errors.extend(msg.get("error").and_then(Json::as_str).map(str::to_string)),
            _ => {}
        });
        std::thread::sleep(Duration::from_millis(200));
    }
    let secs = started.elapsed().as_secs_f64();
    println!("M1: {bots} bots, {ticks} ticks in {secs:.2} s = {:.0} ticks/s", ticks as f64 / secs);
    assert!(errors.is_empty(), "player errors: {errors:?}");

    // the history mod: chunks of the bot's room on disk, served by /room-history (HTTP-ROOM-HISTORY)
    let replay = data.join("replay-data").join(&room);
    let base = std::fs::read_dir(&replay).unwrap_or_else(|e| panic!("{}: {e}", replay.display()))
        .filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".json")?.parse::<u64>().ok())
        .min()
        .expect("a history chunk");
    let (status, body) = http(&addr, "GET", &format!("/room-history?room={room}&time={base}"), None, None).unwrap();
    let chunk = Json::parse(&body).unwrap_or_else(|e| panic!("room-history {status}: {e}: {body}"));
    assert_eq!((status, chunk.get("room").and_then(Json::as_str), chunk.get("base").and_then(Json::as_f64)), (200, Some(room.as_str()), Some(base as f64)));
    assert!(chunk.get("ticks").and_then(|t| t.get(&base.to_string())).is_some(), "chunk without its base tick");
    assert_eq!(http(&addr, "GET", &format!("/room-history?room=../{room}&time={base}"), None, None).unwrap().0, 500);

    // the followed stream equals a fresh subscription once both have seen the same tick
    let mut fresh_sock = Socket::open(&addr, &token, &[format!("room:{room}")]);
    let mut fresh = RoomState::default();
    let synced = Instant::now() + Duration::from_secs(60);
    loop {
        stream.drain(|_, msg| follower.apply(msg));
        fresh_sock.drain(|_, msg| fresh.apply(msg));
        if fresh.time.is_some() && fresh.time == follower.time {
            break;
        }
        assert!(Instant::now() < synced, "streams never met: {:?} vs {:?}", follower.time, fresh.time);
        std::thread::sleep(Duration::from_millis(50));
    }
    follower.assert_consistent_with(&fresh);

    let mine = |t: &str| -> Vec<&Json> {
        let is = |o: &&Json, k: &str, v: &str| o.get(k).and_then(Json::as_str) == Some(v);
        fresh.objects.values().filter(|o| is(o, "type", t) && is(o, "user", &uid)).collect()
    };
    let level = mine("controller").first().and_then(|c| c.get("level")).and_then(Json::as_f64).unwrap_or(0.0);
    let creeps = mine("creep").len();
    println!("M1: tick {}, {room} RCL {level}, {creeps} creeps", game_time(&addr));
    assert!(level >= 2.0, "RCL {level} < 2");
    assert!(creeps >= 4, "{creeps} creeps < 4");

    // the admin CLI over TCP (CLI-SERVER … CLI-BOTS-*)
    let mut cli = Cli::connect(cli_port);
    assert!(cli.greeting.starts_with("Screeps server v") && cli.greeting.ends_with("< \r\n"), "{}", cli.greeting);
    assert!(cli.run("help()").starts_with("< The supported commands are:"));
    assert_eq!(cli.run("storage.db.users.findOne({username: 'm1bot'}).then(u => print(u.username, typeof u.password))"), "'m1bot' 'string'\r\n");
    // a falsy promise result prints nothing (CLI-OUTPUT); --tick-rate 0
    assert_eq!(cli.run("storage.env.get(storage.env.keys.MAIN_LOOP_MIN_DURATION)"), "");
    assert_eq!(cli.run("storage.env.get('gameTime').then(t => print(t > 0))"), "true\r\n");
    assert_eq!(cli.run("system.pauseSimulation()"), "< 'OK'\r\n");
    std::thread::sleep(Duration::from_millis(500));
    let paused_at = game_time(&addr);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(game_time(&addr), paused_at, "paused");
    assert_eq!(cli.run("system.runTicks(5)"), "< 'OK'\r\n");
    wait_for(Duration::from_secs(10), "5 ticks while paused", || game_time(&addr) == paused_at + 5);
    assert_eq!(cli.run("system.resumeSimulation()"), "< 'OK'\r\n");
    let bots = data.join("bots").join("m1");
    std::fs::create_dir_all(&bots).unwrap();
    std::fs::write(bots.join("main.js"), BOT).unwrap();
    let spawned = cli.run("bots.spawn('m1', 'W7N3', {username: 'Clibot', x: 20, y: 20})");
    assert_eq!(spawned, "< 'User Clibot with bot AI \"m1\" spawned in W7N3'\r\n", "{spawned}");
    assert_eq!(cli.run("storage.db['rooms.objects'].findOne({room: 'W7N3', type: 'controller'}).then(c => print(c.level, c.user === undefined))"), "1 false\r\n");
    assert_eq!(cli.run("bots.removeUser('Clibot')"), "< 'User removed successfully'\r\n");
    assert_eq!(cli.run("storage.db.users.findOne({username: 'Clibot'}).then(u => print(u))"), "null\r\n");
    assert_eq!(cli.run("map.closeRoom('W7N3')"), "< { modified: 1 }\r\n");
    assert_eq!(cli.run("map.openRoom('W7N3')"), "< { modified: 1 }\r\n");
    assert_eq!(cli.run("system.save()"), "< 'OK'\r\n");
    drop(cli);

    // the devtools mod (PROTO-DEVTOOLS)
    let dev = |body: &str| ok(http(&addr, "POST", "/api/dev/control", Some(body), None));
    assert_eq!(dev(r#"{"action":"pause"}"#).get("paused").and_then(Json::as_bool), Some(true));
    let at = game_time(&addr);
    dev(r#"{"action":"step","n":3}"#);
    wait_for(Duration::from_secs(10), "3 devtools steps", || game_time(&addr) == at + 3);
    assert_eq!(dev(r#"{"action":"resume"}"#).get("paused").and_then(Json::as_bool), Some(false));
    assert!(dev(r#"{"action":"nope"}"#).get("error").is_some());

    let log = server.log();
    for bad in ["panicked", "Error processing room", "command panicked", "Error while main loop"] {
        assert!(!log.contains(bad), "server log contains {bad:?}:\n{log}");
    }

    // persistence (§9): a checkpoint written while running survives a hard kill; the world, the account and
    // the bot come back
    let ckpt = data.join("world.bin");
    let mtime = || std::fs::metadata(&ckpt).and_then(|m| m.modified()).ok();
    let before = mtime();
    wait_for(Duration::from_secs(30), "a checkpoint after the run", || mtime() > before);
    let checkpointed = game_time(&addr);
    drop(server);
    let server = Server::start(&data, port, cli_port);
    wait_for(Duration::from_secs(120), "server up again", || http(&addr, "GET", "/api/version", None, None).is_ok());
    let resumed = game_time(&addr);
    assert!(resumed + 200 > checkpointed && resumed > start + ticks / 2, "resumed at {resumed}, checkpoint near {checkpointed}");
    let signin = r#"{"email":"m1bot","password":"m1-password"}"#;
    let r = ok(http(&addr, "POST", "/api/auth/signin", Some(signin), None));
    let token = r.get("token").and_then(Json::as_str).unwrap_or_else(|| panic!("signin after restart: {r}")).to_string();
    let mut sock = Socket::open(&addr, &token, &[format!("room:{room}")]);
    let mut restored = RoomState::default();
    wait_for(Duration::from_secs(30), "a room snapshot after restart", || {
        sock.drain(|_, msg| restored.apply(msg));
        restored.time.is_some()
    });
    let mine = |t: &str| restored.objects.values().filter(|o| o.get("type").and_then(Json::as_str) == Some(t) && o.get("user").and_then(Json::as_str) == Some(uid.as_str())).count();
    assert!(mine("spawn") == 1 && mine("creep") >= 1, "after restart: {} spawns, {} creeps", mine("spawn"), mine("creep"));
    let start_again = game_time(&addr);
    wait_for(Duration::from_secs(60), "ticks after restart", || game_time(&addr) > start_again + 50);
    println!("M1: restarted from the checkpoint at tick {resumed} (checkpoint near {checkpointed}), still running");

    if std::env::var("CHITIN_M1_KEEP").is_ok() {
        println!("server left running on http://{addr} (data {}); Ctrl-C to stop", data.display());
        std::mem::forget(server);
    }
}

fn chitin(data: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_krebs"));
    c.arg("--data").arg(data).current_dir(data.parent().unwrap());
    c
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn wait_for(limit: Duration, what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + limit;
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// `krebs serve`; its output goes to a log file. Killed on drop.
struct Server {
    child: Child,
    log: PathBuf,
}

impl Server {
    fn start(data: &Path, port: u16, cli_port: u16) -> Server {
        let log = data.join("server.log");
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&log).unwrap();
        let config = data.join("krebs.json");
        std::fs::write(&config, r#"{"persist": {"intervalS": 2}}"#).unwrap();
        let child = chitin(data)
            .args(["serve", "--config", config.to_str().unwrap(), "--port", &port.to_string(), "--cli-port", &cli_port.to_string(), "--tick-rate", "0", "--mods", "history,devtools"])
            .stdout(file.try_clone().unwrap())
            .stderr(file)
            .spawn()
            .unwrap();
        Server { child, log }
    }

    fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---- CLI ----

/// A CLI connection: each `run` sends one line and returns its output (a `print('##')` marks the end).
struct Cli {
    s: TcpStream,
    greeting: String,
}

impl Cli {
    fn connect(port: u16) -> Cli {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let greeting = read_until(&mut s, |t| t.ends_with("< \r\n"));
        Cli { s, greeting }
    }

    fn run(&mut self, line: &str) -> String {
        eprintln!("cli> {line}");
        self.s.write_all(format!("{line}\r\nprint('##')\r\n").as_bytes()).unwrap();
        let out = read_until(&mut self.s, |t| t.ends_with("'##'\r\n< "));
        out.strip_suffix("'##'\r\n< ").unwrap().to_string()
    }
}

fn read_until(s: &mut TcpStream, done: impl Fn(&str) -> bool) -> String {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    while !done(&String::from_utf8_lossy(&out)) {
        let n = s.read(&mut buf).unwrap_or_else(|e| panic!("CLI output: {e}; so far {:?}", String::from_utf8_lossy(&out)));
        assert!(n > 0, "CLI closed: {}", String::from_utf8_lossy(&out));
        out.extend_from_slice(&buf[..n]);
    }
    String::from_utf8(out).unwrap()
}

// ---- HTTP ----

/// One request (`Connection: close`); returns the status and the body.
fn http(addr: &str, method: &str, path: &str, body: Option<&str>, token: Option<&str>) -> Result<(u16, String), String> {
    let mut s = TcpStream::connect(addr).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    if let Some(t) = token {
        req += &format!("X-Token: {t}\r\nX-Username: m1\r\n");
    }
    let body = body.unwrap_or("");
    if method == "POST" {
        req += &format!("Content-Type: application/json\r\nContent-Length: {}\r\n", body.len());
    }
    req += "\r\n";
    req += body;
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, rest) = text.split_once("\r\n\r\n").ok_or("malformed response")?;
    let status = head.split(' ').nth(1).and_then(|c| c.parse().ok()).ok_or("no status")?;
    if !head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        return Ok((status, rest.to_string()));
    }
    let (mut out, mut rest) = (String::new(), rest);
    while let Some((size, tail)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).map_err(|_| "bad chunk")?;
        if n == 0 || tail.len() < n {
            break;
        }
        out.push_str(&tail[..n]);
        rest = tail[n..].trim_start_matches("\r\n");
    }
    Ok((status, out))
}

fn ok(r: Result<(u16, String), String>) -> Json {
    let (status, body) = r.unwrap();
    Json::parse(&body).unwrap_or_else(|e| panic!("HTTP {status}: {e}: {body}"))
}

fn game_time(addr: &str) -> u64 {
    ok(http(addr, "GET", "/api/game/time", None, None)).get("time").and_then(Json::as_f64).unwrap_or(0.0) as u64
}

/// The first open plain spot with a free 3×3 around it, nearest the room centre, in the first room that takes
/// it (the backend rejects walls, exits, objects and owned rooms).
/// Register, sign in, place a spawn and deploy the M1 bot; returns (token, user id, room).
fn join(addr: &str, name: &str) -> (String, String, String) {
    let creds = format!(r#"{{"username":"{name}","email":"{name}@example.com","password":"m1-password"}}"#);
    let r = ok(http(addr, "POST", "/api/register/submit", Some(&creds), None));
    assert!(r.get("error").is_none(), "register: {r}");
    let signin = format!(r#"{{"email":"{name}","password":"m1-password"}}"#);
    let r = ok(http(addr, "POST", "/api/auth/signin", Some(&signin), None));
    let token = r.get("token").and_then(Json::as_str).unwrap_or_else(|| panic!("signin: {r}")).to_string();
    let me = ok(http(addr, "GET", "/api/auth/me", None, Some(&token)));
    let uid = me.get("_id").and_then(Json::as_str).unwrap_or_else(|| panic!("auth/me: {me}")).to_string();
    let room = place_spawn(addr, &token);
    let code = Json::Obj(vec![("modules".into(), Json::Obj(vec![("main".into(), Json::Str(BOT.into()))]))]).stringify();
    let r = ok(http(addr, "POST", "/api/user/code", Some(&code), Some(&token)));
    assert!(r.get("error").is_none(), "code upload: {r}");
    (token, uid, room)
}

fn place_spawn(addr: &str, token: &str) -> String {
    for room in ROOMS {
        let t = ok(http(addr, "GET", &format!("/api/game/room-terrain?room={room}&encoded=1"), None, None));
        let Some(terrain) = t.get("terrain").and_then(|a| a.at(0)).and_then(|d| d.get("terrain")).and_then(Json::as_str) else { continue };
        let wall = |x: i32, y: i32| terrain.as_bytes()[(y * 50 + x) as usize] & 1 == 1;
        let mut spots: Vec<(i32, i32)> = (4..46).flat_map(|y| (4..46).map(move |x| (x, y))).filter(|&(x, y)| (-1..=1).all(|dy| (-1..=1).all(|dx| !wall(x + dx, y + dy)))).collect();
        spots.sort_by_key(|&(x, y)| (x - 25).pow(2) + (y - 25).pow(2));
        for (x, y) in spots.into_iter().take(20) {
            let body = format!(r#"{{"room":"{room}","x":{x},"y":{y},"name":"Spawn1"}}"#);
            let r = ok(http(addr, "POST", "/api/game/place-spawn", Some(&body), Some(token)));
            if r.get("error").is_none() {
                return room.to_string();
            }
        }
    }
    panic!("no room accepted a spawn");
}

// ---- raw websocket (SOCK-WS-RAW) ----

struct Socket {
    tx: TcpStream,
    rx: Receiver<String>,
}

impl Socket {
    /// Connects to `/socket/websocket`, authenticates and subscribes.
    fn open(addr: &str, token: &str, channels: &[String]) -> Socket {
        let mut s = TcpStream::connect(addr).unwrap();
        let req = format!("GET /socket/websocket HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n");
        s.write_all(req.as_bytes()).unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut status = String::new();
        r.read_line(&mut status).unwrap();
        assert!(status.contains(" 101 "), "websocket upgrade: {status}");
        loop {
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        let (send, rx) = channel();
        let mut pong = s.try_clone().unwrap();
        std::thread::spawn(move || {
            while let Some((op, payload)) = read_frame(&mut r) {
                match op {
                    1 if send.send(String::from_utf8_lossy(&payload).to_string()).is_err() => return,
                    9 => write_frame(&mut pong, 10, &payload),
                    8 => return,
                    _ => {}
                }
            }
        });
        let mut sock = Socket { tx: s, rx };
        sock.send(&format!("auth {token}"));
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let m = sock.rx.recv_timeout(end.saturating_duration_since(Instant::now())).expect("no auth reply");
            assert_ne!(m, "auth failed");
            if m.starts_with("auth ok") {
                break;
            }
        }
        for c in channels {
            sock.send(&format!("subscribe {c}"));
        }
        sock
    }

    fn send(&mut self, text: &str) {
        write_frame(&mut self.tx, 1, text.as_bytes());
    }

    /// Every `[channel, data]` message received so far.
    fn drain(&mut self, mut f: impl FnMut(&str, &Json)) {
        while let Ok(m) = self.rx.try_recv() {
            let Ok(Json::Arr(a)) = Json::parse(&m) else { continue };
            if let (Some(ch), Some(data)) = (a.first().and_then(Json::as_str), a.get(1)) {
                f(ch, data);
            }
        }
    }
}

fn read_frame(r: &mut impl Read) -> Option<(u8, Vec<u8>)> {
    let (mut op, mut payload) = (0, Vec::new());
    loop {
        let mut h = [0u8; 2];
        r.read_exact(&mut h).ok()?;
        let mut len = (h[1] & 0x7f) as u64;
        if len == 126 {
            let mut b = [0u8; 2];
            r.read_exact(&mut b).ok()?;
            len = u16::from_be_bytes(b) as u64;
        } else if len == 127 {
            let mut b = [0u8; 8];
            r.read_exact(&mut b).ok()?;
            len = u64::from_be_bytes(b);
        }
        let mask = if h[1] & 0x80 != 0 {
            let mut m = [0u8; 4];
            r.read_exact(&mut m).ok()?;
            Some(m)
        } else {
            None
        };
        let mut data = vec![0u8; len as usize];
        r.read_exact(&mut data).ok()?;
        if let Some(m) = mask {
            data.iter_mut().enumerate().for_each(|(i, b)| *b ^= m[i % 4]);
        }
        if h[0] & 0x0f != 0 {
            op = h[0] & 0x0f;
        }
        if op >= 8 {
            return Some((op, data)); // control frames are never fragmented
        }
        payload.extend(data);
        if h[0] & 0x80 != 0 {
            return Some((op, payload));
        }
    }
}

/// A masked client frame (RFC 6455 §5.3).
fn write_frame(w: &mut impl Write, op: u8, payload: &[u8]) {
    let mut f = vec![0x80 | op];
    match payload.len() {
        n if n < 126 => f.push(0x80 | n as u8),
        n if n < 65536 => {
            f.push(0x80 | 126);
            f.extend((n as u16).to_be_bytes());
        }
        n => {
            f.push(0x80 | 127);
            f.extend((n as u64).to_be_bytes());
        }
    }
    let mask = [0x37, 0xfa, 0x21, 0x3d];
    f.extend(mask);
    f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    let _ = w.write_all(&f);
}

// ---- room stream (SOCK-ROOM-SUBSCRIBE + FMT-ROOM-DIFF, applied as the official client does) ----

#[derive(Default)]
struct RoomState {
    objects: BTreeMap<String, Json>,
    time: Option<u64>,
}

impl RoomState {
    fn apply(&mut self, msg: &Json) {
        if let Some(Json::Obj(objs)) = msg.get("objects") {
            for (id, diff) in objs {
                match (diff, self.objects.get_mut(id)) {
                    (Json::Null, _) => {
                        self.objects.remove(id);
                    }
                    (Json::Obj(d), Some(Json::Obj(cur))) => {
                        for (k, v) in d {
                            let slot = cur.iter().position(|(ck, _)| ck == k);
                            match (v, slot) {
                                (Json::Null, Some(i)) => {
                                    cur.remove(i);
                                }
                                (Json::Obj(sub), Some(i)) if matches!(cur[i].1, Json::Obj(_) | Json::Arr(_)) => {
                                    for (sk, sv) in sub {
                                        set_sub(&mut cur[i].1, sk, sv.clone());
                                    }
                                }
                                (_, Some(i)) => cur[i].1 = v.clone(),
                                (_, None) => cur.push((k.clone(), v.clone())),
                            }
                        }
                    }
                    (d, _) => {
                        self.objects.insert(id.clone(), d.clone());
                    }
                }
            }
        }
        if let Some(t) = msg.get("gameTime").and_then(Json::as_f64) {
            self.time = Some(t as u64);
        }
    }

    /// Same objects and same values, except what FMT-ROOM-DIFF cannot express: a deleted nested key stays in
    /// a follower, and a key set to null reads as deleted.
    fn assert_consistent_with(&self, fresh: &RoomState) {
        let ids = |s: &RoomState| s.objects.keys().cloned().collect::<Vec<_>>();
        assert_eq!(ids(self), ids(fresh), "object ids differ at tick {:?}", fresh.time);
        for (id, f) in &fresh.objects {
            let o = &self.objects[id];
            for (k, fv) in f.as_object().into_iter().flatten() {
                let ov = o.get(k).unwrap_or(&Json::Null);
                let same = match (fv, ov) {
                    (Json::Obj(fs), Json::Obj(_)) => fs.iter().all(|(sk, sv)| ov.get(sk).unwrap_or(&Json::Null) == sv || sv.is_null()),
                    _ => fv == ov,
                };
                assert!(same, "{id}.{k}: stream {ov} vs snapshot {fv} at tick {:?}", fresh.time);
            }
        }
    }
}

fn set_sub(target: &mut Json, k: &str, v: Json) {
    match target {
        Json::Obj(m) => match m.iter().position(|(mk, _)| mk == k) {
            Some(i) => m[i].1 = v,
            None => m.push((k.to_string(), v)),
        },
        Json::Arr(a) => {
            if let Ok(i) = k.parse::<usize>() {
                if i >= a.len() {
                    a.resize(i + 1, Json::Null);
                }
                a[i] = v;
            }
        }
        _ => {}
    }
}
