//! mirror: reference/backend/lib/game/api/power-creeps.js
//! parity: DIVERGENT(PC-UNDEFINED-ID): delete/cancel-delete without `id` answer 'invalid id'; the original's
//! `findOne({_id: undefined})` reaches storage as `findOne({})` (the world's first power creep) and, when that one is
//! the caller's, `update({})` sets/unsets `deleteTime` on every power creep of the world (SEC, not replicated)
//!
//! Checked against the official router in tests/fixtures/api (server.rs oracle), including the `remove` TypeError
//! of PC-DELETE during experimentation (the storage wrapper has no `remove`); non-string ids in the unit tests below.
//! Timestamps use vclock(G) (CLK-PC-TIMERS); writes run at the next tick boundary.
#![allow(unused_variables)]

use crate::http::{obj, Request, Response};
use crate::{edit_user, field, install_room, obj_set, s, str_field, truthy, vnow, write, ApiCtx};
use engine::constants as C;
use engine::doc::{Doc, DocRef, Obj, Val};
use engine::world::{RoomIdx, UserIdx, World};
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

fn reply<T>(r: Result<T, String>, f: impl FnOnce(T) -> Json) -> Response {
    match r {
        Ok(v) => Response::ok(f(v)),
        Err(e) => Response::error(&e),
    }
}

fn json_of(o: &Obj) -> Json {
    Val::Obj(o.clone()).to_json()
}

/// `calcFreePowerLevels(user, userPowerCreeps)`.
fn free_power_levels(user: &Obj, pcs: &[DocRef]) -> f64 {
    let power = field(user, "power").and_then(Val::as_f64).filter(|p| *p != 0.0 && !p.is_nan()).unwrap_or(0.0);
    let level = (power / C::POWER_LEVEL_MULTIPLY as f64).powf(1.0 / C::POWER_LEVEL_POW as f64).floor();
    // _.sum(userPowerCreeps, 'level'): numbers only
    let used = pcs.len() as f64 + pcs.iter().filter_map(|p| field(&p.obj, "level").and_then(Val::as_f64)).sum::<f64>();
    level - used
}

/// `("" + name).substring(0, 50)` (UTF-16 units).
fn pc_name(v: Option<&Json>) -> String {
    let full: Vec<u16> = crate::socket::user::js_to_string(v).encode_utf16().collect();
    String::from_utf16_lossy(&full[..full.len().min(50)])
}

/// `users.power_creeps.findOne({_id})` over every user.
fn find_pc(w: &World, id: &str) -> Option<(UserIdx, DocRef)> {
    w.users.iter().enumerate().find_map(|(u, st)| st.power_creeps.iter().find(|p| p.id().as_str() == id).map(|p| (u as UserIdx, p.clone())))
}

/// Writes the user's power creep doc with the same `_id` (same seq) and moves it to the front of the user's list:
/// `users.power_creeps` has an adaptive `user` index, so every written doc leads the next `find({user})`
/// (ORDER-POWER-CREEPS).
fn put_pc(w: &mut World, u: UserIdx, doc: Obj) {
    edit_user(w, u, |st| {
        let id = crate::str_field(&doc, "_id").unwrap_or("").to_string();
        let seq = st.power_creeps.iter().find(|p| p.id().as_str() == id).map_or(0, |p| p.seq);
        let mut v = vec![Doc::new(seq, doc)];
        v.extend(st.power_creeps.iter().filter(|p| p.id().as_str() != id).cloned());
        st.power_creeps = v.into();
    });
}

/// The spawned `rooms.objects` doc of a power creep, if any.
fn room_pc(w: &World, id: &str) -> Option<(RoomIdx, DocRef)> {
    w.rooms.iter().enumerate().find_map(|(r, s)| s.objects.iter().find(|d| d.id().as_str() == id).map(|d| (r as RoomIdx, d.clone())))
}

