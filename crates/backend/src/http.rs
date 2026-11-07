//! HTTP/1.1 keep-alive server on std::net: Content-Length and chunked bodies, JSON/urlencoded bodies, q-json
//! envelope, CORS, server-password gate (ARCHITECTURE.md §8.1). Owner: WP04.
//!
//! Not a mirror: this replaces Express + body-parser + q-json-response (spec 10 HTTP-LISTEN, HTTP-BODY,
//! HTTP-ENVELOPE, HTTP-ERRORS, HTTP-CORS). The middleware order lives in `server.rs`.

use std::cell::Cell;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, Instant};
use util::json::Json;

/// One parsed request. Header names are lowercase.
pub struct Request {
    pub method: String,
    /// Path without the query string.
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Request bodies above this are rejected with 413 (HTTP-BODY; code uploads are limited to 5 MiB later).
pub const MAX_BODY: usize = 10 << 20;
/// The body a request may send before its head has shown who it is. Signing in and the routes that need no account fit
/// well inside it, and a stranger cannot make the server hold or parse megabytes (`server::body_limit`).
pub const ANON_BODY: usize = 64 << 10;
/// How much of a body over the limit is read and dropped before the 413. Past this, the client may not get to see it.
const MAX_DISCARD: usize = 64 << 20;
const MAX_HEAD: usize = 64 << 10;
/// The slowest upload a request's deadline allows for, in bytes a second: 10 MiB in about 11 minutes.
const SLOWEST_UPLOAD: usize = 16 << 10;
/// A stuck client holds a thread and a slot for at most this long per write (`write_pieces`).
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const WRITE_PIECE: usize = 64 << 10;
/// qs's `depth`: this many `[key]` levels nest, and the rest of the key stays one literal key.
const QS_DEPTH: usize = 5;
/// qs's `parameterLimit`: pairs past this are not looked at.
const QS_PARAMS: usize = 1000;

impl Request {
    /// First header with this (lowercase) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// First query parameter with this name (`request.query[name]` for flat keys).
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    fn content_type(&self) -> &str {
        self.header("content-type").unwrap_or("").split(';').next().unwrap_or("").trim()
    }

    /// body-parser.json's `type: 'application/json'` (type-is: exact type/subtype, case-insensitive).
    fn is_json(&self) -> bool {
        self.content_type().eq_ignore_ascii_case("application/json")
    }

    /// `request.body` as body-parser leaves it: JSON or urlencoded (qs "extended") objects, `{}` for any
    /// other content type or an empty body. An invalid JSON body never reaches a handler (`body_error`).
    pub fn body(&self) -> Json {
        if self.body.is_empty() {
            return Json::Obj(vec![]);
        }
        if self.is_json() {
            return std::str::from_utf8(&self.body).ok().and_then(|s| Json::parse(s).ok()).unwrap_or(Json::Obj(vec![]));
        }
        if self.content_type().eq_ignore_ascii_case("application/x-www-form-urlencoded") {
            return parse_qs(&String::from_utf8_lossy(&self.body));
        }
        Json::Obj(vec![])
    }

    /// body-parser's 400 for a JSON body that does not parse (strict mode: only objects and arrays). It builds nothing,
    /// so a handler's own parse, or none for one that reads the bytes, is the only one a large body costs.
    pub(crate) fn body_error(&self) -> Option<Response> {
        if self.body.is_empty() || !self.is_json() {
            return None;
        }
        match std::str::from_utf8(&self.body) {
            Ok(text) if Json::is_container(text) => None,
            _ => Some(Response::text(400, "text/plain; charset=utf-8", "Bad Request")),
        }
    }
}

impl Response {
    /// `res.json(v)` with an explicit status.
    pub fn json(status: u16, v: &Json) -> Response {
        Response {
            status,
            headers: vec![("Content-Type".into(), "application/json; charset=utf-8".into())],
            body: v.stringify().into_bytes(),
        }
    }

    /// q-json-response success: `_.extend({ok:1}, data)` (HTTP-ENVELOPE). `data` that is not an object adds
    /// nothing (null/undefined); an array adds its indices as keys.
    pub fn ok(data: Json) -> Response {
        let mut out = vec![("ok".to_string(), Json::Num(1.0))];
        // `o[k] = v` in JS own-property order: canonical array-index keys first, ascending
        let mut put = |k: String, v: Json| match out.iter_mut().find(|(o, _)| *o == k) {
            Some(slot) => slot.1 = v,
            None => {
                let at = match engine::doc::array_index(&k) {
                    None => out.len(),
                    Some(i) => out.iter().position(|(x, _)| engine::doc::array_index(x).is_none_or(|j| j > i)).unwrap_or(out.len()),
                };
                out.insert(at, (k, v));
            }
        };
        match data {
            Json::Obj(m) => m.into_iter().for_each(|(k, v)| put(k, v)),
            Json::Arr(a) => a.into_iter().enumerate().for_each(|(i, v)| put(i.to_string(), v)),
            _ => {}
        }
        Response::json(200, &Json::Obj(out))
    }

    /// q-json-response failure: HTTP 200 `{error: ""+e}`.
    pub fn error(msg: &str) -> Response {
        Response::json(200, &obj(vec![("error", Json::Str(msg.into()))]))
    }

