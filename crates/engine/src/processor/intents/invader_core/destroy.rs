//! mirror: reference/engine/src/processor/intents/invader-core/destroy.js
//! parity: FAITHFUL
//!
//! The `strongholdDestroyed` mod event is not emitted (no engine hook for it yet).

use super::tick::release_controller;
use crate::atom;
use crate::doc::Atom;
use crate::live::{from_val, to_js_string, truthy, Crash, O, V};
use crate::processor::global::obj;
use crate::processor::{get_or_undef, update, RoomScope};
use crate::utils::calc_reward;

pub fn destroy(object: &O, scope: &mut RoomScope) -> Result<(), Crash> {
    release_controller(scope)?;
    let data = from_val(crate::strongholds::data());
    let g = |v: &V, k: &str| get_or_undef(v, Atom::intern(k));
    let deposit_type = object.get(atom!("depositType"));
    let template_name = object.get(atom!("templateName"));
    let rewards_all = if truthy(&deposit_type) { g(&g(&data, "coreRewards"), &to_js_string(&deposit_type)) } else { V::Undef };
    let template = if truthy(&template_name) { g(&g(&data, "templates"), &to_js_string(&template_name)) } else { V::Undef };
    let some_reward = matches!(&rewards_all, V::Arr(a) if a.to_vec().iter().any(crate::live::truthy));
    if !truthy(&deposit_type) || !truthy(&rewards_all) || !some_reward || !truthy(&template_name) || !truthy(&template) {
        return Ok(());
    }
    let reward_level = crate::live::to_number(&g(&template, "rewardLevel"));
    let take = (1.0 + reward_level).max(0.0) as usize;
    let rng = scope.rng.rng();
    let rewards: Vec<V> = match &rewards_all {
        V::Arr(a) => a
            .to_vec()
            .into_iter()
            .take(take)
            .map(|r| match &r {
                V::Arr(choices) => crate::policy::lodash_sample(rng, &choices.to_vec()).cloned().unwrap_or(V::Undef),
                _ => r,
            })
            .collect(),
        _ => Vec::new(),
    };
    let densities: Vec<V> = match g(&data, "coreDensities") {
        V::Arr(a) => a.to_vec().into_iter().take(take).collect(),
        _ => Vec::new(),
    };
    // _.object(rewards, densities)
    let table = O::new();
    for (i, r) in rewards.iter().enumerate() {
        table.set(Atom::intern(&to_js_string(r)), densities.get(i).cloned().unwrap_or(V::Undef));
    }
    let amount = crate::live::to_number(&get_or_undef(&g(&data, "coreAmounts"), Atom::intern(&to_js_string(&V::Num(reward_level)))));
    let store = calc_reward(&table, amount, None, scope.rng.rng());
    update(scope, object, &obj(vec![("store", store)]))
}
