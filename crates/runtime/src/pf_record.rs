//! `CHITIN_PF_RECORD=<dir>`: every `nativeMod.search` of the process is appended to `<dir>/world.txt` and
//! `<dir>/cases-00.txt` in crates/pathfinder's fixture format (tools/pf-gen-fixture.js), with the result this
//! build returned. `CHITIN_PF_REPLAY=<dir> cargo test -p pathfinder --release --test differential replay --
//! --ignored` then replays them, so a change to the search can be checked against real bots' queries.
//!
//! Rooms and matrices are written once (matrices deduplicated by content); each search gets its own callback
//! profile. A search whose callback rewrote a matrix it had returned earlier is left out: the profile can only
//! hold one snapshot per room.

use pathfinder::{Goal, Pos, RoomXY, SearchError, SearchOpts, SearchResult};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::{Mutex, OnceLock};

/// What the room callback returned for one room, as the fixture profile codes it.
pub(crate) enum Code {
    Terrain,
    /// A snapshot of the bytes and the live matrix, compared again when the search ends.
    Matrix(Vec<u8>, *const Cell<u8>),
    Blocked,
    Throw,
}

/// `cost` as recorded. A view of the wrong length reaches the search as `Terrain`; the fixture keeps it as such.
pub(crate) fn code(cost: &pathfinder::RoomCost) -> Code {
    match cost {
        pathfinder::RoomCost::Terrain => Code::Terrain,
        pathfinder::RoomCost::Matrix(m) => Code::Matrix(m.iter().map(Cell::get).collect(), m.as_ptr()),
        pathfinder::RoomCost::Blocked => Code::Blocked,
        pathfinder::RoomCost::Abort => Code::Throw,
    }
}

struct Recorder {
    world: std::fs::File,
    cases: std::fs::File,
    rooms: HashSet<RoomXY>,
    matrices: HashMap<Vec<u8>, usize>,
    profiles: usize,
}

fn recorder() -> Option<&'static Mutex<Recorder>> {
    static R: OnceLock<Option<Mutex<Recorder>>> = OnceLock::new();
    R.get_or_init(|| {
        let dir = std::path::PathBuf::from(std::env::var_os("CHITIN_PF_RECORD")?);
        std::fs::create_dir_all(&dir).ok()?;
        let open = |f: &str| std::fs::File::create(dir.join(f)).ok();
        let r = Recorder { world: open("world.txt")?, cases: open("cases-00.txt")?, rooms: HashSet::new(), matrices: HashMap::new(), profiles: 0 };
        Some(Mutex::new(r))
    })
    .as_ref()
}

pub(crate) fn enabled() -> bool {
    recorder().is_some()
}

fn name(r: RoomXY) -> String {
    let x = if r.x >= 0 { format!("E{}", r.x) } else { format!("W{}", -r.x - 1) };
    let y = if r.y >= 0 { format!("S{}", r.y) } else { format!("N{}", -r.y - 1) };
    x + &y
}

fn pos(p: Pos) -> String {
    format!("{} {} {}", name(p.room), p.x, p.y)
}

// The fixture's path encoding: directions 1..8 (TOP..TOP_LEFT) from the origin.
fn dirs(origin: Pos, path: &[Pos]) -> String {
    let world = |p: Pos| (p.room.x as i64 * 50 + p.x as i64, p.room.y as i64 * 50 + p.y as i64);
    let mut prev = world(origin);
    let mut out = String::new();
    for &p in path {
        let w = world(p);
        out.push(match (w.0 - prev.0, w.1 - prev.1) {
            (0, -1) => '1',
            (1, -1) => '2',
            (1, 0) => '3',
            (1, 1) => '4',
            (0, 1) => '5',
            (-1, 1) => '6',
            (-1, 0) => '7',
            (-1, -1) => '8',
            _ => '?',
        });
        prev = w;
    }
    if out.is_empty() { "-".into() } else { out }
}

/// Appends one search. Call it while the returned matrices are still alive.
#[allow(clippy::too_many_arguments)]
pub(crate) fn record<'a>(
    origin: Pos,
    goals: &[Goal],
    opts: &SearchOpts,
    has_callback: bool,
    calls: &[(RoomXY, Code)],
    terrain: impl Fn(RoomXY) -> Option<&'a [u8; 2500]>,
    result: &Result<SearchResult, SearchError>,
) {
    let Some(rec) = recorder() else { return };
    for (_, code) in calls {
        if let Code::Matrix(snap, live) = code {
            // SAFETY: the caller keeps every returned backing store alive until after this call.
            if snap.iter().enumerate().any(|(i, &b)| unsafe { (*live.add(i)).get() } != b) {
                return;
            }
        }
    }
    let mut rec = rec.lock().unwrap_or_else(|e| e.into_inner());
    let mut world = String::new();
    for (r, _) in calls {
        if rec.rooms.insert(*r) {
            if let Some(t) = terrain(*r) {
                let tiles: String = t.iter().map(|&c| char::from(b'0' + (c & 3))).collect();
                let _ = writeln!(world, "room {} {tiles}", name(*r));
            }
        }
    }
    let mut profile = format!("profile {} {} ", rec.profiles, if has_callback { "cb" } else { "none" });
    for (r, code) in calls {
        let c = match code {
            Code::Terrain => continue,
            Code::Blocked => "F".to_string(),
            Code::Throw => "T".to_string(),
            Code::Matrix(snap, _) => {
                let next = rec.matrices.len();
                let id = *rec.matrices.entry(snap.clone()).or_insert(next);
                if id == next {
                    let hex: String = snap.iter().map(|b| format!("{b:02x}")).collect();
                    let _ = writeln!(world, "matrix {id} {hex}");
                }
                format!("M{id}")
            }
        };
        let _ = write!(profile, "{}={c} ", name(*r));
    }
    let _ = writeln!(world, "{}", profile.trim_end());
    let id = rec.profiles;
    rec.profiles += 1;

    let mut case = format!("O {} G {}", pos(origin), goals.len());
    for g in goals {
        let _ = write!(case, " {} {}", g.range, pos(g.pos));
    }
    let o = opts;
    let _ = write!(
        case,
        " J 0 0 - 0 0 0 0 N {} {} {} {} {} {} {} P {id} C {}",
        o.plain_cost, o.swamp_cost, o.max_rooms, o.max_ops, o.max_cost, o.flee as u8, o.heuristic_weight, calls.len()
    );
    for (r, _) in calls {
        let _ = write!(case, " {}", name(*r));
    }
    match result {
        Ok(r) => {
            let _ = write!(case, " = ok {} {} {} {}", r.ops, r.cost, r.incomplete as u8, dirs(origin, &r.path));
        }
        Err(SearchError::NoTerrain(_)) => case.push_str(" = err terrain"),
        Err(SearchError::Aborted) => case.push_str(" = err throw"),
        Err(e) => {
            let _ = write!(case, " = err {e:?}");
        }
    }
    let _ = rec.world.write_all(world.as_bytes());
    let _ = writeln!(rec.cases, "{case}");
}
