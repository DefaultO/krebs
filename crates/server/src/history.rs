//! mirror: reference/driver/lib/history.js
//! parity: FAITHFUL
//!
//! Room history (POST-HISTORY, FMT-HISTORY-CHUNK): `saveTick` into the `roomHistory:<room>` hash, `upload` of the
//! previous chunk with `common.getDiff` between consecutive ticks, then the `saveRoomHistory` engine event. The
//! caller's chaining is processor.js:522-536 `saveRoomHistory` (`save`). Kept in memory like vanilla's env (Redis):
//! when the base tick of a chunk was not saved, the hash is not deleted and stale ticks accumulate (R).

use engine::doc::{Obj, Val};
use std::collections::HashMap;
use util::json::Json;

/// `fn(roomId, baseTime, result)` listeners of `config.engine.emit('saveRoomHistory', …)`.
pub type Sink = fn(&str, u64, &Json);

/// One room's saveRoomHistory call.
pub struct Tick {
    pub room: Box<str>,
    pub game_time: u64,
    pub data: Box<str>,
    pub chunk: u64,
    pub now_ms: i64,
}

/// The writer thread: calls run in send order, one at a time (vanilla's process-global promise chain).
pub fn spawn(sinks: Vec<Sink>) -> std::sync::mpsc::Sender<Tick> {
    let (tx, rx) = std::sync::mpsc::channel::<Tick>();
    std::thread::Builder::new()
        .name("history".into())
        .spawn(move || {
            let mut h = History::default();
            for t in rx {
                h.save(&t.room, t.game_time, t.data, t.chunk, t.now_ms, &sinks);
            }
        })
        .expect("spawn the history thread");
    tx
}

#[derive(Default)]
pub struct History {
    /// `env.keys.ROOM_HISTORY + roomId` → hash `{"<gameTime>": JSON}`.
    hashes: HashMap<Box<str>, HashMap<String, Box<str>>>,
}

impl History {
    /// processor.js:522 saveRoomHistory(roomId, objects, gameTime) with `objects` already stringified.
    pub fn save(&mut self, room: &str, game_time: u64, data: Box<str>, chunk: u64, now_ms: i64, sinks: &[Sink]) {
        if chunk == 0 || game_time % chunk == 0 {
            // `Math.floor((gameTime - 1) / chunk) * chunk`
            let base = ((game_time as f64 - 1.0) / chunk as f64).floor() * chunk as f64;
            self.upload(room, base, now_ms, sinks);
        }
        self.save_tick(room, game_time, data);
    }

    /// history.js:7 saveTick: `env.hmset(ROOM_HISTORY + roomId, {[gameTime]: data})`.
    fn save_tick(&mut self, room: &str, game_time: u64, data: Box<str>) {
        self.hashes.entry(room.into()).or_default().insert(game_time.to_string(), data);
    }

    /// history.js:11 upload(roomId, baseTime).
    fn upload(&mut self, room: &str, base_time: f64, now_ms: i64, sinks: &[Sink]) {
        let Some(data) = self.hashes.get(room) else { return };
        let key = |t: f64| util::num::to_js_string(t);
        let Some(first) = data.get(&key(base_time)) else { return };

        let mut cur_tick = base_time;
        let mut cur_objects = parse(first);
        let mut ticks: Vec<(String, Json)> = vec![(key(cur_tick), cur_objects.clone())];

        cur_tick += 1.0;
        while let Some(text) = data.get(&key(cur_tick)) {
            let objects = parse(text);
            let diff = engine::common::get_diff(&values(&cur_objects), &values(&objects));
            ticks.push((key(cur_tick), diff));
            cur_objects = objects;
            cur_tick += 1.0;
        }

        let result = Json::Obj(vec![
            ("timestamp".into(), Json::Num(now_ms as f64)),
            ("room".into(), Json::Str(room.into())),
            ("base".into(), Json::Num(base_time)),
            ("ticks".into(), Json::Obj(ticks)),
        ]);
        for f in sinks {
            f(room, base_time as u64, &result);
        }
        self.hashes.remove(room);
    }
}

/// `JSON.parse` of a stored tick (an object keyed by `_id`).
fn parse(text: &str) -> Json {
    Json::parse(text).unwrap_or(Json::Null)
}

