//! engine::policy against tools/oracle/lib/rng.js, vclock.js and the real lodash 3.10.1
//! draw helpers (fixture: node tools/oracle/gen/policy-fixture.js).

use engine::policy::*;
use util::json::Json;
use util::rng::Rng;

const FIXTURE: &str = include_str!("fixtures/policy/cases.ndjson");

fn hex(j: &Json) -> u64 {
    u64::from_str_radix(j.as_str().unwrap(), 16).unwrap()
}

fn s<'a>(c: &'a Json, k: &str) -> &'a str {
    c.get(k).and_then(Json::as_str).unwrap()
}

fn n(c: &Json, k: &str) -> f64 {
    c.get(k).and_then(Json::as_f64).unwrap()
}

fn arr<'a>(c: &'a Json, k: &str) -> &'a Vec<Json> {
    c.get(k).and_then(Json::as_array).unwrap()
}

fn nums(c: &Json, k: &str) -> Vec<f64> {
    arr(c, k).iter().map(|x| x.as_f64().unwrap()).collect()
}

#[test]
fn matches_rng_js() {
    let mut counts = std::collections::BTreeMap::new();
    for line in FIXTURE.lines() {
        let c = Json::parse(line).unwrap();
        let kind = s(&c, "kind");
        *counts.entry(kind.to_string()).or_insert(0) += 1;
        match kind {
            "fnv" => assert_eq!(fnv1a64(s(&c, "s").as_bytes()), hex(c.get("h").unwrap()), "{line}"),
            "mix" => assert_eq!(mix64(hex(c.get("z").unwrap())), hex(c.get("h").unwrap()), "{line}"),
            "seed" => {
                let parts: Vec<u64> = arr(&c, "parts").iter().map(hex).collect();
                assert_eq!(seed(hex(c.get("ws").unwrap()), s(&c, "tag"), &parts), hex(c.get("h").unwrap()), "{line}");
            }
            "stream" => {
                let mut r = Rng::new(hex(c.get("h").unwrap()));
                let next: Vec<u64> = arr(&c, "next").iter().map(hex).collect();
                assert_eq!((0..4).map(|_| r.next_u64()).collect::<Vec<_>>(), next, "{line}");
                assert_eq!((0..4).map(|_| r.next_f64()).collect::<Vec<_>>(), nums(&c, "random"), "{line}");
            }
            "streams" => {
                let (ws, t) = (hex(c.get("ws").unwrap()), n(&c, "T") as u64);
                let mut room = room_stream(ws, t, s(&c, "room"));
                assert_eq!((0..3).map(|_| room.random()).collect::<Vec<_>>(), nums(&c, "room_r"), "{line}");
                let mut global = global_stream(ws, t);
                assert_eq!((0..3).map(|_| global.next_f64()).collect::<Vec<_>>(), nums(&c, "global_r"), "{line}");
                let mut cron = cron_stream(ws, t, s(&c, "job"), s(&c, "unit"));
                assert_eq!((0..3).map(|_| cron.next_f64()).collect::<Vec<_>>(), nums(&c, "cron_r"), "{line}");
            }
            "order" => {
                let (ws, t) = (hex(c.get("ws").unwrap()), n(&c, "T") as u64);
                let mut uids: Vec<&str> = arr(&c, "uids").iter().map(|u| u.as_str().unwrap()).collect();
                let keys: Vec<u64> = arr(&c, "keys").iter().map(hex).collect();
                assert_eq!(uids.iter().map(|u| order_key(ws, t, u)).collect::<Vec<_>>(), keys, "{line}");
                sort_users(ws, t, &mut uids, |u| u);
                let sorted: Vec<&str> = arr(&c, "sorted").iter().map(|u| u.as_str().unwrap()).collect();
                assert_eq!(uids, sorted, "{line}");
            }
            "random" => {
                let mut r = Rng::new(hex(c.get("h").unwrap()));
                for call in arr(&c, "calls") {
                    let a = call.as_array().unwrap();
                    let got = lodash_random(&mut r, a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_bool().unwrap());
                    assert_eq!(got, a[3].as_f64().unwrap(), "{line}");
                }
                assert_eq!(r.next_f64(), n(&c, "after"), "draw count: {line}");
            }
            "shuffle" => {
                let mut r = Rng::new(hex(c.get("h").unwrap()));
                assert_eq!(lodash_shuffle(&mut r, &nums(&c, "a")), nums(&c, "out"), "{line}");
                assert_eq!(r.next_f64(), n(&c, "after"), "draw count: {line}");
            }
            "sample" => {
                let mut r = Rng::new(hex(c.get("h").unwrap()));
                let a = nums(&c, "a");
                assert_eq!(lodash_sample(&mut r, &a).copied(), c.get("out").unwrap().as_f64(), "{line}");
                assert_eq!(r.next_f64(), n(&c, "after"), "draw count: {line}");
            }
            "vclock" => {
                let clk =
                    |j: &Json| VClock { base_ms: n(j, "baseMs") as i64, base_tick: n(j, "baseTick") as u64, nominal_tick_ms: n(j, "nominalTickMs") as u32 };
                let mut clock = clk(c.get("clock").unwrap());
                let t = n(&c, "T") as u64;
                assert_eq!(clock.at(t) as f64, n(&c, "at"), "{line}");
                clock.rebase(n(&c, "rebaseT") as u64, n(&c, "nominal") as u32);
                assert_eq!(clock, clk(c.get("after").unwrap()), "{line}");
                assert_eq!(clock.at(t) as f64, n(&c, "at2"), "{line}");
            }
            other => panic!("unknown case kind {other}"),
        }
    }
    assert!(counts.values().all(|&c| c >= 50), "{counts:?}");
}
