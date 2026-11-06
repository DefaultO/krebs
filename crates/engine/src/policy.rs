//! Spec 01 §16 and §19A: ORD-POLICY, RNG-SEED, CLOCK-POLICY, COMPAT-TABLE. Nothing else
//! reads a clock or an unseeded RNG for game state.

use crate::doc::array_index;
use util::rng::Rng;

// ---- RNG-SEED ----

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

pub fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// `seed(tag, parts…)`: parts are ticks, or `fnv1a64(name)` for names.
pub fn seed(world_seed: u64, tag: &str, parts: &[u64]) -> u64 {
    parts.iter().fold(mix64(world_seed ^ fnv1a64(tag.as_bytes())), |h, &p| mix64(h ^ p))
}

/// Room stream of (room, T), created on its first draw.
pub fn room_stream(world_seed: u64, t: u64, room: &str) -> LazyRng {
    LazyRng::new(seed(world_seed, "room", &[t, fnv1a64(room.as_bytes())]))
}

/// Global stream of T (ORD-POINTS 4).
pub fn global_stream(world_seed: u64, t: u64) -> Rng {
    Rng::new(seed(world_seed, "global", &[t]))
}

/// Cron stream of (job, G, unit).
pub fn cron_stream(world_seed: u64, g: u64, job: &str, unit: &str) -> Rng {
    Rng::new(seed(world_seed, "cron", &[g, fnv1a64(job.as_bytes()), fnv1a64(unit.as_bytes())]))
}

/// A stream seeded only when first drawn from (`util::rng::Rng::new(h)` is `stream(h)`).
pub struct LazyRng {
    seed: u64,
    rng: Option<Rng>,
}

impl LazyRng {
    pub fn new(seed: u64) -> LazyRng {
        LazyRng { seed, rng: None }
    }

    pub fn rng(&mut self) -> &mut Rng {
        let seed = self.seed;
        self.rng.get_or_insert_with(|| Rng::new(seed))
    }

    /// `Math.random()`.
    pub fn random(&mut self) -> f64 {
        self.rng().next_f64()
    }
}

// lodash 3.10.1 draw helpers. Draw counts must match exactly (RNG-SITES), so these
// follow lodash.js statement by statement. `Rng::shuffle` is not lodash-compatible.

/// lodash.js:2501 baseRandom: one draw.
fn base_random(rng: &mut Rng, min: f64, max: f64) -> f64 {
    min + (rng.next_f64() * (max - min + 1.0)).floor()
}

/// `_.random(min, max, floating)` with both bounds given (lodash.js:10165): one draw.
pub fn lodash_random(rng: &mut Rng, min: f64, max: f64, floating: bool) -> f64 {
    let or0 = |x: f64| if x.is_nan() || x == 0.0 { 0.0 } else { x }; // `+x || 0`
    let (min, max) = (or0(min), or0(max));
    if floating || min % 1.0 != 0.0 || max % 1.0 != 0.0 {
        let rand = rng.next_f64();
        let digits = util::num::to_js_string(rand).len() - 1;
        let eps: f64 = format!("1e-{digits}").parse().unwrap(); // parseFloat('1e-' + …)
        let v = min + rand * (max - min + eps);
        return if v.is_nan() || max.is_nan() { f64::NAN } else { v.min(max) };
    }
    base_random(rng, min, max)
}

/// `_.shuffle(a)`: `a.len()` draws (the last index draws too).
pub fn lodash_shuffle<T: Clone>(rng: &mut Rng, a: &[T]) -> Vec<T> {
    let mut out = a.to_vec();
    let last = out.len() as f64 - 1.0;
    for i in 0..out.len() {
        let r = base_random(rng, i as f64, last) as usize;
        out.swap(i, r);
    }
    out
}

/// `_.sample(a)`: one draw, none for an empty slice.
pub fn lodash_sample<'a, T>(rng: &mut Rng, a: &'a [T]) -> Option<&'a T> {
    if a.is_empty() {
        return None;
    }
    let i = base_random(rng, 0.0, a.len() as f64 - 1.0) as usize;
    Some(&a[i])
}

// ---- ORD-POLICY ----

/// ORD-USER-KEY: the rank key of player `uid` in tick T.
pub fn order_key(world_seed: u64, t: u64, uid: &str) -> u64 {
    seed(world_seed, "user-order", &[t, fnv1a64(uid.as_bytes())])
}

