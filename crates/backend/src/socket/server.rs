//! mirror: reference/backend/lib/game/socket/server.js
//! parity: DIVERGENT(SOCK-SUBSCRIBE): `user:<id>/*` and `mapVisual:<id>` need the same authenticated user (SEC fix),
//! each connection's messages run in order (SOCK-AUTH race fixed), gzip keeps message order (SOCK-GZIP),
//! listeners are routed by exact channel (no regex firehose)
//!
//! Greeting `time <ms>` + `protocol 14`; `auth <token>`; `gzip on|off`; subscribe/unsubscribe and the
//! channel table (ARCHITECTURE.md §8.4). Owner: WP04.

use super::{map, rooms, system, user};
use crate::{authlib, Published};
use engine::doc::DocRef;
use engine::world::{UserIdx, WorldView};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;
use util::json::Json;

/// A socket channel handler: called on subscribe and after every tick for each subscribed channel, with
/// the connection's user (None = unauthenticated) and the full channel name. `None` rejects the
/// subscription / sends nothing this tick; `Some(msg)` is pushed as `[channel, msg]`.
pub type ChannelFn = fn(Option<UserIdx>, &str, &WorldView) -> Option<Json>;

/// Built-in channels by name prefix; mods add theirs (`Registry::socket_channels`, `Published::mods`).
pub const CHANNELS: &[(&str, ChannelFn)] = &[
    ("room:", rooms::room),
    ("roomMap2:", map::room_map2),
    ("user:", user::user),
    ("server-message", system::server_message),
];

/// Per-connection send queue cap (ARCHITECTURE.md §8.1): beyond it event messages are dropped and the
/// connection's room baselines are reset, so the next push is a full `getDiff([], …)` snapshot.
pub const OUT_CAP: usize = 4 << 20;

/// What the writer thread sends next.
pub(crate) enum Out {
    /// One protocol message (framed `a[…]` on SockJS, one text frame on raw websockets).
    Text(String),
    Pong(Vec<u8>),
    Close(u16, String),
}

#[derive(Default)]
struct OutState {
    items: VecDeque<Out>,
    bytes: usize,
    closed: bool,
}

/// Bounded queue drained by the connection's writer thread; producers never block.
#[derive(Default)]
pub(crate) struct Outbox {
    st: Mutex<OutState>,
    cv: Condvar,
}

impl Outbox {
    fn lock(&self) -> MutexGuard<'_, OutState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// False when the queue is full (message dropped) or closed.
    pub(crate) fn push_text(&self, s: String) -> bool {
        let mut st = self.lock();
        if st.closed || (st.bytes + s.len() > OUT_CAP && !st.items.is_empty()) {
            return false;
        }
        st.bytes += s.len();
        st.items.push_back(Out::Text(s));
        self.cv.notify_one();
        true
    }

    pub(crate) fn push_control(&self, o: Out) {
        let mut st = self.lock();
        if !st.closed {
            st.items.push_back(o);
            self.cv.notify_one();
        }
    }

