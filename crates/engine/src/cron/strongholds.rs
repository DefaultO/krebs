//! mirror: reference/backend/lib/strongholds.js
//! parity: FAITHFUL
//!
//! genStrongholds / expandStrongholds cron jobs. The map grid (buildMapGrid: plain-tile exit counts of the
//! accessible rooms) comes from `TerrainStore::exit_counts` and the AccessSnapshot instead of the env's
//! compressed terrain blob; same values.

use super::cronjobs::{activate, field, in_sector, insert_objects, name_of, obj, objects, objects_in, rooms, set_object, sv, wall_xy, CronCtx};
use crate::atom;
use crate::constants as C;
use crate::doc::{Atom, DocRef, Obj, Val};
use crate::live::{from_val, to_js_string, to_number, truthy, V};
use crate::policy;
use crate::processor::global::exact_room;
use crate::world::{RoomIdx, World};

const STRONGHOLD_DEPLOY_TIME: f64 = 5000.0;

/// `spawnStronghold(roomName, opts)` options (strongholds.js:14-26; CLI-STRONGHOLDS-SPAWN).
#[derive(Default)]
pub struct SpawnOpts {
    pub template_name: Option<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    /// `opts.user || "2"`
    pub user: Option<String>,
    /// `opts.deployTime`, 1 when falsy.
    pub deploy_time: Option<f64>,
}

/// What `spawnStronghold` resolved to: spawned, or (the GOTCHA of :44-45) `opts` returned unspawned when the
/// template and both coordinates were given.
pub enum Spawned {
    Spawned,
    OptsReturned,
}

