//! mirror: reference/engine/src/processor/intents/creeps/invaders/shootAtWill.js
//! parity: FAITHFUL

use super::pretick::Context;
use crate::atom;
use crate::constants as C;
use crate::live::{Crash, O, V};
use crate::processor::common::fake_runtime::has_active_bodyparts;
use crate::processor::get_or_undef;
use crate::processor::global::obj;
use crate::processor::intents::creeps::keepers::pretick::lodash_min_by;
use crate::utils::dist;

pub fn run(creep: &O, cx: &Context) -> Result<(), Crash> {
    if !has_active_bodyparts(creep, C::RANGED_ATTACK) {
        return Ok(());
    }
    let cv = V::Obj(creep.clone());
    let targets: Vec<O> = cx.hostiles.iter().filter(|c| dist(&cv, &V::Obj((*c).clone())) <= 3.0).cloned().collect();
    if targets.is_empty() {
        return Ok(());
    }
    let target = lodash_min_by(&targets, atom!("hits"));
    cx.intents.set(&creep.get(atom!("_id")), "rangedAttack", obj(vec![("id", get_or_undef(&target, atom!("_id")))]));
    Ok(())
}
