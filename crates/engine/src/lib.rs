//! Game rules and the world model: no V8, no networking (it does own checkpoint/import file I/O).
//! Module tree per ARCHITECTURE.md §13; frozen after WP00, owned by WP11.

pub mod bulk;
pub mod constants;
pub mod doc;
pub mod ids;
pub mod live;
pub mod policy;
pub mod room_name;
pub mod terrain;

pub mod coll;
pub mod common;
pub mod hooks;
pub mod intents;
pub mod loki;
pub mod mapgen;
pub mod persist;
pub mod processor;
pub mod runner;
pub mod scenario;
pub mod strongholds;
pub mod system;
pub mod utils;
pub mod world;

/// CRON-STAGE (§5.9).
pub mod cron {
    pub mod cronjobs;
    pub mod strongholds;

    pub use cronjobs::{CronCtx, JOBS};
}