/// The documents `_.forEach(data, …)` visits in getDiff: an object's values (non-objects are skipped, they
/// have no `_id` a room object would have).
fn values(data: &Json) -> Vec<Obj> {
    match data {
        Json::Obj(m) => m.iter().filter_map(|(_, v)| match Val::from_json(v) {
            Val::Obj(o) => Some(o),
            _ => None,
        }).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static GOT: Mutex<Vec<(String, u64, String)>> = Mutex::new(Vec::new());
    fn sink(room: &str, base: u64, result: &Json) {
        GOT.lock().unwrap().push((room.into(), base, result.stringify()));
    }

    /// Chunk of 20: ticks 20..=39 saved, the upload at 40 emits base 20 with full + diffs and clears the hash;
    /// a room whose base tick is missing keeps accumulating.
    #[test]
    fn chunk_upload() {
        let mut h = History::default();
        let tick = |x: u32| format!(r#"{{"a":{{"_id":"a","type":"creep","x":{x},"y":1}}}}"#).into_boxed_str();
        for t in 20..40 {
            h.save("W1N1", t, tick(t as u32 % 3), 20, 7, &[sink]);
        }
        h.save("W2N1", 21, tick(1), 20, 7, &[sink]);
        h.save("W1N1", 40, tick(9), 20, 7, &[sink]);
        h.save("W2N1", 40, tick(2), 20, 7, &[sink]);
        let got = GOT.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        let (room, base, json) = &got[0];
        assert_eq!((room.as_str(), *base), ("W1N1", 20));
        assert!(json.starts_with(r#"{"timestamp":7,"room":"W1N1","base":20,"ticks":{"20":{"a":{"_id":"a","type":"creep","x":2,"y":1}},"21":{"a":{"x":0}},"22":{"a":{"x":1}}"#), "{json}");
        assert!(json.contains(r#""39":{"a":{"x":0}}}"#), "{json}");
        assert_eq!(h.hashes["W1N1"].len(), 1, "only tick 40 remains");
        assert_eq!(h.hashes["W2N1"].len(), 2, "no base 20: nothing uploaded or deleted");
    }
}

/// Replays crates/server/tests/fixtures/oracle/history (`node tools/oracle/history.js gen …`: the official
/// history.js and processor.js saveRoomHistory over the real storage env): the same `saveRoomHistory` events in
/// the same order, and the same hash fields left behind.
#[cfg(test)]
mod oracle {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static EVENTS: RefCell<Vec<Json>> = const { RefCell::new(Vec::new()) };
    }
    fn sink(room: &str, base: u64, result: &Json) {
        EVENTS.with(|e| e.borrow_mut().push(Json::Arr(vec![Json::Str(room.into()), Json::Num(base as f64), result.clone()])));
    }

    #[test]
    fn history_oracle() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oracle/history");
        let mut n = 0;
        for f in std::fs::read_dir(&dir).unwrap() {
            let text = std::fs::read_to_string(f.unwrap().path()).unwrap();
            for line in text.lines().filter(|l| !l.trim().is_empty()) {
                let case = Json::parse(line).unwrap();
                let name = case.get("name").and_then(Json::as_str).unwrap().to_string();
                let s = case.get("scenario").unwrap();
                let chunk = s.get("chunk").and_then(Json::as_f64).unwrap() as u64;
                EVENTS.with(|e| e.borrow_mut().clear());
                let mut h = History::default();
                for c in s.get("calls").and_then(Json::as_array).unwrap() {
                    let room = c.get("room").and_then(Json::as_str).unwrap();
                    let t = c.get("gameTime").and_then(Json::as_f64).unwrap() as u64;
                    let now = c.get("nowMs").and_then(Json::as_f64).unwrap() as i64;
                    h.save(room, t, c.get("objects").unwrap().stringify().into(), chunk, now, &[sink]);
                }
                let r = case.get("result").unwrap();
                let events = Json::Arr(EVENTS.with(|e| e.borrow().clone()));
                assert_eq!(events.stringify(), r.get("events").unwrap().stringify(), "{name}: events");
                let mut left: Vec<(String, Json)> = h
                    .hashes
                    .iter()
                    .filter(|(_, m)| !m.is_empty())
                    .map(|(room, m)| {
                        let mut ticks: Vec<u64> = m.keys().map(|k| k.parse().unwrap()).collect();
                        ticks.sort_unstable();
                        (format!("roomHistory:{room}"), Json::Arr(ticks.iter().map(|t| Json::Str(t.to_string())).collect()))
                    })
                    .collect();
                left.sort_by(|a, b| a.0.cmp(&b.0));
                assert_eq!(Json::Obj(left).stringify(), r.get("hashes").unwrap().stringify(), "{name}: hashes");
                n += 1;
            }
        }
        assert!(n > 0, "no history fixtures in {}", dir.display());
    }
}
