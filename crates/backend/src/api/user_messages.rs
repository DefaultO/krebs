//! mirror: reference/backend/lib/game/api/user-messages.js
//! parity: DIVERGENT(MSG-INDEX): the index groups by respondent (spec 10 Q8: the original keys every entry by the
//! requester); a missing `respondent` is rejected instead of writing owner-less messages, and mark-read with a
//! missing `id` answers 'invalid id' (the original's `update({_id: undefined})` marks every message of the world
//! read after the RPC drops the key); dates are real time. Send, list, unread-count, mark-read and MSG-NOTIFY are replayed in tests/fixtures/api.
#![allow(unused_variables)]

use crate::http::{obj, Request, Response};
use crate::{edit_user, field, iso_date, obj_set, s, str_field, user_brief, write, ApiCtx};
use engine::doc::{Obj, Val};
use engine::world::{UserIdx, World};
use std::sync::Arc;
use util::json::Json;

macro_rules! auth {
    ($ctx:expr) => {
        match $ctx.user {
            Some(u) => u,
            None => return Response::unauthorized(),
        }
    };
}

fn to_json(o: &Obj) -> Json {
    Json::Obj(o.iter().map(|(k, v)| (k.as_str().to_string(), v.to_json())).collect())
}

fn push_message(w: &mut World, owner: UserIdx, m: Obj) {
    edit_user(w, owner, |st| {
        let mut inbox = engine::world::Inbox { notifications: st.inbox.notifications.clone(), messages: st.inbox.messages.clone() };
        inbox.messages.push(Arc::new(m));
        st.inbox = Arc::new(inbox);
    });
}

/// `sendMessageNotification(userId, message)` (MSG-NOTIFY): unless the respondent disabled message notifications,
/// or is online (last 10 min) without `sendOnline`, upsert a `users.notifications` record and count it. Real time
/// (CLK-NOTIFY); the notification cron delivers it.
fn notify(w: &mut World, p: &crate::Published, ru: UserIdx, message: &str) {
    let prefs = field(&w.users[ru as usize].doc, "notifyPrefs").cloned();
    let pref = |k: &str| crate::truthy(prefs.as_ref().and_then(Val::as_obj).and_then(|o| field(o, k)));
    if crate::truthy(prefs.as_ref()) && pref("disabledOnMessages") {
        return;
    }
    let now = engine::policy::real_now_ms("CLK-NOTIFY") as f64;
    if !crate::truthy(prefs.as_ref()) || !pref("sendOnline") {
        // parseInt(userOnline) > Date.now() - 10 min (never seen: NaN, not online)
        if p.user_online(ru).is_some_and(|t| t as f64 > now - 600_000.0) {
            return;
        }
    }
    let rid = w.users[ru as usize].id.as_str().to_string();
    let matching = |n: &Obj| str_field(n, "user") == Some(rid.as_str()) && str_field(n, "message") == Some(message) && field(n, "date").and_then(Val::as_f64).is_some_and(|d| d <= now) && str_field(n, "type") == Some("msg");
    // update(..., {upsert: true}): every match, else one insert with a new `_id`
    let new_id = (!w.users[ru as usize].inbox.notifications.iter().any(|n| matching(n))).then(|| w.ids.next());
    edit_user(w, ru, |st| {
        let mut notes: Vec<Arc<Obj>> = st.inbox.notifications.clone();
        for n in notes.iter_mut().filter(|n| matching(n)) {
            let mut o = (**n).clone();
            obj_set(&mut o, "user", s(&rid));
            obj_set(&mut o, "message", s(message));
            obj_set(&mut o, "date", Val::Num(now));
            obj_set(&mut o, "type", s("msg"));
            let c = field(&o, "count").and_then(Val::as_f64).filter(|c| *c != 0.0 && !c.is_nan()).unwrap_or(0.0);
            obj_set(&mut o, "count", Val::Num(c + 1.0));
            *n = Arc::new(o);
        }
        if let Some(id) = new_id {
            let mut o = Obj::new();
            obj_set(&mut o, "user", s(&rid));
            obj_set(&mut o, "message", s(message));
            obj_set(&mut o, "date", Val::Num(now));
            obj_set(&mut o, "type", s("msg"));
            obj_set(&mut o, "count", Val::Num(1.0));
            obj_set(&mut o, "_id", s(id.as_str()));
            notes.push(Arc::new(o));
        }
        st.inbox = Arc::new(engine::world::Inbox { notifications: notes, messages: st.inbox.messages.clone() });
    });
}

/// Messages of `u` sorted by date descending (stable).
fn by_date_desc(msgs: &[Arc<Obj>]) -> Vec<&Arc<Obj>> {
    let mut v: Vec<&Arc<Obj>> = msgs.iter().collect();
    v.sort_by(|a, b| str_field(b, "date").cmp(&str_field(a, "date")));
    v
}