    /// tokenAuth failure: HTTP 401 `{error:'unauthorized'}` (AUTH-TOKEN-MW).
    pub fn unauthorized() -> Response {
        Response::json(401, &obj(vec![("error", Json::Str("unauthorized".into()))]))
    }

    pub fn text(status: u16, content_type: &str, body: &str) -> Response {
        Response { status, headers: vec![("Content-Type".into(), content_type.into())], body: body.as_bytes().to_vec() }
    }

    /// Express' default 404 page.
    pub fn not_found(method: &str, path: &str) -> Response {
        let body = format!("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Error</title>\n</head>\n<body>\n<pre>Cannot {method} {path}</pre>\n</body>\n</html>\n");
        Response::text(404, "text/html; charset=utf-8", &body)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// Builds a JSON object from `(key, value)` pairs in order.
pub fn obj(pairs: Vec<(&str, Json)>) -> Json {
    Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

// ---- wire format ----

pub(crate) enum ReadError {
    /// Connection closed or broken; nothing to answer.
    Closed,
    /// Answer with this status, then close.
    Status(u16),
}

impl From<io::Error> for ReadError {
    fn from(_: io::Error) -> ReadError {
        ReadError::Closed
    }
}

fn read_line(r: &mut impl BufRead, budget: &mut usize) -> Result<String, ReadError> {
    let mut line = Vec::new();
    let n = r.take(*budget as u64 + 1).read_until(b'\n', &mut line)?;
    if n == 0 {
        return Err(ReadError::Closed);
    }
    if n > *budget {
        return Err(ReadError::Status(431));
    }
    *budget -= n;
    while line.last().is_some_and(|c| *c == b'\n' || *c == b'\r') {
        line.pop();
    }
    String::from_utf8(line).map_err(|_| ReadError::Status(400))
}

/// Reads one request (head and body). `Closed` on a clean EOF before the request line.
#[cfg(test)]
pub(crate) fn read_request(r: &mut impl BufRead) -> Result<Request, ReadError> {
    read_request_with(r, &|_| MAX_BODY, &mut || ())
}

/// `read_request`, calling `on_continue` before reading a body announced with `Expect: 100-continue` (curl). `body_limit`
/// gets the request with its head read, and says how much body it may send. Only a body past `ANON_BODY` asks it, as HTTP
/// Basic costs a password hash.
pub(crate) fn read_request_with(r: &mut impl BufRead, body_limit: &dyn Fn(&Request) -> usize, on_continue: &mut dyn FnMut()) -> Result<Request, ReadError> {
    let mut budget = MAX_HEAD;
    let mut line = read_line(r, &mut budget)?;
    while line.is_empty() {
        line = read_line(r, &mut budget)?; // tolerate stray CRLF between keep-alive requests
    }
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ReadError::Status(400));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(ReadError::Status(505));
    }
    let mut headers = Vec::new();
    loop {
        let h = read_line(r, &mut budget)?;
        if h.is_empty() {
            break;
        }
        let Some((k, v)) = h.split_once(':') else { return Err(ReadError::Status(400)) };
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    let (path, qs) = target.split_once('?').unwrap_or((target, ""));
    let mut req = Request { method: method.to_string(), path: path.to_string(), query: parse_query(qs), headers, body: Vec::new() };
    let chunked = req.header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let len = req.header("content-length").filter(|_| !chunked).map(str::parse::<usize>).transpose().map_err(|_| ReadError::Status(400))?;
    let expects = req.header("expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue"));
    let limit = if chunked || len.is_some_and(|len| len > ANON_BODY) { body_limit(&req) } else { ANON_BODY };
    if let Some(len) = len.filter(|len| *len > limit) {
        // a client waiting for the 100 has sent nothing, so it can have its answer now
        if !expects {
            discard(r, len);
        }
        return Err(ReadError::Status(413));
    }
    if expects {
        on_continue();
    }
    if chunked {
        req.body = read_chunked(r, limit)?;
    } else if let Some(len) = len {
        read_body(r, &mut req.body, len)?;
    }
    // it grew by doubling, and a large one may now wait a while for its turn (`server::chain`)
    req.body.shrink_to_fit();
    Ok(req)
}

/// Adds `len` bytes to `body`. It grows as they arrive, as an announced length alone must not cost anything.
fn read_body(r: &mut impl BufRead, body: &mut Vec<u8>, len: usize) -> Result<(), ReadError> {
    if r.by_ref().take(len as u64).read_to_end(body)? < len {
        return Err(ReadError::Closed);
    }
    Ok(())
}

/// Reads and drops the rest of a body that is too large, so that its 413 arrives. A socket closed with data unread resets
/// the connection, which can take the reply with it, and the client sees that instead. Whatever goes wrong here, the reply
/// is still tried.
fn discard(r: &mut impl BufRead, len: usize) {
    let _ = io::copy(&mut r.take(len.min(MAX_DISCARD) as u64), &mut io::sink());
}

fn read_chunked(r: &mut impl BufRead, limit: usize) -> Result<Vec<u8>, ReadError> {
    let mut body = Vec::new();
    let mut sent = 0usize;
    loop {
        let mut budget = 1024;
        let line = read_line(r, &mut budget)?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16).map_err(|_| ReadError::Status(400))?;
        if size == 0 {
            // trailers until the empty line
            let mut budget = MAX_HEAD;
            while !read_line(r, &mut budget)?.is_empty() {}
            return if sent > limit { Err(ReadError::Status(413)) } else { Ok(body) };
        }
        sent = sent.saturating_add(size);
        if sent > MAX_DISCARD {
            return Err(ReadError::Status(413));
        }
        if sent > limit {
            body = Vec::new();
            discard(r, size);
        } else {
            read_body(r, &mut body, size)?;
        }
        let mut budget = 2;
        match read_line(r, &mut budget) {
            Ok(l) if l.is_empty() => {}
            Ok(_) | Err(ReadError::Status(_)) => return Err(ReadError::Status(400)), // chunk longer than declared
            Err(e) => return Err(e),
        }
    }
}

pub(crate) fn reason(status: u16) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "Unknown",
    }
}

