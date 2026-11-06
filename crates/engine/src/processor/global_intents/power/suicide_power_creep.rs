//! mirror: reference/engine/src/processor/global-intents/power/suicidePowerCreep.js
//! parity: FAITHFUL

use crate::atom;
use crate::live::{O, V};
use crate::processor::global::Scope;
use crate::processor::global_intents::power::_die_power_creep::die;
use crate::processor::{get_or_undef, loose_eq};

pub(crate) fn run(intent: &V, user: &O, s: &mut Scope) {
    let (uid, id) = (user.get(atom!("_id")), get_or_undef(intent, atom!("id")));
    let Some(pc) = s.power_creeps().into_iter().find(|c| loose_eq(&c.get(atom!("user")), &uid) && loose_eq(&c.get(atom!("_id")), &id)) else { return };
    die(&pc, s);
}
