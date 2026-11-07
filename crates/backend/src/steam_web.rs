//! Asks Steam whether a ticket is good right now, for a server other people can reach. A ticket's signature proves Steam
//! made it, but not that it is still in use: the client cancels each ticket once it has signed in, and only Steam knows
//! that. The question needs a Steam Web API key, which the host gets from Steam and Krebs keeps in the data folder.
//!
//! std has no TLS, so curl asks, told what to fetch on its stdin so that neither the key nor a ticket is on a command line.
//! Only hex goes into what curl reads, which is what stops a ticket from adding options of its own to it.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
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
    write_private(&data.join(KEY_FILE), key)
}

/// A program that comes with the system, by its full path. A bare name would run a file of that name beside krebs.exe
/// first, and that folder can be Downloads.
#[cfg(windows)]
pub fn system_program(name: &str) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = [0u16; 512];
    // SAFETY: Windows writes at most `size` units to a buffer that holds that many.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    let folder = buffer.get(..length).filter(|f| !f.is_empty()).map_or(PathBuf::from(r"C:\Windows\System32"), |f| PathBuf::from(std::ffi::OsString::from_wide(f)));
    folder.join(format!("{name}.exe"))
}

#[cfg(not(windows))]
pub fn system_program(name: &str) -> PathBuf {
    PathBuf::from(name)
}

/// Closes a file to everyone but its owner. A drive with no permissions to set, like a USB stick, cannot do that, and it
/// only gets a warning: the file is then as open as the folder, and refusing would stop a game that worked before.
pub fn make_private(path: &Path) {
    if let Err(e) = restrict(path, "*S-1-3-4:F") {
        warn_open(path, &e);
    }
}

fn warn_open(path: &Path, why: &std::io::Error) {
    // not eprintln, which panics if stderr has gone, and this is only a warning
    let _ = writeln!(std::io::stderr(), "Krebs could not keep {} to this account alone ({why}), so others on this PC may be able to read it.", path.display());
}

/// S-1-3-4 is whoever owns the file. With inheritance off, the list holds nobody but what `grant` names.
#[cfg(windows)]
fn restrict(path: &Path, grant: &str) -> std::io::Result<()> {
    let out = Command::new(system_program("icacls")).arg(path).args(["/inheritance:r", "/grant:r", grant]).output()?;
    match out.status.code() {
        Some(0) => Ok(()),
        // icacls ends with the system's own error code
        Some(code) => Err(std::io::Error::from_raw_os_error(code)),
        None => Err(std::io::Error::other(String::from_utf8_lossy(&out.stdout).trim().to_string())),
    }
}

#[cfg(not(windows))]
fn restrict(path: &Path, _grant: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// Whether the drive under `folder` keeps permissions at all. Only the answer no lets a file go unclosed, so that no failure
/// someone can cause, such as holding the folder open, leaves a file as open as the folder around it.
#[cfg(windows)]
fn keeps_permissions(folder: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, RawHandle};
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const PERSISTENT_ACLS: u32 = 0x8;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetVolumeInformationByHandleW(file: RawHandle, name: *mut u16, name_size: u32, serial: *mut u32, longest_name: *mut u32, flags: *mut u32, system: *mut u16, system_size: u32) -> i32;
    }
    let Ok(open) = std::fs::OpenOptions::new().read(true).custom_flags(BACKUP_SEMANTICS).open(folder) else { return true };
    let mut flags = 0u32;
    let null = std::ptr::null_mut();
    // SAFETY: the handle is open for this call, `flags` is written, and Windows takes null for what is not wanted.
    let read = unsafe { GetVolumeInformationByHandleW(open.as_raw_handle(), null, 0, null.cast(), null.cast(), &mut flags, null, 0) };
    read == 0 || flags & PERSISTENT_ACLS != 0
}

/// A new folder that only this account can get into, and whatever is made in it closed the same way from the start.
#[cfg(windows)]
fn new_private_folder(folder: &Path) -> std::io::Result<()> {
    std::fs::create_dir(folder)?;
    match restrict(folder, "*S-1-3-4:(OI)(CI)F") {
        Err(e) if keeps_permissions(folder) => {
            let _ = std::fs::remove_dir(folder);
            Err(e)
        }
        Err(e) => {
            warn_open(folder, &e);
            Ok(())
        }
        Ok(()) => Ok(()),
    }
}