pub(crate) fn write_response(w: &mut impl Write, res: &Response, keep_alive: bool) -> io::Result<()> {
    write_head_and_body(w, res, keep_alive, true)
}

/// The response; `with_body: false` for HEAD (headers incl. the GET body's Content-Length, as Node sends them).
fn write_head_and_body(w: &mut impl Write, res: &Response, keep_alive: bool, with_body: bool) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", res.status, reason(res.status));
    for (k, v) in &res.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n", res.body.len()));
    head.push_str(if keep_alive { "Connection: keep-alive\r\n\r\n" } else { "Connection: close\r\n\r\n" });
    w.write_all(head.as_bytes())?;
    if with_body {
        write_pieces(w, &res.body)?;
    }
    w.flush()
}

/// `write_all` a piece at a time. A socket's write timeout covers one call, and on Windows one call can take a whole body,
/// which a slow but live client could not finish in time.
pub(crate) fn write_pieces(w: &mut impl Write, data: &[u8]) -> io::Result<()> {
    data.chunks(WRITE_PIECE).try_for_each(|piece| w.write_all(piece))
}

/// Whether this request's `Origin` may use the API: none at all (curl, bots, the server's own page on a GET), the stock
/// client (NW.js pages run at a chrome-extension:// origin), or a page this same server gave out. Everything else, `null`
/// from a sandboxed frame included, is a page from somewhere else in the browser of someone who can reach this port.
pub(crate) fn origin_allowed(req: &Request) -> bool {
    let Some(origin) = req.header("origin") else { return true };
    if origin.starts_with("chrome-extension://") {
        return true;
    }
    let authority = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://"));
    authority.is_some_and(|a| req.header("host").is_some_and(|host| a.eq_ignore_ascii_case(host)))
}

/// Whether a `Host` value, or `http.host`, names this PC. A page cannot fake it: its own name is what the browser sends.
pub(crate) fn names_this_pc(host: &str) -> bool {
    let local = |name: &str| name.eq_ignore_ascii_case("localhost") || name.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
    match host.strip_prefix('[').and_then(|h| h.split_once(']')) {
        Some((v6, _)) => local(v6),
        None => local(host) || host.rsplit_once(':').is_some_and(|(name, _)| local(name)),
    }
}

/// CORS for the origins `origin_allowed` lets in, and no headers for any other (HTTP-CORS: the original has none; browser
/// tools need it). The stock client sends every API call with `withCredentials`, so those keep Allow-Credentials. The
/// server uses no cookies, which leaves that header with nothing to give away.
pub(crate) fn add_cors(req: &Request, res: &mut Response) {
    res.headers.push(("Vary".into(), "Origin".into()));
    if !origin_allowed(req) {
        return;
    }
    let origin = req.header("origin").unwrap_or("*").to_string();
    if origin != "*" {
        res.headers.push(("Access-Control-Allow-Credentials".into(), "true".into()));
    }
    res.headers.push(("Access-Control-Allow-Origin".into(), origin));
    let allow = req.header("access-control-request-headers").unwrap_or("Content-Type, X-Token, X-Username, X-Server-Password, Authorization");
    res.headers.push(("Access-Control-Allow-Headers".into(), allow.to_string()));
    res.headers.push(("Access-Control-Allow-Methods".into(), "GET, POST, OPTIONS".into()));
    res.headers.push(("Access-Control-Expose-Headers".into(), "X-Token".into()));
}

/// Whether the client wants the connection kept open after this request.
pub(crate) fn keep_alive(req: &Request) -> bool {
    !req.header("connection").is_some_and(|c| c.to_ascii_lowercase().contains("close"))
}

/// A connection's one socket, held by what reads it and what writes it. `try_clone` would give each a handle of its own, but
/// on Windows a clone can be inherited and `Command` hands what can be to every program it starts: a match server or a curl
/// started at that moment held the connection open past its end here, then reset it as it exited. What accept returns cannot
/// be inherited.
#[derive(Clone)]
pub(crate) struct Socket(Arc<TcpStream>);

impl Socket {
    pub(crate) fn new(stream: TcpStream) -> Socket {
        Socket(Arc::new(stream))
    }
}

impl Deref for Socket {
    type Target = TcpStream;

