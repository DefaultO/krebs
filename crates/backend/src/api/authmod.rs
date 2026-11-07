//! not a mirror: the screepsmod-auth surface (signin, password, register/submit, HTTP Basic) (§8.5). Owner: WP04.
//!
//! Shapes follow the client and xxscreeps' password mod (spec 10 AUTHMOD-*): `email` may be a username or an
//! e-mail; passwords are PBKDF2-HMAC-SHA1 (100 000 iterations, 16-byte salt) in `UserState.auth`
//! `{password, salt}` (hex). Imported screepsmod-auth hashes are not verified (§8.5).

use crate::api::auth::{default_code, new_user_doc};
use crate::http::{obj, Request, Response};
use crate::{authlib, edit_user, obj_set, s, str_field, ApiCtx};
use engine::doc::Obj;
use engine::world::{UserIdx, WorldView};
use std::sync::Arc;
use util::json::Json;

pub const PBKDF2_ITERATIONS: u32 = 100_000;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    s.len().is_multiple_of(2).then(|| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect())?
}

fn hash(password: &str, salt: &[u8]) -> [u8; 20] {
    let mut out = [0u8; 20];
    util::hmac::pbkdf2_sha1(password.as_bytes(), salt, PBKDF2_ITERATIONS, &mut out);
    out
}

/// `UserState.auth` with a fresh salt and hash for `password`.
fn auth_for(password: &str, keep: &Obj) -> Obj {
    let mut salt = [0u8; 16];
    util::sys::os_random(&mut salt);
    let mut a = keep.clone();
    obj_set(&mut a, "password", s(&hex(&hash(password, &salt))));
    obj_set(&mut a, "salt", s(&hex(&salt)));
    a
}

fn check_password(auth: &Obj, password: &str) -> bool {
    let (Some(h), Some(salt)) = (str_field(auth, "password").and_then(unhex), str_field(auth, "salt").and_then(unhex)) else { return false };
    let got = hash(password, &salt);
    h.len() == 20 && h.iter().zip(got).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

/// A username, or an e-mail when it contains `@` (xxscreeps findUserByUsernameOrEmail).
fn find_user(view: &WorldView, name: &str) -> Option<UserIdx> {
    let lower = name.to_lowercase();
    if name.contains('@') {
        return view.users.iter().position(|u| str_field(&u.doc, "email") == Some(lower.as_str())).map(|i| i as UserIdx);
    }
    view.usernames.get(lower.as_str()).copied()
}

/// HTTP Basic (`Authorization: Basic base64(user:password)`) → the user, when the password matches.
pub(crate) fn basic_user(view: &WorldView, header: &str) -> Option<UserIdx> {
    let b64 = header.strip_prefix("Basic ")?;
    let raw = String::from_utf8(util::base64::decode(b64.trim())?).ok()?;
    let (name, password) = raw.split_once(':')?;
    let u = find_user(view, name)?;
    check_password(&view.users[u as usize].auth, password).then_some(u)
}

fn str_param<'a>(b: &'a Json, k: &str) -> Option<&'a str> {
    b.get(k).and_then(Json::as_str)
}

/// POST /api/auth/signin
pub fn signin(req: &Request, ctx: &ApiCtx) -> Response {
    let b = req.body();
    let (Some(email), Some(password)) = (str_param(&b, "email"), str_param(&b, "password")) else {
        return Response::unauthorized();
    };
    let view = ctx.published.get();
    match find_user(&view, email) {
        Some(u) if check_password(&view.users[u as usize].auth, password) => Response::ok(obj(vec![("token", Json::Str(authlib::gen_token(view.users[u as usize].id.as_str())))])),
        _ => Response::unauthorized(),
    }
}

/// POST /api/user/password
pub fn password(req: &Request, ctx: &ApiCtx) -> Response {
    let Some(u) = ctx.user else { return Response::unauthorized() };
    let b = req.body();
    let (Some(old), Some(new)) = (str_param(&b, "oldPassword"), str_param(&b, "password")) else {
        return Response::error("Invalid password");
    };
    let auth = ctx.published.get().users[u as usize].auth.clone();
    // a user without a password (Steam sign-up) may set one without the old password
    if new.encode_utf16().count() < 8 || (str_field(&auth, "password").is_some() && !check_password(&auth, old)) {
        return Response::error("Invalid password");
    }
    let next = auth_for(new, &auth);
    match crate::write(ctx, move |w| {
        edit_user(w, u, |st| st.auth = Arc::new(next));
        Ok(())
    }) {
        Ok(()) => Response::ok(Json::Null),
        Err(e) => Response::error(&e),
    }
}