    pub(crate) fn close(&self) {
        self.lock().closed = true;
        self.cv.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn is_closed(&self) -> bool {
        self.lock().closed
    }

    /// Everything queued, waiting up to `timeout` for the first item: `Some(vec![])` on timeout, `None` once
    /// closed and empty.
    pub(crate) fn drain(&self, timeout: Duration) -> Option<Vec<Out>> {
        let mut st = self.lock();
        if st.items.is_empty() && !st.closed {
            st = self.cv.wait_timeout(st, timeout).unwrap_or_else(|e| e.into_inner()).0;
        }
        if st.items.is_empty() {
            return if st.closed { None } else { Some(vec![]) };
        }
        st.bytes = 0;
        Some(st.items.drain(..).collect())
    }
}

/// One socket connection (SockJS session or raw websocket).
pub(crate) struct Conn {
    pub id: u64,
    user: Mutex<Option<UserIdx>>,
    gzip: AtomicBool,
    pub out: Outbox,
    /// Pongs received (heartbeat).
    pub pongs: AtomicU64,
}

impl Conn {
    pub(crate) fn user(&self) -> Option<UserIdx> {
        *self.user.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `conn.write`: hello and auth replies, never gzipped.
    pub(crate) fn write_raw(&self, s: String) -> bool {
        self.out.push_text(s)
    }

    /// `conn._writeEventRaw`: deflated when gzip is on and that is shorter (SOCK-GZIP).
    pub(crate) fn write_event_raw(&self, message: String) -> bool {
        if self.gzip.load(Ordering::Relaxed) {
            let gz = format!("gz:{}", util::base64::encode(&util::deflate::zlib_compress(message.as_bytes())));
            if gz.len() < message.len() {
                return self.out.push_text(gz);
            }
        }
        self.out.push_text(message)
    }

    /// `conn._writeEvent(channel, data)` with `data` already JSON text.
    pub(crate) fn write_event(&self, channel: &str, data: &str) -> bool {
        let mut m = String::with_capacity(channel.len() + data.len() + 8);
        m.push('[');
        util::json::write_quoted(&mut m, channel);
        m.push(',');
        m.push_str(data);
        m.push(']');
        self.write_event_raw(m)
    }
}

/// One `room:<name>` subscriber (rooms.js `connectedToRooms[room][i]`).
pub(crate) struct RoomSub {
    pub conn: u64,
    pub user: UserIdx,
    /// The objects this subscriber last received (`i.objects`); diffs are taken against it.
    pub objects: Arc<[DocRef]>,
    /// Ids already sent in `users` (`i.users`).
    pub users: HashSet<Box<str>>,
}

/// One `user:<id>/memory/<path>` watch (user.js `connectedToMemory[user][path]`).
pub(crate) struct MemWatch {
    pub path: String,
    pub conns: Vec<u64>,
    /// Memory version last pushed (pushes happen only when Memory changed or the watch is new).
    pub sent: Option<u64>,
}

/// Subscription registry: one per backend, shared by connection threads, HTTP handlers (code/branch
/// events) and the broadcaster.
#[derive(Default)]
pub(crate) struct HubState {
    next_id: u64,
    pub conns: HashMap<u64, Arc<Conn>>,
    /// Exact channel → (connection, registerListener count) for pubsub-relayed channels.
    pub listeners: HashMap<String, Vec<(u64, u32)>>,
    pub rooms: BTreeMap<String, Vec<RoomSub>>,
    pub maps: BTreeMap<String, Vec<u64>>,
    pub memory: BTreeMap<UserIdx, Vec<MemWatch>>,
    pub resources: BTreeMap<UserIdx, Vec<u64>>,
    pub map_visual: BTreeMap<UserIdx, Vec<u64>>,
    /// rooms.js `usersLimit`, recounted on every broadcast.
    pub users_limit: HashMap<UserIdx, u32>,
    /// Visuals of the last broadcast tick: (user, room | "" | "map") → buffer.
    pub visuals: HashMap<(UserIdx, Box<str>), Arc<str>>,
}

#[derive(Default)]
pub(crate) struct Hub {
    st: Mutex<HubState>,
}

impl Hub {
    pub(crate) fn lock(&self) -> MutexGuard<'_, HubState> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn add_conn(&self) -> Arc<Conn> {
        let mut st = self.lock();
        st.next_id += 1;
        let c = Arc::new(Conn { id: st.next_id, user: Mutex::new(None), gzip: AtomicBool::new(false), out: Outbox::default(), pongs: AtomicU64::new(0) });
        st.conns.insert(c.id, c.clone());
        c
    }

    /// `conn.on('close')`: every listener and subscription of the connection goes.
    pub(crate) fn remove_conn(&self, id: u64) {
        let mut st = self.lock();
        if let Some(c) = st.conns.remove(&id) {
            c.out.close();
        }
        st.listeners.retain(|_, v| {
            v.retain(|(c, _)| *c != id);
            !v.is_empty()
        });
        st.rooms.values_mut().for_each(|v| v.retain(|s| s.conn != id));
        st.maps.values_mut().for_each(|v| v.retain(|c| *c != id));
        st.memory.values_mut().for_each(|w| w.iter_mut().for_each(|m| m.conns.retain(|c| *c != id)));
        st.resources.values_mut().for_each(|v| v.retain(|c| *c != id));
        st.map_visual.values_mut().for_each(|v| v.retain(|c| *c != id));
    }

