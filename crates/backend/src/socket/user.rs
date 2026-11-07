//! mirror: reference/backend/lib/game/socket/user.js
//! parity: DIVERGENT(SOCK-USER-MEMORY): memory watches push only when Memory changed (or the watch is new);
//! inherited Object.prototype members on memory paths read as undefined; cpu is not globally throttled
//! (SOCK-USER-CPU); subscriptions need the same authenticated user (SOCK-SUBSCRIBE SEC)
//!
//! `user:<id>/console|cpu|code|set-active-branch|newMessage|memory/<path>|resources|money`, authenticated
//! subscriptions only (ARCHITECTURE.md §8.4). Owner: WP04.

use crate::socket::server::{add_once, HubState, MemWatch};
use engine::world::{UserIdx, WorldView};
use std::borrow::Cow;
use util::json::Json;

/// Memory watches kept per user. user.js only ever serves the first 50, so keeping more buys nothing.
pub(crate) const MAX_WATCHES: usize = 50;
/// Longest memory path kept, in bytes: far past any path a person types, and all of them are sent back on every push.
pub(crate) const MAX_WATCH_PATH: usize = 512;

/// The message sent right after subscribing, if any: `user:<id>/cpu` gets `{cpu:0, memory:<length>}` when
/// Memory is non-empty. (Acceptance — own user only — is decided by the socket server.)
pub fn user(user: Option<UserIdx>, channel: &str, view: &WorldView) -> Option<Json> {
    let u = user?;
    let st = view.users.get(u as usize)?;
    let rest = channel.strip_prefix("user:")?.strip_prefix(st.id.as_str())?;
    if rest == "/cpu" && st.memory.utf16_len() > 0 {
        return Some(Json::Obj(vec![("cpu".into(), Json::Num(0.0)), ("memory".into(), Json::Num(st.memory.utf16_len() as f64))]));
    }
    None
}

/// `""+v` for a JSON value (`undefined` = None).
pub(crate) fn js_to_string(v: Option<&Json>) -> String {
    match v {
        None => "undefined".into(),
        Some(Json::Null) => "null".into(),
        Some(Json::Bool(b)) => b.to_string(),
        Some(Json::Num(n)) => util::num::to_js_string(*n),
        Some(Json::Str(s)) => s.clone(),
        Some(Json::Obj(_)) => "[object Object]".into(),
        Some(Json::Arr(a)) => a.iter().map(|e| if e.is_null() { String::new() } else { js_to_string(Some(e)) }).collect::<Vec<_>>().join(","),
    }
}

/// `v[key]` on a parsed JSON value; None = undefined.
fn step<'a>(v: &'a Json, key: &str) -> Option<Cow<'a, Json>> {
    match v {
        Json::Obj(m) => m.iter().rev().find(|(k, _)| k == key).map(|(_, x)| Cow::Borrowed(x)),
        Json::Arr(a) if key == "length" => Some(Cow::Owned(Json::Num(a.len() as f64))),
        Json::Arr(a) => engine::doc::array_index(key).and_then(|i| a.get(i as usize)).map(Cow::Borrowed),
        Json::Str(s) if key == "length" => Some(Cow::Owned(Json::Num(s.encode_utf16().count() as f64))),
        Json::Str(s) => engine::doc::array_index(key).and_then(|i| s.encode_utf16().nth(i as usize)).map(|u| Cow::Owned(Json::Str(char::from_u32(u as u32).unwrap_or('\u{fffd}').to_string()))),
        _ => None,
    }
}

/// `cur = cur[part]` for every part of `path.split('.')`. `Err` = TypeError (a property of undefined/null).
pub(crate) fn walk<'a>(root: &'a Json, path: &str) -> Result<Option<Cow<'a, Json>>, ()> {
    let mut cur: Option<Cow<'a, Json>> = Some(Cow::Borrowed(root));
    for part in path.split('.') {
        cur = match cur {
            None | Some(Cow::Borrowed(Json::Null)) => return Err(()),
            Some(Cow::Owned(Json::Null)) => return Err(()),
            Some(Cow::Borrowed(v)) => step(v, part),
            Some(Cow::Owned(v)) => step(&v, part).map(|c| Cow::Owned(c.into_owned())),
        };
    }
    Ok(cur)
}