/// strongholds.js:14 spawnStronghold.
pub fn spawn_stronghold(w: &mut World, cx: &mut CronCtx, r: RoomIdx, opts: &SpawnOpts) -> Result<Spawned, String> {
    let deploy_offset = opts.deploy_time.filter(|d| *d != 0.0 && !d.is_nan()).unwrap_or(1.0);
    let room_name = name_of(w, r);
    let user = opts.user.as_deref().unwrap_or("2");
    let g = cx.g as f64;
    let objs = objects_in(w, r, |d| d.kind() == C::STRUCTURE_RAMPART || C::OBSTACLE_OBJECT_TYPES.contains(&d.kind()));
    let data = from_val(crate::strongholds::data());
    let templates = match crate::processor::get_or_undef(&data, Atom::intern("templates")) {
        V::Obj(o) => o,
        _ => return Err("no templates".into()),
    };
    let controller = objs.iter().find(|d| d.kind() == C::STRUCTURE_CONTROLLER).cloned();
    if let Some(c) = &controller {
        let reservation = field(&c.obj, "reservation");
        if truthy(&field(&c.obj, "user")) || (truthy(&reservation) && to_js_string(&crate::processor::get_or_undef(&reservation, atom!("user"))).as_ref() != user) {
            return Err("room occupied".into());
        }
    }
    let structures_of = |t: &V| -> Vec<V> {
        match crate::processor::get_or_undef(t, Atom::intern("structures")) {
            crate::live::V::Arr(a) => a.to_vec(),
            _ => Vec::new(),
        }
    };
    // spots[t] for x, y in 4..44, templates in key order; keys appear in first-spot order
    // `opts.templateName && !templates[name]` builds a rejection nobody returns; the unknown name then fails below
    let names: Vec<Atom> = match &opts.template_name {
        Some(t) => vec![Atom::intern(t)],
        None => templates.keys(),
    };
    if opts.template_name.is_some() && opts.x.is_some() && opts.y.is_some() {
        return Ok(Spawned::OptsReturned);
    }
    if let Some(t) = names.iter().find(|t| !matches!(templates.get(**t), V::Obj(_))) {
        return Err(format!("TypeError: Cannot read properties of undefined (reading 'structures') [{t:?}]"));
    }
    let occupied: Vec<(f64, f64)> = objs.iter().map(|d| (to_number(&field(&d.obj, "x")), to_number(&field(&d.obj, "y")))).collect();
    let mut spots: Vec<(Atom, Vec<(i64, i64)>)> = Vec::new();
    for x in 4..45 {
        for y in 4..45 {
            for &t in &names {
                let ok = structures_of(&templates.get(t)).iter().all(|s| {
                    let (dx, dy) = (to_number(&crate::processor::get_or_undef(s, Atom::intern("dx"))), to_number(&crate::processor::get_or_undef(s, Atom::intern("dy"))));
                    let (sx, sy) = (x as f64 + dx, y as f64 + dy);
                    !wall_xy(w, r, sx as i64, sy as i64) && !occupied.iter().any(|&(ox, oy)| ox == sx && oy == sy)
                });
                if ok {
                    match spots.iter_mut().find(|(k, _)| *k == t) {
                        Some((_, v)) => v.push((x, y)),
                        None => spots.push((t, vec![(x, y)])),
                    }
                }
            }
        }
    }
    if spots.is_empty() {
        return Err("No spots".into());
    }
    let keys: Vec<Atom> = spots.iter().map(|(k, _)| *k).collect();
    let name = *policy::lodash_sample(cx.rng(), &keys).expect("non-empty");
    let template = templates.get(name);
    let spot = *policy::lodash_sample(cx.rng(), &spots.iter().find(|(k, _)| *k == name).unwrap().1).expect("non-empty");
    let rewards: Vec<Atom> = match crate::processor::get_or_undef(&data, Atom::intern("coreRewards")) {
        V::Obj(o) => o.keys(),
        _ => Vec::new(),
    };
    let deposit_type = policy::lodash_sample(cx.rng(), &rewards).map(|a| a.as_str().to_string());

    let deploy_time = g + deploy_offset;
    let stronghold_id = format!("{room_name}_{}", util::num::to_js_string(g));
    let invulnerability = || {
        Val::Arr(vec![Val::Obj(obj(vec![
            ("effect", Val::Num(C::EFFECT_INVULNERABILITY as f64)),
            ("power", Val::Num(C::EFFECT_INVULNERABILITY as f64)),
            ("endTime", Val::Num(deploy_time)),
            ("duration", Val::Num(STRONGHOLD_DEPLOY_TIME)),
        ]))])
    };
    let mut docs = Vec::new();
    for s in structures_of(&template) {
        let ty = to_js_string(&crate::processor::get_or_undef(&s, atom!("type")));
        let dx = to_number(&crate::processor::get_or_undef(&s, Atom::intern("dx")));
        let dy = to_number(&crate::processor::get_or_undef(&s, Atom::intern("dy")));
        let (x, y) = (spot.0 as f64 + dx, spot.1 as f64 + dy);
        if &*ty == C::STRUCTURE_INVADER_CORE {
            // _.merge({}, i, {...}) then delete dx, dy
            let mut core = match crate::live::to_val(&s) {
                Some(Val::Obj(o)) => o,
                _ => Obj::new(),
            };
            let level = to_js_string(&crate::processor::get_or_undef(&s, atom!("level")));
            let expand = level.parse::<usize>().ok().and_then(|l| C::INVADER_CORE_EXPAND_TIME.get(l).copied().flatten()).map_or(f64::NAN, |t| t as f64);
            for (k, v) in [
                ("room", sv(&room_name)),
                ("x", Val::Num(x)),
                ("y", Val::Num(y)),
                ("user", sv(user)),
                ("templateName", sv(name.as_str())),
                ("hits", Val::Num(C::INVADER_CORE_HITS as f64)),
                ("hitsMax", Val::Num(C::INVADER_CORE_HITS as f64)),
                ("nextExpandTime", Val::Num(deploy_time + expand)),
                ("depositType", deposit_type.as_deref().map_or(Val::Null, sv)),
                ("deployTime", Val::Num(deploy_time)),
                ("strongholdId", sv(&stronghold_id)),
                ("effects", invulnerability()),
            ] {
                core.set(Atom::intern(k), v);
            }
            core.remove(Atom::intern("dx"));
            core.remove(Atom::intern("dy"));
            docs.push(core);
        }
        if &*ty == C::STRUCTURE_RAMPART {
            docs.push(obj(vec![
                ("type", sv(C::STRUCTURE_RAMPART)),
                ("room", sv(&room_name)),
                ("x", Val::Num(x)),
                ("y", Val::Num(y)),
                ("user", sv(user)),
                ("hits", Val::Num(1.0)),
                ("hitsMax", Val::Num(1.0)),
                ("isPublic", Val::Bool(true)),
                ("strongholdId", sv(&stronghold_id)),
                ("nextDecayTime", Val::Num(deploy_time)),
            ]));
        }
    }
    if let Some(c) = controller {
        let fields = obj(vec![("user", sv(user)), ("level", Val::Num(8.0)), ("progress", Val::Num(0.0)), ("downgradeTime", Val::Num(deploy_time)), ("effects", invulnerability())]);
        set_object(w, &c, fields);
    }
    insert_objects(w, docs);
    activate(w, r);
    Ok(Spawned::Spawned)
}