    fn deref(&self) -> &TcpStream {
        &self.0
    }
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buf)
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        (&*self.0).flush()
    }
}

/// One request's reads. A read timeout starts over with every byte, so a request sent a byte at a time would never run out
/// of it: each read also waits no longer than what is left of `deadline`.
struct Timed<'a> {
    r: &'a mut BufReader<Socket>,
    idle: Duration,
    deadline: &'a Cell<Instant>,
}

impl Timed<'_> {
    fn arm(&self) -> io::Result<()> {
        let left = self.deadline.get().saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if self.r.buffer().is_empty() {
            self.r.get_ref().set_read_timeout(Some(left.min(self.idle)))?;
        }
        Ok(())
    }
}

impl Read for Timed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.arm()?;
        self.r.read(buf)
    }
}

impl BufRead for Timed<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.arm()?;
        self.r.fill_buf()
    }

    fn consume(&mut self, n: usize) {
        self.r.consume(n);
    }
}

/// One keep-alive connection: reads requests and answers each with `handle` until the peer closes, or until `crowded` says
/// the server needs the slot back. A request for which `upgrade` returns true hands the stream over (WebSocket) and ends the
/// loop.
pub(crate) fn serve_connection(
    stream: TcpStream,
    timeout: Duration,
    body_limit: &dyn Fn(&Request) -> usize,
    handle: &dyn Fn(&Request) -> Response,
    upgrade: &dyn Fn(&Request, Socket, BufReader<Socket>) -> Option<Socket>,
    crowded: &dyn Fn() -> bool,
) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(timeout.min(WRITE_TIMEOUT)));
    let mut writer = Socket::new(stream);
    let mut reader = BufReader::new(writer.clone());
    loop {
        // waiting for a request costs a caller nothing either way, so the deadline starts with its first byte
        let _ = reader.get_ref().set_read_timeout(Some(timeout));
        if !reader.fill_buf().is_ok_and(|b| !b.is_empty()) {
            return;
        }
        let deadline = Cell::new(Instant::now() + timeout);
        let limit = |req: &Request| {
            let limit = body_limit(req);
            deadline.set(deadline.get() + Duration::from_secs((limit / SLOWEST_UPLOAD) as u64));
            limit
        };
        let w = &mut writer;
        let req = match read_request_with(&mut Timed { r: &mut reader, idle: timeout, deadline: &deadline }, &limit, &mut || {
            let _ = w.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
        }) {
            Ok(r) => r,
            Err(ReadError::Closed) => return,
            Err(ReadError::Status(s)) => {
                let _ = write_response(&mut writer, &Response::text(s, "text/plain; charset=utf-8", reason(s)), false);
                return;
            }
        };
        if req.header("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket")) {
            let _ = reader.get_ref().set_read_timeout(Some(timeout)); // the deadline may have left less
            match upgrade(&req, writer, reader) {
                None => return,
                Some(w) => {
                    // not a socket route: answer like any other request, then close
                    writer = w;
                    let _ = write_response(&mut writer, &handle(&req), false);
                    return;
                }
            }
        }
        // Express routes GET handlers for HEAD too and Node drops the body
        let head_only = req.method == "HEAD";
        let mut req = req;
        if head_only {
            req.method = "GET".into();
        }
        let res = handle(&req);
        let ka = keep_alive(&req) && !crowded();
        if write_head_and_body(&mut writer, &res, ka, !head_only).is_err() || !ka {
            return;
        }
    }
}

// ---- urlencoded / query strings ----

/// qs 6 `utils.decode` (Express' query parser and body-parser's extended urlencoded): `+` → space, then
/// `decodeURIComponent`, which is all or nothing: a bad escape or invalid UTF-8 leaves the whole (`+`-replaced)
/// string undecoded.
pub fn url_decode(s: &str) -> String {
    let plain = s.replace('+', " ");
    let b = plain.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        match (b.get(i + 1).and_then(|&c| hex(c)), b.get(i + 2).and_then(|&c| hex(c))) {
            (Some(h), Some(l)) => out.push(h << 4 | l),
            _ => return plain, // URIError: malformed escape
        }
        i += 3;
    }
    String::from_utf8(out).unwrap_or(plain) // URIError: not UTF-8
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

fn query_pairs(qs: &str) -> impl Iterator<Item = (String, String)> + '_ {
    qs.split('&').filter(|p| !p.is_empty()).map(|p| {
        let (k, v) = p.split_once('=').unwrap_or((p, ""));
        (url_decode(k), url_decode(v))
    })
}

fn parse_query(qs: &str) -> Vec<(String, String)> {
    query_pairs(qs).collect()
}

/// qs "extended" parsing of an urlencoded body: `a=1&b[c]=2&d[]=3` → `{a:"1", b:{c:"2"}, d:["3"]}`. At most `QS_PARAMS`
/// pairs, as each insert looks through the object, and at most `QS_DEPTH` levels, as the value recurses wherever it goes.
// ponytail: covers `[key]` and `[]`; qs' numeric-index arrays (`a[0]=x`) become objects, add if a tool needs it.
pub fn parse_qs(qs: &str) -> Json {
    let mut root = Json::Obj(vec![]);
    for (k, v) in query_pairs(qs).take(QS_PARAMS) {
        insert_path(&mut root, &key_path(&k), Json::Str(v));
    }
    root
}

