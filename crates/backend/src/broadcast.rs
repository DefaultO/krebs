//! The broadcaster thread: `TickReport` → one room diff per room per tick (same bytes to every subscriber),
//! map views, user channels, throttling (ARCHITECTURE.md §8.4). Owner: WP04, then WP19.
//!
//! Not a mirror: replaces the `roomsDone` pubsub fan-out of socket/{rooms,map,user}.js and the driver's
//! `user:<id>/console|cpu` publishes. One broadcast per `socketUpdateThrottle` window (lodash `_.throttle`
//! leading + trailing); reports arriving meanwhile are merged, so skipped ticks reach rooms as one cumulative
//! diff (SOCK-THROTTLE).

use crate::socket::{map, rooms, user};
use crate::{Published, TickReport};
use engine::world::WorldView;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};
use util::json::Json;

/// `config.backend.socketUpdateThrottle`'s vanilla default (mods may change it: `Published::mods`).
pub const THROTTLE: Duration = Duration::from_millis(200);

/// The latest unbroadcast report; `put` merges into it instead of queueing (never blocks the tick thread).
#[derive(Default)]
pub(crate) struct TickSlot {
    m: Mutex<Option<TickReport>>,
    cv: Condvar,
}

impl TickSlot {
    pub(crate) fn put(&self, r: TickReport) {
        let mut g = self.m.lock().unwrap_or_else(|e| e.into_inner());
        *g = Some(match g.take() {
            None => r,
            Some(mut acc) => {
                // runs on the tick thread: O(n log n), not a contains() per room
                let mut processed = acc.processed.to_vec();
                processed.extend(r.processed.iter());
                processed.sort_unstable();
                processed.dedup();
                acc.view = r.view;
                acc.processed = processed.into();
                acc.console.extend(r.console);
                acc.cpu.extend(r.cpu);
                acc.visuals = r.visuals; // visuals are per tick: only the newest tick's are shown
                acc.user_events.extend(r.user_events);
                acc
            }
        });
        self.cv.notify_one();
    }

    fn take(&self) -> TickReport {
        let g = self.m.lock().unwrap_or_else(|e| e.into_inner());
        let mut g = self.cv.wait_while(g, |r| r.is_none()).unwrap_or_else(|e| e.into_inner());
        g.take().expect("woken with a report")
    }
}

/// The next report, with the view published last: a CLI write publishes a view with no report, and a room subscribed
/// since then must not see a value go back for one broadcast.
fn next_report(p: &Published, slot: &TickSlot) -> TickReport {
    let mut report = slot.take();
    report.view = p.get();
    report
}

pub(crate) fn run(p: &Published, slot: &TickSlot) {
    let mut seed = [0u8; 8];
    util::sys::os_random(&mut seed);
    let mut rng = util::rng::Rng::new(u64::from_le_bytes(seed));
    let mut prev: Option<WorldView> = None;
    loop {
        let report = next_report(p, slot);
        let started = Instant::now();
        // one bad tick must not stop every later room/user push
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| broadcast(p, &report, prev.as_ref(), &mut rng)));
        prev = Some(report.view);
        std::thread::sleep(p.mods().throttle.saturating_sub(started.elapsed()));
    }
}

