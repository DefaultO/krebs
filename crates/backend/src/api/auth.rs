//! mirror: reference/backend/lib/game/api/auth.js
//! parity: DIVERGENT(AUTH-STEAM-TICKET): no Steam Web API/greenworks; `single-user:<name>` maps every ticket to that
//! user (created on first use with its username set), `trust` is not implemented yet (ticket layout unverified,
//! ARCHITECTURE.md §14.7 risk 8), `off` rejects; `auth/me` reads the password flag from `UserState.auth`
#![allow(unused_variables)]

use crate::http::{obj, Request, Response};
use crate::{authlib, field, iso_date, obj_set, s, str_field, truthy, vnow, ApiCtx};
use engine::doc::{Obj, Val};
use engine::world::World;
use util::json::Json;

/// GET /api/auth/me
pub fn me(req: &Request, ctx: &ApiCtx) -> Response {
    let Some(u) = ctx.user else { return Response::unauthorized() };
    let view = ctx.published.get();
    let st = &view.users[u as usize];
    let d = &st.doc;
    let mut out: Vec<(&str, Json)> = vec![("_id", Json::Str(st.id.as_str().into()))];
    let copy = |k: &'static str, out: &mut Vec<(&str, Json)>| {
        if let Some(v) = field(d, k) {
            out.push((k, v.to_json()));
        }
    };
    for k in ["email", "emailDirty", "username", "cpu", "badge"] {
        copy(k, &mut out);
    }
    out.push(("password", Json::Bool(truthy(field(&st.auth, "password")) || truthy(field(d, "password")))));
    for k in ["lastRespawnDate", "notifyPrefs", "gcl", "lastChargeTime", "blocked", "customBadge", "power"] {
        copy(k, &mut out);
    }
    let num_or0 = |k| field(d, k).and_then(Val::as_f64).filter(|n| *n != 0.0 && !n.is_nan()).unwrap_or(0.0);
    out.push(("money", Json::Num(num_or0("money") / 1000.0)));
    // _.pick(user.steam, ['id', 'displayName', 'ownership', 'steamProfileLinkHidden']) ({} without steam)
    let steam = match field(d, "steam").and_then(Val::as_obj) {
        Some(st) => Json::Obj(["id", "displayName", "ownership", "steamProfileLinkHidden"].iter().filter_map(|k| field(st, k).map(|v| (k.to_string(), v.to_json()))).collect()),
        None => Json::Obj(vec![]),
    };
    out.push(("steam", steam));
    // `x || 0`: numbers keep their value, anything falsy becomes 0
    let or0 = |k| match field(d, k) {
        v if truthy(v) => v.unwrap().to_json(),
        _ => Json::Num(0.0),
    };
    out.push(("powerExperimentations", or0("powerExperimentations")));
    out.push(("powerExperimentationTime", or0("powerExperimentationTime")));
    Response::ok(obj(out))
}

/// steamFindOrCreateUser's new-user document (+ `extra` fields), without `_id`.
pub fn new_user_doc(w: &World, extra: Vec<(&str, Val)>) -> Obj {
    let mut d = Obj::new();
    for (k, v) in extra.iter().filter(|(k, _)| *k == "steam") {
        obj_set(&mut d, k, v.clone());
    }
    obj_set(&mut d, "cpu", Val::Num(100.0));
    obj_set(&mut d, "cpuAvailable", Val::Num(0.0));
    obj_set(&mut d, "registeredDate", s(&iso_date(vnow(w))));
    obj_set(&mut d, "credits", Val::Num(0.0));
    obj_set(&mut d, "gcl", Val::Num(0.0));
    obj_set(&mut d, "powerExperimentations", Val::Num(30.0));
    for (k, v) in extra.into_iter().filter(|(k, _)| *k != "steam") {
        obj_set(&mut d, k, v);
    }
    d
}

/// The `users.code` document every new user gets: `{user, modules:{main:''}, branch:'default', activeWorld:true,
/// activeSim:true}` (`user` and `_id` filled by `add_user`).
pub fn default_code(modules: Option<Val>) -> Obj {
    let mut c = Obj::new();
    obj_set(&mut c, "user", Val::Null);
    let mut main = Obj::new();
    obj_set(&mut main, "main", s(""));
    obj_set(&mut c, "modules", modules.unwrap_or(Val::Obj(main)));
    obj_set(&mut c, "branch", s("default"));
    obj_set(&mut c, "activeWorld", Val::Bool(true));
    obj_set(&mut c, "activeSim", Val::Bool(true));
    c
}

