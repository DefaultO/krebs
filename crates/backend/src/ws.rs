//! RFC 6455 WebSocket: handshake (accept key via util::sha1 + util::base64), frames, ping/pong,
//! fragmentation, close; reader + writer thread per connection (ARCHITECTURE.md §8.1). Owner: WP04.
//!
//! Not a mirror (replaces faye-websocket). Only the server side: client frames must be masked, server frames
//! are not; text messages must be UTF-8; extensions (permessage-deflate) are never negotiated.

use crate::http::{Request, Response};
use std::io::{self, Read, Write};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// Largest message accepted from a client. The protocol's client messages are short command lines: `auth <token>` and
/// `subscribe <channel>`, of which the stock client's longest is a memory path someone typed.
pub const MAX_MESSAGE: usize = 64 << 10;

pub const OP_CONT: u8 = 0;
pub const OP_TEXT: u8 = 1;
pub const OP_BINARY: u8 = 2;
pub const OP_CLOSE: u8 = 8;
pub const OP_PING: u8 = 9;
pub const OP_PONG: u8 = 10;

/// `Sec-WebSocket-Accept` for a `Sec-WebSocket-Key`.
pub fn accept_key(key: &str) -> String {
    util::base64::encode(&util::sha1::sha1(format!("{key}{GUID}").as_bytes()))
}

/// The 101 response head for a valid upgrade request, or the error response (400) to send instead.
/// `versions`: accepted `Sec-WebSocket-Version` values (SockJS raw websockets accept 8 and 13).
pub fn handshake(req: &Request, versions: &[&str]) -> Result<String, Response> {
    let bad = |m: &str| Err(Response::text(400, "text/plain; charset=UTF-8", m));
    let upgrade = req.header("connection").is_some_and(|c| c.split(',').any(|t| t.trim().eq_ignore_ascii_case("upgrade")));
    if req.method != "GET" || !upgrade || !req.header("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket")) {
        return bad("Not a valid websocket request");
    }
    if !versions.contains(&req.header("sec-websocket-version").unwrap_or("")) {
        return bad("Only supported WebSocket protocol is RFC 6455.");
    }
    let Some(key) = req.header("sec-websocket-key") else { return bad("Not a valid websocket request") };
    Ok(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n", accept_key(key)))
}

/// One complete message or control frame from the peer.
#[derive(Debug, PartialEq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    /// Close frame: status code and reason when present.
    Close(Option<(u16, String)>),
}

/// Why reading stopped with an error the server should answer with a close frame.
#[derive(Debug, PartialEq)]
pub enum Violation {
    /// 1002: protocol error (unmasked client frame, bad opcode, fragmented control frame, …).
    Protocol,
    /// 1007: text not UTF-8.
    BadText,
    /// 1009: message too big.
    TooBig,
}

pub enum ReadError {
    Io(io::Error),
    Violation(Violation),
}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> ReadError {
        ReadError::Io(e)
    }
}

impl Violation {
    pub fn code(&self) -> u16 {
        match self {
            Violation::Protocol => 1002,
            Violation::BadText => 1007,
            Violation::TooBig => 1009,
        }
    }
}

fn frame_head(r: &mut impl Read) -> Result<(bool, u8, Vec<u8>), ReadError> {
    let mut h = [0u8; 2];
    r.read_exact(&mut h)?;
    let (fin, rsv, op) = (h[0] & 0x80 != 0, h[0] & 0x70, h[0] & 0x0f);
    let masked = h[1] & 0x80 != 0;
    if rsv != 0 || !masked {
        return Err(ReadError::Violation(Violation::Protocol));
    }
    let len = match h[1] & 0x7f {
        126 => {
            let mut b = [0; 2];
            r.read_exact(&mut b)?;
            u16::from_be_bytes(b) as u64
        }
        127 => {
            let mut b = [0; 8];
            r.read_exact(&mut b)?;
            u64::from_be_bytes(b)
        }
        n => n as u64,
    };
    if op >= 8 && (len > 125 || !fin) {
        return Err(ReadError::Violation(Violation::Protocol));
    }
    if len > MAX_MESSAGE as u64 {
        return Err(ReadError::Violation(Violation::TooBig));
    }
    let mut mask = [0; 4];
    r.read_exact(&mut mask)?;
    // it grows as bytes arrive, as an announced length alone must not cost anything
    let mut payload = Vec::new();
    if r.by_ref().take(len).read_to_end(&mut payload)? < len as usize {
        return Err(ReadError::Io(io::ErrorKind::UnexpectedEof.into()));
    }
    for (i, b) in payload.iter_mut().enumerate() {
        *b ^= mask[i & 3];
    }
    Ok((fin, op, payload))
}