/// POST /api/user/messages/send
pub fn send(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let text = match b.get("text") {
        Some(Json::Str(t)) if t.encode_utf16().count() <= 100 * 1024 => t.clone(),
        _ => return Response::error("text too long"),
    };
    let respondent = crate::socket::user::js_to_string(b.get("respondent"));
    let date = iso_date(engine::policy::real_now_ms("CLK-API"));
    let published = ctx.published.clone();
    let r = write(ctx, move |w| {
        let Some(&ru) = engine::doc::Id::new(&respondent).and_then(|i| w.user_ids.get(&i)) else { return Err("invalid respondent".to_string()) };
        let me = w.users[u as usize].id.as_str().to_string();
        let mk = |user: &str, resp: &str, ty: &str| {
            let mut m = Obj::new();
            obj_set(&mut m, "user", s(user));
            obj_set(&mut m, "respondent", s(resp));
            obj_set(&mut m, "date", s(&date));
            obj_set(&mut m, "type", s(ty));
            obj_set(&mut m, "text", s(&text));
            obj_set(&mut m, "unread", Val::Bool(true));
            m
        };
        // vanilla builds inMessage as {respondent, user, …}
        let mut out = mk(&me, &respondent, "out");
        let oid = w.ids.next();
        obj_set(&mut out, "_id", s(oid.as_str()));
        let mut inm = Obj::new();
        obj_set(&mut inm, "respondent", s(&me));
        obj_set(&mut inm, "user", s(&respondent));
        for (k, v) in mk("", "", "in").iter().skip(2) {
            inm.set(k, v.clone());
        }
        obj_set(&mut inm, "outMessage", s(oid.as_str()));
        let iid = w.ids.next();
        obj_set(&mut inm, "_id", s(iid.as_str()));
        push_message(w, u, out.clone());
        push_message(w, ru, inm.clone());
        let from = crate::socket::user::js_to_string(field(&w.users[u as usize].doc, "username").map(Val::to_json).as_ref());
        notify(w, &published, ru, &format!("<a href=\"https://screeps.com/a/#!/messages\">New message</a> from user {from}"));
        Ok((me, to_json(&out), to_json(&inm)))
    });
    match r {
        Ok((me, out, inm)) => {
            let hub = ctx.published.hub();
            let msg = |m: Json| obj(vec![("message", m)]).stringify();
            let resp = crate::socket::user::js_to_string(b.get("respondent"));
            hub.emit(&format!("user:{me}/message:{resp}"), &msg(out));
            hub.emit(&format!("user:{resp}/message:{me}"), &msg(inm.clone()));
            hub.emit(&format!("user:{resp}/newMessage"), &msg(inm));
            Response::ok(Json::Null)
        }
        Err(e) => Response::error(&e),
    }
}

/// GET /api/user/messages/list
pub fn list(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let respondent = req.param("respondent");
    // an absent `respondent` is dropped by the RPC's JSON: every message of the user
    let mut msgs: Vec<Json> = by_date_desc(&view.users[u as usize].inbox.messages).into_iter().filter(|m| respondent.is_none_or(|r| str_field(m, "respondent") == Some(r))).take(100).map(|m| to_json(m)).collect();
    msgs.reverse();
    Response::ok(obj(vec![("messages", Json::Arr(msgs))]))
}

/// GET /api/user/messages/index
pub fn index(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let mut seen: Vec<String> = Vec::new();
    let mut messages = Vec::new();
    for m in by_date_desc(&view.users[u as usize].inbox.messages) {
        let r = str_field(m, "respondent").unwrap_or("").to_string();
        if !seen.contains(&r) {
            messages.push(obj(vec![("_id", Json::Str(r.clone())), ("message", to_json(m))]));
            seen.push(r);
        }
    }
    let mut found: Vec<UserIdx> = seen.iter().filter_map(|id| crate::user_by_id(&view, id)).collect();
    found.sort_unstable();
    let mut users: Vec<(String, String)> = Vec::new();
    for x in found {
        crate::socket::rooms::js_push(&mut users, view.users[x as usize].id.as_str().to_string(), user_brief(&view, x).stringify());
    }
    let users = Json::parse(&crate::socket::rooms::write_members(&users)).unwrap_or(Json::Obj(vec![]));
    Response::ok(obj(vec![("messages", Json::Arr(messages)), ("users", users)]))
}