    /// server.js `emit(channel, data)`: to every connection that registered this exact channel.
    pub(crate) fn emit(&self, channel: &str, data: &str) {
        self.lock().emit(channel, data);
    }
}

impl HubState {
    pub(crate) fn emit(&self, channel: &str, data: &str) {
        for (id, _) in self.listeners.get(channel).into_iter().flatten() {
            if let Some(c) = self.conns.get(id) {
                c.write_event(channel, data);
            }
        }
    }

    pub(crate) fn conn(&self, id: u64) -> Option<&Arc<Conn>> {
        self.conns.get(&id)
    }

    fn register(&mut self, channel: &str, conn: u64) {
        let v = self.listeners.entry(channel.to_string()).or_default();
        match v.iter_mut().find(|(c, _)| *c == conn) {
            Some((_, n)) => *n += 1,
            None => v.push((conn, 1)),
        }
    }

    fn unregister(&mut self, channel: &str, conn: u64) {
        if let Some(v) = self.listeners.get_mut(channel) {
            if let Some(i) = v.iter().position(|(c, _)| *c == conn) {
                v[i].1 -= 1;
                if v[i].1 == 0 {
                    v.remove(i);
                }
            }
            if v.is_empty() {
                self.listeners.remove(channel);
            }
        }
    }
}

/// `conn.write('time …'); conn.write('protocol 14')` (SOCK-HELLO).
pub(crate) fn hello(conn: &Conn) {
    conn.write_raw(format!("time {}", engine::policy::real_now_ms("CLK-INFRA")));
    conn.write_raw(format!("protocol {}", crate::server::PROTOCOL));
}

/// `conn.on('data')`: one client message, handled to completion before the next (SOCK-AUTH fix).
pub(crate) fn on_message(p: &Published, conn: &Arc<Conn>, message: &str) {
    if message.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return; // `.` in the vanilla regexes does not cross line terminators: nothing matches
    }
    if let Some(ch) = message.strip_prefix("subscribe ") {
        subscribe(p, conn, ch);
    }
    if let Some(ch) = message.strip_prefix("unsubscribe ") {
        unsubscribe(p, conn, ch);
    }
    if let Some(token) = message.strip_prefix("auth ") {
        let view = p.get();
        match authlib::check_token(token, &view) {
            Some(u) => {
                *conn.user.lock().unwrap_or_else(|e| e.into_inner()) = Some(u);
                p.stamp_online(u);
                conn.write_raw(format!("auth ok {}", authlib::gen_token(view.users[u as usize].id.as_str())));
            }
            None => {
                conn.write_raw("auth failed".into());
            }
        }
    }
    if message == "gzip on" {
        conn.gzip.store(true, Ordering::Relaxed);
    }
    if message == "gzip off" {
        conn.gzip.store(false, Ordering::Relaxed);
    }
}

/// The `[a-zA-Z0-9_-]+` room-name rule of rooms.js/map.js.
fn room_channel_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// `user:<id>/<rest>` or `mapVisual:<id>` for the connection's own user, else None.
fn own_user(view: &WorldView, user: Option<UserIdx>, id: &str) -> Option<UserIdx> {
    let u = user?;
    (view.users.get(u as usize)?.id.as_str() == id).then_some(u)
}

/// socket/server.js: every module's `onSubscribe` sees every channel, the built-ins first, then the mods'.
fn subscribe(p: &Published, conn: &Arc<Conn>, channel: &str) {
    builtin_subscribe(p, conn, channel);
    mod_subscribe(p, conn, channel);
}

