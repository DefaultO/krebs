//! mirror: reference/engine/src/processor/intents/_calc_spawns.js
//! parity: FAITHFUL
//!
//! Also hosts `support`: the JS operator, lodash and `utils.js` semantics WP10's ports share,
//! until WP09's `crate::utils` (and `Movement::is_tile_busy`) land (interface request).

use crate::atom;
use crate::live::{Crash, O, V};
use crate::processor::RoomScope;
use support::*;

/// `module.exports = function(roomSpawns, roomExtensions, {roomController, bulk})`. Vanilla sorts
/// `roomSpawns` in place; nothing reads that order afterwards (`scope` has no roomSpawns), so the sort
/// works on a copy.
pub fn calc_spawns(room_spawns: &[O], room_extensions: &[O], scope: &mut RoomScope) -> Result<(), Crash> {
    pass(&mut room_spawns.to_vec(), "spawn", scope)?;
    pass(&mut room_extensions.to_vec(), "extension", scope)
}

/// _calc_spawns.js:7-22 (spawns) and :25-40 (extensions): the same statements.
fn pass(list: &mut [O], kind: &str, scope: &mut RoomScope) -> Result<(), Crash> {
    // `roomController.level|0`; BUG-CRASH-CALC-SPAWNS: no controller throws (private) or reads as level 0.
    let level = match &scope.room_controller {
        Some(c) => to_int32(&c.get(atom!("level"))),
        None if scope.ctx.compat.fix("BUG-CRASH-CALC-SPAWNS") => 0,
        None => return Err(crash_read(&V::Undef, "level")),
    };
    let allowed = controller_structures(kind, &n(level as f64))?;
    if gt(&n(list.len() as f64), &allowed) {
        let kept: Vec<O> = match scope.room_controller.clone() {
            Some(ctrl) => {
                stable_sort(list, |a, b| comparator_distance(&ctrl, a, b));
                take(list, &allowed)
            }
            None => Vec::new(), // the fix skips the sort; level 0 allows none
        };
        for i in list.iter() {
            i.set(atom!("_off"), V::Bool(!kept.iter().any(|k| k.same(i))));
        }
    } else {
        for i in list.iter() {
            i.set(atom!("_off"), V::Bool(false));
        }
    }
    for i in list.iter() {
        let off = i.get(atom!("_off"));
        if !strict_eq(&off, &i.get(atom!("off"))) {
            bulk_update_id(&mut scope.bulk, &i.get(atom!("_id")), &obj(vec![(atom!("off"), off)]))?;
        }
    }
    Ok(())
}

/// `_.take(array, n)` for a numeric `n`.
fn take(list: &[O], count: &V) -> Vec<O> {
    let k = to_number(count);
    let k = if k.is_nan() || k < 0.0 { 0 } else { (k as usize).min(list.len()) };
    list[..k].to_vec()
}

/// Shared semantics of WP10's ports: JS operators on `live::V`, the lodash 3.10.1 collection helpers
/// they use, driver helpers (notifications, stats, user rooms) and the processor half of `utils.js`.
/// ponytail: the `utils.js` ports duplicate what WP09's `crate::utils` will export; switch the callers
/// when it lands and delete them here (interface request).
pub(crate) mod support {
    use crate::atom;
    use crate::bulk::{Bulk, Target};
    use crate::constants::{self as C, BodyPart, ResourceType, StructureType};
    use crate::doc::{array_index, Atom, Id};
    use crate::live::{Crash, A, O, V};
    use crate::processor::RoomScope;
    use crate::terrain::RoomTerrain;
    use std::str::FromStr;

    pub use crate::live::{is_equal, strict_eq, to_js_string, to_number, truthy};

    // ---- values ----

    pub fn n(x: f64) -> V {
        V::Num(x)
    }

    pub fn s(x: &str) -> V {
        V::str(x)
    }

    /// An object literal; keys in the given order (JS order rules apply).
    pub fn obj(pairs: Vec<(Atom, V)>) -> V {
        let o = O::new();
        for (k, v) in pairs {
            o.set(k, v);
        }
        V::Obj(o)
    }

    pub fn arr(items: Vec<V>) -> V {
        V::Arr(A::from_vec(items))
    }

    /// A computed property key (`o[k]`): `ToString(k)`.
    pub fn key(k: &V) -> Atom {
        Atom::intern(&to_js_string(k))
    }

    // ---- property access ----

    /// The TypeError of `base.key` on null/undefined (V8 wording).
    pub fn crash_read(base: &V, key: &str) -> Crash {
        let what = if matches!(base, V::Null) { "null" } else { "undefined" };
        Crash(format!("Cannot read properties of {what} (reading '{key}')").into())
    }

    /// The TypeError of `base.key = v` on null/undefined.
    pub fn crash_set(base: &V, key: &str) -> Crash {
        let what = if matches!(base, V::Null) { "null" } else { "undefined" };
        Crash(format!("Cannot set properties of {what} (setting '{key}')").into())
    }

    /// `v[k]` without throwing (lodash's `baseGet` step, `_.matches`): non-objects read as undefined,
    /// strings and arrays expose `length` and their indices.
    pub fn peek(v: &V, k: Atom) -> V {
        match v {
            V::Obj(o) => o.get(k),
            V::Arr(a) if k == atom!("length") => n(a.len() as f64),
            V::Arr(a) => k.array_index().map_or(V::Undef, |i| a.get(i as usize)),
            V::Str(st) if k == atom!("length") => n(st.encode_utf16().count() as f64),
            V::Str(st) => k
                .array_index()
                .and_then(|i| st.encode_utf16().nth(i as usize))
                .map_or(V::Undef, |u| V::Str(String::from_utf16_lossy(&[u]).into())),
            _ => V::Undef,
        }
    }

    /// `v.k` (TypeError on null/undefined).
    pub fn get(v: &V, k: Atom) -> Result<V, Crash> {
        match v {
            V::Undef | V::Null => Err(crash_read(v, k.as_str())),
            _ => Ok(peek(v, k)),
        }
    }