/// POST /api/auth/steam-ticket
pub fn steam_ticket(req: &Request, ctx: &ApiCtx) -> Response {
    let body = req.body();
    if body.get("useNativeAuth").is_some_and(|v| crate::truthy(Some(&Val::from_json(v)))) {
        return Response::error("authentication method is not supported");
    }
    let mode = ctx.published.steam_mode().to_string();
    let Some(name) = mode.strip_prefix("single-user:").map(str::to_string) else {
        return Response::error(if mode == "trust" { "could not authenticate" } else { "authentication method is not supported" });
    };
    let steam_id = "local".to_string();
    // an existing user needs no write (and no wait for the next tick's publish)
    let known = ctx.published.get().usernames.get(name.to_lowercase().as_str()).copied();
    let found = if let Some(u) = known { Ok(u) } else { crate::write(ctx, move |w| {
        let lower = name.to_lowercase();
        if let Some(&u) = w.usernames.get(lower.as_str()) {
            return Ok(u);
        }
        let mut steam = Obj::new();
        obj_set(&mut steam, "id", s("local"));
        let doc = new_user_doc(w, vec![("steam", Val::Obj(steam)), ("username", s(&name)), ("usernameLower", s(&lower))]);
        Ok(crate::add_user(w, doc, Obj::new(), vec![default_code(None)]))
    }) };
    let u = match found {
        Ok(u) => u,
        Err(e) => return Response::error(&e),
    };
    let view = ctx.published.get();
    let Some(st) = view.users.get(u as usize) else { return Response::error("could not authenticate") };
    let steamid = str_field(&st.doc, "steam").map(str::to_string).or_else(|| field(&st.doc, "steam").and_then(Val::as_obj).and_then(|o| str_field(o, "id")).map(str::to_string)).unwrap_or(steam_id);
    Response::ok(obj(vec![("token", Json::Str(authlib::gen_token(st.id.as_str()))), ("steamid", Json::Str(steamid))]))
}

/// GET /api/auth/query-token (official servers only; AUTH-OFFICIAL-TOKENS).
pub fn query_token(req: &Request, ctx: &ApiCtx) -> Response {
    Response::not_found(&req.method, &req.path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    #[test]
    fn me_fields() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        let (st, j) = fake::run(me, &ctx, "GET", "/api/auth/me", "");
        assert_eq!(st, 200);
        let keys: Vec<&str> = j.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["ok", "_id", "username", "cpu", "badge", "password", "gcl", "money", "steam", "powerExperimentations", "powerExperimentationTime"]);
        assert_eq!(j.get("money").and_then(Json::as_f64), Some(12.345));
        assert_eq!(j.get("steam").unwrap().stringify(), r#"{"id":"7656","displayName":"A"}"#);
        assert_eq!(j.get("password"), Some(&Json::Bool(false)));
        let (ctx, _h) = fake::ctx(Some(fake::BOB));
        let (_, j) = fake::run(me, &ctx, "GET", "/api/auth/me", "");
        assert_eq!(j.get("steam").unwrap().stringify(), "{}");
        let (ctx, _h) = fake::ctx(None);
        assert_eq!(fake::run(me, &ctx, "GET", "/api/auth/me", "").0, 401);
    }

    #[test]
    fn single_user_steam_ticket() {
        let (ctx, h) = fake::ctx(None);
        // existing user: alice is the configured default? no: the default name is "player" → created once
        let (st, j) = fake::run(steam_ticket, &ctx, "POST", "/api/auth/steam-ticket", r#"{"ticket":"00ff","useNativeAuth":false}"#);
        assert_eq!(st, 200, "{j}");
        let token = j.get("token").and_then(Json::as_str).unwrap().to_string();
        let view = ctx.published.get();
        let u = authlib::check_token(&token, &view).expect("new user is published before the reply");
        assert_eq!(str_field(&view.users[u as usize].doc, "username"), Some("player"));
        assert_eq!(view.users[u as usize].code.branches.len(), 1);
        // second login: same user
        let (_, j2) = fake::run(steam_ticket, &ctx, "POST", "/api/auth/steam-ticket", r#"{"ticket":"abcd"}"#);
        assert_eq!(authlib::check_token(j2.get("token").and_then(Json::as_str).unwrap(), &ctx.published.get()), Some(u));
        let (_, native) = fake::run(steam_ticket, &ctx, "POST", "/api/auth/steam-ticket", r#"{"ticket":"00","useNativeAuth":true}"#);
        assert_eq!(native.get("error").and_then(Json::as_str), Some("authentication method is not supported"));
        drop(ctx);
        let w = h.join().unwrap();
        let d = &w.users[u as usize].doc;
        let keys: Vec<&str> = d.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["steam", "cpu", "cpuAvailable", "registeredDate", "credits", "gcl", "powerExperimentations", "username", "usernameLower", "_id"]);
        let code = &w.users[u as usize].code.branches[0];
        let ck: Vec<&str> = code.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(ck, ["user", "modules", "branch", "activeWorld", "activeSim", "_id"]);
        assert_eq!(str_field(code, "user"), Some(w.users[u as usize].id.as_str()));
    }
}