/// A mod's channel (`ChannelFn`) whose prefix starts `channel`: `Some(data)` registers the connection (as a truthy
/// `onSubscribe` does) and sends `data` unless it is null; `None` declines. `user:<id>…` channels go to mods
/// only for that user's own connection, as to the built-ins (SOCK-SUBSCRIBE).
fn mod_subscribe(p: &Published, conn: &Arc<Conn>, channel: &str) {
    let channels = &p.mods().channels;
    if channels.is_empty() {
        return;
    }
    let view = p.get();
    let user = conn.user();
    if let Some(rest) = channel.strip_prefix("user:") {
        let id = rest.split_once('/').map_or(rest, |(id, _)| id);
        if own_user(&view, user, id).is_none() {
            return;
        }
    }
    for (_, f) in channels.iter().filter(|(prefix, _)| channel.starts_with(*prefix)) {
        let Some(data) = f(user, channel, &view) else { continue };
        p.hub().lock().register(channel, conn.id);
        if !matches!(data, Json::Null) {
            conn.write_event(channel, &data.stringify());
        }
    }
}

/// After each broadcast: every connection listening on a mod's channel gets that channel's `ChannelFn` result for
/// the new world (nothing when it is `None` or null).
pub(crate) fn push_mod_channels(p: &Published, hub: &HubState, view: &WorldView) {
    let channels = &p.mods().channels;
    if channels.is_empty() {
        return;
    }
    for (channel, listeners) in &hub.listeners {
        for (_, f) in channels.iter().filter(|(prefix, _)| channel.starts_with(*prefix)) {
            for (id, _) in listeners {
                let Some(conn) = hub.conns.get(id) else { continue };
                if let Some(data) = f(conn.user(), channel, view).filter(|d| !matches!(d, Json::Null)) {
                    conn.write_event(channel, &data.stringify());
                }
            }
        }
    }
}

fn builtin_subscribe(p: &Published, conn: &Arc<Conn>, channel: &str) {
    let user = conn.user();
    // system.js returns true and user.js's `async onSubscribe` returns a (truthy) Promise: two registerListener
    // calls, so it takes two `unsubscribe server-message` to stop the messages
    if channel == "server-message" {
        let mut hub = p.hub().lock();
        hub.register(channel, conn.id);
        hub.register(channel, conn.id);
        return;
    }
    // rooms.js
    if let Some(name) = channel.strip_prefix("room:") {
        if let (Some(u), true) = (user, room_channel_name(name)) {
            rooms::subscribe(p, conn, u, name);
        }
        return;
    }
    // user.js
    if let Some(rest) = channel.strip_prefix("user:") {
        let view = p.get();
        let Some((id, sub)) = rest.split_once('/') else { return };
        let Some(u) = own_user(&view, user, id) else { return };
        if let Some(path) = sub.strip_prefix("memory/") {
            if !path.is_empty() {
                user::watch_memory(&mut p.hub().lock(), u, path, conn.id);
            }
            return;
        }
        let mut hub = p.hub().lock();
        hub.register(channel, conn.id);
        if sub == "cpu" {
            let memory = &view.users[u as usize].memory;
            if memory.utf16_len() > 0 {
                hub.emit(channel, &format!("{{\"cpu\":0,\"memory\":{}}}", memory.utf16_len()));
            }
        }
        if sub == "resources" {
            hub.resources.entry(u).or_default().push(conn.id);
        }
        return;
    }
    if let Some(id) = channel.strip_prefix("mapVisual:") {
        let view = p.get();
        let Some(u) = own_user(&view, user, id) else { return };
        let mut hub = p.hub().lock();
        hub.register(channel, conn.id);
        hub.map_visual.entry(u).or_default().push(conn.id);
        let data = user::map_visual_json(&hub, u);
        hub.emit(channel, &data);
        return;
    }
    // map.js
    if let Some(name) = channel.strip_prefix("roomMap2:") {
        if user.is_some() && room_channel_name(name) {
            map::subscribe(p, conn, name);
        }
    }
}