    /// `v.k = x` in sloppy mode: ignored on primitives, TypeError on null/undefined.
    pub fn put(v: &V, k: Atom, x: V) -> Result<(), Crash> {
        match v {
            V::Undef | V::Null => Err(crash_set(v, k.as_str())),
            V::Obj(o) => {
                o.set(k, x);
                Ok(())
            }
            V::Arr(a) => {
                if let Some(i) = k.array_index() {
                    a.set(i as usize, x);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    // ---- operators ----

    fn to_primitive(v: &V) -> V {
        match v {
            V::Obj(_) | V::Arr(_) => V::Str(to_js_string(v)),
            _ => v.clone(),
        }
    }

    /// Abstract relational comparison `a < b`; None = undefined (a NaN operand).
    fn less(a: &V, b: &V) -> Option<bool> {
        let (pa, pb) = (to_primitive(a), to_primitive(b));
        if let (V::Str(x), V::Str(y)) = (&pa, &pb) {
            return Some(x.encode_utf16().lt(y.encode_utf16()));
        }
        let (x, y) = (to_number(&pa), to_number(&pb));
        (!x.is_nan() && !y.is_nan()).then_some(x < y)
    }

    pub fn lt(a: &V, b: &V) -> bool {
        less(a, b) == Some(true)
    }

    pub fn gt(a: &V, b: &V) -> bool {
        less(b, a) == Some(true)
    }

    pub fn le(a: &V, b: &V) -> bool {
        less(b, a) == Some(false)
    }

    pub fn ge(a: &V, b: &V) -> bool {
        less(a, b) == Some(false)
    }

    /// `a == b`.
    pub fn loose_eq(a: &V, b: &V) -> bool {
        match (a, b) {
            (V::Undef | V::Null, V::Undef | V::Null) => true,
            (V::Undef | V::Null, _) | (_, V::Undef | V::Null) => false,
            (V::Bool(x), _) => loose_eq(&n(*x as u8 as f64), b),
            (_, V::Bool(y)) => loose_eq(a, &n(*y as u8 as f64)),
            (V::Num(x), V::Str(_)) => *x == to_number(b),
            (V::Str(_), V::Num(y)) => to_number(a) == *y,
            (V::Obj(_) | V::Arr(_), V::Num(_) | V::Str(_)) => loose_eq(&to_primitive(a), b),
            (V::Num(_) | V::Str(_), V::Obj(_) | V::Arr(_)) => loose_eq(a, &to_primitive(b)),
            _ => strict_eq(a, b),
        }
    }

    /// `a + b`.
    pub fn add(a: &V, b: &V) -> V {
        let (pa, pb) = (to_primitive(a), to_primitive(b));
        if matches!(pa, V::Str(_)) || matches!(pb, V::Str(_)) {
            V::Str(format!("{}{}", to_js_string(&pa), to_js_string(&pb)).into())
        } else {
            n(to_number(&pa) + to_number(&pb))
        }
    }

    /// `ToInt32(v)` (`v | 0`).
    pub fn to_int32(v: &V) -> i32 {
        let x = to_number(v);
        if !x.is_finite() {
            return 0;
        }
        (x.trunc().rem_euclid(4294967296.0) as u32) as i32
    }

    /// `Math.round`: halves round towards +Infinity.
    pub fn js_round(x: f64) -> f64 {
        if !x.is_finite() {
            return x;
        }
        let f = x.floor();
        if x - f >= 0.5 {
            f + 1.0
        } else {
            f
        }
    }

    /// `Math.min(a, b)` (NaN wins, -0 < +0).
    pub fn js_min(a: f64, b: f64) -> f64 {
        if a.is_nan() || b.is_nan() {
            f64::NAN
        } else if a < b || (a == b && a.is_sign_negative()) {
            a
        } else {
            b
        }
    }

    /// `Math.max(a, b)` (NaN wins, +0 > -0).
    pub fn js_max(a: f64, b: f64) -> f64 {
        if a.is_nan() || b.is_nan() {
            f64::NAN
        } else if a > b || (a == b && b.is_sign_negative()) {
            a
        } else {
            b
        }
    }

    /// `Math.max(Math.abs(a.x - b.x), Math.abs(a.y - b.y))` (utils.dist and its inline copies).
    pub fn cheb(a: &O, b: &O) -> f64 {
        let d = |k| (a.num(k) - b.num(k)).abs();
        js_max(d(atom!("x")), d(atom!("y")))
    }

    /// `Array.prototype.join(sep)`.
    pub fn join(v: &V, sep: &str) -> String {
        match v {
            V::Arr(a) => a
                .to_vec()
                .iter()
                .map(|x| if matches!(x, V::Undef | V::Null) { String::new() } else { to_js_string(x).to_string() })
                .collect::<Vec<_>>()
                .join(sep),
            _ => to_js_string(v).to_string(),
        }
    }

    /// `parseInt(s, 10)`.
    pub fn parse_int10(s: &str) -> f64 {
        let t = s.trim_start_matches(util::num::is_js_whitespace);
        let (neg, t) = match t.as_bytes().first() {
            Some(b'-') => (true, &t[1..]),
            Some(b'+') => (false, &t[1..]),
            _ => (false, t),
        };
        let d = t.bytes().take_while(u8::is_ascii_digit).count();
        if d == 0 {
            return f64::NAN;
        }
        let x: f64 = t[..d].parse().unwrap_or(f64::NAN);
        if neg {
            -x
        } else {
            x
        }
    }

    // ---- lodash 3.10.1 ----

    /// The values a lodash collection function visits: array elements, an object's own values in key
    /// order (snapshot), a string's characters; nothing for other values.
    pub fn values(v: &V) -> Vec<V> {
        match v {
            V::Arr(a) => a.to_vec(),
            V::Obj(o) => o.keys().into_iter().map(|k| o.get(k)).collect(),
            V::Str(st) => st.encode_utf16().map(|u| V::Str(String::from_utf16_lossy(&[u]).into())).collect(),
            _ => Vec::new(),
        }
    }

    /// `_.matches(source)(v)` for a source of primitives: `===` per key (NaN matches NaN), and an
    /// undefined source value needs the key present.
    pub fn matches(v: &V, src: &[(Atom, V)]) -> bool {
        if matches!(v, V::Undef | V::Null) {
            return src.is_empty();
        }
        src.iter().all(|(k, want)| {
            let have = peek(v, *k);
            match want {
                V::Num(x) if x.is_nan() => matches!(have, V::Num(y) if y.is_nan()),
                V::Undef => matches!(have, V::Undef) && matches!(v, V::Obj(o) if o.has(*k)),
                _ => strict_eq(&have, want),
            }
        })
    }

    /// `_.find(collection, {k: v, …})`.
    pub fn find_by(collection: &V, src: &[(Atom, V)]) -> Option<V> {
        values(collection).into_iter().find(|x| matches(x, src))
    }

    /// `_.find(object.effects, {power})`.
    pub fn find_power_effect(object: &O, power: u32) -> Option<V> {
        find_by(&object.get(atom!("effects")), &[(atom!("power"), n(power as f64))])
    }

    /// `_.sum(collection, iteratee)` with a one-argument iteratee (`arraySum`: last element first,
    /// `+value || 0`).
    pub fn sum(items: &[V], mut f: impl FnMut(&V) -> Result<V, Crash>) -> Result<f64, Crash> {
        let mut total = 0.0;
        for x in items.iter().rev() {
            let v = to_number(&f(x)?);
            total += if v.is_nan() { 0.0 } else { v };
        }
        Ok(total)
    }

    /// `_.sum(object)`: the values of a store.
    pub fn sum_values(v: &V) -> f64 {
        sum(&values(v), |x| Ok(x.clone())).unwrap()
    }

    /// `_.uniq(array)` (`===`, NaN equal to NaN).
    pub fn uniq(items: Vec<V>) -> Vec<V> {
        let mut out: Vec<V> = Vec::new();
        for x in items {
            let nan = matches!(x, V::Num(f) if f.is_nan());
            if !out.iter().any(|y| strict_eq(y, &x) || (nan && matches!(y, V::Num(f) if f.is_nan()))) {
                out.push(x);
            }
        }
        out
    }

    /// `_.contains(array, x)` (`===`, NaN found).
    pub fn contains(items: &[V], x: &V) -> bool {
        let nan = matches!(x, V::Num(f) if f.is_nan());
        items.iter().any(|y| strict_eq(y, x) || (nan && matches!(y, V::Num(f) if f.is_nan())))
    }

    /// `Array.prototype.sort(cmp)` as V8 runs it for room-sized arrays: stable binary insertion, an
    /// element goes before another only when `cmp(pivot, other) < 0` (so a NaN result never panics).
    /// ponytail: V8's TimSort runs and galloping only differ for inconsistent comparators.
    pub fn stable_sort<T: Clone>(list: &mut [T], cmp: impl Fn(&T, &T) -> f64) {
        for i in 1..list.len() {
            let pivot = list[i].clone();
            let (mut lo, mut hi) = (0, i);
            while lo < hi {
                let mid = (lo + hi) / 2;
                if cmp(&pivot, &list[mid]) < 0.0 {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            list[lo..=i].rotate_right(1);
        }
    }

    // ---- roomObjects (key-order scans) ----

    /// The values of `roomObjects` in key order (snapshot).
    /// ponytail: every `_.find`/`_.filter` is a full O(objects) scan, as in vanilla (07 §16 PERF); add a
    /// per-tile index when the proc_room bench shows these scans.
    pub fn room_values(scope: &RoomScope) -> Vec<O> {
        scope.room_objects.iter().cloned().collect()
    }

    /// `_.find(roomObjects, pred)`: stops at the first match (no other lookups or clones).
    pub fn room_find(scope: &RoomScope, mut pred: impl FnMut(&O) -> bool) -> Option<O> {
        scope.room_objects.iter().find(|o| pred(o)).cloned()
    }

    pub fn room_any(scope: &RoomScope, pred: impl FnMut(&O) -> bool) -> bool {
        room_find(scope, pred).is_some()
    }

    pub fn room_filter(scope: &RoomScope, mut pred: impl FnMut(&O) -> bool) -> Vec<O> {
        scope.room_objects.iter().filter(|o| pred(o)).cloned().collect()
    }

    /// `_.matches` against a room object.
    pub fn room_match(o: &O, src: &[(Atom, V)]) -> bool {
        matches(&V::Obj(o.clone()), src)
    }

    /// `roomObjects[id]`.
    pub fn by_key(scope: &RoomScope, id: &V) -> Option<O> {
        scope.room_objects.get(&to_js_string(id))
    }

    /// `roomObjects` is a plain `{}` (driver mapById): a key it does not own can still name an
    /// Object.prototype member, a truthy value whose game fields (`type`, `store`, …) are all undefined.
    pub fn inherited(id: &V) -> bool {
        matches!(
            &*to_js_string(id),
            "constructor"
                | "__defineGetter__"
                | "__defineSetter__"
                | "hasOwnProperty"
                | "__lookupGetter__"
                | "__lookupSetter__"
                | "isPrototypeOf"
                | "propertyIsEnumerable"
                | "toString"
                | "valueOf"
                | "__proto__"
                | "toLocaleString"
        )
    }

    /// `bulk.remove(object._id); delete roomObjects[object._id];`
    pub fn remove_object(scope: &mut RoomScope, object: &O) {
        let id = object.get(atom!("_id"));
        bulk_remove(&mut scope.bulk, &id);
        scope.room_objects.remove(&to_js_string(&id));
    }

    /// `bulk.remove(id)`: a falsy id is ignored.
    pub fn bulk_remove(bulk: &mut Bulk, id: &V) {
        if truthy(id) {
            bulk.remove(&to_js_string(id));
        }
    }

    /// `bulk.update(id, data)` with an id value: a falsy id is ignored, memory is untouched.
    pub fn bulk_update_id(bulk: &mut Bulk, id: &V, data: &V) -> Result<(), Crash> {
        if !truthy(id) {
            return Ok(());
        }
        bulk.update(Target::Id(&to_js_string(id)), data)
    }

    /// `bulk.update(object, data)`.
    pub fn update(bulk: &mut Bulk, object: &O, data: Vec<(Atom, V)>) -> Result<(), Crash> {
        bulk.update(Target::Obj(object), &obj(data))
    }

    /// `eventLog.push({event, objectId, data})`.
    pub fn event(scope: &mut RoomScope, event: u32, object_id: V, data: Option<V>) {
        let mut pairs = vec![(atom!("event"), n(event as f64)), (atom!("objectId"), object_id)];
        if let Some(d) = data {
            pairs.push((atom!("data"), d));
        }
        scope.event_log.push(obj(pairs));
    }

    // ---- driver ----

    /// `users[uid]` of the room's `getRoomObjects` result.
    pub fn user_doc(scope: &RoomScope, uid: &V) -> Option<O> {
        let uid = to_js_string(uid);
        scope.users.iter().find(|(_, u)| matches!(u.get(atom!("_id")), V::Str(s) if *s == *uid)).map(|(_, u)| u.clone())
    }

    /// The id string of a user index (room intents receive `userId`).
    pub fn user_id(scope: &RoomScope, user: crate::world::UserIdx) -> V {
        V::str(scope.ctx.users[user as usize].id.as_str())
    }

    fn user_idx(scope: &RoomScope, uid: &V) -> Option<crate::world::UserIdx> {
        Id::new(&to_js_string(uid)).and_then(|id| scope.ctx.user_ids.get(&id).copied())
    }

    /// `driver.sendNotification(userId, message)` (fire and forget; unknown users have no inbox).
    pub fn send_notification(scope: &mut RoomScope, uid: &V, message: String) {
        if let Some(u) = user_idx(scope, uid) {
            scope.out.notifications.push((u, message.into()));
        }
    }

    /// `stats.inc(name, userId, amount)` (POST-STATS: mods only).
    pub fn stats_inc(scope: &mut RoomScope, name: &str, uid: &V, amount: f64) {
        if let Some(u) = user_idx(scope, uid) {
            scope.out.stats.push((u, Atom::intern(name), amount));
        }
    }

    /// `user.rooms.indexOf(room)` for an array (or a string, which has indexOf too).
    fn index_of(rooms: &V, room: &V) -> Result<f64, Crash> {
        match rooms {
            V::Arr(a) => Ok(a.to_vec().iter().position(|x| strict_eq(x, room)).map_or(-1.0, |i| i as f64)),
            V::Str(st) => Ok(st.find(&*to_js_string(room)).map_or(-1.0, |i| st[..i].encode_utf16().count() as f64)),
            _ => Err(Crash("user.rooms.indexOf is not a function".into())),
        }
    }

    /// driver/lib/index.js:542 addRoomToUser(roomId, users[uid], bulkUsers).
    pub fn add_room_to_user(scope: &mut RoomScope, room: &V, uid: &V) -> Result<(), Crash> {
        let user = user_doc(scope, uid).ok_or_else(|| crash_read(&V::Undef, "rooms"))?;
        let rooms = user.get(atom!("rooms"));
        if !truthy(&rooms) || index_of(&rooms, room)? == -1.0 {
            scope.bulk_users.add_to_set(Target::Obj(&user), atom!("rooms"), room);
        }
        Ok(())
    }

    /// driver/lib/index.js:548 removeRoomFromUser(roomId, users[uid], bulkUsers).
    pub fn remove_room_from_user(scope: &mut RoomScope, room: &V, uid: &V) -> Result<(), Crash> {
        let user = user_doc(scope, uid).ok_or_else(|| crash_read(&V::Undef, "rooms"))?;
        let rooms = user.get(atom!("rooms"));
        if truthy(&rooms) && index_of(&rooms, room)? != -1.0 {
            scope.bulk_users.pull(Target::Obj(&user), atom!("rooms"), room);
        }
        Ok(())
    }

    /// `Date.now()` in room processing: vclock(T) (CLOCK-POLICY).
    pub fn date_now(scope: &RoomScope) -> V {
        n(scope.ctx.vnow as f64)
    }

    /// `roomInfo.novice && roomInfo.novice > Date.now() || roomInfo.respawnArea && roomInfo.respawnArea > Date.now()`.
    pub fn novice_or_respawn(scope: &RoomScope) -> bool {
        let now = date_now(scope);
        let novice = scope.room_info.get(atom!("novice"));
        let respawn = scope.room_info.get(atom!("respawnArea"));
        truthy(&novice) && gt(&novice, &now) || truthy(&respawn) && gt(&respawn, &now)
    }

    // ---- constants as JS objects ----

    /// `table[k]` of an integer-keyed constant (key `ToString(k)`).
    pub fn by_index<T: Copy + Into<f64>>(table: &[Option<T>], k: &V) -> V {
        if let Some(i) = crate::live::tile_coord(k) {
            return table.get(i as usize).copied().flatten().map_or(V::Undef, |x| n(x.into()));
        }
        array_index(&to_js_string(k)).and_then(|i| table.get(i as usize).copied().flatten()).map_or(V::Undef, |x| n(x.into()))
    }

    /// `C.CONTROLLER_STRUCTURES[type][level]` (TypeError for a type without a row).
    pub fn controller_structures(kind: &str, level: &V) -> Result<V, Crash> {
        let row = StructureType::from_str(kind)
            .ok()
            .and_then(|t| C::CONTROLLER_STRUCTURES.iter().find(|(r, _)| *r == t))
            .ok_or_else(|| crash_read(&V::Undef, &to_js_string(level)))?;
        Ok(by_index(&row.1, level))
    }

    /// `C.CONSTRUCTION_COST[type]` (undefined for other names).
    pub fn construction_cost(kind: &V) -> V {
        match kind {
            V::Str(st) => StructureType::from_str(st).ok().and_then(C::construction_cost).map_or(V::Undef, |c| n(c as f64)),
            _ => construction_cost(&V::Str(to_js_string(kind))),
        }
    }

    /// `C.POWER_INFO[power].effect[level-1]`.
    pub fn power_effect(power: u32, level: &V) -> V {
        let effect = C::power_info(power).and_then(|p| p.effect);
        let i = to_number(level) - 1.0;
        match (effect, array_index(&util::num::to_js_string(i))) {
            (Some(e), Some(i)) if (i as usize) < e.len() => n(e[i as usize]),
            _ => V::Undef,
        }
    }

    pub fn is_obstacle_type(kind: &V) -> bool {
        matches!(kind, V::Str(st) if C::OBSTACLE_OBJECT_TYPES.contains(&&**st))
    }

    pub fn resource(kind: &V) -> Option<ResourceType> {
        match kind {
            V::Str(st) => ResourceType::from_str(st).ok(),
            _ => None,
        }
    }

    // ---- utils.js (processor half; WP09 owns crate::utils) ----

    /// utils.js:333 checkTerrain(terrain, x, y, mask) on the processor's terrain string: `charAt(y*50 + x)`,
    /// "" (0) outside it.
    pub fn check_terrain(terrain: &RoomTerrain, x: &V, y: &V, mask: u8) -> bool {
        let p = to_number(&add(&n(to_number(y) * 50.0), x));
        let p = if p.is_nan() { 0.0 } else { p.trunc() };
        if !(0.0..2500.0).contains(&p) {
            return false;
        }
        let p = p as u32;
        terrain.raw((p % 50) as u8, (p / 50) as u8) & mask != 0
    }

    /// utils.js:65 getOffsetsByDirection (undefined for anything but 1..8).
    pub fn offsets_by_direction(direction: &V) -> Option<(f64, f64)> {
        const OFFSETS: [(f64, f64); 8] = [(0.0, -1.0), (1.0, -1.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0), (-1.0, -1.0)];
        match array_index(&to_js_string(direction)) {
            Some(d @ 1..=8) => Some(OFFSETS[d as usize - 1]),
            _ => None,
        }
    }

    /// utils.js:114 calcCreepCost(body).
    pub fn calc_creep_cost(body: &V) -> Result<f64, Crash> {
        let V::Arr(parts) = body else { return Err(Crash("body.forEach is not a function".into())) };
        let mut result = 0.0;
        for i in parts.to_vec() {
            let part = if matches!(i, V::Obj(_) | V::Arr(_)) { peek(&i, atom!("type")) } else { i };
            match BodyPart::from_str(&to_js_string(&part)) {
                Ok(p) => result += p.cost() as f64,
                Err(()) => return Err(Crash(format!("Invalid body part {}", to_js_string(&part)).into())),
            }
        }
        Ok(result)
    }

    /// utils.js:394 comparatorDistance(target)(a, b) for objects without `pos`.
    pub fn comparator_distance(target: &O, a: &O, b: &O) -> f64 {
        cheb(a, target) - cheb(b, target)
    }

    /// utils.js:374 roomNameToXY.
    pub fn room_name_to_xy(name: &str) -> (f64, f64) {
        let units: Vec<u16> = name.encode_utf16().collect();
        let sub = |from: usize| String::from_utf16_lossy(units.get(from..).unwrap_or(&[]));
        let char_at = |i: usize| units.get(i).copied();
        let mut xx = parse_int10(&sub(1));
        let vertical = if xx >= 100.0 {
            4
        } else if xx >= 10.0 {
            3
        } else {
            2
        };
        let mut yy = parse_int10(&sub(vertical + 1));
        if matches!(char_at(0), Some(c) if c == 'W' as u16 || c == 'w' as u16) {
            xx = -xx - 1.0;
        }
        if matches!(char_at(vertical), Some(c) if c == 'N' as u16 || c == 'n' as u16) {
            yy = -yy - 1.0;
        }
        (xx, yy)
    }

    /// utils.js:644 calcRoomsDistance(room1, room2, continuous).
    pub fn calc_rooms_distance(scope: &RoomScope, room1: &str, room2: &str, continuous: bool) -> f64 {
        let (x1, y1) = room_name_to_xy(room1);
        let (x2, y2) = room_name_to_xy(room2);
        let mut dx = (x2 - x1).abs();
        let mut dy = (y2 - y1).abs();
        if continuous {
            let world_size = scope.ctx.meta.world_size as f64;
            dx = js_min(world_size - dx, dx);
            dy = js_min(world_size - dy, dy);
        }
        js_max(dx, dy)
    }

    /// utils.js:657 calcTerminalEnergyCost. ponytail: `f64::exp` is the platform libm, V8's Math.exp is
    /// fdlibm; they differ in the last ulp for ~10 % of integer ranges, but a check of every range 0..=300
    /// with every amount 1..=300000 against node 22 found no different cost. Port fdlibm `exp` if a
    /// non-integer range or a larger amount ever reaches here.
    pub fn calc_terminal_energy_cost(amount: f64, range: f64) -> f64 {
        (amount * (1.0 - (-range / 30.0).exp())).ceil()
    }

    /// utils.js:615 calcResources(object).
    pub fn calc_resources(object: &O) -> Result<f64, Crash> {
        let store = object.get(atom!("store"));
        if truthy(&store) {
            return Ok(sum_values(&store));
        }
        let all: Vec<V> = C::RESOURCES_ALL.iter().map(|r| s(r.as_str())).collect();
        // `typeof object[i] == 'object' ? object[i].amount : (object[i] || 0)`: typeof null is 'object'
        sum(&all, |r| {
            let v = object.get(key(r));
            match v {
                V::Obj(_) | V::Arr(_) | V::Null => get(&v, atom!("amount")),
                _ if truthy(&v) => Ok(v),
                _ => Ok(n(0.0)),
            }
        })
    }

    /// utils.js:700 getReactionVariants(compound): `[r1, r2]` pairs in REACTIONS key order.
    pub fn get_reaction_variants(compound: &V) -> Vec<(ResourceType, ResourceType)> {
        let mut out = Vec::new();
        for (r1, row) in C::REACTIONS {
            for (r2, product) in row.iter() {
                if loose_eq(&s(product.as_str()), compound) {
                    out.push((*r1, *r2));
                }
            }
        }
        out
    }

    /// utils.js:665 calcTotalReactionsTime(mineral).
    pub fn calc_total_reactions_time(mineral: ResourceType) -> f64 {
        // reagents[product] = the last [r2, r1] in REACTIONS order.
        let reagents = |m: ResourceType| {
            let mut last = None;
            for (r1, row) in C::REACTIONS {
                for (r2, product) in row.iter() {
                    if *product == m {
                        last = Some((*r2, *r1));
                    }
                }
            }
            last
        };
        fn step(m: ResourceType, reagents: &dyn Fn(ResourceType) -> Option<(ResourceType, ResourceType)>) -> f64 {
            match (C::reaction_time(m), reagents(m)) {
                (Some(t), Some((a, b))) if t != 0 => t as f64 + step(a, reagents) + step(b, reagents),
                _ => 0.0,
            }
        }
        step(mineral, &reagents)
    }

    /// utils.js:456 checkStructureAgainstController(object, roomObjects, roomController).
    pub fn check_structure_against_controller(object: &O, scope: &RoomScope) -> Result<bool, Crash> {
        if !truthy(&object.get(atom!("user"))) {
            return Ok(true);
        }
        let Some(ctrl) = &scope.room_controller else { return Ok(false) };
        if lt(&ctrl.get(atom!("level")), &n(1.0)) || !strict_eq(&ctrl.get(atom!("user")), &object.get(atom!("user"))) {
            return Ok(false);
        }
        let kind = to_js_string(&object.get(atom!("type")));
        let mut allowed = controller_structures(&kind, &ctrl.get(atom!("level")))?;
        if strict_eq(&allowed, &n(0.0)) {
            return Ok(false);
        }
        if strict_eq(&controller_structures(&kind, &n(8.0))?, &n(1.0)) {
            return Ok(true);
        }
        let mut found_self = false;
        let object_dist = cheb(object, ctrl);
        let (ty, user) = (object.get(atom!("type")), object.get(atom!("user")));
        let same_type = |o: &O| match &ty {
            V::Str(t) => o.is_str(atom!("type"), t),
            _ => strict_eq(&o.get(atom!("type")), &ty),
        };
        for compare in scope.room_objects.iter() {
            if same_type(compare) && strict_eq(&compare.get(atom!("user")), &user) {
                let compare_dist = cheb(compare, ctrl);
                if compare_dist < object_dist {
                    allowed = n(to_number(&allowed) - 1.0);
                    if strict_eq(&allowed, &n(0.0)) {
                        return Ok(false);
                    }
                } else if !found_self && compare_dist == object_dist {
                    if object.same(compare) {
                        found_self = true;
                    } else {
                        allowed = n(to_number(&allowed) - 1.0);
                        if strict_eq(&allowed, &n(0.0)) {
                            return Ok(false);
                        }
                    }
                }
            }
        }
        Ok(true)
    }

    /// utils.js:338 checkControllerAvailability(type, roomObjects, roomController) (no offset).
    pub fn check_controller_availability(kind: &V, scope: &RoomScope) -> Result<bool, Crash> {
        let mut rcl = n(0.0);
        if let Some(c) = &scope.room_controller {
            if truthy(&c.get(atom!("level"))) && (truthy(&c.get(atom!("user"))) || truthy(&c.get(atom!("owner")))) {
                rcl = c.get(atom!("level"));
            }
        }
        let count = room_values(scope)
            .iter()
            .filter(|i| {
                let t = i.get(atom!("type"));
                loose_eq(&t, kind) || loose_eq(&t, &s("constructionSite")) && loose_eq(&i.get(atom!("structureType")), kind)
            })
            .count();
        let available = add(&controller_structures(&to_js_string(kind), &rcl)?, &n(0.0));
        Ok(lt(&n(count as f64), &available))
    }

    /// utils.js:131-136: the edge tiles next to (x, y) that must be walls.
    fn border_tiles(kind: &V, x: &V, y: &V) -> Option<[(V, V); 3]> {
        let is = |v: &V, k: f64| loose_eq(v, &n(k));
        if loose_eq(kind, &s("road")) || loose_eq(kind, &s("container")) || !(is(x, 1.0) || is(x, 48.0) || is(y, 1.0) || is(y, 48.0)) {
            return None;
        }
        let minus = |v: &V| n(to_number(v) - 1.0);
        let plus = |v: &V| add(v, &n(1.0));
        let mut tiles = None;
        if is(x, 1.0) {
            tiles = Some([(n(0.0), minus(y)), (n(0.0), y.clone()), (n(0.0), plus(y))]);
        }
        if is(x, 48.0) {
            tiles = Some([(n(49.0), minus(y)), (n(49.0), y.clone()), (n(49.0), plus(y))]);
        }
        if is(y, 1.0) {
            tiles = Some([(minus(x), n(0.0)), (x.clone(), n(0.0)), (plus(x), n(0.0))]);
        }
        if is(y, 48.0) {
            tiles = Some([(minus(x), n(49.0)), (x.clone(), n(49.0)), (plus(x), n(49.0))]);
        }
        tiles
    }

    /// utils.js:128 checkConstructionSite(terrainString, type, x, y).
    pub fn check_construction_site_terrain(terrain: &RoomTerrain, kind: &V, x: &V, y: &V) -> bool {
        if let Some(tiles) = border_tiles(kind, x, y) {
            if tiles.iter().any(|(bx, by)| !check_terrain(terrain, bx, by, C::TERRAIN_MASK_WALL)) {
                return false;
            }
        }
        if loose_eq(kind, &s("extractor")) {
            return true;
        }
        loose_eq(kind, &s("road")) || !check_terrain(terrain, x, y, C::TERRAIN_MASK_WALL)
    }

    /// utils.js:128 checkConstructionSite(roomObjects, type, x, y).
    pub fn check_construction_site_objects(scope: &RoomScope, kind: &V, x: &V, y: &V) -> bool {
        let at = |t: V| vec![(atom!("x"), x.clone()), (atom!("y"), y.clone()), (atom!("type"), t)];
        if room_any(scope, |o| room_match(o, &at(kind.clone()))) {
            return false;
        }
        if room_any(scope, |o| room_match(o, &at(s("constructionSite")))) {
            return false;
        }
        if loose_eq(kind, &s("extractor")) {
            return room_any(scope, |o| room_match(o, &at(s("mineral")))) && !room_any(scope, |o| room_match(o, &at(s("extractor"))));
        }
        if !loose_eq(kind, &s("rampart"))
            && !loose_eq(kind, &s("road"))
            && room_any(scope, |i| {
                let t = i.get(atom!("type"));
                loose_eq(&i.get(atom!("x")), x)
                    && loose_eq(&i.get(atom!("y")), y)
                    && !loose_eq(&t, &s("rampart"))
                    && !loose_eq(&t, &s("road"))
                    && truthy(&construction_cost(&t))
            })
        {
            return false;
        }
        if le(x, &n(0.0)) || le(y, &n(0.0)) || ge(x, &n(49.0)) || ge(y, &n(49.0)) {
            return false;
        }
        true
    }

    /// utils.js:438 sendAttackingNotification(target, roomController).
    pub fn send_attacking_notification(scope: &mut RoomScope, target: &O) {
        let kind = target.get(atom!("type"));
        let label = if loose_eq(&kind, &s("creep")) {
            format!("creep {}", to_js_string(&target.get(atom!("name"))))
        } else if loose_eq(&kind, &s("spawn")) {
            format!("spawn {}", to_js_string(&target.get(atom!("name"))))
        } else {
            format!("{} #{}", to_js_string(&kind), to_js_string(&target.get(atom!("_id"))))
        };
        let user = target.get(atom!("user"));
        let user = if truthy(&user) {
            user
        } else {
            match &scope.room_controller {
                Some(c) => c.get(atom!("user")),
                None => V::Null,
            }
        };
        if truthy(&user) {
            let msg = format!("Your {label} in room {} is under attack!", to_js_string(&target.get(atom!("room"))));
            send_notification(scope, &user, msg);
        }
    }

    /// `_(store).keys().filter(k => k != 'energy' && store[k]).first()` (labs: the mineral type).
    /// ponytail: stores are plain objects; an array or string store would list its indices.
    pub fn mineral_type(store: &V) -> V {
        match store {
            V::Obj(o) => o
                .keys()
                .into_iter()
                .find(|k| k.as_str() != C::RESOURCE_ENERGY && truthy(&o.get(*k)))
                .map_or(V::Undef, |k| s(k.as_str())),
            _ => V::Undef,
        }
    }

    /// `ToString(n)` as JS prints numbers (messages, keys).
    pub fn num_str(x: f64) -> String {
        util::num::to_js_string(x)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Expected values are what node prints for the same expressions.
        #[test]
        fn js_operators() {
            let (u, null, nan) = (V::Undef, V::Null, n(f64::NAN));
            let one_arr = arr(vec![n(1.0)]);
            assert!(loose_eq(&null, &u) && loose_eq(&n(0.0), &s("")) && loose_eq(&s("1"), &n(1.0)) && loose_eq(&one_arr, &n(1.0)));
            assert!(loose_eq(&V::Bool(true), &n(1.0)) && !loose_eq(&null, &n(0.0)) && !loose_eq(&nan, &nan) && !loose_eq(&u, &n(0.0)));
            assert!(lt(&s("10"), &s("9")) && !lt(&n(10.0), &s("9")) && ge(&null, &n(0.0)) && !ge(&u, &n(0.0)) && lt(&null, &n(1.0)));
            assert!(!lt(&nan, &n(1.0)) && !ge(&nan, &n(1.0)) && le(&n(1.0), &n(1.0)) && gt(&s("b"), &s("a")));
            assert!(matches!(add(&s("5"), &n(1.0)), V::Str(x) if &*x == "51"));
            assert!(matches!(add(&null, &n(1.0)), V::Num(x) if x == 1.0));
            assert!(matches!(add(&u, &n(1.0)), V::Num(x) if x.is_nan()));
            assert!(matches!(add(&V::Bool(true), &n(1.0)), V::Num(x) if x == 2.0));
            let int32 = |x: f64| to_int32(&n(x));
            assert_eq!((int32(2.7), int32(-2.7), int32(4294967297.0), int32(2147483648.0), int32(f64::NAN)), (2, -2, 1, -2147483648, 0));
            assert_eq!(to_int32(&s("3")), 3);
            assert_eq!((js_round(-2.5), js_round(2.5), js_round(0.49999999999999994), js_round(-0.6)), (-2.0, 3.0, 0.0, -1.0));
            assert!(js_min(1.0, f64::NAN).is_nan() && js_min(0.0, -0.0).is_sign_negative() && js_max(-0.0, 0.0).is_sign_positive());
            assert_eq!((parse_int10("  -12px"), parse_int10("0x10")), (-12.0, 0.0));
            assert!(parse_int10("").is_nan());
            assert_eq!(join(&arr(vec![n(1.0), V::Null, s("a"), V::Undef, arr(vec![n(2.0), n(3.0)])]), "~"), "1~~a~~2,3");
        }

        #[test]
        fn lodash_and_utils() {
            let mut v = vec![(3.0, 'a'), (1.0, 'b'), (3.0, 'c'), (f64::NAN, 'd'), (1.0, 'e')];
            stable_sort(&mut v, |a, b| a.0 - b.0); // node 22 gives the same order, NaN included
            assert_eq!(v.iter().map(|x| x.1).collect::<String>(), "beacd");
            assert_eq!(uniq(vec![n(1.0), s("1"), n(1.0), n(f64::NAN), n(f64::NAN)]).len(), 3);
            assert!(matches(&obj(vec![(atom!("a"), V::Undef)]), &[(atom!("a"), V::Undef)]) && !matches(&obj(vec![]), &[(atom!("a"), V::Undef)]));
            assert_eq!(sum(&[n(1.0), s("2"), s("x"), V::Undef], |x| Ok(x.clone())).unwrap(), 3.0);
            let mut t = RoomTerrain::default();
            t.set_raw(0, 1, C::TERRAIN_MASK_WALL);
            // charAt(y*50 + x): x 50 aliases into the next row; outside the string reads as plain
            assert!(check_terrain(&t, &n(50.0), &n(0.0), C::TERRAIN_MASK_WALL) && check_terrain(&t, &n(0.4), &n(1.0), C::TERRAIN_MASK_WALL));
            assert!(!check_terrain(&t, &n(-1.0), &n(0.0), C::TERRAIN_MASK_WALL) && !check_terrain(&t, &n(0.0), &n(50.0), C::TERRAIN_MASK_WALL));
            assert_eq!((room_name_to_xy("W1N1"), room_name_to_xy("E10S5"), room_name_to_xy("W100N0")), ((-2.0, -2.0), (10.0, 5.0), (-101.0, -1.0)));
            assert_eq!(calc_terminal_energy_cost(1000.0, 10.0), 284.0);
            assert_eq!(calc_total_reactions_time(ResourceType::XGH2O), 140.0); // node: XGH2O GH2O GH OH G ZK UL
            assert_eq!(get_reaction_variants(&s("G")), vec![(ResourceType::ZK, ResourceType::UL), (ResourceType::UL, ResourceType::ZK)]);
            assert!(matches!(calc_creep_cost(&arr(vec![s("move"), obj(vec![(atom!("type"), s("work"))])])), Ok(x) if x == 150.0));
            assert_eq!(calc_creep_cost(&arr(vec![s("wings")])), Err(Crash("Invalid body part wings".into())));
        }

        /// node: utils.calcResources on store-less objects (`typeof null == 'object'`, arrays have no amount).
        #[test]
        fn calc_resources_without_store() {
            let o = |pairs: Vec<(Atom, V)>| match obj(pairs) {
                V::Obj(o) => o,
                _ => unreachable!(),
            };
            let amount = obj(vec![(atom!("amount"), n(3.0))]);
            assert_eq!(calc_resources(&o(vec![(atom!("energy"), n(5.0)), (atom!("H"), amount), (atom!("O"), arr(vec![n(7.0)]))])), Ok(8.0));
            let null = o(vec![(atom!("energy"), n(5.0)), (atom!("power"), V::Null)]);
            assert_eq!(calc_resources(&null), Err(Crash("Cannot read properties of null (reading 'amount')".into())));
        }
    }
}

/// Replays crates/engine/tests/fixtures/structures/cases.ndjson (generated by the vanilla handler files,
/// see gen.js there): same scope, same calls, then crash text, bulk ops, live state, event log,
/// notifications and stats must be identical. A case reaching another package's `todo!()` stub counts
/// as pending until that package lands.
#[cfg(test)]
pub(crate) mod oracle {
    use super::support::*;
    use crate::atom;
    use crate::bulk::{Bulk, DbOp};
    use crate::doc::{Atom, Id, Obj, Val};
    use crate::hooks::ProcessorHooks;
    use crate::live::{clone_deep, same_ref, to_val, Crash, RoomObjects, A, O, V};
    use crate::policy::{room_stream, Compat, Profile};
    use crate::processor::intents::{self as i, movement::Movement};
    use crate::processor::{IntentFn, RoomResult, RoomScope, TickCtx, TickFn};
    use crate::terrain::RoomTerrain;
    use crate::world::{AccessSnapshot, CodeState, EngineConfig, Inbox, RoomMeta, RoomsMeta, TerrainStore, UserIdx, UserState};
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Arc;
    use util::json::Json;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/structures/cases.ndjson");

    /// Handlers by vanilla file name (relative to processor/intents, without `.js`).
    const TICKS: &[(&str, TickFn)] = &[
        ("spawns/tick", i::spawns::tick::tick),
        ("controllers/tick", i::controllers::tick::tick),
        ("sources/tick", i::sources::tick::tick),
        ("energy/tick", i::energy::tick::tick),
        ("extensions/tick", i::extensions::tick::tick),
        ("construction-sites/tick", i::construction_sites::tick::tick),
        ("roads/tick", i::roads::tick::tick),
        ("containers/tick", i::containers::tick::tick),
        ("tombstones/tick", i::tombstones::tick::tick),
        ("ruins/tick", i::ruins::tick::tick),
        ("minerals/tick", i::minerals::tick::tick),
        ("extractors/tick", i::extractors::tick::tick),
        ("constructedWalls/tick", i::constructed_walls::tick::tick),
        ("ramparts/tick", i::ramparts::tick::tick),
        ("storages/tick", i::storages::tick::tick),
        ("deposits/tick", i::deposits::tick::tick),
        ("portals/tick", i::portals::tick::tick),
        ("keeper-lairs/tick", i::keeper_lairs::tick::tick),
        ("links/tick", i::links::tick::tick),
        ("towers/tick", i::towers::tick::tick),
        ("labs/tick", i::labs::tick::tick),
        ("terminal/tick", i::terminal::tick::tick),
        ("factories/tick", i::factories::tick::tick),
        ("nukes/tick", i::nukes::tick::tick),
    ];
    const INTENTS: &[(&str, IntentFn)] = &[
        ("spawns/intents", i::spawns::intents::run),
        ("controllers/activateSafeMode", i::controllers::activate_safe_mode::run),
        ("controllers/unclaim", i::controllers::unclaim::run),
        ("ramparts/set-public", i::ramparts::set_public::run),
        ("links/intents", i::links::intents::run),
        ("towers/intents", i::towers::intents::run),
        ("labs/intents", i::labs::intents::run),
        ("power-spawns/intents", i::power_spawns::intents::run),
        ("factories/produce", i::factories::produce::run),
        ("terminal/send", i::terminal::send::run),
        ("nukers/launch-nuke", i::nukers::launch_nuke::run),
    ];

    // ---- graph codec (tools/oracle/lib/common.js encode) ----

    fn decode(j: &Json, nodes: &mut Vec<V>) -> V {
        match j {
            Json::Null => V::Null,
            Json::Bool(b) => V::Bool(*b),
            Json::Num(x) => V::Num(*x),
            Json::Str(st) => V::str(st),
            Json::Arr(a) => match a[0].as_str().unwrap() {
                "u" => V::Undef,
                "n" => V::Num(match a[1].as_str().unwrap() {
                    "NaN" => f64::NAN,
                    "Infinity" => f64::INFINITY,
                    "-Infinity" => f64::NEG_INFINITY,
                    _ => -0.0,
                }),
                "r" => nodes[a[1].as_f64().unwrap() as usize].clone(),
                "o" => {
                    let o = O::new();
                    nodes.push(V::Obj(o.clone()));
                    for kv in a[2].as_array().unwrap() {
                        let kv = kv.as_array().unwrap();
                        let v = decode(&kv[1], nodes);
                        o.set(Atom::intern(kv[0].as_str().unwrap()), v);
                    }
                    V::Obj(o)
                }
                _ => {
                    let arr = A::new();
                    nodes.push(V::Arr(arr.clone()));
                    for x in a[2].as_array().unwrap() {
                        let v = decode(x, nodes);
                        arr.push(v);
                    }
                    V::Arr(arr)
                }
            },
            Json::Obj(_) => panic!("plain object in a graph"),
        }
    }

    fn tag(t: &str, rest: Vec<Json>) -> Json {
        let mut a = vec![Json::Str(t.into())];
        a.extend(rest);
        Json::Arr(a)
    }

    fn encode(v: &V, seen: &mut Vec<V>) -> Json {
        match v {
            V::Undef => tag("u", vec![]),
            V::Null => Json::Null,
            V::Bool(b) => Json::Bool(*b),
            V::Num(x) if x.is_nan() => tag("n", vec![Json::Str("NaN".into())]),
            V::Num(x) if x.is_infinite() => tag("n", vec![Json::Str(if *x > 0.0 { "Infinity" } else { "-Infinity" }.into())]),
            V::Num(x) if *x == 0.0 && x.is_sign_negative() => tag("n", vec![Json::Str("-0".into())]),
            V::Num(x) => Json::Num(*x),
            V::Str(st) => Json::Str(st.to_string()),
            _ => {
                if let Some(id) = seen.iter().position(|x| same_ref(x, v)) {
                    return tag("r", vec![Json::Num(id as f64)]);
                }
                let id = Json::Num(seen.len() as f64);
                seen.push(v.clone());
                match v {
                    V::Obj(o) => {
                        let pairs = o.keys().into_iter().map(|k| Json::Arr(vec![Json::Str(k.as_str().into()), encode(&o.get(k), seen)])).collect();
                        tag("o", vec![id, Json::Arr(pairs)])
                    }
                    V::Arr(a) => tag("a", vec![id, Json::Arr(a.to_vec().iter().map(|x| encode(x, seen)).collect())]),
                    _ => unreachable!(),
                }
            }
        }
    }

    /// An object node (id 0) with these entries, sharing one id space (`encode(roomObjects)`).
    fn encode_entries(entries: Vec<(String, V)>) -> Json {
        let mut seen = vec![V::Undef];
        let pairs = entries.iter().map(|(k, v)| Json::Arr(vec![Json::Str(k.clone()), encode(v, &mut seen)])).collect();
        tag("o", vec![Json::Num(0.0), Json::Arr(pairs)])
    }

    /// The ops as `bulk.execute()` hands them to storage (after its JSON round trip).
    fn ops_json(ops: Vec<DbOp>) -> Json {
        fn o(pairs: Vec<(&str, Json)>) -> Json {
            Json::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
        }
        let id = |id: &Id| Json::Str(id.as_str().into());
        let upd = |i: &Id, op: &str, k: String, v: Json| o(vec![("op", Json::Str("update".into())), ("id", id(i)), ("update", o(vec![(op, Json::Obj(vec![(k, v)]))]))]);
        Json::Arr(
            ops.into_iter()
                .map(|op| match op {
                    DbOp::Remove(i) => o(vec![("op", Json::Str("remove".into())), ("id", id(&i))]),
                    DbOp::Inc(i, path, x) => {
                        let k = path.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(".");
                        upd(&i, "$inc", k, if x.is_finite() { Json::Num(x) } else { Json::Null })
                    }
                    DbOp::AddToSet(i, k, v) => upd(&i, "$addToSet", k.as_str().into(), v.to_json()),
                    DbOp::Pull(i, k, v) => upd(&i, "$pull", k.as_str().into(), v.to_json()),
                    DbOp::Replace(..) => unreachable!("processor writers never replace"),
                    DbOp::Set(i, set) => o(vec![("op", Json::Str("update".into())), ("id", id(&i)), ("update", o(vec![("$set", Val::Obj(set).to_json())]))]),
                    DbOp::Insert(doc) => o(vec![("op", Json::Str("insert".into())), ("data", Val::Obj(doc).to_json())]),
                })
                .collect(),
        )
    }

    // ---- the world around one room ----

    pub(crate) struct Env {
        pub compat: Compat,
        pub config: EngineConfig,
        pub meta: RoomsMeta,
        pub terrain: TerrainStore,
        pub access: AccessSnapshot,
        pub users: Vec<Arc<UserState>>,
        pub user_ids: crate::world::UserIds,
        pub hooks: ProcessorHooks,
    }

    impl Env {
        /// `users`: user docs in `users` order (UserIdx = position).
        pub fn new(compat: Compat, users: &[Obj], world_size: u32) -> Env {
            let users: Vec<Arc<UserState>> = users
                .iter()
                .enumerate()
                .map(|(seq, doc)| {
                    Arc::new(UserState {
                        id: Id::new(doc.get(atom!("_id")).and_then(Val::as_str).unwrap()).unwrap(),
                        seq: seq as u64,
                        doc: Arc::new(doc.clone()),
                        auth: Arc::new(Obj::new()),
                        code: Arc::new(CodeState { branches: Vec::new(), active_world: None, active_sim: None, version: 0 }),
                        memory: util::jsstr::JsStr::from_str(""),
                        memory_version: 0,
                        segments: Arc::new(BTreeMap::new()),
                        public_segments: "".into(),
                        inter_shard: None,
                        console_queue: Arc::from(Vec::new()),
                        power_creeps: Arc::from(Vec::new()),
                        inbox: Arc::new(Inbox { notifications: Vec::new(), messages: Vec::new() }),
                    })
                })
                .collect();
            let user_ids = users.iter().enumerate().map(|(i, u)| (u.id, i as UserIdx)).collect();
            Env {
                compat,
                config: EngineConfig {
                    main_loop_min_duration_ms: 1000,
                    main_loop_reset_interval_ms: 5000,
                    cpu_max_per_tick: 500,
                    cpu_bucket_size: 10000,
                    history_chunk_size: 20,
                    socket_update_throttle_ms: 200,
                    history: false,
                    disabled_cronjobs: Vec::new(),
                },
                meta: RoomsMeta { x0: 0, y0: 0, w: 0, h: 0, grid: Vec::new(), rooms: Vec::new(), world_size },
                terrain: TerrainStore::new(Vec::new()),
                access: AccessSnapshot { version: 0, accessible: Vec::new(), status: HashMap::new() },
                users,
                user_ids,
                hooks: ProcessorHooks::default(),
            }
        }

        pub fn ctx(&self, time: u64, vnow: i64, seed: u64) -> TickCtx<'_> {
            TickCtx {
                time,
                vnow,
                seed,
                compat: &self.compat,
                config: &self.config,
                meta: &self.meta,
                terrain: &self.terrain,
                access: &self.access,
                users: &self.users,
                user_ids: &self.user_ids,
                hooks: &self.hooks,
                order_override: None,
            }
        }
    }

    /// processor.js `scope` over live objects already in their post-init-loop shape (key order = order
    /// given; the last controller is `roomController`).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_scope<'a>(
        ctx: &'a TickCtx<'a>,
        room_name: &'a str,
        terrain: &'a RoomTerrain,
        objects: &[V],
        users: &[V],
        flags: Vec<O>,
        room_info: O,
        energy_available: f64,
    ) -> RoomScope<'a> {
        let mut room_objects = RoomObjects::new();
        let mut room_controller = None;
        let (mut room_spawns, mut room_extensions) = (Vec::new(), Vec::new());
        for v in objects {
            let V::Obj(o) = v else { panic!("object expected") };
            room_objects.insert(to_js_string(&o.get(atom!("_id"))), o.clone());
            match &*to_js_string(&o.get(atom!("type"))) {
                "controller" => room_controller = Some(o.clone()),
                "spawn" => room_spawns.push(o.clone()),
                "extension" => room_extensions.push(o.clone()),
                _ => {}
            }
        }
        let users = users
            .iter()
            .map(|u| {
                let V::Obj(o) = u else { panic!("user doc expected") };
                let id = Id::new(&to_js_string(&o.get(atom!("_id")))).unwrap();
                (ctx.user_ids[&id], o.clone())
            })
            .collect();
        RoomScope {
            ctx,
            room: 0,
            room_name,
            room_objects,
            room_terrain: terrain,
            game_time: ctx.time,
            room_info,
            room_controller,
            users,
            flags,
            bulk: Bulk::new(),
            bulk_users: Bulk::new(),
            bulk_flags: Bulk::new(),
            event_log: Vec::new(),
            map_view: Default::default(),
            prescan_version: u32::MAX,
            energy_available,
            room_spawns,
            room_extensions,
            keepers: Vec::new(),
            invaders: Vec::new(),
            invader_core: None,
            objects_to_history: Vec::new(),
            movement: Movement::default(),
            rng: room_stream(ctx.seed, ctx.time, room_name),
            out: RoomResult {
                room: 0,
                next: None,
                inserts: Vec::new(),
                crashed: None,
                activate_next: false,
                user_ops: Vec::new(),
                foreign_inserts: Vec::new(),
                notifications: Vec::new(),
                history: None,
                stats: Vec::new(),
                cost_ns: 0,
                index: None,
            },
        }
    }

    fn invoke(call: &V, scope: &mut RoomScope) -> Result<(), Crash> {
        let f = to_js_string(&peek(call, atom!("fn")));
        let object = |scope: &RoomScope| by_key(scope, &peek(call, atom!("object"))).expect("call target");
        let intent = || clone_deep(&peek(call, atom!("intent")));
        match &*f {
            "_calc_spawns" => {
                let (spawns, extensions) = (scope.room_spawns.clone(), scope.room_extensions.clone());
                super::calc_spawns(&spawns, &extensions, scope)
            }
            "room/intents" => {
                let uid = Id::new(&to_js_string(&peek(call, atom!("user")))).unwrap();
                let user = scope.ctx.user_ids[&uid];
                i::room::intents::run(user, &intent(), scope)
            }
            "structures/_destroy" => {
                let attack = peek(call, atom!("attackType"));
                let attack = (!matches!(attack, V::Undef)).then(|| to_number(&attack) as u32);
                i::structures::_destroy::destroy(&object(scope), scope, attack)
            }
            // the intents object itself (its mutation is compared afterwards)
            "nukes/pretick" => i::nukes::pretick::pretick(&object(scope), &peek(call, atom!("intents")), scope),
            f => match TICKS.iter().find(|(k, _)| *k == f) {
                Some((_, t)) => t(&object(scope), scope),
                None => {
                    let (_, h) = INTENTS.iter().find(|(k, _)| *k == f).unwrap_or_else(|| panic!("no handler {f}"));
                    h(&object(scope), &intent(), scope)
                }
            },
        }
    }

    fn graph(j: &Json) -> V {
        decode(j, &mut Vec::new())
    }

    fn run_case(case: &Json) -> Result<(), String> {
        let field = |k: &str| case.get(k).unwrap_or_else(|| panic!("fixture field {k}"));
        let name = field("name").as_str().unwrap().to_string();
        let seed = u64::from_str_radix(field("worldSeed").as_str().unwrap().trim_start_matches("0x"), 16).unwrap();
        let time = field("gameTime").as_f64().unwrap() as u64;
        let vnow = field("vnow").as_f64().unwrap() as i64;
        let room = field("room").as_str().unwrap().to_string();
        let terrain = match field("terrain") {
            Json::Str(t) => RoomTerrain::from_db_str(t).unwrap(),
            _ => RoomTerrain::default(),
        };
        let users = values(&graph(field("users")));
        let user_docs: Vec<Obj> = users
            .iter()
            .map(|u| match to_val(u) {
                Some(Val::Obj(o)) => o,
                _ => panic!("user doc expected"),
            })
            .collect();
        let world_size = field("worldSize").as_f64().unwrap() as u32;
        let mut env = Env::new(Compat::new(Profile::Private), &user_docs, world_size);
        // The square world of `worldSize` (W..E, N..S of the centre) with the case room as RoomIdx 0, so
        // cross-room effects (launchNuke) find their target room.
        let half = world_size as i32 / 2;
        let own = crate::room_name::RoomXY::parse_engine(&room).unwrap();
        let mut rooms = vec![RoomMeta { name: room.as_str().into(), xy: own, removed: false }];
        for y in -half..half {
            for x in -half..half {
                let xy = crate::room_name::RoomXY::new(x, y);
                if xy != own {
                    rooms.push(RoomMeta { name: xy.to_string().into(), xy, removed: false });
                }
            }
        }
        env.meta = RoomsMeta { world_size, ..RoomsMeta::new(rooms) };
        let ctx = env.ctx(time, vnow, seed);
        let objects = values(&graph(field("objects")));
        let flags = values(&graph(field("flags")))
            .into_iter()
            .map(|f| match f {
                V::Obj(o) => o,
                _ => panic!("flag doc expected"),
            })
            .collect();
        let V::Obj(room_info) = graph(field("roomInfo")) else { panic!("roomInfo") };
        let energy = field("energyAvailable").as_f64().unwrap();
        let mut scope = new_scope(&ctx, &room, &terrain, &objects, &users, flags, room_info, energy);

        let mut crash = Json::Null;
        let calls = graph(field("calls"));
        for call in values(&calls) {
            if let Err(Crash(msg)) = invoke(&call, &mut scope) {
                crash = Json::Str(msg.to_string());
                break;
            }
        }

        let expect = field("expect");
        let check = |what: &str, got: Json| -> Result<(), String> {
            let want = expect.get(what).unwrap();
            if &got == want {
                Ok(())
            } else {
                Err(format!("{name}: {what} differs\n  want {}\n  got  {}", want.stringify(), got.stringify()))
            }
        };
        check("crash", crash)?;
        let state = scope.room_objects.keys().iter().map(|k| (k.to_string(), V::Obj(scope.room_objects.get(k).unwrap()))).collect();
        check("state", encode_entries(state))?;
        check("flags", encode(&arr(scope.flags.iter().map(|f| V::Obj(f.clone())).collect()), &mut Vec::new()))?;
        // `users` is an object keyed by id (integer-like ids such as "3" first)
        let users = O::new();
        for (_, u) in &scope.users {
            users.set(Atom::intern(&to_js_string(&u.get(atom!("_id")))), V::Obj(u.clone()));
        }
        check("users", encode(&V::Obj(users), &mut Vec::new()))?;
        check("roomInfo", encode(&V::Obj(scope.room_info.clone()), &mut Vec::new()))?;
        check("eventLog", encode(&arr(scope.event_log.clone()), &mut Vec::new()))?;
        let uid = |u: UserIdx| Json::Str(scope.ctx.users[u as usize].id.as_str().into());
        check("notifications", Json::Arr(scope.out.notifications.iter().map(|(u, m)| Json::Arr(vec![uid(*u), Json::Str(m.to_string())])).collect()))?;
        check("stats", Json::Arr(scope.out.stats.iter().map(|(u, k, x)| Json::Arr(vec![Json::Str(k.as_str().into()), uid(*u), Json::Num(*x)])).collect()))?;
        check("calls", encode(&calls, &mut Vec::new()))?;
        // vanilla inserts a launched nuke through this room's bulk (after its other inserts here)
        let mut objects = std::mem::take(&mut scope.bulk).execute();
        objects.extend(scope.out.foreign_inserts.drain(..).map(|(_, doc)| DbOp::Insert(doc)));
        let ops = Json::Obj(vec![
            ("objects".into(), ops_json(objects)),
            ("users".into(), ops_json(std::mem::take(&mut scope.bulk_users).execute())),
            ("flags".into(), ops_json(std::mem::take(&mut scope.bulk_flags).execute())),
        ]);
        check("ops", ops)
    }

    #[test]
    fn vanilla_handler_cases() {
        let (mut passed, mut pending, mut failed) = (0, Vec::new(), Vec::new());
        for line in FIXTURE.lines().filter(|l| !l.is_empty()) {
            let case = Json::parse(line).unwrap();
            let name = case.get("name").unwrap().as_str().unwrap().to_string();
            match std::panic::catch_unwind(|| run_case(&case)) {
                Ok(Ok(())) => passed += 1,
                Ok(Err(diff)) => failed.push(diff),
                Err(e) => {
                    let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                    if msg.contains("not yet implemented") {
                        pending.push(name);
                    } else {
                        failed.push(format!("{name}: panicked: {msg}"));
                    }
                }
            }
        }
        eprintln!("structures oracle: {passed} passed, {} pending on other packages' stubs: {pending:?}", pending.len());
        assert!(failed.is_empty(), "{} failed:\n{}", failed.len(), failed.join("\n\n"));
        assert!(passed > 0);
    }

    // ---- COMPAT rows the private-profile oracle cannot show: their fixed (mmo) behaviour ----

    pub(crate) fn json_v(text: &str) -> V {
        crate::live::from_val(&Val::from_json(&Json::parse(text).unwrap()))
    }

    /// Runs `f` on a scope over `objects` / `flags` (JSON arrays) with users u1 (owns W1N1) and u2.
    pub(crate) fn with_scope(profile: Profile, objects: &str, flags: &str, f: impl FnOnce(&mut RoomScope)) {
        let users = values(&json_v(r#"[{"_id":"u1","rooms":["W1N1"]},{"_id":"u2","rooms":[]}]"#));
        let docs: Vec<Obj> = users.iter().map(|u| if let Some(Val::Obj(o)) = to_val(u) { o } else { unreachable!() }).collect();
        let env = Env::new(Compat::new(profile), &docs, 12);
        let ctx = env.ctx(1000, 0, 1);
        let terrain = RoomTerrain::default();
        let flags = values(&json_v(flags)).into_iter().map(|f| if let V::Obj(o) = f { o } else { unreachable!() }).collect();
        let mut scope = new_scope(&ctx, "W1N1", &terrain, &values(&json_v(objects)), &users, flags, O::new(), 0.0);
        f(&mut scope);
    }

    fn ops(bulk: &mut Bulk) -> String {
        ops_json(std::mem::take(bulk).execute()).stringify()
    }

    #[test]
    fn calc_spawns_without_controller() {
        let objects = r#"[{"_id":"s1","type":"spawn","x":1,"y":1},{"_id":"e1","type":"extension","x":2,"y":2,"off":true}]"#;
        with_scope(Profile::Private, objects, "[]", |scope| {
            let (s, e) = (scope.room_spawns.clone(), scope.room_extensions.clone());
            assert_eq!(super::calc_spawns(&s, &e, scope), Err(Crash("Cannot read properties of undefined (reading 'level')".into())));
        });
        // fixed: level 0 allows none, so all are off (no sort)
        with_scope(Profile::Mmo, objects, "[]", |scope| {
            let (s, e) = (scope.room_spawns.clone(), scope.room_extensions.clone());
            super::calc_spawns(&s, &e, scope).unwrap();
            assert_eq!(ops(&mut scope.bulk), r#"[{"op":"update","id":"s1","update":{"$set":{"off":true}}}]"#);
        });
    }

    #[test]
    fn cancel_spawning_without_creep() {
        let objects = r#"[{"_id":"s1","type":"spawn","x":1,"y":1,"user":"u1","spawning":{"name":"Bob","spawnTime":1001}}]"#;
        with_scope(Profile::Mmo, objects, "[]", |scope| {
            let spawn = scope.room_objects.get("s1").unwrap();
            i::spawns::cancel_spawning::run(&spawn, &json_v("{}"), scope).unwrap();
            assert_eq!(ops(&mut scope.bulk), r#"[{"op":"update","id":"s1","update":{"$set":{"spawning":null}}}]"#);
        });
    }

    #[test]
    fn flags_of_several_users_fixed() {
        let flags = r#"[{"_id":"f1","user":"u1","room":"W1N1","data":"A~1~1~1~1|K~2~2~2~2"},{"_id":"f2","user":"u2","room":"W1N1","data":"B~1~1~1~1"}]"#;
        let remove = |name: &str| json_v(&format!(r#"{{"removeFlag":[{{"roomName":"W1N1","name":"{name}"}}]}}"#));
        // private: u2's pass re-parses f1 from its stale data, so u1's removal is lost
        with_scope(Profile::Private, "[]", flags, |scope| {
            i::room::intents::run(0, &remove("A"), scope).unwrap();
            i::room::intents::run(1, &remove("B"), scope).unwrap();
            assert_eq!(
                ops(&mut scope.bulk_flags),
                r#"[{"op":"update","id":"f1","update":{"$set":{"data":"A~1~1~1~1|K~2~2~2~2"}}},{"op":"update","id":"f2","update":{"$set":{"data":""}}}]"#
            );
        });
        // fixed: edits accumulate; a doc created this tick is inserted once, with its final data
        with_scope(Profile::Mmo, "[]", r#"[{"_id":"f2","user":"u2","room":"W1N1","data":"B~1~1~1~1"}]"#, |scope| {
            let create = json_v(r#"{"createFlag":[{"roomName":"W1N1","x":1,"y":2,"name":"F","color":5,"secondaryColor":5}]}"#);
            i::room::intents::run(0, &create, scope).unwrap();
            i::room::intents::run(1, &remove("B"), scope).unwrap();
            let again = json_v(r#"{"createFlag":[{"roomName":"W1N1","x":3,"y":4,"name":"G","color":1,"secondaryColor":2}]}"#);
            i::room::intents::run(0, &again, scope).unwrap();
            assert_eq!(
                ops(&mut scope.bulk_flags),
                r#"[{"op":"update","id":"f2","update":{"$set":{"data":""}}},{"op":"insert","data":{"data":"F~5~5~1~2|G~1~2~3~4","user":"u1","room":"W1N1"}}]"#
            );
        });
    }
}
