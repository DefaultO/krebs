//! mirror: reference/backend/lib/game/api/leaderboard.js
//! parity: FAITHFUL
//!
//! Three constant stubs (LB-*); checked against the official router in tests/fixtures/api (server.rs oracle).

use crate::http::{obj, Request, Response};
use crate::ApiCtx;
use util::json::Json;

/// GET /api/leaderboard/list
pub fn list(_req: &Request, _ctx: &ApiCtx) -> Response {
    Response::ok(obj(vec![("list", Json::Arr(vec![])), ("count", Json::Num(0.0)), ("users", Json::Obj(vec![]))]))
}

/// GET /api/leaderboard/find
pub fn find(req: &Request, _ctx: &ApiCtx) -> Response {
    if req.param("season").is_some_and(|s| !s.is_empty()) {
        return Response::error("result not found");
    }
    Response::ok(obj(vec![("list", Json::Arr(vec![]))]))
}

/// GET /api/leaderboard/seasons
pub fn seasons(_req: &Request, _ctx: &ApiCtx) -> Response {
    let s = |id: &str| obj(vec![("_id", Json::Str(id.into())), ("name", Json::Str("\u{2014}".into()))]);
    Response::ok(obj(vec![("seasons", Json::Arr(vec![s("empty1"), s("empty2")]))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    #[test]
    fn stubs() {
        let (ctx, _h) = fake::ctx(None);
        assert_eq!(fake::run(list, &ctx, "GET", "/x", "").1.stringify(), r#"{"ok":1,"list":[],"count":0,"users":{}}"#);
        assert_eq!(fake::run(find, &ctx, "GET", "/x?season=2020", "").1.stringify(), r#"{"error":"result not found"}"#);
        assert_eq!(fake::run(find, &ctx, "GET", "/x?username=a", "").1.stringify(), r#"{"ok":1,"list":[]}"#);
        assert_eq!(fake::run(seasons, &ctx, "GET", "/x", "").1.stringify(), "{\"ok\":1,\"seasons\":[{\"_id\":\"empty1\",\"name\":\"\u{2014}\"},{\"_id\":\"empty2\",\"name\":\"\u{2014}\"}]}");
    }
}