/// One broadcast: runner relays (console, cpu, user events) first, as they precede `roomsDone` in vanilla,
/// then rooms, map views and the user.js `roomsDone` channels.
pub(crate) fn broadcast(p: &Published, r: &TickReport, prev: Option<&WorldView>, rng: &mut util::rng::Rng) {
    let view = &r.view;
    let mut hub = p.hub().lock();
    let id = |u: u32| view.users.get(u as usize).map(|s| s.id.as_str().to_string());
    for (u, log, results, error) in &r.console {
        let Some(id) = id(*u) else { continue };
        if id == "2" || id == "3" {
            continue; // NPC consoles go to stdout only (driver sendConsoleMessages)
        }
        let list = |v: &Vec<String>| Json::Arr(v.iter().map(|s| Json::Str(s.clone())).collect());
        let messages = Json::Obj(vec![("log".into(), list(log)), ("results".into(), list(results))]);
        let channel = format!("user:{id}/console");
        hub.emit(&channel, &Json::Obj(vec![("messages".into(), messages)]).stringify());
        if let Some(e) = error {
            hub.emit(&channel, &Json::Obj(vec![("error".into(), Json::Str(e.clone()))]).stringify());
        }
    }
    for (u, used, memory) in &r.cpu {
        if let Some(id) = id(*u) {
            hub.emit(&format!("user:{id}/cpu"), &format!("{{\"cpu\":{used},\"memory\":{memory}}}"));
        }
    }
    for ev in &r.user_events {
        if let Some(id) = id(ev.user) {
            hub.emit(&format!("user:{id}/{}", ev.channel), &ev.data.stringify());
        }
    }
    hub.visuals = r.visuals.iter().map(|(u, room, v)| ((*u, room.clone()), v.clone())).collect();
    // rooms nobody watches any more (not observable: the originals skip empty lists)
    hub.rooms.retain(|_, v| !v.is_empty());
    hub.maps.retain(|_, v| !v.is_empty());
    rooms::broadcast(&mut hub, view, rng);
    map::broadcast(&hub, view, prev);
    user::broadcast(&mut hub, view);
    crate::socket::server::push_mod_channels(p, &hub, view);
}

#[cfg(test)]
mod report {
    use super::*;
    use crate::fake;
    use std::sync::Arc;

    /// A write published while a report waits out the throttle: that report goes out with the newer view.
    #[test]
    fn a_report_goes_out_with_the_view_published_since() {
        let world = fake::world();
        let p = Published::new(fake::view(&world));
        let (older, slot) = (p.get(), TickSlot::default());
        let older_time = older.time;
        slot.put(TickReport { view: older, processed: Arc::from(Vec::new()), console: Vec::new(), cpu: Vec::new(), visuals: Vec::new(), user_events: Vec::new() });
        let mut newer = p.get();
        newer.time = older_time + 1;
        p.set(newer);
        assert_eq!(next_report(&p, &slot).view.time, older_time + 1);
    }
}

/// End to end over real sockets: FLOW-CONNECT (version, steam ticket, auth/me), SockJS and raw websockets, the
/// room snapshot/diff stream from a scripted publisher, gzip, user channels.
#[cfg(test)]
mod e2e {
    use crate::fake;
    use crate::ws::tests::{client_frame, server_frame};
    use crate::*;
    use engine::doc::Doc;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    use util::json::Json;

    struct NoCli;
    impl CliEval for NoCli {
        fn open(&self) -> Box<dyn CliSession> {
            struct S;
            impl CliSession for S {
                fn eval(&mut self, _: &str) -> String {
                    String::new()
                }
            }
            Box::new(S)
        }
    }

    fn boot(password: Option<&str>) -> (Backend, Arc<Published>) {
        let p = Arc::new(Published::new(fake::view(&fake::world())));
        let (tx, _h) = fake::tick_thread(p.clone());
        let config = BackendConfig {
            host: "127.0.0.1".into(),
            port: 0,
            cli_host: "127.0.0.1".into(),
            cli_port: 0,
            max_connections: 64,
            request_timeout_s: 120,
            server_password: password.map(Into::into),
            steam: "single-user:alice".into(),
        };
        let b = start(BackendDeps { published: p.clone(), commands: tx, cli: Arc::new(NoCli), config, routes: vec![], socket_channels: vec![], server_data: vec![] });
        (b, p)
    }

