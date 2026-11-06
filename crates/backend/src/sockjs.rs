//! SockJS 0.3.24: `/socket/info`, `/socket/<server>/<session>/websocket` (`o`/`a[…]`/`h`/`c`, extended
//! escaping, 25 s heartbeat) and raw `/socket/websocket`; no XHR fallbacks (ARCHITECTURE.md §8.1). Owner: WP04.
//!
//! Not a mirror of a reference file: reproduces srvmods/sockjs/lib/{sockjs,transport,trans-websocket,
//! chunking-test,utils}.js for the websocket transports (spec 10 SOCK-PREFIX, SOCK-INFO, SOCK-WS-SOCKJS,
//! SOCK-WS-RAW, SOCK-HEARTBEAT; SOCK-FALLBACK omitted).

use crate::http::{Request, Response};
use crate::socket::server::{self as sock, Conn, Out};
use crate::ws;
use crate::Published;
use std::io::{BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use util::json::Json;

const PREFIX: &str = "/socket";
/// sockjs `heartbeat_delay`.
pub const HEARTBEAT: Duration = Duration::from_secs(25);
/// trans-websocket.js: close if a ping is not answered within 10 s.
pub const PONG_TIMEOUT: Duration = Duration::from_secs(10);

/// `^/socket([/].+|[/]?)$`: requests sockjs consumes before Express (SOCK-PREFIX).
pub fn is_socket_path(path: &str) -> bool {
    path.strip_prefix(PREFIX).is_some_and(|r| r.is_empty() || r.starts_with('/'))
}

enum Route {
    Welcome,
    Info,
    Raw,
    Session,
    NotFound,
}

fn route(path: &str) -> Route {
    let rest = path.strip_prefix(PREFIX).unwrap_or("");
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let seg = |s: &str| !s.is_empty() && !s.contains(['/', '.']);
    match rest {
        "" => Route::Welcome,
        "/info" => Route::Info,
        "/websocket" => Route::Raw,
        r => match r.strip_prefix('/').map(|r| r.split('/').collect::<Vec<_>>()).as_deref() {
            Some([s, sess, "websocket"]) if seg(s) && seg(sess) => Route::Session,
            _ => Route::NotFound,
        },
    }
}

/// sockjs `xhr_cors` filter.
fn xhr_cors(req: &Request, res: &mut Response) {
    match req.header("origin") {
        Some(o) => {
            res.headers.push(("Access-Control-Allow-Credentials".into(), "true".into()));
            res.headers.push(("Access-Control-Allow-Origin".into(), o.into()));
        }
        None => res.headers.push(("Access-Control-Allow-Origin".into(), "*".into())),
    }
    res.headers.push(("Vary".into(), "Origin".into()));
    if let Some(h) = req.header("access-control-request-headers") {
        res.headers.push(("Access-Control-Allow-Headers".into(), h.into()));
    }
}

/// Plain-HTTP requests under the prefix: welcome, `/info` (GET, OPTIONS), 404s.
pub fn http(req: &Request) -> Response {
    match (req.method.as_str(), route(&req.path)) {
        ("GET", Route::Welcome) => Response::text(200, "text/plain; charset=UTF-8", "Welcome to SockJS!\n"),
        ("GET", Route::Info) => {
            let mut e = [0u8; 4];
            util::sys::os_random(&mut e);
            let info = Json::Obj(vec![
                ("websocket".into(), Json::Bool(true)),
                ("origins".into(), Json::Arr(vec![Json::Str("*:*".into())])),
                ("cookie_needed".into(), Json::Bool(false)),
                ("entropy".into(), Json::Num(u32::from_le_bytes(e) as f64)),
            ]);
            let mut res = Response::text(200, "application/json; charset=UTF-8", &info.stringify());
            xhr_cors(req, &mut res);
            res.headers.push(("Cache-Control".into(), "no-store, no-cache, no-transform, must-revalidate, max-age=0".into()));
            res
        }
        ("OPTIONS", Route::Info) => {
            let mut res = Response { status: 204, headers: vec![], body: vec![] };
            xhr_cors(req, &mut res);
            res.headers.push(("Cache-Control".into(), format!("public, max-age={}", 365 * 24 * 3600)));
            res.headers.push(("Access-Control-Max-Age".into(), (365 * 24 * 3600).to_string()));
            res.headers.push(("Access-Control-Allow-Methods".into(), "OPTIONS, GET".into()));
            res
        }
        ("GET", Route::Raw | Route::Session) => Response::text(400, "text/plain; charset=UTF-8", "Not a valid websocket request"),
        _ => Response::text(404, "text/plain; charset=UTF-8", "404 Error: Page not found"),
    }
}

/// sockjs `utils.quote`: `JSON.stringify(s)` plus `\uXXXX` for
/// `[\x00-\x1f\ud800-\udfff‌-‏ - ⁠-⁯￰-￿]` (per UTF-16 unit, so astral
/// characters become two escaped surrogates).
pub fn quote(s: &str) -> String {
    let mut q = String::with_capacity(s.len() + 2);
    util::json::write_quoted(&mut q, s);
    if q.is_ascii() {
        return q;
    }
    let mut out = String::with_capacity(q.len() + 16);
    for c in q.chars() {
        let mut units = [0u16; 2];
        let units = c.encode_utf16(&mut units);
        let escape = units.len() == 2 || matches!(units[0], 0x200c..=0x200f | 0x2028..=0x202f | 0x2060..=0x206f | 0xfff0..=0xffff);
        if escape {
            for u in units.iter() {
                out.push_str(&format!("\\u{u:04x}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The `a[…]` frame for queued messages (transport.js doSendBulk).
pub fn frame_messages(msgs: &[String]) -> String {
    let mut f = String::from("a[");
    for (i, m) in msgs.iter().enumerate() {
        if i > 0 {
            f.push(',');
        }
        f.push_str(&quote(m));
    }
    f.push(']');
    f
}

/// Client → server SockJS frame: a JSON array of messages or one JSON string (WebSocketReceiver.didMessage).
/// `Err` = broken framing (close 3000).
pub fn parse_client_frame(payload: &str) -> Result<Vec<String>, ()> {
    if payload.is_empty() {
        return Ok(vec![]);
    }
    let v = Json::parse(payload).map_err(|_| ())?;
    Ok(match v {
        Json::Arr(a) if payload.starts_with('[') => a.into_iter().filter_map(|m| if let Json::Str(s) = m { Some(s) } else { None }).collect(),
        Json::Str(s) => vec![s],
        _ => vec![],
    })
}

/// Takes over an upgraded `/socket/...` request. `Err(stream)` hands the stream back when the path is not a
/// websocket route.
pub(crate) fn upgrade(p: &Arc<Published>, req: &Request, mut stream: TcpStream, reader: BufReader<TcpStream>) -> Result<(), TcpStream> {
    let framed = match route(&req.path) {
        Route::Session => true,
        Route::Raw => false,
        _ => return Err(stream),
    };
    let versions: &[&str] = if framed { &["13", "8", "7"] } else { &["8", "13"] };
    let head = match ws::handshake(req, versions) {
        Ok(h) => h,
        Err(res) => {
            let _ = crate::http::write_response(&mut stream, &res, false);
            return Ok(());
        }
    };
    if stream.write_all(head.as_bytes()).is_err() {
        return Ok(());
    }
    let _ = stream.set_read_timeout(None);
    let conn = p.hub().add_conn();
    let writer_conn = conn.clone();
    let spawned = std::thread::Builder::new()
        .name("ws-writer".into())
        .stack_size(128 << 10)
        .spawn(move || write_loop(stream, &writer_conn, framed));
    if spawned.is_err() {
        p.hub().remove_conn(conn.id);
        return Ok(());
    }
    sock::hello(&conn);
    // the subscriptions go even if a handler panics (and the writer thread ends with the closed outbox)
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_loop(p, &conn, reader, framed)));
    p.hub().remove_conn(conn.id);
    Ok(())
}

fn read_loop(p: &Published, conn: &Arc<Conn>, reader: BufReader<TcpStream>, framed: bool) {
    let mut rd = ws::Reader::new(reader);
    loop {
        match rd.next() {
            Ok(ws::Message::Text(t)) => {
                if !framed {
                    sock::on_message(p, conn, &t);
                    continue;
                }
                match parse_client_frame(&t) {
                    Ok(msgs) => msgs.iter().for_each(|m| sock::on_message(p, conn, m)),
                    Err(()) => {
                        conn.out.push_control(Out::Close(3000, "Broken framing.".into()));
                        return;
                    }
                }
            }
            Ok(ws::Message::Binary(_)) => {}
            Ok(ws::Message::Ping(d)) => conn.out.push_control(Out::Pong(d)),
            Ok(ws::Message::Pong(_)) => {
                conn.pongs.fetch_add(1, Ordering::Relaxed);
            }
            Ok(ws::Message::Close(_)) => {
                conn.out.push_control(Out::Close(1000, "Normal closure".into()));
                return;
            }
            Err(ws::ReadError::Violation(v)) => {
                conn.out.push_control(Out::Close(v.code(), String::new()));
                return;
            }
            Err(ws::ReadError::Io(_)) => return,
        }
    }
}

fn write_loop(mut stream: TcpStream, conn: &Conn, framed: bool) {
    if framed && ws::write_frame(&mut stream, ws::OP_TEXT, b"o").is_err() {
        return;
    }
    let mut ping: Option<(Instant, u64)> = None; // outstanding ping: sent at, pong count then
    let mut last_flush = Instant::now();
    loop {
        let wait = match ping {
            Some((at, _)) => PONG_TIMEOUT.saturating_sub(at.elapsed()),
            None => HEARTBEAT.saturating_sub(last_flush.elapsed()),
        };
        let Some(items) = conn.out.drain(wait) else { break };
        if let Some((at, seen)) = ping {
            if conn.pongs.load(Ordering::Relaxed) > seen {
                ping = None;
            } else if items.is_empty() && at.elapsed() >= PONG_TIMEOUT {
                // session.close(3000, 'No response from heartbeat')
                let _ = ws::write_frame(&mut stream, ws::OP_TEXT, br#"c[3000,"No response from heartbeat"]"#);
                let _ = ws::write_close(&mut stream, 3000, "No response from heartbeat");
                break;
            }
        }
        if items.is_empty() {
            if ping.is_none() && last_flush.elapsed() >= HEARTBEAT {
                // raw websockets have no heartbeat (RawWebsocketSessionReceiver)
                if framed {
                    if ws::write_frame(&mut stream, ws::OP_PING, b"").is_err() {
                        break;
                    }
                    ping = Some((Instant::now(), conn.pongs.load(Ordering::Relaxed)));
                }
                last_flush = Instant::now();
            }
            continue;
        }
        let mut texts: Vec<String> = Vec::new();
        let mut ok = true;
        let mut closing = None;
        for item in items {
            match item {
                Out::Text(t) => texts.push(t),
                Out::Pong(d) => ok &= flush(&mut stream, &mut texts, framed) && ws::write_frame(&mut stream, ws::OP_PONG, &d).is_ok(),
                Out::Close(code, reason) => {
                    closing = Some((code, reason));
                    break;
                }
            }
        }
        ok &= flush(&mut stream, &mut texts, framed);
        last_flush = Instant::now();
        if let Some((code, reason)) = closing {
            let _ = ws::write_close(&mut stream, code, &reason);
            break;
        }
        if !ok {
            break;
        }
    }
    conn.out.close();
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

fn flush(stream: &mut TcpStream, texts: &mut Vec<String>, framed: bool) -> bool {
    if texts.is_empty() {
        return true;
    }
    let ok = if framed {
        ws::write_frame(stream, ws::OP_TEXT, frame_messages(texts).as_bytes()).is_ok()
    } else {
        texts.iter().all(|t| ws::write_frame(stream, ws::OP_TEXT, t.as_bytes()).is_ok())
    };
    texts.clear();
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        assert!(is_socket_path("/socket") && is_socket_path("/socket/") && is_socket_path("/socket/info"));
        assert!(!is_socket_path("/sockets") && !is_socket_path("/api/socket"));
        assert!(matches!(route("/socket/123/abcdefgh/websocket"), Route::Session));
        assert!(matches!(route("/socket/123/abcdefgh/websocket/"), Route::Session));
        assert!(matches!(route("/socket/1.2/abc/websocket"), Route::NotFound));
        assert!(matches!(route("/socket/websocket"), Route::Raw));
        assert!(matches!(route("/socket/"), Route::Welcome));
    }

    #[test]
    fn info_and_options() {
        let req = |m: &str| Request { method: m.into(), path: "/socket/info".into(), query: vec![], headers: vec![], body: vec![] };
        let r = http(&req("GET"));
        assert_eq!(r.status, 200);
        let j = Json::parse(std::str::from_utf8(&r.body).unwrap()).unwrap();
        assert_eq!(j.get("websocket"), Some(&Json::Bool(true)));
        assert_eq!(j.get("origins").unwrap().stringify(), r#"["*:*"]"#);
        assert_eq!(j.get("cookie_needed"), Some(&Json::Bool(false)));
        assert!(j.get("entropy").and_then(Json::as_f64).is_some());
        assert_eq!(r.header("access-control-allow-origin"), Some("*"));
        let o = http(&req("OPTIONS"));
        assert_eq!((o.status, o.header("access-control-allow-methods")), (204, Some("OPTIONS, GET")));
    }

    /// Expected strings from node: sockjs `utils.quote(s)` (tests/fixtures/sockjs/quote.json).
    #[test]
    fn quote_matches_sockjs() {
        let fx = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sockjs/quote.json")).unwrap();
        for c in Json::parse(&fx).unwrap().as_array().unwrap() {
            let (input, want) = (c.at(0).unwrap().as_str().unwrap(), c.at(1).unwrap().as_str().unwrap());
            assert_eq!(quote(input), want, "{input:?}");
        }
        assert_eq!(frame_messages(&["time 1".into(), "[\"x\",{}]".into()]), r#"a["time 1","[\"x\",{}]"]"#);
    }

    #[test]
    fn client_frames() {
        assert_eq!(parse_client_frame(r#"["subscribe a","auth b",3]"#), Ok(vec!["subscribe a".into(), "auth b".into()]));
        assert_eq!(parse_client_frame(r#""gzip on""#), Ok(vec!["gzip on".into()]));
        assert_eq!(parse_client_frame(""), Ok(vec![]));
        assert_eq!(parse_client_frame("[bad"), Err(()));
    }
}
