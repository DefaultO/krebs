//! Asks Steam whether a ticket is good right now, for a server other people can reach. A ticket's signature proves Steam
//! made it, but not that it is still in use: the client cancels each ticket once it has signed in, and only Steam knows
//! that. The question needs a Steam Web API key, which the host gets from Steam and Krebs keeps in the data folder.
//!
//! std has no TLS, so curl asks, told what to fetch on its stdin so that neither the key nor a ticket is on a command line.
//! Only hex goes into what curl reads, which is what stops a ticket from adding options of its own to it.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use util::json::Json;

/// In the data folder, which no repository or download holds.
pub const KEY_FILE: &str = "steam-api-key.txt";
/// Where a host gets a key.
pub const KEY_PAGE: &str = "https://steamcommunity.com/dev/apikey";
/// Screeps on Steam.
const SCREEPS: u32 = 464_350;
/// Steam answers in under a second, and the client is still waiting.
const MAX_SECONDS: &str = "4";
/// A ticket no key can pass, so that Steam's reply says only whether it took the key.
const NO_TICKET: &str = "00";
/// What a key looks like, in hex digits.
const KEY_DIGITS: std::ops::RangeInclusive<usize> = 32..=64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Answer {
    /// This player, with this ticket, right now.
    Good(u64),
    /// Steam answered and the ticket is not good.
    Bad,
    KeyRefused,
    NoAnswer,
}

fn is_hex(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn is_key(text: &str) -> bool {
    KEY_DIGITS.contains(&text.len()) && is_hex(text)
}

/// The key in the data folder, if there is one.
pub fn key_in(data: &Path) -> Option<String> {
    let text = std::fs::read_to_string(data.join(KEY_FILE)).ok()?;
    Some(text.trim_start_matches('\u{feff}').trim().to_string()).filter(|key| is_key(key))
}

pub fn save_key(data: &Path, key: &str) -> std::io::Result<()> {
    std::fs::write(data.join(KEY_FILE), key)
}

/// What Steam says about a ticket (hex).
pub fn ask(key: &str, ticket: &str) -> Answer {
    ask_with("curl", key, ticket)
}

/// `Bad` when Steam took the key, as it was asked about no ticket at all.
pub fn try_key(key: &str) -> Answer {
    ask(key, NO_TICKET)
}

fn ask_with(program: &str, key: &str, ticket: &str) -> Answer {
    if !is_key(key) || !is_hex(ticket) {
        return Answer::Bad;
    }
    let child = Command::new(program)
        // -q comes first or it does nothing: a .curlrc left on this PC has no say in where a key goes
        .args(["-q", "-sS", "--proto", "=https", "--max-time", MAX_SECONDS, "--max-filesize", "100000", "-w", "\n%{http_code}", "-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return Answer::NoAnswer };
    let url = format!("url = \"https://api.steampowered.com/ISteamUserAuth/AuthenticateUserTicket/v1/?key={key}&appid={SCREEPS}&ticket={ticket}\"\n");
    let sent = child.stdin.take().is_some_and(|mut stdin| stdin.write_all(url.as_bytes()).is_ok());
    let Ok(out) = child.wait_with_output() else { return Answer::NoAnswer };
    if !sent {
        return Answer::NoAnswer;
    }
    read_reply(&String::from_utf8_lossy(&out.stdout))
}

/// curl's output: the body, a line break, the status.
fn read_reply(output: &str) -> Answer {
    let Some((body, status)) = output.rsplit_once('\n') else { return Answer::NoAnswer };
    match status.trim() {
        "200" => {}
        "400" => return Answer::Bad,
        "401" | "403" => return Answer::KeyRefused,
        _ => return Answer::NoAnswer,
    }
    let Some(response) = Json::parse(body).ok().and_then(|reply| reply.get("response").cloned()) else { return Answer::NoAnswer };
    let Some(params) = response.get("params") else { return Answer::Bad };
    let id = params.get("steamid").and_then(Json::as_str).and_then(|id| id.parse().ok());
    match (params.get("result").and_then(Json::as_str), id) {
        (Some("OK"), Some(id)) => Answer::Good(id),
        _ => Answer::Bad,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789ABCDEF0123456789abcdef";

    #[test]
    fn a_good_ticket_comes_back_with_its_player() {
        let reply = r#"{"response":{"params":{"result":"OK","steamid":"76561197960287930","ownersteamid":"76561197960287930","vacbanned":false,"publisherbanned":false}}}"#;
        assert_eq!(read_reply(&format!("{reply}\n200")), Answer::Good(76561197960287930));
    }

    #[test]
    fn anything_but_ok_is_not_good() {
        for reply in [r#"{"response":{"error":{"errorcode":101,"errordesc":"Invalid ticket"}}}"#, r#"{"response":{"params":{"result":"Invalid"}}}"#, r#"{"response":{"params":{"result":"OK"}}}"#, r#"{"response":{"params":{"result":"OK","steamid":"nope"}}}"#, r#"{"response":{}}"#] {
            assert_eq!(read_reply(&format!("{reply}\n200")), Answer::Bad, "{reply}");
        }
        assert_eq!(read_reply("<html>Bad Request</html>\n400"), Answer::Bad);
    }

    #[test]
    fn a_refused_key_is_told_from_no_answer() {
        assert_eq!(read_reply("<html>Forbidden</html>\n403"), Answer::KeyRefused);
        assert_eq!(read_reply("\n401"), Answer::KeyRefused);
        for output in ["", "\n000", "<html>busy</html>\n503", "\n429", "not json\n200", r#"{"nothing":1}"#, "[]\n200"] {
            assert_eq!(read_reply(output), Answer::NoAnswer, "{output:?}");
        }
    }

    #[test]
    fn nothing_but_hex_gets_as_far_as_curl() {
        for ticket in ["", "00\"\noutput = \"evil", "zz", "00 11", "00\n"] {
            assert_eq!(ask_with("krebs-no-such-program", KEY, ticket), Answer::Bad, "{ticket:?}");
        }
        for key in ["", "abc", "0123456789ABCDEF0123456789abcdeg", "0123456789ABCDEF0123456789abcde\"", &"0".repeat(65)] {
            assert_eq!(ask_with("krebs-no-such-program", key, "00"), Answer::Bad, "{key:?}");
        }
    }

    #[test]
    fn no_curl_is_no_answer() {
        assert_eq!(ask_with("krebs-no-such-program", KEY, "00"), Answer::NoAnswer);
    }

    #[test]
    fn a_saved_key_is_read_back_and_junk_is_not() {
        let dir = std::env::temp_dir().join(format!("krebs-steam-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(key_in(&dir), None);
        save_key(&dir, KEY).unwrap();
        assert_eq!(key_in(&dir).as_deref(), Some(KEY));
        std::fs::write(dir.join(KEY_FILE), format!("\u{feff}{KEY}")).unwrap();
        assert_eq!(key_in(&dir).as_deref(), Some(KEY), "a byte order mark from Notepad");
        std::fs::write(dir.join(KEY_FILE), format!("  {KEY}\r\n")).unwrap();
        assert_eq!(key_in(&dir).as_deref(), Some(KEY), "a line break from an editor");
        std::fs::write(dir.join(KEY_FILE), "paste your key here").unwrap();
        assert_eq!(key_in(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