#[cfg(not(windows))]
fn new_private_folder(folder: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(folder)
}

#[cfg(windows)]
fn new_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::create_new(path)
}

#[cfg(not(windows))]
fn new_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

/// A random name, so that nobody can have a file waiting for it.
fn unguessable(prefix: &str) -> String {
    let mut random = [0u8; 8];
    util::sys::os_random(&mut random);
    format!("{prefix}-{}", u64::from_le_bytes(random))
}

/// A file's access list, as text that reads the same in every language of Windows.
#[cfg(windows)]
fn access_list(file: &Path) -> std::io::Result<String> {
    // this account's own temporary folder, as what is read here must not come from where the file is
    let saved = std::env::temp_dir().join(unguessable("krebs-access-list"));
    let out = Command::new(system_program("icacls")).arg(file).arg("/save").arg(&saved).output()?;
    let raw = std::fs::read(&saved);
    let _ = std::fs::remove_file(&saved);
    match out.status.code() {
        Some(0) => {}
        Some(code) => return Err(std::io::Error::from_raw_os_error(code)),
        None => return Err(std::io::ErrorKind::Other.into()),
    }
    let units: Vec<u16> = raw?.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    Ok(String::from_utf16_lossy(&units))
}

/// Whether a list lets in the owner and nobody else.
#[cfg(windows)]
fn owner_only(list: &str) -> bool {
    list.matches("(A;").count() == 1 && list.contains(";;;OW)")
}

/// A folder replaced before it could be closed would be someone else's to open up again, and a file made in it would take
/// their rights. So the new file's own list decides, while the file is still empty.
#[cfg(windows)]
fn check_closed(file: &Path, folder: &Path) -> std::io::Result<()> {
    if !keeps_permissions(folder) || owner_only(&access_list(file)?) {
        return Ok(());
    }
    Err(std::io::Error::other("the new file is open to someone else"))
}

#[cfg(not(windows))]
fn check_closed(_file: &Path, _folder: &Path) -> std::io::Result<()> {
    Ok(())
}

/// `text` in a new file `name` in `folder`, which has to hold nothing yet: anything there came from someone else.
fn fill(folder: &Path, name: &OsStr, text: &str) -> std::io::Result<PathBuf> {
    if std::fs::read_dir(folder)?.next().is_some() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "something else got into the private folder"));
    }
    let inside = folder.join(name);
    let mut file = new_private_file(&inside)?;
    let written = check_closed(&inside, folder).and_then(|()| file.write_all(text.as_bytes()));
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&inside);
        return Err(e);
    }
    Ok(inside)
}

/// `text` in a file only this account can read, from its first moment. Windows checks access when a file is opened, so a
/// handle someone got before a file was closed would go on working after. So the file starts in a new folder beside it that
/// is closed already, and keeps that access when it moves into place, over any old file of that name. It is closed once
/// more there, to stand on its own. A drive with no permissions only gets the warning `make_private` gives.
pub fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let name = path.file_name().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let folder = path.with_file_name(unguessable("krebs-private"));
    new_private_folder(&folder)?;
    let placed = fill(&folder, name, text).and_then(|inside| {
        let moved = std::fs::rename(&inside, path);
        if moved.is_err() {
            let _ = std::fs::remove_file(&inside);
        }
        moved
    });
    // not remove_dir_all: this takes only what it made, and anything else stays for someone to look at
    let _ = std::fs::remove_dir(&folder);
    if placed.is_ok() {
        make_private(path);
    }
    placed
}

/// What Steam says about a ticket (hex).
pub fn ask(key: &str, ticket: &str) -> Answer {
    ask_with(system_program("curl"), key, ticket)
}

/// `Bad` when Steam took the key, as it was asked about no ticket at all.
pub fn try_key(key: &str) -> Answer {
    ask(key, NO_TICKET)
}

