//! Built-in `history` mod: room history (§5.8) and `/room-history`. Off by default in config. Owner: WP21.
//! Behaves like vanilla's example mod (reference/launcher/init_dist/example-mods/room-history.js): every
//! `saveRoomHistory` chunk is written to `<data>/replay-data/<room>/<base>.json`, and
//! `config.backend.onGetRoomHistory` serves that file as text (HTTP-ROOM-HISTORY), errors as HTTP 500.
//! Unlike the example, `room` and `time` must be a room name and a number: a path in either is refused
//! (as ENOENT) instead of reading outside replay-data.

use backend::http::{Request, Response};
use backend::mods::{Manifest, Mod, Registry};
use backend::{ApiCtx, Route};
use std::path::PathBuf;
use std::sync::OnceLock;
use util::json::Json;

pub struct History;
pub static MOD: History = History;

/// `path.resolve(__dirname, 'replay-data')`.
static DIR: OnceLock<PathBuf> = OnceLock::new();

impl Mod for History {
    fn manifest(&self) -> Manifest {
        Manifest { name: "history", version: env!("CARGO_PKG_VERSION"), dependencies: &[] }
    }
    fn register(&self, r: &mut Registry) {
        let _ = DIR.set(r.data_dir.join("replay-data"));
        r.engine.save_room_history.push(save_room_history);
        // `app.use('/room-history', …)`
        r.routes.push(Route { method: "GET", path: "/room-history", handler: get_room_history });
        r.routes.push(Route { method: "GET", path: "/room-history/*", handler: get_room_history });
    }
}

/// room-history.js:13 `config.engine.on('saveRoomHistory', …)`: mkdir as needed, then a fire-and-forget write.
fn save_room_history(room: &str, base: u64, data: &Json) {
    let Some(dir) = DIR.get() else { return };
    let dir = dir.join(room);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(format!("{base}.json")), data.stringify());
}

/// room-history.js:36 `onGetRoomHistory(roomName, baseTime, callback)`: the file as utf8 text, or the error.
fn get_room_history(req: &Request, _ctx: &ApiCtx) -> Response {
    let Some(dir) = DIR.get() else { return Response::text(500, "text/html; charset=utf-8", "not implemented") };
    // `path.resolve(dir, undefined)` throws a TypeError, which `res.send` serializes as `{}`
    let Some(room) = req.param("room") else { return Response::json(500, &Json::Obj(vec![])) };
    // `baseTime+'.json'`: a missing time reads "undefined.json"
    let time = req.param("time").unwrap_or("undefined");
    let file = dir.join(room).join(format!("{time}.json"));
    let safe = engine::room_name::RoomXY::parse_engine(room).is_some() && room.bytes().all(|b| b.is_ascii_alphanumeric()) && time.bytes().all(|b| b.is_ascii_digit());
    match safe.then(|| std::fs::read_to_string(&file)) {
        Some(Ok(text)) => Response::text(200, "text/html; charset=utf-8", &text),
        _ => {
            let err = Json::Obj(vec![
                ("errno".into(), Json::Num(-2.0)),
                ("code".into(), Json::Str("ENOENT".into())),
                ("syscall".into(), Json::Str("open".into())),
                ("path".into(), Json::Str(file.to_string_lossy().into())),
            ]);
            Response::json(500, &err)
        }
    }
}