/// The sector-centre name of a room (`/^([WE]\d*)\d([NS]\d*)\d$/` → `p1 5 p2 5`), or the name itself.
fn sector_center(name: &str) -> String {
    match super::cronjobs::sector_prefixes(name) {
        Some((p1, p2)) => format!("{p1}5{p2}5"),
        None => name.to_string(),
    }
}

/// `^[WE]\d*5[NS]\d*5$`.
fn is_center_name(name: &str) -> bool {
    let b = name.as_bytes();
    let Some(ns) = b.iter().position(|c| matches!(c, b'N' | b'S')) else { return false };
    let ok = |s: &[u8]| !s.is_empty() && *s.last().unwrap() == b'5' && s[..s.len() - 1].iter().all(u8::is_ascii_digit);
    matches!(b.first(), Some(b'W' | b'E')) && ok(&b[1..ns]) && ok(&b[ns + 1..])
}

/// strongholds.js:224 genStrongholds.
pub fn gen_strongholds(w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    let centers: Vec<RoomIdx> = rooms(w).into_iter().filter(|(r, room)| is_center_name(&name_of(w, *r)) && !matches!(room.get(atom!("status")), Some(Val::Str(s)) if &**s == "out of borders")).map(|(r, _)| r).collect();
    let cores = objects(w, |d| d.kind() == "invaderCore" || (d.kind() == "ruin" && matches!(d.obj.get(atom!("structure")).and_then(Val::as_obj).and_then(|s| s.get(atom!("type"))), Some(Val::Str(t)) if &**t == "invaderCore")));
    let core_centers: Vec<String> = cores.iter().map(|c| sector_center(&to_js_string(&field(&c.obj, "room")))).collect();
    let without: Vec<RoomIdx> = centers.into_iter().filter(|r| !core_centers.contains(&name_of(w, *r))).collect();
    for s in without {
        select_room(w, cx, s)?;
    }
    Ok(())
}

/// strongholds.js:155 selectRoom.
fn select_room(w: &mut World, cx: &mut CronCtx, center: RoomIdx) -> Result<(), String> {
    let center_name = name_of(w, center);
    // sectorRegex = center.replace(/^([WE]\d*)5([NS]\d*)5$/, '^p1\dp2\d$')
    let b = center_name.as_bytes();
    let ns = b.iter().position(|c| matches!(c, b'N' | b'S')).unwrap_or(0);
    let prefixes = Some((center_name[..ns - 1].to_string(), center_name[ns..center_name.len() - 1].to_string()));
    cx.unit(&center_name);
    let sector_rooms: Vec<(RoomIdx, std::sync::Arc<Obj>)> = rooms(w).into_iter().filter(|(r, _)| in_sector(&name_of(w, *r), &prefixes, &center_name)).collect();
    let sector_objects: Vec<DocRef> =
        objects(w, |d| matches!(d.kind(), "creep" | "controller") && d.obj.get(atom!("room")).and_then(Val::as_str).is_some_and(|n| in_sector(n, &prefixes, &center_name)));
    let possible: Vec<RoomIdx> = sector_rooms
        .iter()
        .filter(|(r, room)| {
            let name = name_of(w, *r);
            let (x, y) = crate::utils::room_name_to_xy(&name);
            if matches!(room.get(atom!("bus")), Some(v) if crate::live::truthy(&from_val(v)))
                || !super::cronjobs::is_center(x, y)
                || super::cronjobs::is_very_center(x, y)
                || name.ends_with('0')
                || name.contains("0N")
                || name.contains("0S")
            {
                return false;
            }
            let in_room = |d: &&DocRef| d.obj.get(atom!("room")).and_then(Val::as_str) == Some(name.as_str());
            let Some(controller) = sector_objects.iter().filter(in_room).find(|d| d.kind() == "controller") else { return true };
            if truthy(&field(&controller.obj, "user")) || truthy(&field(&controller.obj, "reservation")) {
                return false;
            }
            !sector_objects.iter().filter(in_room).any(|o| {
                o.kind() == "creep" && matches!(o.obj.get(atom!("body")), Some(Val::Arr(b)) if b.iter().any(|p| matches!(p.as_obj().and_then(|p| p.get(atom!("type"))), Some(Val::Str(t)) if &**t == "claim")))
            })
        })
        .map(|(r, _)| *r)
        .collect();
    if possible.is_empty() {
        return Ok(());
    }
    let r = *policy::lodash_sample(cx.rng(), &possible).expect("non-empty");
    spawn_stronghold(w, cx, r, &SpawnOpts { deploy_time: Some(STRONGHOLD_DEPLOY_TIME), ..SpawnOpts::default() }).map(|_| ())
}

