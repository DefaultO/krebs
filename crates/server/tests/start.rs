//! `krebs` with no command, the way a double-click runs it: the exe in a folder of its own, the current directory
//! somewhere else, and no mods, so these pass on the core alone. stdin is empty, so a wait for Enter ends at once.

use std::fs::File;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A folder with the exe and a krebs.json in it, as someone would unzip it. Hard links keep the copies free.
struct Install {
    dir: PathBuf,
    port: u16,
}

impl Install {
    fn new(name: &str, port: u16) -> Install {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("start-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(Path::new(env!("CARGO_BIN_EXE_krebs")).file_name().unwrap());
        if std::fs::hard_link(env!("CARGO_BIN_EXE_krebs"), &exe).is_err() {
            std::fs::copy(env!("CARGO_BIN_EXE_krebs"), &exe).unwrap();
        }
        let config = format!(r#"{{"http":{{"port":{port}}},"cli":{{"port":{}}}}}"#, free_port());
        std::fs::write(dir.join("krebs.json"), config).unwrap();
        Install { dir, port }
    }

    /// Started from anywhere but its own folder.
    fn command(&self) -> Command {
        let mut c = Command::new(self.dir.join(Path::new(env!("CARGO_BIN_EXE_krebs")).file_name().unwrap()));
        c.current_dir(self.dir.parent().unwrap()).stdin(Stdio::null());
        c
    }

    /// Runs it to its end; everything it said, and how it ended.
    fn output(&self) -> (String, Output) {
        let out = self.command().output().unwrap();
        (format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)), out)
    }
}

impl Drop for Install {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A running one, killed on drop, its output in a file.
struct Running {
    child: Child,
    log: PathBuf,
}

impl Running {
    fn start(install: &Install) -> Running {
        let log = install.dir.join("console.txt");
        let file = File::create(&log).unwrap();
        let child = install.command().stdout(file.try_clone().unwrap()).stderr(file).spawn().unwrap();
        let mut running = Running { child, log };
        let end = Instant::now() + Duration::from_secs(90);
        while !version(install.port).contains("\"protocol\"") {
            assert!(Instant::now() < end, "it did not start: {}", running.said());
            assert!(running.child.try_wait().unwrap().is_none(), "it ended: {}", running.said());
            std::thread::sleep(Duration::from_millis(200));
        }
        running
    }

    fn said(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn version(port: u16) -> String {
    let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) else { return String::new() };
    let _ = s.write_all(b"GET /api/version HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    let mut reply = String::new();
    let _ = s.read_to_string(&mut reply);
    reply
}

#[test]
fn the_first_start_does_everything_in_the_exes_folder() {
    let install = Install::new("first", free_port());
    let running = Running::start(&install);
    assert!(version(install.port).contains("\"useNativeAuth\":false"), "the stock client's API");
    assert!(install.dir.join("data").join("world.json").exists() && install.dir.join("data").join("world.bin").exists(), "the world goes beside the exe");
    assert!(!install.dir.parent().unwrap().join("data").exists(), "not into the current directory");
    let end = Instant::now() + Duration::from_secs(10);
    while !running.said().contains("Close it to stop") {
        assert!(Instant::now() < end, "{}", running.said());
        std::thread::sleep(Duration::from_millis(100));
    }
    let said = running.said();
    for words in ["Creating your world", "Krebs is running", "Open Screeps in Steam", &format!("Host 127.0.0.1 and Port {}", install.port)] {
        assert!(said.contains(words), "{words}: {said}");
    }

    // a second start finds the first and says so
    let (said, out) = install.output();
    assert!(said.contains("Krebs is already running on port"), "{said}");
    assert_eq!(out.status.code(), Some(1));
    assert!(!said.contains("usage:"), "{said}");
}

#[test]
fn a_port_another_program_holds_is_a_plain_sentence() {
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = other.local_addr().unwrap().port();
    let (accept_end, install) = (other.try_clone().unwrap(), Install::new("port", port));
    std::thread::spawn(move || {
        // answers like any program that is not a Krebs
        for s in accept_end.incoming().flatten() {
            drop(s);
        }
    });
    let (said, out) = install.output();
    assert!(said.contains(&format!("Port {port} is already used by another program")), "{said}");
    assert_eq!(out.status.code(), Some(1));
    assert!(!said.contains("usage:") && !said.contains("panicked"), "{said}");
}

#[test]
fn a_damaged_world_is_left_as_it_was() {
    let install = Install::new("damaged", free_port());
    let data = install.dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("world.json"), "{}").unwrap();
    std::fs::write(data.join("world.bin"), "this is not a world").unwrap();
    let (said, out) = install.output();
    assert!(said.contains("could not be read") && said.contains("Krebs did not change it"), "{said}");
    assert!(said.contains("Press Enter"), "{said}");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(data.join("world.bin")).unwrap(), "this is not a world");
    assert_eq!(version(install.port), "", "no server stayed behind");
}

#[test]
fn friends_are_not_let_in_as_one_shared_account() {
    let install = Install::new("shared", free_port());
    std::fs::write(install.dir.join("krebs.json"), format!(r#"{{"http":{{"port":{}}},"cli":{{"port":{}}},"mods":[]}}"#, install.port, free_port())).unwrap();
    let out = install.command().arg("--friends").output().unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(said.contains("Friends cannot join without the bare mod"), "{said}");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(version(install.port), "", "no server stayed behind");
}

#[test]
fn a_mistake_in_krebs_json_is_a_plain_sentence() {
    let install = Install::new("settings", free_port());
    std::fs::write(install.dir.join("krebs.json"), "{ not json").unwrap();
    let (said, out) = install.output();
    assert!(said.contains("Krebs could not read its settings") && said.contains("krebs.json"), "{said}");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn a_mistake_that_is_still_json_says_so_too() {
    let install = Install::new("types", free_port());
    for (text, words) in [(r#"{ "http": { "port": "21030" } }"#, "http.port has to be a number"), (r#"{ "http": { "port": 99999 } }"#, "http.port has to be a number"), (r#"{ "mods": "bare" }"#, "mods has to be a list")] {
        std::fs::write(install.dir.join("krebs.json"), text).unwrap();
        let (said, out) = install.output();
        assert!(said.contains("Krebs could not read its settings") && said.contains(words), "{text}: {said}");
        assert_eq!(out.status.code(), Some(1), "{text}");
    }
}

#[test]
fn a_command_works_as_before_and_friends_needs_none() {
    let install = Install::new("commands", free_port());
    let (said, out) = {
        let out = install.command().args(["new", "--world", "1x1", "--data", "here"]).output().unwrap();
        (String::from_utf8_lossy(&out.stdout).into_owned(), out)
    };
    assert!(out.status.success() && said.contains("world 1x1 sectors"), "{said}");
    assert!(install.dir.parent().unwrap().join("here").join("world.bin").exists(), "a command stays in the current directory");
    let _ = std::fs::remove_dir_all(install.dir.parent().unwrap().join("here"));
    let out = install.command().args(["serve", "--friends"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--friends only works with no command"));
    let out = install.command().arg("--help").output().unwrap();
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("usage:"));
}