/// Reads the next message, joining fragments; control frames are returned as they arrive, even between
/// the fragments of a data message.
pub struct Reader<R> {
    r: R,
    partial: Option<(u8, Vec<u8>)>,
}

impl<R: Read> Reader<R> {
    pub fn new(r: R) -> Reader<R> {
        Reader { r, partial: None }
    }

    pub fn next(&mut self) -> Result<Message, ReadError> {
        loop {
            let (fin, op, payload) = frame_head(&mut self.r)?;
            match op {
                OP_PING => return Ok(Message::Ping(payload)),
                OP_PONG => return Ok(Message::Pong(payload)),
                OP_CLOSE => {
                    if payload.len() < 2 {
                        return Ok(Message::Close(None));
                    }
                    let code = u16::from_be_bytes([payload[0], payload[1]]);
                    let reason = String::from_utf8(payload[2..].to_vec()).map_err(|_| ReadError::Violation(Violation::BadText))?;
                    return Ok(Message::Close(Some((code, reason))));
                }
                OP_TEXT | OP_BINARY if self.partial.is_none() => self.partial = Some((op, payload)),
                OP_CONT if self.partial.is_some() => {
                    let (_, buf) = self.partial.as_mut().unwrap();
                    if buf.len() + payload.len() > MAX_MESSAGE {
                        return Err(ReadError::Violation(Violation::TooBig));
                    }
                    buf.extend_from_slice(&payload);
                }
                _ => return Err(ReadError::Violation(Violation::Protocol)),
            }
            if fin {
                let (op, data) = self.partial.take().unwrap();
                return if op == OP_TEXT {
                    String::from_utf8(data).map(Message::Text).map_err(|_| ReadError::Violation(Violation::BadText))
                } else {
                    Ok(Message::Binary(data))
                };
            }
        }
    }
}