/// GET /api/game/power-creeps/list
pub fn list(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let view = ctx.published.get();
    let id = view.users[u as usize].id;
    let spawned: Vec<&DocRef> = crate::api::user::user_objects(&view, u).into_iter().filter(|d| d.kind() == "powerCreep").collect();
    let list = view.users[u as usize]
        .power_creeps
        .iter()
        .map(|pc| {
            let mut o = pc.obj.clone();
            // Object.assign(creep, roomObject)
            if let Some(room) = spawned.iter().find(|d| d.id() == pc.id()) {
                for (k, v) in room.obj.iter() {
                    o.set(k, v.clone());
                }
            }
            json_of(&o)
        })
        .collect();
    let _ = id;
    Response::ok(obj(vec![("list", Json::Arr(list))]))
}

/// POST /api/game/power-creeps/create
pub fn create(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let class = b.get("className").cloned();
    let name = pc_name(b.get("name"));
    let user_doc = ctx.published.get().users[u as usize].doc.clone();
    reply(
        write(ctx, move |w| {
            let pcs = w.users[u as usize].power_creeps.clone();
            if free_power_levels(&user_doc, &pcs) <= 0.0 {
                return Err("not enough power level".into());
            }
            // Object.values(C.POWER_CLASS).indexOf(className) === -1
            if class.as_ref().and_then(Json::as_str) != Some(C::POWER_CLASS_OPERATOR) {
                return Err("invalid class".into());
            }
            if pcs.iter().any(|p| str_field(&p.obj, "name") == Some(name.as_str())) {
                return Err("name already exists".into());
            }
            let mut d = Obj::new();
            obj_set(&mut d, "name", s(&name));
            obj_set(&mut d, "className", s(C::POWER_CLASS_OPERATOR));
            obj_set(&mut d, "user", s(w.users[u as usize].id.as_str()));
            obj_set(&mut d, "level", Val::Num(0.0));
            obj_set(&mut d, "hitsMax", Val::Num(1000.0));
            obj_set(&mut d, "store", Val::Obj(Obj::new()));
            obj_set(&mut d, "storeCapacity", Val::Num(100.0));
            obj_set(&mut d, "spawnCooldownTime", Val::Num(0.0));
            obj_set(&mut d, "powers", Val::Obj(Obj::new()));
            let id = w.ids.next();
            obj_set(&mut d, "_id", s(id.as_str()));
            let seq = w.next_seq;
            w.next_seq += 1;
            let out = json_of(&d);
            // an insert lands at the front of the user's adaptive index range (ORDER-POWER-CREEPS)
            edit_user(w, u, |st| {
                let mut v = vec![Doc::new(seq, d)];
                v.extend(st.power_creeps.iter().cloned());
                st.power_creeps = v.into();
            });
            Ok(out)
        }),
        |d| d,
    )
}

/// The owner check shared by delete and cancel-delete: `findOne({_id: body.id})` is strict, so only a string id
/// can match (an absent one is answered 'invalid id' too, see the parity note).
fn own_pc(w: &World, u: UserIdx, id: Option<&str>) -> Result<DocRef, String> {
    match id.and_then(|id| find_pc(w, id)) {
        Some((_, d)) if str_field(&d.obj, "user") == Some(w.users[u as usize].id.as_str()) => Ok(d),
        _ => Err("invalid id".into()),
    }
}

fn spawned(o: &Obj) -> bool {
    field(o, "spawnCooldownTime") == Some(&Val::Null) || truthy(field(o, "shard"))
}

/// POST /api/game/power-creeps/delete
pub fn delete(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let id = req.body().get("id").and_then(Json::as_str).map(str::to_string);
    let experimenting = ctx.published.get().users[u as usize].doc.clone();
    reply(
        write(ctx, move |w| {
            let pc = own_pc(w, u, id.as_deref())?;
            if spawned(&pc.obj) {
                return Err("spawned".into());
            }
            if truthy(field(&pc.obj, "deleteTime")) {
                return Err("already being deleted".into());
            }
            let now = vnow(w) as f64;
            if field(&experimenting, "powerExperimentationTime").and_then(Val::as_f64).filter(|t| !t.is_nan()).unwrap_or(0.0) > now {
                // PC-DELETE GOTCHA: the storage wrapper has no `remove`
                return Err("TypeError: db.users.power_creeps.remove is not a function".into());
            }
            let mut o = pc.obj.clone();
            obj_set(&mut o, "deleteTime", Val::Num(now + C::POWER_CREEP_DELETE_COOLDOWN as f64));
            put_pc(w, u, o);
            Ok(())
        }),
        |_| Json::Null,
    )
}