/// buildMapGrid: `"x,y"` of every accessible room → its t/r/b/l plain-tile exit counts (non-zero only).
fn map_grid_entry(w: &World, x: f64, y: f64) -> Option<Vec<&'static str>> {
    let name = super::cronjobs::room_name_from_xy(x, y);
    let r = exact_room(w, &name)?;
    if !w.access.is_accessible(r) {
        return None;
    }
    let counts = w.terrain.exit_counts(r);
    Some(["t", "r", "b", "l"].iter().zip(counts).filter(|(_, n)| *n > 0).map(|(d, _)| *d).collect())
}

/// strongholds.js:280: a highway room (`/(W|E)\d*0/` or `/(N|S)\d*0/`, unanchored: BUG-SH-HIGHWAY-REGEX).
fn is_highway_name(w: &World, name: &str) -> bool {
    if w.compat.fix("BUG-SH-HIGHWAY-REGEX") {
        let (x, y) = crate::utils::room_name_to_xy(name);
        let shown = |c: f64| if c < 0.0 { -c - 1.0 } else { c };
        return shown(x) % 10.0 == 0.0 || shown(y) % 10.0 == 0.0;
    }
    let has = |letters: [char; 2]| {
        let chars: Vec<char> = name.chars().collect();
        (0..chars.len()).any(|i| letters.contains(&chars[i]) && chars[i + 1..].iter().take_while(|c| c.is_ascii_digit()).any(|&c| c == '0'))
    };
    has(['W', 'E']) || has(['N', 'S'])
}

