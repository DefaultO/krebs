//! mirror: reference/backend/lib/cli/server.js
//! parity: FAITHFUL
//! Verified by tests in this file (readline line splitting, greeting, per-line output) and
//! crates/runtime/tests/cli.rs (the output rules of the JS half, js/cli/server.js).
//!
//! CLI TCP transport on cli_host:cli_port (127.0.0.1:21026): per connection a log line, a session
//! (`CliEval::open`, CLI-SANDBOX), the greeting, then readline over the socket: each line is evaluated and its
//! output written (CLI-SERVER, CLI-OUTPUT). No authentication: bind to localhost (ARCHITECTURE.md §8.6).

use crate::CliEval;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

/// server.js:48 startServer: listen, one thread per connection. A port that cannot be bound is reported, not
/// fatal (the game server keeps running without a CLI).
pub fn start(host: &str, port: u16, cli: Arc<dyn CliEval>) {
    let listener = match TcpListener::bind((host, port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("The admin console is off, as port {port} is taken ({e}). The game is not affected.");
            return;
        }
    };
    println!("CLI listening on {host}:{port}");
    std::thread::Builder::new()
        .name("cli-accept".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let cli = cli.clone();
                let _ = std::thread::Builder::new().name("cli".into()).spawn(move || connection(stream, &*cli));
            }
        })
        .expect("spawn the CLI listener");
}

/// server.js:6 connectionListener.
fn connection(mut socket: TcpStream, cli: &dyn CliEval) {
    let desc = socket.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    println!("[{desc}] Incoming CLI connection");
    let mut session = cli.open();
    if socket.write_all(session.greeting().as_bytes()).is_err() {
        return;
    }
    let mut lines = LineSplitter::default();
    let mut buf = [0u8; 4096];
    let mut first = true;
    loop {
        match socket.read(&mut buf) {
            Ok(0) => {
                println!("[{desc}] CLI connection closed");
                return;
            }
            Ok(n) => {
                for line in lines.push(&buf[..n]) {
                    if std::mem::take(&mut first) && not_a_command(&line) {
                        println!("[{desc}] CLI connection closed: that was not a command");
                        return;
                    }
                    let out = session.eval(&line);
                    if !out.is_empty() && socket.write_all(out.as_bytes()).is_err() {
                        return;
                    }
                }
            }
            Err(_) => {
                println!("[{desc}] CLI connection reset");
                return;
            }
        }
    }
}

/// Not in server.js. The CLI has no password and runs every line, and any web page can make the visitor's browser
/// connect here, so without this the lines of a request's body would run as commands. Everything a browser sends
/// starts with an HTTP request line or with binary (a TLS handshake, STUN), and a command is neither. LineSplitter
/// only hands over whole lines, so sending this one in pieces changes nothing.
fn not_a_command(line: &str) -> bool {
    let binary = line.chars().any(|c| c == char::REPLACEMENT_CHARACTER || (c.is_control() && c != '\t'));
    binary || line.rsplit_once(' ').is_some_and(|(_, version)| version.starts_with("HTTP/"))
}

/// node's readline `line` events: a line ends at `\n`, `\r\n` or a lone `\r`; text after the last end waits.
#[derive(Default)]
struct LineSplitter {
    pending: Vec<u8>,
    after_cr: bool,
}

impl LineSplitter {
    fn push(&mut self, data: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &b in data {
            match b {
                b'\n' if self.after_cr => self.after_cr = false,
                b'\n' | b'\r' => {
                    out.push(String::from_utf8_lossy(&self.pending).into_owned());
                    self.pending.clear();
                    self.after_cr = b == b'\r';
                }
                _ => {
                    self.after_cr = false;
                    self.pending.push(b);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CliSession;

    #[test]
    fn readline_splitting() {
        let mut s = LineSplitter::default();
        assert_eq!(s.push(b"a\r\nb\nc\rd"), ["a", "b", "c"]);
        assert_eq!(s.push(b"e\r"), ["de"]);
        assert_eq!(s.push(b"\nf\n\n"), ["f", ""]);
    }

    struct Echo;
    impl CliEval for Echo {
        fn open(&self) -> Box<dyn CliSession> {
            struct S;
            impl CliSession for S {
                fn greeting(&self) -> String {
                    "hello\r\n< \r\n".into()
                }
                fn eval(&mut self, line: &str) -> String {
                    format!("< {line}!\r\n")
                }
            }
            Box::new(S)
        }
    }

    #[test]
    fn a_web_request_runs_nothing() {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        start("127.0.0.1", port, Arc::new(Echo));
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(b"POST / HTTP/1.1\r\nHost: x\r\n\r\nsystem.pauseSimulation()\r\n").unwrap();
        c.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut got = Vec::new();
        let _ = c.read_to_end(&mut got); // Closing with lines unread can reset the connection.
        assert!(!String::from_utf8_lossy(&got).contains('!'), "a line ran: {}", String::from_utf8_lossy(&got));
    }

    #[test]
    fn a_request_line_sent_in_pieces_runs_nothing() {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        start("127.0.0.1", port, Arc::new(Echo));
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        for piece in [&b"PO"[..], b"ST / HT", b"TP/1.1", b"\r\n1+1\r\n"] {
            c.write_all(piece).unwrap();
            c.flush().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        c.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut got = Vec::new();
        let _ = c.read_to_end(&mut got);
        assert!(!String::from_utf8_lossy(&got).contains('!'), "a line ran: {}", String::from_utf8_lossy(&got));
    }

    #[test]
    fn a_tls_handshake_runs_nothing() {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        start("127.0.0.1", port, Arc::new(Echo));
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(b"\x16\x03\x01\x02\x00\x01\nsystem.pauseSimulation()\n").unwrap();
        c.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut got = Vec::new();
        let _ = c.read_to_end(&mut got);
        assert!(!String::from_utf8_lossy(&got).contains('!'), "a line ran: {}", String::from_utf8_lossy(&got));
    }

    #[test]
    fn what_counts_as_a_command() {
        for line in ["POST / HTTP/1.1", "GET /socket HTTP/1.0", "PRI * HTTP/2.0", "\u{16}\u{3}\u{1}", "\u{0}\u{3}", "\u{fffd}"] {
            assert!(not_a_command(line), "{line:?}");
        }
        for line in ["system.pauseSimulation()", "storage.db.users.find({ username: 'HTTP/1.1' })", "\tmap.generateRoom('W1N1')", "'Jürgen'", ""] {
            assert!(!not_a_command(line), "{line:?}");
        }
    }

    #[test]
    fn session_over_tcp() {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        start("127.0.0.1", port, Arc::new(Echo));
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(b"1+1\r\nx\n").unwrap();
        c.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut got = String::new();
        let mut buf = [0u8; 256];
        while !got.ends_with("< x!\r\n") {
            let n = c.read(&mut buf).unwrap();
            got.push_str(std::str::from_utf8(&buf[..n]).unwrap());
        }
        assert_eq!(got, "hello\r\n< \r\n< 1+1!\r\n< x!\r\n");
    }
}