/// `a[b][c]` as a, b, c. What is left past `QS_DEPTH` is one more key, brackets included: `[g][h]`.
fn key_path(k: &str) -> Vec<String> {
    let (head, rest) = k.split_once('[').map_or((k, ""), |(h, r)| (h, r));
    let mut path = vec![head.to_string()];
    let mut seen = 0;
    for seg in rest.split('[') {
        if path.len() > QS_DEPTH {
            path.push(format!("[{}", &rest[seen..]));
            break;
        }
        seen += seg.len() + 1;
        if let Some(s) = seg.strip_suffix(']') {
            path.push(s.to_string());
        }
    }
    path
}

fn insert_path(cur: &mut Json, path: &[String], v: Json) {
    let Some((key, rest)) = path.split_first() else { return };
    if let Json::Arr(a) = cur {
        if key.is_empty() && rest.is_empty() {
            a.push(v);
        }
        return;
    }
    let Json::Obj(m) = cur else { return };
    if rest.is_empty() {
        match m.iter_mut().find(|(k, _)| k == key) {
            Some((_, Json::Arr(a))) => a.push(v),
            Some(slot) => slot.1 = Json::Arr(vec![std::mem::replace(&mut slot.1, Json::Null), v]),
            None => m.push((key.clone(), v)),
        }
        return;
    }
    if !m.iter().any(|(k, _)| k == key) {
        let child = if rest[0].is_empty() { Json::Arr(vec![]) } else { Json::Obj(vec![]) };
        m.push((key.clone(), child));
    }
    let child = &mut m.iter_mut().find(|(k, _)| k == key).unwrap().1;
    insert_path(child, rest, v);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(raw: &str) -> Result<Request, u16> {
        read_request(&mut Cursor::new(raw.as_bytes().to_vec())).map_err(|e| match e {
            ReadError::Closed => 0,
            ReadError::Status(s) => s,
        })
    }

    #[test]
    fn content_length_and_query() {
        let r = parse("POST /api/user/code?a=1&b=x+y%21 HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}").unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/api/user/code"));
        assert_eq!(r.param("b"), Some("x y!"));
        assert_eq!(r.header("content-type"), Some("application/json"));
        assert_eq!(r.body().stringify(), r#"{"a":1}"#);
        assert!(r.body_error().is_none());
    }

    #[test]
    fn chunked_body_and_keep_alive_pipeline() {
        let raw = "POST /a HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\n4\r\n{\"x\"\r\n3;ext=1\r\n:2}\r\n0\r\nX-Trailer: 1\r\n\r\nGET /b HTTP/1.1\r\n\r\n";
        let mut c = Cursor::new(raw.as_bytes().to_vec());
        let a = read_request(&mut c).ok().unwrap();
        assert_eq!(a.body, b"{\"x\":2}");
        let b = read_request(&mut c).ok().unwrap();
        assert_eq!(b.path, "/b");
        assert!(matches!(read_request(&mut c), Err(ReadError::Closed)));
    }

    #[test]
    fn limits_and_bad_requests() {
        assert_eq!(parse(&format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1)).err(), Some(413));
        assert_eq!(parse("GARBAGE\r\n\r\n").err(), Some(400));
        assert_eq!(parse("GET / SPDY/3\r\n\r\n").err(), Some(505));
        let big = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(MAX_HEAD));
        assert_eq!(parse(&big).err(), Some(431));
        let r = parse("POST / HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 3\r\n\r\n\"x\"").unwrap();
        assert_eq!(r.body_error().map(|r| r.status), Some(400));
    }

    /// What a client that sends all of `raw` reads back from one connection.
    fn over_a_socket(raw: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, Duration::from_secs(120), &|_| MAX_BODY, &|_| Response::ok(Json::Null), &|_, stream, _| Some(stream), &|| false);
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(&raw).unwrap();
        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();
        server.join().unwrap();
        reply
    }

    #[test]
    fn a_body_over_the_limit_gets_a_real_413() {
        let body = vec![b'x'; MAX_BODY + 1];
        let plain = [format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes(), &body].concat();
        let reply = over_a_socket(plain);
        assert!(reply.starts_with("HTTP/1.1 413 Payload Too Large\r\n") && reply.ends_with("Payload Too Large"), "{reply}");
        let chunked = [format!("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n", body.len()).as_bytes(), &body, b"\r\n0\r\n\r\n"].concat();
        assert!(over_a_socket(chunked).starts_with("HTTP/1.1 413 "));
        let at_the_limit = [format!("POST / HTTP/1.1\r\nConnection: close\r\nContent-Length: {MAX_BODY}\r\n\r\n").as_bytes(), &body[1..]].concat();
        assert!(over_a_socket(at_the_limit).starts_with("HTTP/1.1 200 "), "the limit itself is fine");
    }

    #[test]
    fn a_crowded_server_ends_keep_alive_with_the_response() {
        let answer = |crowded: bool| {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                serve_connection(stream, Duration::from_millis(200), &|_| MAX_BODY, &|_| Response::ok(Json::Null), &|_, stream, _| Some(stream), &|| crowded);
            });
            let mut client = TcpStream::connect(addr).unwrap();
            client.write_all(b"GET / HTTP/1.1\r\n\r\n").unwrap();
            client.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut reply = String::new();
            client.read_to_string(&mut reply).unwrap(); // to the end of the connection, which the 200 ms ends at the latest
            server.join().unwrap();
            reply
        };
        assert!(answer(false).contains("Connection: keep-alive\r\n"));
        let reply = answer(true);
        assert!(reply.starts_with("HTTP/1.1 200 ") && reply.contains("Connection: close\r\n"), "{reply}");
    }

    #[test]
    fn a_body_that_stops_short_is_a_closed_connection() {
        for raw in ["POST / HTTP/1.1\r\nContent-Length: 10485760\r\n\r\nhi", "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\na00000\r\nhi"] {
            assert_eq!(parse(raw).err(), Some(0), "{raw}");
        }
        let r = parse("POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhelloGET / HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(r.body, b"hello", "and one that arrives stops where it said");
    }

    #[test]
    fn a_quiet_connection_is_dropped_after_the_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, Duration::from_millis(100), &|_| MAX_BODY, &|_| Response::ok(Json::Null), &|_, stream, _| Some(stream), &|| false);
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(b"POST / HTTP/1.1\r\nContent-Length: 10485760\r\n\r\n").unwrap();
        server.join().unwrap(); // this ends only because the server gave up on the body
    }

    #[test]
    fn a_client_that_waits_is_told_before_it_sends() {
        let raw = format!("POST / HTTP/1.1\r\nExpect: 100-continue\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1);
        let mut continued = 0;
        assert!(matches!(read_request_with(&mut Cursor::new(raw.into_bytes()), &|_| MAX_BODY, &mut || continued += 1), Err(ReadError::Status(413))));
        assert_eq!(continued, 0, "no 100, so no body");
        let raw = "POST / HTTP/1.1\r\nExpect: 100-continue\r\nContent-Length: 2\r\n\r\nhi";
        assert!(read_request_with(&mut Cursor::new(raw.as_bytes().to_vec()), &|_| MAX_BODY, &mut || continued += 1).is_ok());
        assert_eq!(continued, 1);
    }

    /// A connection must end when this server ends it, whatever programs were started while it was open.
    #[cfg(windows)]
    #[test]
    fn a_program_started_meanwhile_does_not_get_the_connection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let handle = |_: &Request| {
                // `ping` has no reason to stop soon, so it outlives the reply below if it got a handle to the socket
                let pings = std::process::Command::new("ping").args(["-n", "4", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn();
                drop(pings);
                Response::ok(Json::Null)
            };
            serve_connection(stream, Duration::from_secs(10), &|_| MAX_BODY, &handle, &|_, stream, _| Some(stream), &|| false);
        });
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n").unwrap();
        client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut reply = String::new();
        client.read_to_string(&mut reply).expect("the end of the reply comes now, not when that program exits");
        assert!(reply.starts_with("HTTP/1.1 200 "), "{reply}");
        server.join().unwrap();
    }

    #[test]
    fn a_request_sent_a_byte_at_a_time_ends_at_its_deadline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, Duration::from_millis(400), &|_| ANON_BODY, &|_| Response::ok(Json::Null), &|_, stream, _| Some(stream), &|| false);
        });
        let started = Instant::now();
        let mut client = TcpStream::connect(addr).unwrap();
        client.write_all(b"GET / HTTP/1.1\r\nX-Long: ").unwrap();
        // each byte is well inside the read timeout, which would let this go on for good
        while !server.is_finished() && started.elapsed() < Duration::from_secs(10) {
            let _ = client.write_all(b"x");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(started.elapsed() < Duration::from_secs(5), "still reading after {:?}", started.elapsed());
        server.join().unwrap();
    }

    #[test]
    fn a_slow_upload_gets_time_for_what_it_may_send() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, Duration::from_millis(300), &|_| MAX_BODY, &|_| Response::ok(Json::Null), &|_, stream, _| Some(stream), &|| false);
        });
        let mut client = TcpStream::connect(addr).unwrap();
        let len = ANON_BODY + 8;
        client.write_all(format!("POST / HTTP/1.1\r\nConnection: close\r\nContent-Length: {len}\r\n\r\n").as_bytes()).unwrap();
        client.write_all(&vec![b'x'; ANON_BODY]).unwrap();
        // the last bytes come after the 300 ms a request with a small body would get
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(100));
            client.write_all(b"x").unwrap();
        }
        let mut reply = String::new();
        client.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 "), "{reply}");
        server.join().unwrap();
    }

    #[test]
    fn a_stranger_gets_the_small_limit() {
        let limit = |req: &Request| if req.header("x-token").is_some() { MAX_BODY } else { ANON_BODY };
        let status = |head: &str, body: &[u8]| {
            let raw = [head.as_bytes(), body].concat();
            read_request_with(&mut Cursor::new(raw), &limit, &mut || ()).err().map(|e| if let ReadError::Status(s) = e { s } else { 0 })
        };
        let at = vec![b'x'; ANON_BODY];
        let over = vec![b'x'; ANON_BODY + 1];
        assert_eq!(status(&format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", at.len()), &at), None);
        assert_eq!(status(&format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", over.len()), &over), Some(413));
        assert_eq!(status(&format!("POST / HTTP/1.1\r\nX-Token: t\r\nContent-Length: {}\r\n\r\n", over.len()), &over), None);
        let chunked = |extra: &str| format!("POST / HTTP/1.1\r\n{extra}Transfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n", over.len(), String::from_utf8_lossy(&over));
        assert_eq!(status(&chunked(""), b""), Some(413));
        assert_eq!(status(&chunked("X-Token: t\r\n"), b""), None);
    }

    #[test]
    fn asking_who_it_is_waits_for_a_body_that_needs_it() {
        let asked = Cell::new(0);
        let limit = |_: &Request| {
            asked.set(asked.get() + 1);
            MAX_BODY
        };
        let read = |raw: String| assert!(read_request_with(&mut Cursor::new(raw.into_bytes()), &limit, &mut || ()).is_ok());
        read("GET / HTTP/1.1\r\n\r\n".into());
        read("POST / HTTP/1.1\r\nContent-Length: 2\r\n\r\nhi".into());
        assert_eq!(asked.get(), 0, "HTTP Basic costs a password hash");
        read(format!("POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n{}", ANON_BODY + 1, "x".repeat(ANON_BODY + 1)));
        assert_eq!(asked.get(), 1);
    }

    #[test]
    fn origins() {
        let req = |origin: &str, host: &str| Request { method: "POST".into(), path: "/".into(), query: vec![], headers: vec![("origin".into(), origin.into()), ("host".into(), host.into())], body: vec![] };
        for (origin, host) in [
            ("chrome-extension://abcdefgh", "127.0.0.1:21025"),
            ("http://127.0.0.1:21025", "127.0.0.1:21025"),
            ("http://LOCALHOST:21025", "localhost:21025"),
            ("https://game.example", "game.example"),
            ("http://[::1]:21025", "[::1]:21025"),
        ] {
            assert!(origin_allowed(&req(origin, host)), "{origin} on {host}");
        }
        for (origin, host) in [
            ("https://evil.example", "127.0.0.1:21025"),
            ("http://127.0.0.1:21026", "127.0.0.1:21025"),
            ("http://127.0.0.1.evil.example", "127.0.0.1"),
            ("null", "127.0.0.1:21025"),
            ("file://", "127.0.0.1:21025"),
            ("127.0.0.1:21025", "127.0.0.1:21025"),
        ] {
            assert!(!origin_allowed(&req(origin, host)), "{origin} on {host}");
        }
        let mut none = req("x", "y");
        none.headers.clear();
        assert!(origin_allowed(&none), "curl sends no Origin");
    }

    #[test]
    fn names_for_this_pc() {
        for host in ["127.0.0.1", "127.0.0.1:21025", "127.0.0.2", "localhost", "LOCALHOST:21025", "::1", "[::1]", "[::1]:21025"] {
            assert!(names_this_pc(host), "{host}");
        }
        for host in ["evil.example", "evil.example:21025", "0.0.0.0", "192.168.1.5:21025", "localhost.evil.example", "[2001:db8::1]:21025", "x"] {
            assert!(!names_this_pc(host), "{host}");
        }
    }

    #[test]
    fn cors_only_for_allowed_origins() {
        let req = |headers: &[(&str, &str)]| Request { method: "GET".into(), path: "/".into(), query: vec![], headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), body: vec![] };
        let cors = |r: &Request| {
            let mut res = Response::ok(Json::Null);
            add_cors(r, &mut res);
            res
        };
        let plain = cors(&req(&[]));
        assert_eq!(plain.header("access-control-allow-origin"), Some("*"));
        assert!(plain.header("access-control-allow-credentials").is_none());
        let client = cors(&req(&[("origin", "chrome-extension://abc")]));
        assert_eq!(client.header("access-control-allow-origin"), Some("chrome-extension://abc"));
        assert_eq!(client.header("access-control-allow-credentials"), Some("true"), "its $http calls set withCredentials");
        let page = cors(&req(&[("origin", "https://evil.example"), ("access-control-request-headers", "x-token")]));
        for h in ["access-control-allow-origin", "access-control-allow-credentials", "access-control-allow-headers", "access-control-expose-headers"] {
            assert!(page.header(h).is_none(), "{h}");
        }
    }

    #[test]
    fn a_long_body_goes_out_a_piece_at_a_time() {
        struct Counting(Vec<usize>);
        impl Write for Counting {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.push(b.len());
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut w = Counting(vec![]);
        write_pieces(&mut w, &vec![0; WRITE_PIECE * 2 + 1]).unwrap();
        assert_eq!(w.0, [WRITE_PIECE, WRITE_PIECE, 1], "the write timeout counts per piece");
    }

    #[test]
    fn discarding_has_a_bound() {
        let mut c = Cursor::new(vec![b'x'; 16]);
        discard(&mut c, 4);
        assert_eq!(c.position(), 4);
        discard(&mut c, usize::MAX);
        assert_eq!(c.position(), 16, "it stops at the end of what there is");
        discard(&mut BufReader::new(io::repeat(b'x')), usize::MAX);
        let raw = format!("POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n", MAX_DISCARD + 1);
        assert_eq!(parse(&raw).err(), Some(413), "a chunk past the bound is not waited for");
    }

    #[test]
    fn urlencoded_extended() {
        let r = parse("POST / HTTP/1.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 30\r\n\r\na=1&b%5Bc%5D=2&d[]=3&d[]=4&a=5").unwrap();
        assert_eq!(r.body().stringify(), r#"{"a":["1","5"],"b":{"c":"2"},"d":["3","4"]}"#);
        assert_eq!(url_decode("%zz%4"), "%zz%4");
        // qs.parse('a=…').a in node: decodeURIComponent is all or nothing
        for (raw, want) in [("a+b%zz%41", "a b%zz%41"), ("%E2%82%AC+x", "\u{20ac} x"), ("%4", "%4"), ("%41%42+", "AB "), ("%ED%A0%80", "%ED%A0%80"), ("%41%ff", "%41%ff")] {
            assert_eq!(url_decode(raw), want, "{raw}");
        }
    }

    #[test]
    fn urlencoded_keys_stop_where_qs_stops() {
        assert_eq!(parse_qs("a[b][c][d][e][f]=x").stringify(), r#"{"a":{"b":{"c":{"d":{"e":{"f":"x"}}}}}}"#);
        assert_eq!(parse_qs("a[b][c][d][e][f][g][h]=x").stringify(), r#"{"a":{"b":{"c":{"d":{"e":{"f":{"[g][h]":"x"}}}}}}}"#, "depth 5, then a literal key");
        let many: String = (0..5000).map(|i| format!("k{i}=&")).collect();
        let parsed = parse_qs(&many);
        assert_eq!(parsed.as_object().unwrap().len(), QS_PARAMS);
        assert_eq!(parsed.as_object().unwrap().last().unwrap().0, "k999");
    }

    /// 900 brackets used to end the whole process, and catch_unwind cannot stop a stack overflow.
    #[test]
    fn a_deep_key_cannot_reach_the_stack() {
        for depth in [1000, 100_000] {
            for bracket in ["[b]", "%5Bb%5D"] {
                let body = format!("a{}=1", bracket.repeat(depth));
                let run = move || {
                    let parsed = parse_qs(&body);
                    let mut levels = 0;
                    let mut at = &parsed;
                    while let Json::Obj(m) = at {
                        levels += 1;
                        at = &m[0].1;
                    }
                    assert_eq!(levels, QS_DEPTH + 2, "the root, a, and each level below it");
                    assert_eq!(at.as_str(), Some("1"));
                    assert!(parsed.stringify().ends_with(r#""1"}}}}}}}"#));
                };
                std::thread::Builder::new().stack_size(256 << 10).spawn(run).unwrap().join().unwrap();
            }
        }
    }

    /// 2 MB of distinct keys took 73 s when every key was looked up in all the others.
    #[test]
    fn distinct_keys_cost_no_more_than_the_limit() {
        let many: String = (0..1_000_000).map(|i| format!("k{i:07}=&")).collect();
        let started = Instant::now();
        assert_eq!(parse_qs(&many).as_object().unwrap().len(), QS_PARAMS);
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
    }

    /// A handler runs on top of this, so the depth limit has to leave it room on the thread that really runs it.
    #[test]
    fn the_depth_limit_fits_an_http_thread() {
        let run = || {
            let text = "[".repeat(util::json::MAX_DEPTH) + &"]".repeat(util::json::MAX_DEPTH);
            let raw = format!("POST / HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{text}", text.len());
            let r = parse(&raw).unwrap();
            assert!(r.body_error().is_none());
            assert_eq!(r.body().stringify(), text);
        };
        std::thread::Builder::new().stack_size(crate::HTTP_STACK).spawn(run).unwrap().join().unwrap();
    }

    #[test]
    fn envelope() {
        let body = |r: Response| String::from_utf8(r.body).unwrap();
        assert_eq!(body(Response::ok(Json::Null)), r#"{"ok":1}"#);
        assert_eq!(body(Response::ok(obj(vec![("a", Json::Num(2.0)), ("ok", Json::Num(5.0))]))), r#"{"ok":5,"a":2}"#);
        assert_eq!(body(Response::ok(Json::Arr(vec![Json::Bool(true)]))), r#"{"0":true,"ok":1}"#);
        let e = Response::error("invalid params");
        assert_eq!((e.status, body(e)), (200, r#"{"error":"invalid params"}"#.to_string()));
        let u = Response::unauthorized();
        assert_eq!((u.status, body(u)), (401, r#"{"error":"unauthorized"}"#.to_string()));
    }

    #[test]
    fn response_wire() {
        let mut out = Vec::new();
        write_response(&mut out, &Response::ok(Json::Null), true).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: 8\r\nConnection: keep-alive\r\n\r\n{\"ok\":1}"));
        // HEAD: the GET response's headers, no body
        let mut out = Vec::new();
        write_head_and_body(&mut out, &Response::ok(Json::Null), true, false).unwrap();
        assert!(String::from_utf8(out).unwrap().ends_with("Content-Length: 8\r\nConnection: keep-alive\r\n\r\n"));
    }
}
