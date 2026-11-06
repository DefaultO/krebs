//! mirror: reference/engine/src/processor/intents/room/intents.js
//! parity: FAITHFUL

use super::{create_construction_site, create_flag, destroy_structure, gen_energy, remove_construction_site, remove_flag};
use crate::atom;
use crate::doc::Atom;
use crate::live::{Crash, V};
use crate::processor::intents::_calc_spawns::support::*;
use crate::processor::RoomScope;
use crate::world::UserIdx;

/// `module.exports = function(userId, intent, scope)` of a `room/*.js` handler.
pub type RoomIntentFn = fn(UserIdx, &V, &mut RoomScope) -> Result<(), Crash>;

/// intents.js:13-41 (ROOM-INTENTS): in this order when truthy; `true` = called once per array element
/// (`_.forEach`), `false` = called with the value itself.
pub const ACTIONS: &[(&str, bool, RoomIntentFn)] = &[
    ("removeFlag", true, remove_flag::run),
    ("createFlag", true, create_flag::run),
    ("createConstructionSite", true, create_construction_site::run),
    ("removeConstructionSite", true, remove_construction_site::run),
    ("destroyStructure", true, destroy_structure::run),
    ("genEnergy", false, gen_energy::run),
];

/// `i.data.split("|").map(j => j.split("~"))`.
fn parse(data: &V) -> Result<V, Crash> {
    match data {
        V::Str(text) => Ok(arr(text.split('|').map(|j| arr(j.split('~').map(s).collect())).collect())),
        V::Undef | V::Null => Err(crash_read(data, "split")),
        _ => Err(Crash("i.data.split is not a function".into())),
    }
}

pub fn run(user: UserIdx, object_intents: &V, scope: &mut RoomScope) -> Result<(), Crash> {
    // BUG-FLAGS-REPARSE (fix): each doc is parsed once per room tick, so the edits of every user
    // accumulate. BUG-CRASH-FLAG-FIRST (fix): a doc created this tick (no `data`) is not re-parsed.
    let parse_once = scope.ctx.compat.fix("BUG-FLAGS-REPARSE");
    let skip_new = scope.ctx.compat.fix("BUG-CRASH-FLAG-FIRST");
    for i in scope.flags.clone() {
        if parse_once && i.has(atom!("_parsed")) || skip_new && matches!(i.get(atom!("data")), V::Undef) {
            continue;
        }
        i.set(atom!("_parsed"), parse(&i.get(atom!("data")))?);
    }

    for &(name, each, f) in ACTIONS {
        let intents = get(object_intents, Atom::intern(name))?;
        if truthy(&intents) {
            if each {
                for i in values(&intents) {
                    f(user, &i, scope)?;
                }
            } else {
                f(user, &intents, scope)?;
            }
        }
    }

    for i in scope.flags.clone() {
        if truthy(&i.get(atom!("_modified"))) {
            let mut items = Vec::new();
            for j in values(&i.get(atom!("_parsed"))) {
                match j {
                    V::Arr(_) => items.push(join(&j, "~")),
                    _ => return Err(Crash("j.join is not a function".into())),
                }
            }
            let data = s(&items.join("|"));

            let id = i.get(atom!("_id"));
            let pending = i.get(atom!("_insertCnt"));
            if truthy(&id) {
                bulk_update_id(&mut scope.bulk_flags, &id, &obj(vec![(atom!("data"), data)]))?;
            } else if truthy(&pending) {
                // Only reachable when one of the C rows above is fixed (otherwise the next user's pass
                // throws first): the doc inserted by an earlier pass gets its final data, not a second
                // insert.
                bulk_update_id(&mut scope.bulk_flags, &pending, &obj(vec![(atom!("data"), data)]))?;
            } else {
                let doc = obj(vec![(atom!("data"), data), (atom!("user"), i.get(atom!("user"))), (atom!("room"), i.get(atom!("room")))]);
                let counter = scope.bulk_flags.insert(&doc, None);
                if parse_once || skip_new {
                    i.set(atom!("_insertCnt"), s(&counter.to_string()));
                }
            }
        }
    }
    Ok(())
}