/// Writes one unmasked server frame with FIN set.
pub fn write_frame(w: &mut impl Write, op: u8, payload: &[u8]) -> io::Result<()> {
    let mut head = Vec::with_capacity(10);
    head.push(0x80 | op);
    match payload.len() {
        n if n < 126 => head.push(n as u8),
        n if n <= u16::MAX as usize => {
            head.push(126);
            head.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            head.push(127);
            head.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    w.write_all(&head)?;
    crate::http::write_pieces(w, payload)
}

pub fn write_close(w: &mut impl Write, code: u16, reason: &str) -> io::Result<()> {
    let mut p = code.to_be_bytes().to_vec();
    p.extend_from_slice(&reason.as_bytes()[..reason.len().min(123)]);
    write_frame(w, OP_CLOSE, &p)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// A masked client frame (tests and the in-crate test client).
    pub(crate) fn client_frame(fin: bool, op: u8, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![if fin { 0x80 } else { 0 } | op];
        let mask = [0x12, 0x34, 0x56, 0x78];
        match payload.len() {
            n if n < 126 => f.push(0x80 | n as u8),
            n if n <= 65535 => {
                f.push(0x80 | 126);
                f.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                f.push(0x80 | 127);
                f.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        f.extend_from_slice(&mask);
        f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i & 3]));
        f
    }

    /// Parses one unmasked server frame: (fin, opcode, payload).
    pub(crate) fn server_frame(r: &mut impl Read) -> io::Result<(bool, u8, Vec<u8>)> {
        let mut h = [0u8; 2];
        r.read_exact(&mut h)?;
        assert_eq!(h[1] & 0x80, 0, "server frames are unmasked");
        let len = match h[1] & 0x7f {
            126 => {
                let mut b = [0; 2];
                r.read_exact(&mut b)?;
                u16::from_be_bytes(b) as usize
            }
            127 => {
                let mut b = [0; 8];
                r.read_exact(&mut b)?;
                u64::from_be_bytes(b) as usize
            }
            n => n as usize,
        };
        let mut p = vec![0; len];
        r.read_exact(&mut p)?;
        Ok((h[0] & 0x80 != 0, h[0] & 0x0f, p))
    }

    #[test]
    fn accept_key_rfc_example() {
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn fragments_controls_and_violations() {
        let mut raw = client_frame(false, OP_TEXT, b"hel");
        raw.extend(client_frame(true, OP_PING, b"p"));
        raw.extend(client_frame(true, OP_CONT, b"lo"));
        raw.extend(client_frame(true, OP_TEXT, &vec![b'x'; MAX_MESSAGE]));
        raw.extend(client_frame(true, OP_CLOSE, &[0x03, 0xe8, b'b', b'y']));
        let mut r = Reader::new(Cursor::new(raw));
        assert_eq!(r.next().ok(), Some(Message::Ping(b"p".to_vec())));
        assert_eq!(r.next().ok(), Some(Message::Text("hello".into())));
        assert!(matches!(r.next(), Ok(Message::Text(s)) if s.len() == MAX_MESSAGE));
        assert_eq!(r.next().ok(), Some(Message::Close(Some((1000, "by".into())))));

        let unmasked = vec![0x81, 0x01, b'a'];
        assert!(matches!(Reader::new(Cursor::new(unmasked)).next(), Err(ReadError::Violation(Violation::Protocol))));
        let bad_utf8 = client_frame(true, OP_TEXT, &[0xff]);
        assert!(matches!(Reader::new(Cursor::new(bad_utf8)).next(), Err(ReadError::Violation(Violation::BadText))));
        let stray_cont = client_frame(true, OP_CONT, b"x");
        assert!(matches!(Reader::new(Cursor::new(stray_cont)).next(), Err(ReadError::Violation(Violation::Protocol))));
        let frag_ping = client_frame(false, OP_PING, b"x");
        assert!(matches!(Reader::new(Cursor::new(frag_ping)).next(), Err(ReadError::Violation(Violation::Protocol))));
    }

    #[test]
    fn a_frame_that_stops_short_is_an_error() {
        let mut raw = client_frame(true, OP_TEXT, &vec![b'x'; MAX_MESSAGE]);
        raw.truncate(1000);
        assert!(matches!(Reader::new(Cursor::new(raw)).next(), Err(ReadError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof));
    }

    #[test]
    fn a_message_over_the_limit_is_refused_before_it_is_read() {
        let big = client_frame(true, OP_TEXT, &vec![b'x'; MAX_MESSAGE + 1]);
        assert!(matches!(Reader::new(Cursor::new(big)).next(), Err(ReadError::Violation(Violation::TooBig))));
        // a head alone: what it announces is never waited for
        let announced = [&[0x81, 0x80 | 127][..], &(1u64 << 40).to_be_bytes()].concat();
        assert!(matches!(Reader::new(Cursor::new(announced)).next(), Err(ReadError::Violation(Violation::TooBig))));
        let mut pieces = client_frame(false, OP_TEXT, &vec![b'x'; MAX_MESSAGE]);
        pieces.extend(client_frame(true, OP_CONT, b"x"));
        assert!(matches!(Reader::new(Cursor::new(pieces)).next(), Err(ReadError::Violation(Violation::TooBig))));
    }

    #[test]
    fn the_longest_stock_client_commands_fit() {
        let token = format!("{}.{:x}.{}", "a".repeat(24), i64::MAX, "f".repeat(40));
        let channel = format!("user:{}/memory/{}", "a".repeat(24), "p".repeat(crate::socket::user::MAX_WATCH_PATH));
        for command in [format!("auth {token}"), format!("subscribe {channel}"), format!("unsubscribe {channel}")] {
            // SockJS wraps each one as a JSON array of one string
            let framed = util::json::Json::Arr(vec![util::json::Json::Str(command)]).stringify();
            assert!(framed.len() < MAX_MESSAGE / 16, "{}", framed.len());
        }
    }

    #[test]
    fn server_frames_round_trip() {
        for n in [0, 125, 126, 65535, 65536] {
            let mut out = Vec::new();
            write_frame(&mut out, OP_TEXT, &vec![b'a'; n]).unwrap();
            let (fin, op, p) = server_frame(&mut Cursor::new(out)).unwrap();
            assert!(fin && op == OP_TEXT && p.len() == n);
        }
    }

    #[test]
    fn handshake_checks() {
        let req = |h: Vec<(&str, &str)>| Request {
            method: "GET".into(),
            path: "/socket/websocket".into(),
            query: vec![],
            headers: h.into_iter().map(|(k, v)| (k.into(), v.into())).collect(),
            body: vec![],
        };
        let good = req(vec![("connection", "keep-alive, Upgrade"), ("upgrade", "websocket"), ("sec-websocket-version", "13"), ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")]);
        assert!(handshake(&good, &["13"]).ok().unwrap().contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
        let old = req(vec![("connection", "Upgrade"), ("upgrade", "websocket"), ("sec-websocket-version", "7"), ("sec-websocket-key", "x")]);
        assert_eq!(handshake(&old, &["8", "13"]).unwrap_err().status, 400);
    }
}