/// POST /api/user/messages/mark-read
pub fn mark_read(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let id = crate::socket::user::js_to_string(req.body().get("id"));
    let r = write(ctx, move |w| {
        let msg = w.users[u as usize].inbox.messages.iter().find(|m| str_field(m, "_id") == Some(id.as_str()) && str_field(m, "type") == Some("in")).cloned().ok_or("invalid id")?;
        let set_read = |w: &mut World, owner: UserIdx, mid: &str| -> bool {
            let found = w.users[owner as usize].inbox.messages.iter().any(|m| str_field(m, "_id") == Some(mid));
            edit_user(w, owner, |st| {
                let messages = st
                    .inbox
                    .messages
                    .iter()
                    .map(|m| {
                        if str_field(m, "_id") == Some(mid) {
                            let mut x = (**m).clone();
                            obj_set(&mut x, "unread", Val::Bool(false));
                            Arc::new(x)
                        } else {
                            m.clone()
                        }
                    })
                    .collect();
                st.inbox = Arc::new(engine::world::Inbox { notifications: st.inbox.notifications.clone(), messages });
            });
            found
        };
        set_read(w, u, &id);
        let out_id = str_field(&msg, "outMessage").unwrap_or("").to_string();
        let resp = str_field(&msg, "respondent").unwrap_or("").to_string();
        // update({_id: outMessage}): {modified: 1}, or {} when the sender's copy is gone
        let out_found = engine::doc::Id::new(&resp).and_then(|i| w.user_ids.get(&i)).copied().is_some_and(|ru| set_read(w, ru, &out_id));
        Ok((str_field(&msg, "user").unwrap_or("").to_string(), resp, out_id, out_found))
    });
    match r {
        Ok((me, resp, out_id, out_found)) => {
            let hub = ctx.published.hub();
            let id = crate::socket::user::js_to_string(req.body().get("id"));
            let m = |i: &str| obj(vec![("message", obj(vec![("_id", Json::Str(i.into())), ("unread", Json::Bool(false))]))]).stringify();
            hub.emit(&format!("user:{me}/message:{resp}"), &m(&id));
            hub.emit(&format!("user:{resp}/message:{me}"), &m(&out_id));
            // q.all([publish, publish, update]) → the update result at index 2
            let update = if out_found { obj(vec![("modified", Json::Num(1.0))]) } else { Json::Obj(vec![]) };
            Response::ok(obj(vec![("2", update)]))
        }
        Err(e) => Response::error(&e),
    }
}

/// GET /api/user/messages/unread-count
pub fn unread_count(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let n = view.users[u as usize].inbox.messages.iter().filter(|m| str_field(m, "type") == Some("in") && field(m, "unread") == Some(&Val::Bool(true))).count();
    Response::ok(obj(vec![("count", Json::Num(n as f64))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    #[test]
    fn conversation() {
        let (ctx, _h) = fake::ctx(Some(fake::ALICE));
        assert_eq!(fake::run(send, &ctx, "POST", "/x", r#"{"respondent":"nobody","text":"hi"}"#).1.get("error").and_then(Json::as_str), Some("invalid respondent"));
        assert_eq!(fake::run(send, &ctx, "POST", "/x", r#"{"respondent":"b0b000000000002","text":5}"#).1.get("error").and_then(Json::as_str), Some("text too long"));
        assert_eq!(fake::run(send, &ctx, "POST", "/x", r#"{"respondent":"b0b000000000002","text":"hi bob"}"#).1.stringify(), r#"{"ok":1}"#);
        let bob = crate::ApiCtx { published: ctx.published.clone(), commands: ctx.commands.clone(), user: Some(fake::BOB) };
        assert_eq!(fake::run(unread_count, &bob, "GET", "/x", "").1.stringify(), r#"{"ok":1,"count":1}"#);
        assert_eq!(fake::run(unread_count, &ctx, "GET", "/x", "").1.stringify(), r#"{"ok":1,"count":0}"#);
        let (_, l) = fake::run(list, &bob, "GET", "/x?respondent=a11ce0000000001", "");
        let m = l.get("messages").unwrap().at(0).unwrap();
        let keys: Vec<&str> = m.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["respondent", "user", "date", "type", "text", "unread", "outMessage", "_id"]);
        let (_, idx) = fake::run(index, &bob, "GET", "/x", "");
        assert_eq!(idx.get("messages").unwrap().at(0).unwrap().get("_id").and_then(Json::as_str), Some("a11ce0000000001"));
        assert_eq!(idx.get("users").unwrap().get("a11ce0000000001").unwrap().get("username").and_then(Json::as_str), Some("alice"));
        let id = m.get("_id").and_then(Json::as_str).unwrap();
        assert_eq!(fake::run(mark_read, &bob, "POST", "/x", &format!(r#"{{"id":"{id}"}}"#)).1.stringify(), r#"{"2":{"modified":1},"ok":1}"#);
        assert_eq!(fake::run(unread_count, &bob, "GET", "/x", "").1.stringify(), r#"{"ok":1,"count":0}"#);
        assert_eq!(fake::run(mark_read, &ctx, "POST", "/x", &format!(r#"{{"id":"{id}"}}"#)).1.get("error").and_then(Json::as_str), Some("invalid id"));
        // the sender's copy is gone: update({_id: outMessage}) answers {}
        fake::run(send, &ctx, "POST", "/x", r#"{"respondent":"b0b000000000002","text":"again"}"#);
        write(&ctx, |w| {
            edit_user(w, fake::ALICE, |st| st.inbox = Arc::new(engine::world::Inbox { notifications: vec![], messages: vec![] }));
            Ok(())
        })
        .unwrap();
        let (_, l) = fake::run(list, &bob, "GET", "/x?respondent=a11ce0000000001", "");
        let id2 = l.get("messages").unwrap().at(1).unwrap().get("_id").and_then(Json::as_str).unwrap().to_string();
        assert_eq!(fake::run(mark_read, &bob, "POST", "/x", &format!(r#"{{"id":"{id2}"}}"#)).1.stringify(), r#"{"2":{},"ok":1}"#);
    }
}