/// ORD-POINTS 1: integer-like ids (NPCs "2", "3") ascending first, then players by
/// `(order_key, uid bytes)`.
pub fn sort_users<T>(world_seed: u64, t: u64, items: &mut [T], uid: impl Fn(&T) -> &str) {
    items.sort_by_cached_key(|x| {
        let u = uid(x);
        match array_index(u) {
            Some(i) => (0u8, i as u64, Vec::new()),
            None => (1, order_key(world_seed, t, u), u.as_bytes().to_vec()),
        }
    });
}

// ---- CLOCK-VIRTUAL / CLOCK-POLICY ----

/// `vclock(T) = baseMs + (T − baseTick) × nominalTickMs` (persisted world state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VClock {
    pub base_ms: i64,
    pub base_tick: u64,
    pub nominal_tick_ms: u32,
}

impl VClock {
    pub fn at(&self, t: u64) -> i64 {
        self.base_ms + (t as i64 - self.base_tick as i64) * self.nominal_tick_ms as i64
    }

    /// Switches the rate at tick `t`, keeping the clock continuous.
    pub fn rebase(&mut self, t: u64, nominal_tick_ms: u32) {
        *self = VClock { base_ms: self.at(t), base_tick: t, nominal_tick_ms };
    }
}

/// The only wall-clock read for game-adjacent code. `row` names the CLOCK-POLICY row
/// that allows real time at the call site.
pub fn real_now_ms(row: &'static str) -> i64 {
    debug_assert!(matches!(row, "CLK-NOTIFY" | "CLK-INFRA" | "CLK-API" | "CLOCK-VIRTUAL" | "ENV-EXPIRY"), "{row} is not a real-time CLOCK-POLICY row");
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

// ---- COMPAT-TABLE ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Private,
    Mmo,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Profile> {
        match s {
            "private" => Some(Profile::Private),
            "mmo" => Some(Profile::Mmo),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Profile::Private => "private",
            Profile::Mmo => "mmo",
        }
    }
}

/// R: replicate everywhere; F: fix everywhere; C: the profile decides (private
/// replicates, mmo fixes) unless overridden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    R,
    F,
    C,
}

/// Spec 01 §19A COMPAT-TABLE (a test keeps this in step with the spec).
pub const COMPAT_TABLE: &[(&str, Decision)] = &[
    ("BUG-DIE-DOUBLE", Decision::R),
    ("BUG-INSERT-COUNTER", Decision::F),
    ("BUG-DOTKEY-INC", Decision::F),
    ("BUG-CRASH-WITHDRAW", Decision::C),
    ("BUG-CRASH-OPEXT", Decision::C),
    ("BUG-CRASH-NUKE-PB", Decision::C),
    ("BUG-CRASH-SH-PC", Decision::C),
    ("BUG-CRASH-CANCEL-SPAWN", Decision::C),
    ("BUG-CRASH-REVERSE-LAB", Decision::C),
    ("BUG-CRASH-CALC-SPAWNS", Decision::C),
    ("BUG-CRASH-FLAG-FIRST", Decision::C),
    ("BUG-CRASH-UNDEF-ROOM", Decision::F),
    ("BUG-CRASH-MKT-USER", Decision::F),
    ("BUG-FLAGS-REPARSE", Decision::C),
    ("BUG-DEPOSIT-REGEX", Decision::C),
    ("BUG-WORLDSIZE", Decision::R),
    ("BUG-SH-HIGHWAY-REGEX", Decision::C),
    ("BUG-CALC-REWARD", Decision::R),
    ("BUG-PC-DEATH-CONTAINER", Decision::R),
    ("BUG-DISMANTLE-NAN", Decision::R),
    ("BUG-STATUSDATA-COLD", Decision::F),
    ("ID-SHARD-NAME", Decision::C),
    ("ID-PC-SHARD", Decision::C),
    ("ID-LENGTH", Decision::C),
    ("ID-FOREIGN-SEG0", Decision::C),
];

#[derive(Clone, Debug)]
pub struct Compat {
    pub profile: Profile,
    overrides: Vec<(&'static str, bool)>,
}

impl Compat {
    pub fn new(profile: Profile) -> Compat {
        Compat { profile, overrides: Vec::new() }
    }

    fn row(id: &str) -> (&'static str, Decision) {
        *COMPAT_TABLE.iter().find(|(r, _)| *r == id).unwrap_or_else(|| panic!("unknown COMPAT-TABLE row {id}"))
    }

    /// `compat.override.<ID> = "replicate" | "fix"`; only C rows can be overridden.
    pub fn set_override(&mut self, id: &str, fix: bool) -> Result<(), String> {
        match COMPAT_TABLE.iter().find(|(r, _)| *r == id) {
            Some(&(r, Decision::C)) => {
                self.overrides.retain(|(o, _)| *o != r);
                self.overrides.push((r, fix));
                Ok(())
            }
            Some(_) => Err(format!("{id} is not configurable")),
            None => Err(format!("unknown COMPAT-TABLE row {id}")),
        }
    }

