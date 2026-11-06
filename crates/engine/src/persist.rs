//! Checkpoints (ARCHITECTURE.md §9): the world in a native binary store at `<data>/world.bin`, written from a
//! published `WorldView` on a writer thread while the world keeps ticking; atomic (temp file, fsync, rename), the
//! previous one kept as `world.bin.prev` and loaded when the newest fails its checksums. Loss on a crash ≤ one
//! interval, as with Loki's autosave (STORE-PERSISTENCE).
//!
//! It holds what a Loki export holds (`loki::docs` of every collection, the env keys `loki::import` reads, the
//! `chitin` resume key) without what import rebuilds (terrainData, accessibleRooms, roomStatusData), terrain
//! packed; loading builds the world through the same `loki::build`. `chitin export` still writes a Loki db.json;
//! a `<data>/db.json` from before the store is loaded once and renamed `db.json.migrated`.
//!
//! Format: `MAGIC`, then sections `tag u8 | len u64 LE | payload | crc32(payload) u32 LE`, then tag 0.
//!   b'E' env: an Obj (one per file, first)        b'K' env keys of one user: n, (key, Val)*
//!   b'T' terrain: n, 625 B per room (rooms order)  b'C' collection: name, n, (loki, Obj)*
//! Values: 0 null, 1 false, 2 true, 3 f64 LE, 4 integer (zigzag varint), 5 string, 6 array (n, values), 7 object
//! (n, (key, value)*). Keys: varint 0 then the name (it gets the next number), else number + 1, per section.
// ponytail: one full rewrite per checkpoint (40k rooms: 0.24 s, 37.5 MB; db.json was ~5 s, 154 MB). Terrain is 2/3
// of it and rarely changes: a file of its own when checkpoint bytes matter. No per-tick change log: bot Memory
// churn alone outweighs a snapshot every few seconds.

use crate::doc::{Atom, Obj, Val};
use crate::loki::{self, Db, ImportOpts, ImportReport};
use crate::policy::Profile;
use crate::terrain::RoomTerrain;
use crate::world::{World, WorldView};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;

const MAGIC: &[u8; 8] = b"CHITINW\x01";

pub fn path(data: &Path) -> PathBuf {
    data.join("world.bin")
}

fn prev(data: &Path) -> PathBuf {
    data.join("world.bin.prev")
}

/// The checkpoint format before the store (and what `chitin export` writes).
pub fn legacy_path(data: &Path) -> PathBuf {
    data.join("db.json")
}

/// True when `data` holds a world (the store or a legacy db.json).
pub fn exists(data: &Path) -> bool {
    path(data).exists() || prev(data).exists() || legacy_path(data).exists()
}

/// Writes `bytes` to `target` atomically (temp file in the same directory, fsync, rename).
pub fn write_atomic(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_with(target, None, |f| f.write_all(bytes))
}

