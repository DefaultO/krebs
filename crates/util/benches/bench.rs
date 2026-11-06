//! Throughput and overhead numbers for util. Run: cargo bench -p util

use std::hint::black_box;
use std::time::{Duration, Instant};
use util::json::Json;
use util::pool::Pool;
use util::rng::Rng;
use util::{deflate, num};

/// Best time of `runs` calls of `f` (dropping the result is not timed).
fn best<T>(runs: usize, mut f: impl FnMut() -> T) -> Duration {
    (0..runs)
        .map(|_| {
            let t = Instant::now();
            let r = black_box(f());
            let d = t.elapsed();
            drop(r);
            d
        })
        .min()
        .unwrap()
}

fn mbps(bytes: usize, d: Duration) -> f64 {
    bytes as f64 / d.as_secs_f64() / 1e6
}

/// A Screeps Memory-like document of roughly `target` bytes of JSON.
fn memory_doc(target: usize) -> Json {
    let mut rng = Rng::new(1);
    let roles = ["harvester", "upgrader", "builder", "hauler", "claimer"];
    let room = |rng: &mut Rng| Json::Str(format!("W{}N{}", rng.below(60), rng.below(60)));
    let mut creeps = Vec::new();
    let mut size = 0;
    while size < target * 8 / 10 {
        let path: String = (0..rng.below(50)).map(|_| char::from(b'1' + rng.below(8) as u8)).collect();
        let c = Json::Obj(vec![
            ("role".into(), Json::Str(roles[rng.below(5) as usize].into())),
            ("room".into(), room(&mut rng)),
            ("working".into(), Json::Bool(rng.below(2) == 0)),
            ("target".into(), if rng.below(4) == 0 { Json::Null } else { Json::Str(format!("{:015x}", rng.next_u64() >> 4)) }),
            (
                "_move".into(),
                Json::Obj(vec![
                    (
                        "dest".into(),
                        Json::Obj(vec![("x".into(), Json::Num(rng.below(50) as f64)), ("y".into(), Json::Num(rng.below(50) as f64)), ("room".into(), room(&mut rng))]),
                    ),
                    ("time".into(), Json::Num(60_000_000.0 + rng.below(100_000) as f64)),
                    ("path".into(), Json::Str(path)),
                ]),
            ),
            ("cost".into(), Json::Num(rng.next_f64() * 1000.0)),
        ]);
        size += c.stringify().len() + 20;
        creeps.push((format!("{}_{}", roles[rng.below(5) as usize], creeps.len()), c));
    }
    let stats: Vec<(String, Json)> = (0..2000)
        .map(|i| {
            let v = Json::Obj(vec![("cpu".into(), Json::Num(rng.next_f64() * 50.0)), ("bucket".into(), Json::Num(rng.below(10_001) as f64))]);
            (format!("{}", 60_000_000 + i * 10), v)
        })
        .collect();
    Json::Obj(vec![("creeps".into(), Json::Obj(creeps)), ("stats".into(), Json::Obj(stats)), ("version".into(), Json::Str("1.0".into()))])
}

fn main() {
    // JSON
    let text = memory_doc(2_400_000).stringify();
    let doc = Json::parse(&text).unwrap();
    let parse = best(20, || Json::parse(&text).unwrap());
    let stringify = best(20, || doc.stringify());
    let pretty = best(10, || doc.stringify_pretty(2));
    // clone() makes exactly the allocations parse makes: the allocator's share.
    let clone = best(20, || doc.clone());
    println!("json: {:.2} MB doc", text.len() as f64 / 1e6);
    println!("  parse      {:7.1} MB/s ({:?})", mbps(text.len(), parse), parse);
    println!("  clone      {:7.1} MB/s ({:?}) <- allocation floor for parse", mbps(text.len(), clone), clone);
    println!("  stringify  {:7.1} MB/s ({:?})", mbps(text.len(), stringify), stringify);
    println!("  pretty(2)  {:7.1} MB/s of input ({:?})", mbps(text.len(), pretty), pretty);

    // num
    let mut rng = Rng::new(2);
    let xs: Vec<f64> = (0..100_000).map(|_| (rng.next_f64() - 0.5) * 10f64.powi(rng.range(-10, 20) as i32)).collect();
    let mut s = String::new();
    let t = best(5, || {
        s.clear();
        for &x in &xs {
            num::write_js_string(&mut s, x);
        }
        s.len()
    });
    println!("num: to_js_string {:.0} ns/op (non-integers)", t.as_nanos() as f64 / xs.len() as f64);
    let t = best(5, || xs.iter().map(|&x| num::to_fixed(x, 2).len()).sum::<usize>());
    println!("num: to_fixed(2)  {:.0} ns/op", t.as_nanos() as f64 / xs.len() as f64);

    // deflate
    let raw = text.as_bytes();
    let comp = deflate::deflate(raw);
    let dt = best(5, || deflate::deflate(raw));
    let it = best(10, || deflate::inflate(&comp, usize::MAX).unwrap());
    println!("deflate: {} -> {} bytes ({:.1}%)", raw.len(), comp.len(), 100.0 * comp.len() as f64 / raw.len() as f64);
    println!("  deflate    {:7.1} MB/s", mbps(raw.len(), dt));
    println!("  inflate    {:7.1} MB/s (output)", mbps(raw.len(), it));
    println!("  crc32      {:7.1} MB/s", mbps(raw.len(), best(10, || deflate::crc32(raw))));
    println!("  adler32    {:7.1} MB/s", mbps(raw.len(), best(10, || deflate::adler32(raw))));

    // pool
    for threads in [4, 16] {
        let pool = Pool::new(threads);
        let calls = 20_000;
        let empty = best(5, || {
            for _ in 0..calls {
                pool.for_each(threads, |i| {
                    black_box(i);
                });
            }
        });
        let tiny = best(5, || {
            for _ in 0..calls / 10 {
                pool.for_each(10_000, |i| {
                    black_box(i);
                });
            }
        });
        println!(
            "pool({threads}): {:.2} us/call (n = threads, empty items), {:.2} us/call (10k tiny items)",
            empty.as_secs_f64() * 1e6 / calls as f64,
            tiny.as_secs_f64() * 1e6 / (calls / 10) as f64
        );
    }
}