/// POST /api/game/power-creeps/cancel-delete
pub fn cancel_delete(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let id = req.body().get("id").and_then(Json::as_str).map(str::to_string);
    reply(
        write(ctx, move |w| {
            let pc = own_pc(w, u, id.as_deref())?;
            if !truthy(field(&pc.obj, "deleteTime")) {
                return Err("not being deleted".into());
            }
            let mut o = pc.obj.clone();
            o.remove(engine::atom!("deleteTime"));
            put_pc(w, u, o);
            Ok(())
        }),
        |_| Json::Null,
    )
}

/// `POWER_INFO[k]` for a body key ("1".."19").
fn power_info(k: &str) -> Option<&'static C::PowerInfo> {
    engine::doc::array_index(k).and_then(C::power_info)
}

/// power-creeps.js `powersAreValid(powers)`: replays level-ups in body key order.
fn powers_are_valid(powers: &[(String, f64)]) -> bool {
    let final_level: f64 = powers.iter().map(|(_, v)| v).sum();
    let mut rec: std::collections::HashMap<&str, usize> = powers.iter().map(|(k, _)| (k.as_str(), 0)).collect();
    let mut level = 0.0;
    while level < final_level {
        let next = powers.iter().find(|(k, v)| {
            let r = rec[k.as_str()];
            r < 5 && (r as f64) < *v && power_info(k).is_some_and(|pi| pi.level[r] as f64 <= level)
        });
        let Some((k, _)) = next else { return false };
        *rec.get_mut(k.as_str()).unwrap() += 1;
        level = rec.values().sum::<usize>() as f64;
    }
    true
}