/// `write_atomic` streaming through `fill`; `keep`: where the replaced file goes.
fn write_with(target: &Path, keep: Option<&Path>, fill: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> std::io::Result<()>) -> std::io::Result<()> {
    // unique per writer: a CLI `system.save()` may run while the checkpoint thread writes
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = target.with_extension(format!("{}.{}.tmp", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let res = (|| {
        let mut f = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&tmp)?);
        fill(&mut f)?;
        f.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        if let Some(keep) = keep {
            match std::fs::rename(target, keep) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        std::fs::rename(&tmp, target)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res?;
    #[cfg(unix)]
    if let Some(dir) = target.parent() {
        std::fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// One checkpoint, synchronously.
pub fn save(data: &Path, view: &WorldView, profile: Profile) -> std::io::Result<()> {
    write_with(&path(data), Some(&prev(data)), |f| encode(view, profile, f))
}

/// The newest checkpoint of `data`, if there is one: the store (else its previous version, when the newest is
/// missing or damaged), else a legacy db.json, which is then migrated (the store written, db.json renamed).
pub fn load(data: &Path, opts: &ImportOpts) -> Option<Result<(World, ImportReport), String>> {
    // a file that is there and cannot be read is a damaged world, not a missing one: a missing one gets generated afresh,
    // and the next save would replace what is on disk
    let read = |p: &Path| match std::fs::read(p) {
        Ok(b) => Some(decode(&b, opts).map_err(|e| format!("{}: {e}", p.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(Err(format!("{}: {e}", p.display()))),
    };
    let newest = match read(&path(data)) {
        Some(Ok(w)) => return Some(Ok(w)),
        other => other,
    };
    if let Some(Ok(w)) = read(&prev(data)) {
        let why = match &newest {
            Some(Err(e)) => e.clone(),
            _ => format!("{} is missing", path(data).display()),
        };
        eprintln!("warning: {why}; loaded {} (tick {})", prev(data).display(), w.0.time);
        return Some(Ok(w));
    }
    if newest.is_some() {
        return newest;
    }
    let text = std::fs::read_to_string(legacy_path(data)).ok()?;
    Some(loki::import(&text, opts).and_then(|(w, report)| {
        let err = |e: std::io::Error| format!("migrating {}: {e}", legacy_path(data).display());
        save(data, &w.view(), w.compat.profile).map_err(err)?;
        std::fs::rename(legacy_path(data), data.join("db.json.migrated")).map_err(err)?;
        println!("migrated {} to {}", legacy_path(data).display(), path(data).display());
        Ok((w, report))
    }))
}

// ---- encoding ----

/// One section's payload, with its key table.
#[derive(Default)]
struct Enc {
    out: Vec<u8>,
    keys: HashMap<Atom, u64>,
}

impl Enc {
    fn uint(&mut self, mut n: u64) {
        while n >= 0x80 {
            self.out.push(n as u8 | 0x80);
            n >>= 7;
        }
        self.out.push(n as u8);
    }

    fn bytes(&mut self, b: &[u8]) {
        self.uint(b.len() as u64);
        self.out.extend_from_slice(b);
    }

    fn key(&mut self, k: Atom) {
        match self.keys.get(&k) {
            Some(&i) => self.uint(i + 1),
            None => {
                self.uint(0);
                self.bytes(k.as_str().as_bytes());
                self.keys.insert(k, self.keys.len() as u64);
            }
        }
    }

    fn val(&mut self, v: &Val) {
        match v {
            Val::Null => self.out.push(0),
            Val::Bool(b) => self.out.push(1 + *b as u8),
            // integers within ±2^53, but not -0
            Val::Num(n) if n.fract() == 0.0 && n.abs() <= 9007199254740992.0 && !(*n == 0.0 && n.is_sign_negative()) => {
                self.out.push(4);
                let i = *n as i64;
                self.uint(((i << 1) ^ (i >> 63)) as u64);
            }
            Val::Num(n) => {
                self.out.push(3);
                self.out.extend_from_slice(&n.to_le_bytes());
            }
            Val::Str(s) => {
                self.out.push(5);
                self.bytes(s.as_bytes());
            }
            Val::Arr(a) => {
                self.out.push(6);
                self.uint(a.len() as u64);
                for x in a {
                    self.val(x);
                }
            }
            Val::Obj(o) => {
                self.out.push(7);
                self.obj(o);
            }
        }
    }

    fn obj(&mut self, o: &Obj) {
        self.uint(o.len() as u64);
        for (k, v) in o.iter() {
            self.key(k);
            self.val(v);
        }
    }
}

fn section(f: &mut impl Write, tag: u8, fill: impl FnOnce(&mut Enc)) -> std::io::Result<()> {
    let mut e = Enc::default();
    fill(&mut e);
    f.write_all(&[tag])?;
    f.write_all(&(e.out.len() as u64).to_le_bytes())?;
    f.write_all(&e.out)?;
    f.write_all(&util::deflate::crc32(&e.out).to_le_bytes())
}

fn encode(v: &WorldView, profile: Profile, f: &mut impl Write) -> std::io::Result<()> {
    f.write_all(MAGIC)?;
    section(f, b'E', |e| e.obj(&loki::env_obj(v, profile)))?;
    // a section per user: Memory is up to 2 MiB each
    for u in v.users.iter().filter(|u| !loki::is_removed_user(u)) {
        let keys = loki::user_env(u);
        if !keys.is_empty() {
            section(f, b'K', |e| {
                e.uint(keys.len() as u64);
                for (k, val) in &keys {
                    e.bytes(k.as_bytes());
                    e.val(val);
                }
            })?;
        }
    }
    section(f, b'T', |e| {
        let live: Vec<usize> = (0..v.rooms.len()).filter(|&r| !v.meta.rooms[r].removed).collect();
        e.uint(live.len() as u64);
        for r in live {
            e.out.extend_from_slice(v.terrain.get(r as _).packed());
        }
    })?;
    for &(coll, _) in loki::COLLECTIONS.iter().filter(|c| c.0 != "rooms.terrain") {
        let docs = loki::docs(v, coll).unwrap_or_default();
        section(f, b'C', |e| {
            e.bytes(coll.as_bytes());
            e.uint(docs.len() as u64);
            for (loki, d) in &docs {
                e.uint(*loki);
                e.obj(d.obj());
            }
        })?;
    }
    f.write_all(&[0])
}

// ---- decoding ----

struct Dec<'a> {
    b: &'a [u8],
    at: usize,
    keys: Vec<Atom>,
}

impl<'a> Dec<'a> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.b.get(self.at).ok_or("truncated")?;
        self.at += 1;
        Ok(b)
    }

    fn uint(&mut self) -> Result<u64, String> {
        let (mut n, mut shift) = (0u64, 0u32);
        loop {
            let b = self.byte()?;
            if shift > 63 {
                return Err("bad varint".into());
            }
            n |= ((b & 0x7f) as u64) << shift;
            if b < 0x80 {
                return Ok(n);
            }
            shift += 7;
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.b.get(self.at..self.at.checked_add(n).ok_or("truncated")?).ok_or("truncated")?;
        self.at += n;
        Ok(s)
    }

    fn str(&mut self) -> Result<&'a str, String> {
        let n = self.uint()? as usize;
        std::str::from_utf8(self.take(n)?).map_err(|_| "bad UTF-8".to_string())
    }

    fn key(&mut self) -> Result<Atom, String> {
        match self.uint()? as usize {
            0 => {
                let k = Atom::intern(self.str()?);
                self.keys.push(k);
                Ok(k)
            }
            i => self.keys.get(i - 1).copied().ok_or_else(|| "bad key".into()),
        }
    }

    /// A count, checked against what is left (an item takes a byte at least): a bad one fails, not allocates.
    fn len(&mut self) -> Result<usize, String> {
        let n = self.uint()? as usize;
        if n > self.b.len() - self.at {
            return Err("truncated".into());
        }
        Ok(n)
    }

    fn val(&mut self) -> Result<Val, String> {
        Ok(match self.byte()? {
            0 => Val::Null,
            1 => Val::Bool(false),
            2 => Val::Bool(true),
            3 => Val::Num(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            4 => {
                let z = self.uint()?;
                Val::Num(((z >> 1) as i64 ^ -((z & 1) as i64)) as f64)
            }
            5 => Val::Str(self.str()?.into()),
            6 => {
                let n = self.len()?;
                let mut a = Vec::with_capacity(n);
                for _ in 0..n {
                    a.push(self.val()?);
                }
                Val::Arr(a)
            }
            7 => Val::Obj(self.obj()?),
            t => return Err(format!("bad value tag {t}")),
        })
    }

    /// Keys come in the order the `Obj` held them, so they are not re-sorted.
    fn obj(&mut self) -> Result<Obj, String> {
        let n = self.len()?;
        let mut o = Vec::with_capacity(n);
        for _ in 0..n {
            o.push((self.key()?, self.val()?));
        }
        Ok(Obj(o))
    }
}

fn decode(b: &[u8], opts: &ImportOpts) -> Result<(World, ImportReport), String> {
    if b.get(..8) != Some(&MAGIC[..]) {
        return Err("not a Krebs world store".into());
    }
    let mut at = 8;
    let mut db = Db { env: None, version: Some(9.0), colls: Vec::new() };
    let mut terrain = None;
    loop {
        let tag = *b.get(at).ok_or("truncated")?;
        if tag == 0 {
            break;
        }
        let len = u64::from_le_bytes(b.get(at + 1..at + 9).ok_or("truncated")?.try_into().unwrap());
        let body = b.get(at + 9..).and_then(|r| r.get(..usize::try_from(len).ok()?)).ok_or("truncated")?;
        let end = at + 9 + body.len();
        let crc = u32::from_le_bytes(b.get(end..end + 4).ok_or("truncated")?.try_into().unwrap());
        if util::deflate::crc32(body) != crc {
            return Err(format!("checksum mismatch in section {:?} at byte {at}", tag as char));
        }
        let mut d = Dec { b: body, at: 0, keys: Vec::new() };
        match tag {
            b'E' => db.env = Some(d.obj()?),
            b'K' => {
                let n = d.len()?;
                let env = db.env.as_mut().ok_or("user keys before env")?;
                for _ in 0..n {
                    let k = Atom::intern(d.str()?);
                    env.0.push((k, d.val()?));
                }
            }
            b'T' => {
                let n = d.len()?;
                terrain = Some((0..n).map(|_| Ok(RoomTerrain::from_packed(d.take(625)?.try_into().unwrap()))).collect::<Result<Vec<_>, String>>()?);
            }
            b'C' => {
                let name = d.str()?.to_string();
                let n = d.len()?;
                let mut docs = Vec::with_capacity(n);
                for _ in 0..n {
                    docs.push((d.uint()? as f64, d.obj()?));
                }
                db.colls.push((name, docs));
            }
            t => return Err(format!("unknown section {:?} at byte {at}", t as char)),
        }
        if d.at != body.len() {
            return Err(format!("section {:?} at byte {at} has trailing bytes", tag as char));
        }
        at = end + 4;
    }
    if db.env.is_none() || terrain.is_none() {
        return Err("no env or terrain section".into());
    }
    loki::build(db, terrain, opts)
}

// ---- the checkpoint thread ----

/// The checkpoint thread: `offer` hands it a view unless a write is still running (that checkpoint is skipped).
pub struct Writer {
    tx: SyncSender<WorldView>,
    busy: Arc<AtomicBool>,
    /// Ends with how the last save went.
    thread: std::thread::JoinHandle<Result<(), String>>,
}

impl Writer {
    /// `report` hears how each save went, on the writer's thread.
    pub fn spawn(data: PathBuf, profile: Profile, mut report: impl FnMut(Result<(), &str>) + Send + 'static) -> Writer {
        let (tx, rx) = sync_channel::<WorldView>(1);
        let busy = Arc::new(AtomicBool::new(false));
        let flag = busy.clone();
        let thread = std::thread::Builder::new()
            .name("persist".into())
            .spawn(move || {
                let mut last = Ok(());
                for view in rx {
                    last = save(&data, &view, profile).map_err(|e| e.to_string());
                    report(last.as_ref().map(|_| ()).map_err(String::as_str));
                    flag.store(false, Ordering::Release);
                }
                last
            })
            .expect("spawn the persist thread");
        Writer { tx, busy, thread }
    }

    /// True when the view was taken.
    pub fn offer(&self, view: WorldView) -> bool {
        if self.busy.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.tx.send(view).is_ok()
    }

    /// The last checkpoint (shutdown): written after any running one, then the thread ends. Says how that one went.
    pub fn finish(self, view: WorldView) -> Result<(), String> {
        let _ = self.tx.send(view);
        drop(self.tx);
        self.thread.join().unwrap_or_else(|_| Err("the save stopped with an internal error".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let j = r#"{"a":null,"b":[true,false,0,-0,1,-1,1.5,9007199254740993,1e300,-123456789],"c":{"2":"x","1":"ü€"},"d":{"a":1}}"#;
        let Val::Obj(o) = Val::from_json(&util::json::Json::parse(j).unwrap()) else { panic!() };
        let mut e = Enc::default();
        e.obj(&o);
        e.obj(&o);
        let mut d = Dec { b: &e.out, at: 0, keys: Vec::new() };
        for _ in 0..2 {
            let back = d.obj().unwrap();
            assert_eq!(back, o);
            let Some(Val::Arr(b)) = back.get(Atom::intern("b")) else { panic!() };
            assert!(matches!(b[3], Val::Num(z) if z == 0.0 && z.is_sign_negative()));
        }
        assert_eq!(d.at, e.out.len());
        let mut cut = Dec { b: &e.out[..e.out.len() - 1], at: 0, keys: Vec::new() };
        assert!(cut.obj().and_then(|_| cut.obj()).is_err());
    }

    #[test]
    fn the_writer_says_how_each_save_went() {
        let spec = crate::mapgen::WorldSpec { sectors_x: -1..0, sectors_y: -1..0, seed: 3, wide_coords: false, open: true, portals: None };
        let w = crate::mapgen::generate_world(&spec, &util::pool::Pool::new(2), &crate::hooks::EngineHooks::default());
        let dir = std::env::temp_dir().join(format!("krebs-writer-{}", std::process::id()));
        let blocked = dir.join("blocked");
        std::fs::create_dir_all(&dir).unwrap();
        // a file where the data folder should be fails the same way on every system
        std::fs::write(&blocked, "").unwrap();
        let (tx, heard) = std::sync::mpsc::channel();
        let writer = Writer::spawn(blocked.clone(), Profile::Private, move |saved| tx.send(saved.is_ok()).unwrap());
        assert!(writer.offer(w.view()));
        assert_eq!(heard.recv(), Ok(false));
        assert!(writer.finish(w.view()).is_err(), "the end of the run hears of a last save that failed");
        std::fs::remove_file(&blocked).unwrap();
        std::fs::create_dir_all(&blocked).unwrap();
        let (tx, heard) = std::sync::mpsc::channel();
        let writer = Writer::spawn(blocked, Profile::Private, move |saved| tx.send(saved.is_ok()).unwrap());
        assert!(writer.offer(w.view()));
        assert_eq!(heard.recv(), Ok(true));
        assert_eq!(writer.finish(w.view()), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A legacy db.json is migrated once; a damaged store falls back to the previous one.
    #[test]
    fn migrate_and_fall_back() {
        let spec = crate::mapgen::WorldSpec { sectors_x: -1..0, sectors_y: -1..0, seed: 3, wide_coords: false, open: true, portals: None };
        let mut w = crate::mapgen::generate_world(&spec, &util::pool::Pool::new(2), &crate::hooks::EngineHooks::default());
        let opts = ImportOpts { seed: 3, profile: Profile::Private, now_ms: 1, nominal_tick_ms: 1000 };
        let dir = std::env::temp_dir().join(format!("chitin-persist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        w.time = 10;
        std::fs::write(legacy_path(&dir), loki::export(&w.view(), Profile::Private)).unwrap();
        assert_eq!(load(&dir, &opts).unwrap().unwrap().0.time, 10);
        assert!(path(&dir).exists() && !legacy_path(&dir).exists() && dir.join("db.json.migrated").exists());

        w.time = 20;
        save(&dir, &w.view(), Profile::Private).unwrap();
        let mut bytes = std::fs::read(path(&dir)).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 1;
        std::fs::write(path(&dir), &bytes).unwrap();
        assert_eq!(load(&dir, &opts).unwrap().unwrap().0.time, 10);
        std::fs::remove_file(prev(&dir)).unwrap();
        assert!(load(&dir, &opts).unwrap().err().unwrap().contains("checksum"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