    /// Whether row `id` takes its fixed behaviour. Panics on an unknown row (typo guard).
    pub fn fix(&self, id: &str) -> bool {
        match Compat::row(id) {
            (_, Decision::R) => false,
            (_, Decision::F) => true,
            (r, Decision::C) => self.overrides.iter().find(|(o, _)| *o == r).map_or(self.profile == Profile::Mmo, |&(_, f)| f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_spec() {
        let spec = include_str!("../../../docs/spec/01-tick-loop.md");
        let start = spec.find("### COMPAT-TABLE").unwrap();
        let rows: Vec<(String, String)> = spec[start..]
            .lines()
            .skip_while(|l| !l.starts_with("| ID"))
            .skip(2)
            .take_while(|l| l.starts_with('|'))
            .map(|l| {
                let l = l.replace("\\|", "/"); // escaped pipes inside cells
                let c: Vec<&str> = l.split('|').map(str::trim).collect();
                (c[1].to_string(), c[4].trim_matches('*').to_string())
            })
            .collect();
        let ours: Vec<(String, String)> = COMPAT_TABLE.iter().map(|(id, d)| (id.to_string(), format!("{d:?}"))).collect();
        assert_eq!(ours, rows);
    }

    /// ARCHITECTURE §2.7: wall-clock reads only through `real_now_ms`; game state never
    /// shuffles with `Rng::shuffle` (not lodash's draw pattern).
    #[test]
    fn no_stray_clock_or_shuffle() {
        fn rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    rs_files(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        for c in std::fs::read_dir(&crates).unwrap().flatten() {
            if c.path().join("src").is_dir() {
                rs_files(&c.path().join("src"), &mut files);
            }
        }
        let clock = ["SystemTime", "::now()"].concat();
        let shuffle = [".shuffle", "(&mut"].concat();
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            let p = f.to_string_lossy().replace('\\', "/");
            assert!(p.ends_with("engine/src/policy.rs") || !text.contains(&clock), "{p} reads the wall clock; use policy::real_now_ms");
            assert!(!p.contains("engine/src/") || !text.contains(&shuffle), "{p} uses Rng::shuffle; use policy::lodash_shuffle");
        }
    }

    #[test]
    fn compat_decisions() {
        let mut private = Compat::new(Profile::Private);
        let mmo = Compat::new(Profile::Mmo);
        assert!(!private.fix("BUG-DIE-DOUBLE") && !mmo.fix("BUG-DIE-DOUBLE"));
        assert!(private.fix("BUG-INSERT-COUNTER") && mmo.fix("BUG-INSERT-COUNTER"));
        assert!(!private.fix("BUG-CRASH-WITHDRAW") && mmo.fix("BUG-CRASH-WITHDRAW"));
        private.set_override("BUG-CRASH-WITHDRAW", true).unwrap();
        assert!(private.fix("BUG-CRASH-WITHDRAW"));
        assert!(private.set_override("BUG-WORLDSIZE", true).is_err());
        assert!(private.set_override("BUG-NOPE", true).is_err());
        assert!(std::panic::catch_unwind(|| mmo.fix("BUG-TYPO")).is_err());
        assert_eq!(Profile::parse("mmo"), Some(Profile::Mmo));
    }

    #[test]
    fn vclock() {
        let mut c = VClock { base_ms: 1_000_000, base_tick: 10, nominal_tick_ms: 1000 };
        assert_eq!(c.at(10), 1_000_000);
        assert_eq!(c.at(15), 1_005_000);
        assert_eq!(c.at(5), 995_000);
        c.rebase(20, 50);
        assert_eq!((c.at(20), c.at(22)), (1_010_000, 1_010_100));
    }

    #[test]
    fn lazy_and_user_order() {
        let mut lazy = room_stream(1, 5, "W1N1");
        assert!(lazy.rng.is_none());
        let a = lazy.random();
        assert_eq!(a, Rng::new(seed(1, "room", &[5, fnv1a64(b"W1N1")])).next_f64());
        let mut users = vec!["b", "3", "a", "2", "10"];
        sort_users(7, 1, &mut users, |u| u);
        assert_eq!(&users[..3], &["2", "3", "10"]);
        let players = &users[3..];
        assert!(order_key(7, 1, players[0]) <= order_key(7, 1, players[1]));
    }
}