    /// One request on an open connection: (status, headers, body).
    fn request(s: &mut BufReader<TcpStream>, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, Vec<(String, String)>, String) {
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n", body.len());
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        s.get_mut().write_all(head.as_bytes()).unwrap();
        s.get_mut().write_all(body.as_bytes()).unwrap();
        let mut status = String::new();
        s.read_line(&mut status).unwrap();
        let mut hs = vec![];
        loop {
            let mut l = String::new();
            s.read_line(&mut l).unwrap();
            if l.trim().is_empty() {
                break;
            }
            let (k, v) = l.split_once(':').unwrap();
            hs.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
        let len: usize = hs.iter().find(|(k, _)| k == "content-length").unwrap().1.parse().unwrap();
        let mut b = vec![0; len];
        s.read_exact(&mut b).unwrap();
        (status.split(' ').nth(1).unwrap().parse().unwrap(), hs, String::from_utf8(b).unwrap())
    }

    fn connect(b: &Backend) -> BufReader<TcpStream> {
        let s = TcpStream::connect(b.local_addr()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        BufReader::new(s)
    }

    struct Ws {
        s: BufReader<TcpStream>,
        framed: bool,
        pending: Vec<String>,
    }

    impl Ws {
        fn open(b: &Backend, path: &str) -> Ws {
            let mut s = connect(b);
            let req = format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n");
            s.get_mut().write_all(req.as_bytes()).unwrap();
            let mut line = String::new();
            s.read_line(&mut line).unwrap();
            assert!(line.starts_with("HTTP/1.1 101"), "{line}");
            loop {
                line.clear();
                s.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
            }
            let framed = path != "/socket/websocket";
            let mut ws = Ws { s, framed, pending: vec![] };
            if framed {
                assert_eq!(ws.frame(), "o");
            }
            ws
        }
        fn frame(&mut self) -> String {
            loop {
                let (_, op, p) = server_frame(&mut self.s).unwrap();
                if op == crate::ws::OP_TEXT {
                    return String::from_utf8(p).unwrap();
                }
            }
        }
        /// The next protocol message (unpacking `a[…]` frames).
        fn next(&mut self) -> String {
            while self.pending.is_empty() {
                let f = self.frame();
                if !self.framed {
                    return f;
                }
                let arr = Json::parse(f.strip_prefix('a').expect("a-frame")).unwrap();
                self.pending = arr.as_array().unwrap().iter().map(|m| m.as_str().unwrap().to_string()).collect();
            }
            self.pending.remove(0)
        }
        /// Skips messages until one starts with `prefix`.
        fn until(&mut self, prefix: &str) -> String {
            loop {
                let m = self.next();
                if m.starts_with(prefix) {
                    return m;
                }
            }
        }
        fn send(&mut self, msg: &str) {
            let payload = if self.framed { Json::Arr(vec![Json::Str(msg.into())]).stringify() } else { msg.to_string() };
            self.s.get_mut().write_all(&client_frame(true, crate::ws::OP_TEXT, payload.as_bytes())).unwrap();
        }
    }

    fn event(m: &str) -> (String, Json) {
        let j = Json::parse(m).unwrap();
        (j.at(0).unwrap().as_str().unwrap().to_string(), j.at(1).unwrap().clone())
    }

    #[test]
    fn connect_flow_over_http() {
        let (b, _p) = boot(Some("pw"));
        let mut c = connect(&b);
        // keep-alive: several requests on one connection; version is exempt from the password gate
        let (st, _, body) = request(&mut c, "GET", "/api/version", &[], "");
        assert_eq!(st, 200);
        assert_eq!(Json::parse(&body).unwrap().get("protocol").and_then(Json::as_f64), Some(14.0));
        let (_, _, body) = request(&mut c, "POST", "/api/auth/steam-ticket", &[("Content-Type", "application/json")], r#"{"ticket":"ab"}"#);
        assert_eq!(body, r#"{"error":"incorrect server password"}"#);
        let pw = ("X-Server-Password", "pw");
        let (_, _, body) = request(&mut c, "POST", "/api/auth/steam-ticket", &[pw, ("Content-Type", "application/json")], r#"{"ticket":"ab","useNativeAuth":false}"#);
        let token = Json::parse(&body).unwrap().get("token").and_then(Json::as_str).unwrap().to_string();
        let (st, hs, body) = request(&mut c, "GET", "/api/auth/me", &[pw, ("X-Token", &token), ("X-Username", &token)], "");
        assert_eq!(st, 200, "{body}");
        assert_eq!(Json::parse(&body).unwrap().get("username").and_then(Json::as_str), Some("alice"));
        assert!(hs.iter().any(|(k, _)| k == "x-token"));
        assert!(hs.iter().any(|(k, v)| k == "access-control-allow-origin" && v == "*"));
        let (st, _, body) = request(&mut c, "GET", "/api/user/world-status", &[pw], "");
        assert_eq!((st, body.as_str()), (401, r#"{"error":"unauthorized"}"#));
        // chunked upload
        let chunked = "POST /api/user/console HTTP/1.1\r\nHost: x\r\nX-Server-Password: pw\r\nX-Token: TOKEN\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n6\r\n{\"expr\r\n14\r\nession\":\"Game.time\"}\r\n0\r\n\r\n".replace("TOKEN", &token);
        c.get_mut().write_all(chunked.as_bytes()).unwrap();
        let mut status = String::new();
        c.read_line(&mut status).unwrap();
        assert!(status.starts_with("HTTP/1.1 200"), "{status}");
        let mut rest = String::new();
        while rest != "\r\n" {
            rest.clear();
            c.read_line(&mut rest).unwrap();
        }
        // curl's `Expect: 100-continue`: the interim response comes before the body is sent
        let mut c3 = connect(&b);
        let head = format!("POST /api/user/console HTTP/1.1\r\nHost: x\r\nX-Server-Password: pw\r\nX-Token: {token}\r\nContent-Type: application/json\r\nExpect: 100-continue\r\nContent-Length: 26\r\n\r\n");
        c3.get_mut().write_all(head.as_bytes()).unwrap();
        let mut line = String::new();
        c3.read_line(&mut line).unwrap();
        assert_eq!(line, "HTTP/1.1 100 Continue\r\n");
        c3.read_line(&mut line).unwrap();
        c3.get_mut().write_all(br#"{"expression":"Game.time"}"#).unwrap();
        line.clear();
        c3.read_line(&mut line).unwrap();
        assert!(line.starts_with("HTTP/1.1 200"), "{line}");
        // sockjs info bypasses the gate
        let mut c2 = connect(&b);
        let (st, _, body) = request(&mut c2, "GET", "/socket/info", &[], "");
        assert_eq!(st, 200);
        assert!(body.contains(r#""websocket":true"#));
    }

    #[test]
    fn room_stream_and_user_channels() {
        let (b, p) = boot(None);
        let token = authlib::gen_token("a11ce0000000001");
        let mut a = Ws::open(&b, "/socket/123/abcdefgh/websocket");
        assert!(a.next().starts_with("time "));
        assert_eq!(a.next(), "protocol 14");
        // unauthenticated room subscriptions are ignored; subscribe right after auth is processed in order
        a.send(&format!("auth {token}"));
        a.send("subscribe room:W2N1");
        a.send("subscribe user:a11ce0000000001/console");
        a.send("subscribe user:a11ce0000000001/cpu");
        a.send("subscribe user:b0b000000000002/console"); // someone else's: ignored
        a.send("subscribe roomMap2:W2N1");
        a.send("subscribe user:a11ce0000000001/memory/a.s");
        let ok = a.until("auth ok ");
        assert!(authlib::check_token(ok.strip_prefix("auth ok ").unwrap(), &p.get()).is_some());
        let (ch, snap) = event(&a.until("[\"room:W2N1\""));
        assert_eq!(ch, "room:W2N1");
        assert!(snap.get("gameTime").is_none());
        assert_eq!(snap.get("objects").unwrap().as_object().unwrap().len(), 4);
        let (_, cpu) = event(&a.until("[\"user:a11ce0000000001/cpu\""));
        assert_eq!(cpu.get("cpu").and_then(Json::as_f64), Some(0.0));
        assert!(a.until("[\"roomMap2:W2N1\"").starts_with(r#"["roomMap2:W2N1",{"w":[]"#));

        // a raw-websocket viewer of the same room
        let mut r = Ws::open(&b, "/socket/websocket");
        r.until("protocol 14");
        r.send(&format!("auth {}", authlib::gen_token("b0b000000000002")));
        r.until("auth ok");
        r.send("subscribe room:W2N1");
        r.until("[\"room:W2N1\"");

        // scripted publisher: tick 1001 changes the creep's store, removes the site, adds a source
        let mut w = fake::world();
        let objs: Vec<_> = w.rooms[1]
            .objects
            .iter()
            .filter(|d| d.kind() != "constructionSite")
            .map(|d| {
                if d.kind() == "creep" {
                    let mut o = d.obj.clone();
                    o.set(engine::doc::Atom::intern("store"), engine::doc::Val::from_json(&Json::parse(r#"{"energy":20}"#).unwrap()));
                    Doc::new(d.seq, o)
                } else {
                    d.clone()
                }
            })
            .chain([Doc::new(500, fake::obj(r#"{"_id":"new1","type":"source","room":"W2N1","x":1,"y":2}"#))])
            .collect();
        install_room(&mut w, 1, objs, None, None);
        edit_user(&mut w, fake::ALICE, |st| {
            st.memory = util::jsstr::JsStr::from_str(r#"{"a":{"s":"changed"}}"#);
            st.memory_version = 4;
        });
        w.time = 1001;
        let view = fake::view(&w);
        p.set(view.clone());
        b.on_tick(TickReport {
            view,
            processed: Arc::from(vec![1u32]),
            console: vec![(fake::ALICE, vec!["hello".into()], vec!["1001".into()], Some("Error: boom".into()))],
            cpu: vec![(fake::ALICE, 7, 20)],
            visuals: vec![(fake::ALICE, "W2N1".into(), Arc::from("{\"t\":\"c\"}\n"))],
            user_events: vec![],
        });
        let (_, console) = event(&a.until("[\"user:a11ce0000000001/console\""));
        assert_eq!(console.stringify(), r#"{"messages":{"log":["hello"],"results":["1001"]}}"#);
        let (_, err) = event(&a.until("[\"user:a11ce0000000001/console\""));
        assert_eq!(err.stringify(), r#"{"error":"Error: boom"}"#);
        let room_a = a.until("[\"room:W2N1\"");
        let (_, tick) = event(&room_a);
        assert_eq!(tick.get("objects").unwrap().stringify(), r#"{"creep2":{"store":{"energy":20}},"site2":null,"new1":{"_id":"new1","type":"source","room":"W2N1","x":1,"y":2}}"#);
        assert_eq!(tick.get("gameTime").and_then(Json::as_f64), Some(1001.0));
        assert_eq!(tick.get("visual").and_then(Json::as_str), Some("{\"t\":\"c\"}\n"));
        assert_eq!(tick.get("info").unwrap().stringify(), r#"{"mode":"world"}"#);
        assert!(tick.get("users").is_none(), "alice was in the snapshot");
        let (_, mem) = event(&a.until("[\"user:a11ce0000000001/memory/a.s\""));
        assert_eq!(mem.as_str(), Some("changed"));
        // bob sees the same object diff, his own (empty) visual and no flags
        let (_, tick_b) = event(&r.until("[\"room:W2N1\""));
        assert_eq!(tick_b.get("objects"), tick.get("objects"));
        assert_eq!(tick_b.get("visual").and_then(Json::as_str), Some(""));
        assert!(tick_b.get("flags").is_none() && tick.get("flags").is_some());

        // gzip: large messages are deflated (zlib, level-agnostic), small ones are not
        a.send("gzip on");
        a.send("subscribe room:W2N1");
        let gz = a.until("gz:");
        let raw = util::deflate::zlib_decompress(&util::base64::decode(gz.strip_prefix("gz:").unwrap()).unwrap(), 1 << 20).unwrap();
        assert!(String::from_utf8(raw).unwrap().starts_with("[\"room:W2N1\""));
    }
}