/// `^[a-zA-Z0-9_-]{3,}$` (register.js set-username), and at most 32 long: names go out to every page on every poll.
/// A new name only: worlds keep the names they hold.
pub fn valid_username(name: &str) -> bool {
    (3..=32).contains(&name.len()) && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// POST /api/register/submit
pub fn submit(req: &Request, ctx: &ApiCtx) -> Response {
    let b = req.body();
    let (Some(username), Some(email), Some(password)) = (str_param(&b, "username"), str_param(&b, "email"), str_param(&b, "password")) else {
        return Response::error("invalid params");
    };
    if !valid_username(username) {
        return Response::error("invalid");
    }
    let (username, email) = (username.to_string(), email.to_lowercase());
    let auth = auth_for(password, &Obj::new());
    // Here and not in `write`, which runs on the tick thread.
    let modules = match b.get("modules").filter(|m| matches!(m, Json::Obj(_))).map(|m| crate::api::user::checked_modules(m.clone())).transpose() {
        Ok(m) => m,
        Err(e) => return Response::error(&e),
    };
    let r = crate::write(ctx, move |w| {
        let lower = username.to_lowercase();
        if w.usernames.contains_key(lower.as_str()) {
            return Err("exists".to_string());
        }
        let doc = new_user_doc(w, vec![("email", s(&email)), ("username", s(&username)), ("usernameLower", s(&lower))]);
        crate::add_user(w, doc, auth, vec![default_code(modules)]);
        Ok(())
    });
    match r {
        Ok(()) => Response::ok(Json::Null),
        Err(e) => Response::error(&e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    #[test]
    fn register_signin_basic_and_password_change() {
        let (ctx, _h) = fake::ctx(None);
        let (_, j) = fake::run(submit, &ctx, "POST", "/api/register/submit", r#"{"username":"Carol","email":"C@x.io","password":"hunter22"}"#);
        assert_eq!(j.stringify(), r#"{"ok":1}"#);
        let (_, dup) = fake::run(submit, &ctx, "POST", "/api/register/submit", r#"{"username":"carol","email":"d@x.io","password":"x"}"#);
        assert_eq!(dup.get("error").and_then(Json::as_str), Some("exists"));
        let (_, bad) = fake::run(submit, &ctx, "POST", "/api/register/submit", r#"{"username":"c d","email":"d@x.io","password":"x"}"#);
        assert_eq!(bad.get("error").and_then(Json::as_str), Some("invalid"));

        let (st, j) = fake::run(signin, &ctx, "POST", "/api/auth/signin", r#"{"email":"CAROL","password":"hunter22"}"#);
        assert_eq!(st, 200);
        let view = ctx.published.get();
        let carol = authlib::check_token(j.get("token").and_then(Json::as_str).unwrap(), &view).unwrap();
        assert_eq!(fake::run(signin, &ctx, "POST", "/api/auth/signin", r#"{"email":"c@x.io","password":"hunter22"}"#).0, 200);
        assert_eq!(fake::run(signin, &ctx, "POST", "/api/auth/signin", r#"{"email":"carol","password":"wrong"}"#).0, 401);
        let basic = format!("Basic {}", util::base64::encode(b"carol:hunter22"));
        assert_eq!(basic_user(&view, &basic), Some(carol));
        assert_eq!(basic_user(&view, &format!("Basic {}", util::base64::encode(b"carol:nope"))), None);

        let ctx = crate::ApiCtx { published: ctx.published.clone(), commands: ctx.commands.clone(), user: Some(carol) };
        let (_, short) = fake::run(password, &ctx, "POST", "/api/user/password", r#"{"oldPassword":"hunter22","password":"short"}"#);
        assert_eq!(short.get("error").and_then(Json::as_str), Some("Invalid password"));
        let (_, ok) = fake::run(password, &ctx, "POST", "/api/user/password", r#"{"oldPassword":"hunter22","password":"longer-pass"}"#);
        assert_eq!(ok.stringify(), r#"{"ok":1}"#);
        assert_eq!(fake::run(signin, &ctx, "POST", "/api/auth/signin", r#"{"email":"carol","password":"longer-pass"}"#).0, 200);
    }

    #[test]
    fn usernames_have_a_length() {
        assert!(!valid_username("ab"));
        assert!(valid_username("abc"));
        assert!(valid_username(&"n".repeat(32)));
        assert!(!valid_username(&"n".repeat(33)));
        assert!(!valid_username(&"n".repeat(10 << 20)));
    }

    /// The modules of a sign-up are converted before `write`, with the same limits as an upload.
    #[test]
    fn submit_checks_modules_as_an_upload_does() {
        let (ctx, _h) = fake::ctx(None);
        let body = |name: &str, modules: String| format!(r#"{{"username":"{name}","email":"{name}@x.io","password":"hunter22","modules":{modules}}}"#);
        let many: Vec<String> = (0..=crate::api::user::MAX_MODULES).map(|i| format!(r#""m{i}":"""#)).collect();
        let (_, j) = fake::run(submit, &ctx, "POST", "/api/register/submit", &body("many", format!("{{{}}}", many.join(","))));
        assert_eq!(j.get("error").and_then(Json::as_str), Some("too many modules"));
        assert!(ctx.published.get().usernames.get("many").is_none());
        let (_, j) = fake::run(submit, &ctx, "POST", "/api/register/submit", &body("fine", r#"{"main":"x","a.b":"y"}"#.into()));
        assert_eq!(j.stringify(), r#"{"ok":1}"#);
    }
}