/// What a memory watch shows: `""+value`, or 'Incorrect memory path' when the walk throws.
pub(crate) fn memory_watch_value(memory: &Json, path: &str) -> String {
    match walk(memory, path) {
        Ok(v) => js_to_string(v.as_deref()),
        Err(()) => "Incorrect memory path".into(),
    }
}

pub(crate) fn watch_memory(hub: &mut HubState, u: UserIdx, path: &str, conn: u64) {
    if path.len() > MAX_WATCH_PATH {
        return;
    }
    let watches = hub.memory.entry(u).or_default();
    match watches.iter().position(|w| w.path == path) {
        Some(i) => {
            add_once(&mut watches[i].conns, conn);
            watches[i].sent = None; // the new watcher gets the current value on the next broadcast
        }
        None if watches.len() < MAX_WATCHES => watches.push(MemWatch { path: path.to_string(), conns: vec![conn], sent: None }),
        None => {}
    }
}

/// The `mapVisual:<id>` payload: the user's map visual of the last tick, or null.
pub(crate) fn map_visual_json(hub: &HubState, u: UserIdx) -> String {
    match hub.visuals.get(&(u, "map".into())) {
        Some(v) => Json::Str(v.to_string()).stringify(),
        None => "null".into(),
    }
}

/// Throttled `roomsDone` part of user.js: memory watches (at most 50 paths per user), resources, map visuals.
pub(crate) fn broadcast(hub: &mut HubState, view: &WorldView) {
    let mut memory = std::mem::take(&mut hub.memory);
    for (&u, watches) in memory.iter_mut() {
        let Some(st) = view.users.get(u as usize) else { continue };
        let version = st.memory_version;
        if st.memory.utf16_len() == 0 || watches.iter().all(|w| w.sent == Some(version)) {
            continue;
        }
        let Ok(parsed) = Json::parse(&st.memory.to_string_lossy()) else { continue };
        let id = st.id.as_str();
        for w in watches.iter_mut() {
            if w.sent == Some(version) {
                continue;
            }
            w.sent = Some(version);
            let value = Json::Str(memory_watch_value(&parsed, &w.path)).stringify();
            let channel = format!("user:{id}/memory/{}", w.path);
            for c in w.conns.iter().filter_map(|c| hub.conns.get(c)) {
                c.write_event(&channel, &value);
            }
        }
    }
    hub.memory = memory;

    for (&u, conns) in &hub.resources {
        let Some(st) = view.users.get(u as usize) else { continue };
        if conns.is_empty() {
            continue;
        }
        // _.merge({}, user.resources, {credits: (user.money || 0) / 1000})
        let mut m = match crate::field(&st.doc, "resources").map(|v| v.to_json()) {
            Some(Json::Obj(m)) => m,
            _ => vec![],
        };
        let money = crate::field(&st.doc, "money").and_then(|v| v.as_f64()).filter(|n| *n != 0.0 && !n.is_nan()).unwrap_or(0.0);
        let credits = Json::Num(money / 1000.0);
        match m.iter_mut().find(|(k, _)| k == "credits") {
            Some(slot) => slot.1 = credits,
            None => m.push(("credits".into(), credits)),
        }
        let data = Json::Obj(m).stringify();
        let channel = format!("user:{}/resources", st.id);
        for c in conns.iter().filter_map(|c| hub.conns.get(c)) {
            c.write_event(&channel, &data);
        }
    }

    for (&u, conns) in &hub.map_visual {
        let Some(st) = view.users.get(u as usize) else { continue };
        let data = map_visual_json(hub, u);
        let channel = format!("mapVisual:{}", st.id);
        for c in conns.iter().filter_map(|c| hub.conns.get(c)) {
            c.write_event(&channel, &data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected values from node: `""+walk(JSON.parse(m), path)` with user.js's loop (tests/fixtures/user/memory.json).
    #[test]
    fn memory_paths() {
        let fx = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/user/memory.json")).unwrap();
        let fx = Json::parse(&fx).unwrap();
        let mem = Json::parse(fx.get("memory").unwrap().as_str().unwrap()).unwrap();
        let mut n = 0;
        for c in fx.get("watch").unwrap().as_array().unwrap() {
            let (path, want) = (c.at(0).unwrap().as_str().unwrap(), c.at(1).unwrap().as_str().unwrap());
            assert_eq!(memory_watch_value(&mem, path), want, "path {path:?}");
            n += 1;
        }
        assert!(n >= 10);
    }
}
