//! mirror: reference/backend/lib/game/socket/system.js
//! parity: DIVERGENT(SOCK-DISPATCH): `serverMessage` is published directly to `server-message` listeners instead of
//! through pubsub; untested without fixtures beyond the payload shape
//!
//! `server-message` (ARCHITECTURE.md §8.4). Owner: WP04.

use crate::Published;
use engine::world::{UserIdx, WorldView};
use util::json::Json;

/// Anyone may subscribe; nothing is sent on subscribe.
pub fn server_message(user: Option<UserIdx>, channel: &str, view: &WorldView) -> Option<Json> {
    let _ = (user, channel, view);
    None
}

/// `pubsub.publish('serverMessage', message)` (CLI `system.sendServerMessage`) → `["server-message", <text>]`.
pub fn send_server_message(p: &Published, message: &str) {
    p.hub().emit("server-message", &Json::Str(message.into()).stringify());
}