/// strongholds.js:242 expandStronghold.
pub fn expand_stronghold(w: &mut World, cx: &mut CronCtx, core: &DocRef) -> Result<bool, String> {
    let g = cx.g as f64;
    let now = cx.now as f64;
    let core_room = to_js_string(&field(&core.obj, "room")).to_string();
    let Some(cr) = exact_room(w, &core_room) else { return Err("Cannot read properties of null (reading 'novice')".into()) };
    let blocked = |info: &Obj| to_number(&field(info, "novice")) > now || to_number(&field(info, "respawnArea")) > now;
    if blocked(&w.rooms[cr as usize].info.clone()) {
        return Ok(false);
    }
    let level = to_js_string(&field(&core.obj, "level"));
    let expand = level.parse::<usize>().ok().and_then(|l| C::INVADER_CORE_EXPAND_TIME.get(l).copied().flatten()).map_or(f64::NAN, |t| t as f64);
    set_object(w, core, obj(vec![("nextExpandTime", Val::Num(g + expand))]));

    let (mut open, mut closed) = (vec![core_room.clone()], Vec::<String>::new());
    let mut found: Option<(RoomIdx, DocRef)> = None;
    while found.is_none() && !open.is_empty() {
        let room = open.remove(0);
        closed.push(room.clone());
        let (x, y) = crate::utils::room_name_to_xy(&room);
        let Some(dirs) = map_grid_entry(w, x, y) else { return Err("Cannot convert undefined or null to object".into()) };
        for dir in policy::lodash_shuffle(cx.rng(), &dirs) {
            let (dx, dy) = match dir {
                "t" => (0.0, -1.0),
                "b" => (0.0, 1.0),
                "l" => (-1.0, 0.0),
                _ => (1.0, 0.0),
            };
            if map_grid_entry(w, x + dx, y + dy).is_none() {
                continue;
            }
            let next = super::cronjobs::room_name_from_xy(x + dx, y + dy);
            if is_highway_name(w, &next) {
                continue;
            }
            let Some(nr) = exact_room(w, &next) else { return Err("Cannot read properties of null (reading 'novice')".into()) };
            if blocked(&w.rooms[nr as usize].info.clone()) {
                continue;
            }
            let controller = objects_in(w, nr, |d| d.kind() == "controller").into_iter().next();
            let has_core = !objects_in(w, nr, |d| d.kind() == "invaderCore").is_empty();
            if let Some(c) = &controller {
                if !truthy(&field(&c.obj, "user")) && !has_core {
                    found = Some((nr, c.clone()));
                    break;
                }
            }
            if (controller.is_none() || has_core) && !open.contains(&next) && !closed.contains(&next) {
                open.push(next);
            }
        }
    }
    let Some((fr, controller)) = found else { return Ok(false) };
    let (cx_, cy_) = (to_number(&field(&controller.obj, "x")), to_number(&field(&controller.obj, "y")));
    let rect = (crate::processor::js_max(1.0, cx_ - 5.0), crate::processor::js_min(48.0, cx_ + 5.0), crate::processor::js_max(1.0, cy_ - 5.0), crate::processor::js_min(48.0, cy_ + 5.0));
    let Some((x, y)) = find_free_pos(w, cx, fr, 1, rect) else { return Ok(false) };
    let doc = obj(vec![
        ("type", sv("invaderCore")),
        ("level", Val::Num(0.0)),
        ("room", sv(&name_of(w, fr))),
        ("x", Val::Num(x)),
        ("y", Val::Num(y)),
        ("user", sv("2")),
        ("hits", Val::Num(C::INVADER_CORE_HITS as f64)),
        ("hitsMax", Val::Num(C::INVADER_CORE_HITS as f64)),
        ("strongholdId", core.obj.get(Atom::intern("strongholdId")).cloned().unwrap_or(Val::Null)),
        ("effects", core.obj.get(atom!("effects")).cloned().unwrap_or(Val::Null)),
    ]);
    insert_objects(w, vec![doc]);
    activate(w, fr);
    Ok(true)
}

/// backend utils.js:319 findFreePos (no exclude).
fn find_free_pos(w: &World, cx: &mut CronCtx, r: RoomIdx, distance: i64, (x1, x2, y1, y2): (f64, f64, f64, f64)) -> Option<(f64, f64)> {
    let objs = objects_in(w, r, |_| true);
    let blocking = |t: &str| C::OBSTACLE_OBJECT_TYPES.contains(&t) || t == "rampart" || t == "portal";
    let mut counter = 0;
    loop {
        let x = x1 + (cx.random() * (x2 - x1)).floor();
        let y = y1 + (cx.random() * (y2 - y1)).floor();
        let spot = (-distance..=distance).all(|dx| (-distance..=distance).all(|dy| !wall_xy(w, r, x as i64 + dx, y as i64 + dy)));
        let has_objects = objs.iter().any(|o| {
            (to_number(&field(&o.obj, "x")) - x).abs() <= distance as f64 && (to_number(&field(&o.obj, "y")) - y).abs() <= distance as f64 && blocking(o.kind())
        });
        counter += 1;
        if !((!spot || has_objects) && counter < 500) {
            return (spot && !has_objects).then_some((x, y));
        }
    }
}

/// strongholds.js:328 expandStrongholds.
pub fn expand_strongholds(w: &mut World, cx: &mut CronCtx) -> Result<(), String> {
    let g = cx.g as f64;
    // {type: 'invaderCore', deployTime: null (Loki ===), level: {$gt: 0}, nextExpandTime: {$lt: G}}
    let cores = objects(w, |d| {
        d.kind() == "invaderCore"
            && matches!(d.obj.get(atom!("deployTime")), Some(Val::Null))
            && matches!(d.obj.get(atom!("level")), Some(Val::Num(l)) if *l > 0.0)
            && matches!(d.obj.get(Atom::intern("nextExpandTime")), Some(Val::Num(t)) if *t < g)
    });
    for core in cores {
        cx.unit(core.id().as_str());
        expand_stronghold(w, cx, &core)?;
    }
    Ok(())
}