/// POST /api/game/power-creeps/upgrade
pub fn upgrade(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let id = crate::socket::user::js_to_string(b.get("id"));
    // the PC is found with ==, but update({_id: body.id}) only matches a string id
    let writes = matches!(b.get("id"), Some(Json::Str(_)));
    let powers = b.get("powers").cloned();
    let user_doc = ctx.published.get().users[u as usize].doc.clone();
    reply(
        write(ctx, move |w| {
            let pcs = w.users[u as usize].power_creeps.clone();
            let creep = pcs.iter().find(|p| p.id().as_str() == id).cloned().ok_or("invalid id")?;
            let body: Vec<(String, Json)> = match &powers {
                Some(Json::Obj(m)) => m.clone(),
                Some(Json::Arr(a)) => a.iter().enumerate().map(|(i, v)| (i.to_string(), v.clone())).collect(),
                _ => return Err("invalid powers".into()),
            };
            let class = str_field(&creep.obj, "className").unwrap_or("");
            let mut current: Vec<(String, f64)> = match field(&creep.obj, "powers") {
                Some(Val::Obj(p)) => p.iter().map(|(k, v)| (k.as_str().to_string(), v.as_obj().and_then(|x| field(x, "level")).and_then(Val::as_f64).unwrap_or(f64::NAN))).collect(),
                _ => vec![],
            };
            let mut values: Vec<(String, f64)> = Vec::new();
            for (k, v) in &body {
                let pi = power_info(k).ok_or_else(|| format!("invalid power {k}"))?;
                if pi.class_name != class {
                    return Err(format!("invalid class for power {k}"));
                }
                if !current.iter().any(|(c, _)| c == k) {
                    current.push((k.clone(), 0.0));
                }
                let Json::Num(n) = v else { return Err(format!("invalid value for power {k}")) };
                let lvl = current.iter().find(|(c, _)| c == k).map_or(0.0, |(_, l)| *l);
                if *n < lvl {
                    return Err(format!("cannot downgrade power {k}"));
                }
                if *n > 5.0 {
                    return Err(format!("invalid max value for power {k}"));
                }
                values.push((k.clone(), *n));
            }
            // `for (power in creep.powers)`: power ids are integer-like keys, enumerated ascending
            current.sort_by_key(|(k, _)| engine::doc::array_index(k).map_or(u64::MAX, u64::from));
            for (k, lvl) in &current {
                let want = values.iter().find(|(x, _)| x == k).map_or(0.0, |(_, v)| *v);
                if want < *lvl {
                    return Err(format!("cannot downgrade power {k}"));
                }
            }
            let new_level: f64 = values.iter().map(|(_, v)| v).sum();
            if new_level > C::POWER_CREEP_MAX_LEVEL as f64 {
                return Err("max level".into());
            }
            let mut merge_powers = Obj::new();
            for (k, v) in &values {
                if *v == 0.0 {
                    continue;
                }
                let need = (v.fract() == 0.0 && *v >= 1.0).then(|| power_info(k).map(|pi| pi.level[*v as usize - 1] as f64)).flatten();
                if need.is_some_and(|need| new_level < need) {
                    return Err(format!("not enough level for power {k}"));
                }
                let mut l = Obj::new();
                obj_set(&mut l, "level", Val::Num(*v));
                obj_set(&mut merge_powers, k, Val::Obj(l));
            }
            if !powers_are_valid(&values) {
                return Err("powers set is not valid".into());
            }
            let level = field(&creep.obj, "level").and_then(Val::as_f64).unwrap_or(f64::NAN);
            if free_power_levels(&user_doc, &pcs) < new_level - level {
                return Err("not enough power level".into());
            }
            // $merge into users.power_creeps and the spawned rooms.objects doc
            let merge = |o: &Obj| {
                let mut o = o.clone();
                let mut p = match field(&o, "powers") {
                    Some(Val::Obj(p)) => p.clone(),
                    _ => Obj::new(),
                };
                for (k, v) in merge_powers.iter() {
                    let mut entry = p.get(k).and_then(Val::as_obj).cloned().unwrap_or_default();
                    obj_set(&mut entry, "level", v.as_obj().and_then(|x| field(x, "level")).cloned().unwrap_or(Val::Null));
                    p.set(k, Val::Obj(entry));
                }
                obj_set(&mut o, "powers", Val::Obj(p));
                obj_set(&mut o, "level", Val::Num(new_level));
                obj_set(&mut o, "hitsMax", Val::Num(1000.0 * (new_level + 1.0)));
                obj_set(&mut o, "storeCapacity", Val::Num(100.0 * (new_level + 1.0)));
                o
            };
            let modified = |n: bool| if n { obj(vec![("modified", Json::Num(1.0))]) } else { Json::Obj(vec![]) };
            if !writes {
                return Ok(Json::Arr(vec![modified(false), modified(false)]));
            }
            put_pc(w, u, merge(&creep.obj));
            let room = room_pc(w, &id);
            let second = match room {
                Some((r, d)) => {
                    let objs = w.rooms[r as usize].objects.iter().map(|x| if x.id() == d.id() { Doc::new(x.seq, merge(&x.obj)) } else { x.clone() }).collect();
                    install_room(w, r, objs, None, None);
                    true
                }
                None => false,
            };
            Ok(Json::Arr(vec![modified(true), modified(second)]))
        }),
        |r| r,
    )
}

/// POST /api/game/power-creeps/rename
pub fn rename(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let b = req.body();
    let id = crate::socket::user::js_to_string(b.get("id"));
    let writes = matches!(b.get("id"), Some(Json::Str(_)));
    let name = pc_name(b.get("name"));
    reply(
        write(ctx, move |w| {
            let pcs = w.users[u as usize].power_creeps.clone();
            let creep = pcs.iter().find(|p| p.id().as_str() == id).cloned().ok_or("invalid id")?;
            if spawned(&creep.obj) {
                return Err("spawned".into());
            }
            if pcs.iter().any(|p| str_field(&p.obj, "name") == Some(name.as_str())) {
                return Err("name already exists".into());
            }
            if !writes {
                return Ok(()); // update({_id: <non-string>}) matches nothing
            }
            let mut o = creep.obj.clone();
            obj_set(&mut o, "name", s(&name));
            put_pc(w, u, o);
            Ok(())
        }),
        |_| Json::Null,
    )
}

