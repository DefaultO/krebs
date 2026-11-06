//! mirror: reference/backend/lib/game/socket/map.js
//! parity: DIVERGENT(SOCK-ROOMMAP2): per-tick pushes only for rooms whose snapshot changed (spec 10 PERF note:
//! the client redraws on every message, skipping unchanged rooms is invisible)
//!
//! `roomMap2:<room>` = the room snapshot's `map_view` (ARCHITECTURE.md §8.4). Owner: WP04.

use crate::socket::server::{Conn, HubState};
use crate::{room_idx, Published};
use engine::world::{UserIdx, WorldView};
use std::sync::Arc;
use util::json::Json;

/// The `mapView:<room>` text (`"{}"` when absent).
fn map_view_text<'a>(view: &'a WorldView, name: &str) -> &'a str {
    match room_idx(view, name) {
        Some(r) if !view.rooms[r as usize].map_view.is_empty() => &view.rooms[r as usize].map_view,
        _ => "{}",
    }
}

/// The message payload for `roomMap2:<name>` (needs a user, as map.js).
pub fn room_map2(user: Option<UserIdx>, channel: &str, view: &WorldView) -> Option<Json> {
    user?;
    Json::parse(map_view_text(view, channel.strip_prefix("roomMap2:")?)).ok()
}

/// `["roomMap2:<name>",<mapView>]`, built by concatenation as map.js does.
fn message(name: &str, map_view: &str) -> String {
    format!("[\"roomMap2:{name}\",{map_view}]")
}

/// `onSubscribe('roomMap2:<name>')`: registers and immediately writes the current map view (no existence check).
pub(crate) fn subscribe(p: &Published, conn: &Arc<Conn>, name: &str) {
    let view = p.get();
    p.hub().lock().maps.entry(name.to_string()).or_default().push(conn.id);
    conn.write_event_raw(message(name, map_view_text(&view, name)));
}

/// Throttled `roomsDone`: rooms whose snapshot changed since the previous broadcast.
pub(crate) fn broadcast(hub: &HubState, view: &WorldView, prev: Option<&WorldView>) {
    for (name, conns) in &hub.maps {
        if conns.is_empty() {
            continue;
        }
        let Some(r) = room_idx(view, name) else { continue };
        let changed = prev.is_none_or(|pv| pv.rooms.get(r as usize).is_none_or(|old| !Arc::ptr_eq(old, &view.rooms[r as usize])));
        if !changed {
            continue;
        }
        let msg = message(name, map_view_text(view, name));
        for c in conns.iter().filter_map(|id| hub.conn(*id)) {
            c.write_event_raw(msg.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    #[test]
    fn payloads() {
        let w = fake::world();
        let v = fake::view(&w);
        assert_eq!(message("W1N1", map_view_text(&v, "W1N1")), r#"["roomMap2:W1N1",{"w":[],"r":[],"s":[[10,10]],"room":"W1N1"}]"#);
        assert_eq!(map_view_text(&v, "W9N9"), "{}");
        assert!(room_map2(None, "roomMap2:W1N1", &v).is_none());
        assert_eq!(room_map2(Some(fake::ALICE), "roomMap2:W2N1", &v).unwrap().get("room").and_then(Json::as_str), Some("W2N1"));
    }
}