fn unsubscribe(p: &Published, conn: &Arc<Conn>, channel: &str) {
    let mut hub = p.hub().lock();
    if let Some(name) = channel.strip_prefix("room:") {
        if let Some(v) = hub.rooms.get_mut(name) {
            v.retain(|s| s.conn != conn.id);
        }
    }
    if let Some(name) = channel.strip_prefix("roomMap2:") {
        if let Some(v) = hub.maps.get_mut(name) {
            v.retain(|c| *c != conn.id);
        }
    }
    if let Some(rest) = channel.strip_prefix("user:") {
        let view = p.get();
        if let Some((id, sub)) = rest.split_once('/') {
            if let Some(&u) = engine::doc::Id::new(id).and_then(|i| view.user_ids.get(&i)) {
                if let Some(path) = sub.strip_prefix("memory/") {
                    for w in hub.memory.get_mut(&u).into_iter().flatten().filter(|w| w.path == path) {
                        w.conns.retain(|c| *c != conn.id);
                    }
                }
                if sub == "resources" {
                    if let Some(v) = hub.resources.get_mut(&u) {
                        v.retain(|c| *c != conn.id);
                    }
                }
            }
        }
    }
    if let Some(id) = channel.strip_prefix("mapVisual:") {
        let view = p.get();
        if let Some(&u) = engine::doc::Id::new(id).and_then(|i| view.user_ids.get(&i)) {
            if let Some(v) = hub.map_visual.get_mut(&u) {
                v.retain(|c| *c != conn.id);
            }
        }
    }
    hub.unregister(channel, conn.id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbox_cap_and_close() {
        let o = Outbox::default();
        assert!(o.push_text("x".repeat(OUT_CAP))); // one oversized message still goes out alone
        assert!(!o.push_text("y".into()));
        let items = o.drain(Duration::from_millis(1)).unwrap();
        assert_eq!(items.len(), 1);
        assert!(o.push_text("z".into()));
        assert!(o.drain(Duration::from_millis(1)).unwrap().len() == 1);
        assert!(o.drain(Duration::from_millis(1)).unwrap().is_empty());
        o.close();
        assert!(o.drain(Duration::from_millis(1)).is_none());
        assert!(!o.push_text("w".into()));
    }

    #[test]
    fn gzip_only_when_shorter() {
        let hub = Hub::default();
        let c = hub.add_conn();
        c.gzip.store(true, Ordering::Relaxed);
        c.write_event("a", "1");
        let long = format!("\"{}\"", "abc".repeat(500));
        c.write_event("user:x/console", &long);
        let items = c.out.drain(Duration::from_millis(1)).unwrap();
        let texts: Vec<&str> = items.iter().map(|o| if let Out::Text(t) = o { t.as_str() } else { "" }).collect();
        assert_eq!(texts[0], "[\"a\",1]");
        let gz = texts[1].strip_prefix("gz:").unwrap();
        let raw = util::deflate::zlib_decompress(&util::base64::decode(gz).unwrap(), 1 << 20).unwrap();
        assert_eq!(String::from_utf8(raw).unwrap(), format!("[\"user:x/console\",{long}]"));
    }

    /// system.js + user.js's async onSubscribe both accept `server-message`: one unsubscribe is not enough.
    #[test]
    fn server_message_needs_two_unsubscribes() {
        let p = crate::Published::new(crate::fake::view(&crate::fake::world()));
        let c = p.hub().add_conn();
        on_message(&p, &c, "subscribe server-message");
        on_message(&p, &c, "unsubscribe server-message");
        system::send_server_message(&p, "maintenance");
        let got = c.out.drain(Duration::ZERO).unwrap();
        assert!(matches!(&got[..], [Out::Text(t)] if t == r#"["server-message","maintenance"]"#));
        on_message(&p, &c, "unsubscribe server-message");
        system::send_server_message(&p, "again");
        assert!(c.out.drain(Duration::ZERO).unwrap().is_empty());
    }

    #[test]
    fn listener_refcount_and_emit() {
        let hub = Hub::default();
        let a = hub.add_conn();
        let b = hub.add_conn();
        {
            let mut st = hub.lock();
            st.register("server-message", a.id);
            st.register("server-message", a.id);
            st.register("server-message", b.id);
            st.unregister("server-message", a.id);
        }
        hub.emit("server-message", "\"hi\"");
        assert_eq!(a.out.drain(Duration::ZERO).unwrap().len(), 1);
        hub.lock().unregister("server-message", a.id);
        hub.emit("server-message", "\"hi\"");
        assert!(a.out.drain(Duration::ZERO).unwrap().is_empty());
        assert_eq!(b.out.drain(Duration::ZERO).unwrap().len(), 2);
        hub.remove_conn(b.id);
        assert!(hub.lock().listeners.is_empty());
        assert!(b.out.is_closed());
    }
}
