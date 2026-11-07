//! mirror: reference/backend/lib/game/api/register.js
//! parity: FAITHFUL
//!
//! Fixtures: tests/fixtures/api (official router replay, server.rs oracle) and the step tests below follow
//! (rejection order and texts as register.js); the e-mail regex is checked against node in
//! tests/fixtures/register/email.json.

use crate::api::authmod::valid_username;
use crate::http::{obj, Request, Response};
use crate::{edit_user, field, obj_set, s, ApiCtx};
use std::sync::Arc;
use util::json::Json;

/// `/^[\w\d-\.\+&]+\@[\w\d\-\.&]+\.[\w\d\-\.&]{2,}$/` (no `u` flag: `\w` is ASCII).
pub(crate) fn valid_email(e: &str) -> bool {
    let local_ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '&');
    let domain_ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '&');
    let Some((local, domain)) = e.split_once('@') else { return false };
    !local.is_empty()
        && local.chars().all(local_ok)
        && domain.chars().all(domain_ok)
        && domain.char_indices().any(|(i, c)| c == '.' && i >= 1 && domain.len() - i > 2)
}

/// `request.query[k]` truthiness for strings.
fn query<'a>(req: &'a Request, k: &str) -> Option<&'a str> {
    req.param(k).filter(|v| !v.is_empty())
}

/// GET /api/register/check-email
pub fn check_email(req: &Request, ctx: &ApiCtx) -> Response {
    let Some(email) = query(req, "email") else { return Response::error("invalid email") };
    let lower = email.to_lowercase();
    let view = ctx.published.get();
    if view.users.iter().any(|u| crate::str_field(&u.doc, "email") == Some(lower.as_str())) {
        return Response::error("exists");
    }
    Response::ok(Json::Null)
}

/// GET /api/register/check-username
pub fn check_username(req: &Request, ctx: &ApiCtx) -> Response {
    let Some(name) = query(req, "username") else { return Response::error("invalid username") };
    if ctx.published.get().usernames.contains_key(name.to_lowercase().as_str()) {
        return Response::error("exists");
    }
    Response::ok(Json::Null)
}

/// POST /api/register/set-username
pub fn set_username(req: &Request, ctx: &ApiCtx) -> Response {
    let Some(u) = ctx.user else { return Response::unauthorized() };
    let b = req.body();
    let Some(username) = b.get("username").and_then(Json::as_str).map(str::to_string) else {
        return Response::error("TypeError: Cannot read properties of undefined (reading 'toLowerCase')");
    };
    let mut email = None;
    if let Some(e) = b.get("email").filter(|e| engine::doc::json_truthy(e)) {
        match e.as_str() {
            Some(e) if valid_email(e) => email = Some(e.to_lowercase()),
            _ => return Response::error("invalid email"),
        }
    }
    let r = crate::write(ctx, move |w| {
        if crate::truthy(field(&w.users[u as usize].doc, "username")) {
            return Err("username already set".into());
        }
        if !valid_username(&username) {
            return Err("invalid username".into());
        }
        let lower = username.to_lowercase();
        if w.usernames.contains_key(lower.as_str()) {
            return Err("invalid username".into());
        }
        edit_user(w, u, |st| {
            let mut d = (*st.doc).clone();
            obj_set(&mut d, "username", s(&username));
            obj_set(&mut d, "usernameLower", s(&lower));
            if let Some(e) = &email {
                obj_set(&mut d, "email", s(e));
            }
            st.doc = Arc::new(d);
        });
        Arc::make_mut(&mut w.usernames).insert(lower.into(), u);
        Ok(())
    });
    match r {
        Ok(()) => Response::ok(obj(vec![("modified", Json::Num(1.0))])),
        Err(e) => Response::error(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    #[test]
    fn email_regex_matches_node() {
        let fx = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/register/email.json")).unwrap();
        for c in Json::parse(&fx).unwrap().as_array().unwrap() {
            let (e, want) = (c.at(0).unwrap().as_str().unwrap(), c.at(1).unwrap().as_bool().unwrap());
            assert_eq!(valid_email(e), want, "{e:?}");
        }
    }

    #[test]
    fn checks() {
        let (ctx, _h) = fake::ctx(None);
        assert_eq!(fake::run(check_username, &ctx, "GET", "/x?username=ALICE", "").1.get("error").and_then(Json::as_str), Some("exists"));
        assert_eq!(fake::run(check_username, &ctx, "GET", "/x?username=zed", "").1.stringify(), r#"{"ok":1}"#);
        assert_eq!(fake::run(check_username, &ctx, "GET", "/x", "").1.get("error").and_then(Json::as_str), Some("invalid username"));
        assert_eq!(fake::run(check_email, &ctx, "GET", "/x", "").1.get("error").and_then(Json::as_str), Some("invalid email"));
        assert_eq!(fake::run(check_email, &ctx, "GET", "/x?email=a@b.cd", "").1.stringify(), r#"{"ok":1}"#);
    }

    #[test]
    fn set_username_order() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        // email checked before "already set"
        assert_eq!(fake::run(set_username, &ctx, "POST", "/x", r#"{"username":"zed","email":"bad"}"#).1.get("error").and_then(Json::as_str), Some("invalid email"));
        assert_eq!(fake::run(set_username, &ctx, "POST", "/x", r#"{"username":"zed"}"#).1.get("error").and_then(Json::as_str), Some("username already set"));
        // a user without a username: create one through the single-user steam path
        let (_, t) = fake::run(crate::api::auth::steam_ticket, &ctx, "POST", "/x", "{}");
        let view = ctx.published.get();
        let u = crate::authlib::check_token(t.get("token").and_then(Json::as_str).unwrap(), &view).unwrap();
        // strip its username through a write, then set it
        crate::write(&ctx, move |w| {
            crate::edit_user(w, u, |st| {
                let mut d = (*st.doc).clone();
                d.remove(engine::doc::Atom::intern("username"));
                st.doc = Arc::new(d);
            });
            Ok(())
        })
        .unwrap();
        let ctx2 = crate::ApiCtx { published: ctx.published.clone(), commands: ctx.commands.clone(), user: Some(u) };
        assert_eq!(fake::run(set_username, &ctx2, "POST", "/x", r#"{"username":"ab"}"#).1.get("error").and_then(Json::as_str), Some("invalid username"));
        assert_eq!(fake::run(set_username, &ctx2, "POST", "/x", r#"{"username":"Bob"}"#).1.get("error").and_then(Json::as_str), Some("invalid username"));
        let (_, ok) = fake::run(set_username, &ctx2, "POST", "/x", r#"{"username":"Zed_1","email":"Z@Example.com"}"#);
        assert_eq!(ok.stringify(), r#"{"ok":1,"modified":1}"#);
        let view = ctx.published.get();
        assert_eq!(view.usernames.get("zed_1"), Some(&u));
        assert_eq!(crate::str_field(&view.users[u as usize].doc, "email"), Some("z@example.com"));
    }
}