/// POST /api/game/power-creeps/experimentation
pub fn experimentation(req: &Request, ctx: &ApiCtx) -> Response {
    let u = auth!(ctx);
    let user_doc = ctx.published.get().users[u as usize].doc.clone();
    if field(&user_doc, "powerExperimentations").and_then(Val::as_f64).filter(|n| !n.is_nan()).unwrap_or(0.0) <= 0.0 {
        return Response::error("no power resets");
    }
    reply(
        write(ctx, move |w| {
            let now = vnow(w) as f64;
            edit_user(w, u, |st| {
                let mut d = (*st.doc).clone();
                // updateDocument: $set before $inc
                obj_set(&mut d, "powerExperimentationTime", Val::Num(now + 24.0 * 3600.0 * 1000.0));
                let n = field(&d, "powerExperimentations").and_then(Val::as_f64).filter(|n| *n != 0.0 && !n.is_nan()).unwrap_or(0.0);
                obj_set(&mut d, "powerExperimentations", Val::Num(n - 1.0));
                st.doc = Arc::new(d);
            });
            Ok(())
        }),
        |_| obj(vec![("modified", Json::Num(1.0))]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake;

    fn err(j: &Json) -> Option<&str> {
        j.get("error").and_then(Json::as_str)
    }

    /// Ids that are not strings: findOne is strict (delete/cancel-delete), the `==` find of rename/upgrade matches
    /// but their `update({_id: body.id})` writes nothing.
    #[test]
    fn non_string_ids() {
        let (ctx, h) = fake::ctx(Some(fake::ALICE));
        write(&ctx, |w| {
            edit_user(w, fake::ALICE, |st| {
                let mut d = (*st.doc).clone();
                obj_set(&mut d, "power", Val::Num(50000.0));
                st.doc = Arc::new(d);
            });
            Ok(())
        })
        .unwrap();
        let (_, pc) = fake::run(create, &ctx, "POST", "/x", r#"{"className":"operator","name":"Op"}"#);
        let id = pc.get("_id").and_then(Json::as_str).unwrap().to_string();
        assert_eq!(err(&fake::run(delete, &ctx, "POST", "/x", "{}").1), Some("invalid id"));
        assert_eq!(err(&fake::run(delete, &ctx, "POST", "/x", &format!(r#"{{"id":["{id}"]}}"#)).1), Some("invalid id"));
        assert_eq!(err(&fake::run(cancel_delete, &ctx, "POST", "/x", &format!(r#"{{"id":["{id}"]}}"#)).1), Some("invalid id"));
        assert_eq!(fake::run(rename, &ctx, "POST", "/x", &format!(r#"{{"id":["{id}"],"name":"New"}}"#)).1.stringify(), r#"{"ok":1}"#);
        assert_eq!(fake::run(upgrade, &ctx, "POST", "/x", &format!(r#"{{"id":["{id}"],"powers":{{"1":1}}}}"#)).1.stringify(), r#"{"0":{},"1":{},"ok":1}"#);
        assert_eq!(fake::run(upgrade, &ctx, "POST", "/x", &format!(r#"{{"id":"{id}","powers":{{"1":1}}}}"#)).1.stringify(), r#"{"0":{"modified":1},"1":{},"ok":1}"#);
        drop(ctx);
        let w = h.join().unwrap();
        let p = &w.users[fake::ALICE as usize].power_creeps[0];
        assert_eq!(str_field(&p.obj, "name"), Some("Op"));
        assert_eq!(field(&p.obj, "level"), Some(&Val::Num(1.0)));
    }
}
