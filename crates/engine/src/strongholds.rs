//! mirror: reference/common/lib/strongholds.js
//! parity: FAITHFUL
//!
//! Stronghold templates, rewards, amounts and densities (`driver.strongholds`): game data, embedded as the JSON
//! that tools/engine-gen-strongholds.js writes from the official module. Callers read it as a stored value
//! (`live::from_val` for a fresh object graph per use).

use crate::doc::Val;
use std::sync::OnceLock;

/// The whole `module.exports` (`templates`, `coreRewards`, `coreAmounts`, `coreDensities`, `containerRewards`,
/// `containerAmounts`).
pub fn data() -> &'static Val {
    static DATA: OnceLock<Val> = OnceLock::new();
    DATA.get_or_init(|| Val::from_trusted_json(&util::json::Json::parse(include_str!("strongholds.json")).expect("strongholds.json")))
}

#[cfg(test)]
mod tests {
    #[test]
    fn templates_parse() {
        let d = super::data().to_json();
        let bunker5 = d.get("templates").and_then(|t| t.get("bunker5")).expect("bunker5");
        assert_eq!(bunker5.get("rewardLevel").and_then(|v| v.as_f64()), Some(5.0));
        assert_eq!(d.get("coreAmounts").map(|a| a.stringify()), Some("[0,1000,16000,60000,400000,3000000]".into()));
    }
}