fn ask_with(program: impl AsRef<OsStr>, key: &str, ticket: &str) -> Answer {
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

    #[cfg(windows)]
    #[test]
    fn programs_of_the_system_never_come_from_beside_the_exe() {
        for name in ["curl", "cmd", "taskkill", "icacls", r"WindowsPowerShell\v1.0\powershell"] {
            let program = system_program(name);
            assert!(program.is_absolute() && program.is_file(), "{}", program.display());
            assert!(program.starts_with(std::env::var_os("SystemRoot").unwrap()), "{}", program.display());
        }
    }

    /// What a folder on a second drive hands down: Modify for Authenticated Users.
    #[cfg(windows)]
    fn open_folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("krebs-key-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(Command::new(system_program("icacls")).arg(&dir).args(["/grant", "*S-1-5-11:(OI)(CI)M"]).output().unwrap().status.success());
        dir
    }

    #[cfg(windows)]
    #[test]
    fn a_saved_key_is_this_accounts_alone_whatever_the_folder_allows() {
        let dir = open_folder("private");
        std::fs::write(dir.join("plain.txt"), "").unwrap();
        assert!(access_list(&dir.join("plain.txt")).unwrap().contains(";;;AU)"), "the folder does hand it down");
        save_key(&dir, KEY).unwrap();
        let list = access_list(&dir.join(KEY_FILE)).unwrap();
        assert!(list.contains("D:P") && list.contains(";;;OW)") && list.matches("(A;").count() == 1, "{list}");
        assert_eq!(key_in(&dir).as_deref(), Some(KEY), "and this account can still read it");
        save_key(&dir, "abc").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(KEY_FILE)).unwrap(), "abc", "a new key replaces the old one");
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|f| f.unwrap().file_name()).collect();
        assert_eq!(left.len(), 2, "the private folder goes: {left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_made_in_the_private_folder_is_closed_from_its_first_moment() {
        let dir = open_folder("birth");
        let folder = dir.join("room");
        new_private_folder(&folder).unwrap();
        assert!(new_private_folder(&folder).is_err(), "only a new folder will do");
        let list = access_list(&folder).unwrap();
        assert!(list.contains("D:P") && list.contains("OICI") && list.contains(";;;OW)") && list.matches("(A;").count() == 1, "{list}");
        let file = new_private_file(&folder.join("x")).unwrap();
        let list = access_list(&folder.join("x")).unwrap();
        assert!(owner_only(&list), "before anything was written or closed: {list}");
        drop(file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_is_written_into_a_folder_someone_else_got_into() {
        let dir = std::env::temp_dir().join(format!("krebs-key-stranger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let folder = dir.join("room");
        new_private_folder(&folder).unwrap();
        std::fs::write(folder.join("waiting"), "planted").unwrap();
        assert_eq!(fill(&folder, OsStr::new("key.txt"), "text").unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
        assert!(!folder.join("key.txt").exists());
        std::fs::remove_file(folder.join("waiting")).unwrap();
        fill(&folder, OsStr::new("key.txt"), "text").unwrap();
        assert_eq!(std::fs::read_to_string(folder.join("key.txt")).unwrap(), "text");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a folder someone replaced before it was closed looks like: it hands down their rights. They could be holding the
    /// empty file open already, so it stays empty, and goes.
    #[cfg(windows)]
    #[test]
    fn a_new_file_open_to_someone_else_gets_no_text() {
        let dir = open_folder("swapped");
        let folder = dir.join("room");
        std::fs::create_dir(&folder).unwrap();
        assert!(fill(&folder, OsStr::new("key.txt"), "text").is_err());
        assert!(!folder.join("key.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_list_with_anyone_else_on_it_is_not_owner_only() {
        assert!(owner_only("D:AI(A;ID;FA;;;OW)"));
        assert!(!owner_only("D:AI(A;ID;FA;;;OW)(A;ID;FR;;;AU)"));
        assert!(!owner_only("D:AI(A;ID;FA;;;AU)"));
    }

    #[cfg(not(windows))]
    #[test]
    fn a_saved_key_is_this_accounts_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("krebs-key-private-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        save_key(&dir, KEY).unwrap();
        assert_eq!(std::fs::metadata(dir.join(KEY_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        let room = dir.join("room");
        new_private_folder(&room).unwrap();
        assert_eq!(std::fs::metadata(&room).unwrap().permissions().mode() & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(&dir);
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
