//! Runner-phase data types shared with `runtime` (ARCHITECTURE.md §6.2, FROZEN). No V8 here.

pub mod data;
pub mod make;

pub use data::build_input;

use crate::doc::Val;
use crate::intents;
use crate::world::{RoomBlob, RoomIdx, UserIdx};
use std::sync::Arc;
use util::jsstr::JsStr;

pub struct RunJob {
    pub user: UserIdx,
    pub expected_ms: f32,
    /// Game.cpu.limit
    pub limit: f64,
    /// CPU-LIMIT: min(bucket + cpu, cpuMaxPerTick); Infinity for cpu 0.
    pub tick_limit: f64,
    pub bucket: f64,
}

pub struct RunInput {
    pub user: UserIdx,
    /// Per-user JSON, §6.6.
    pub meta: String,
    /// Parallel to meta.rooms (None for names that are not rooms).
    pub blobs: Vec<Option<RoomBlob>>,
    /// Parallel to `blobs`: the room of each blob (None: no doc reuse for it).
    pub blob_rooms: Vec<Option<RoomIdx>>,
    /// Parallel to the vision rooms.
    pub event_logs: Vec<Arc<str>>,
    /// Sorted; INT-STORE routing.
    pub visible: Vec<RoomIdx>,
    pub memory: JsStr,
    pub memory_version: u64,
    pub segments: Vec<(u8, JsStr)>,
    pub foreign_segment: Option<(Box<str>, u8, JsStr)>,
    /// Shipped only when the VM's code version differs.
    pub code: Arc<CodeBundle>,
    pub vnow: i64,
    pub nominal_tick_ms: u32,
}

pub struct CodeBundle {
    pub version: u64,
    /// (unescaped name, module)
    pub modules: Vec<(Box<str>, Module)>,
}

pub enum Module {
    Text(Arc<str>),
    Binary(Arc<[u8]>),
}

pub enum RunStatus {
    Done,
    /// Saved, billed.
    PartialError,
    /// Discarded, unbilled.
    TotalError,
    Skipped,
    Deactivated,
    /// Discarded.
    Reset(ResetReason),
}

/// Why a run ended in a VM reset with its output discarded (02 VM-RESET-SUMMARY).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResetReason {
    /// Game.cpu.halt()
    Halt,
    /// VM-HEAP-LIMIT
    Heap,
    /// Wall hard timeout.
    HardTimeout,
    /// Memory governor (§6.11).
    Governor,
}

pub struct RunOutput {
    pub user: UserIdx,
    pub status: RunStatus,
    pub error: Option<String>,
    /// None ⇒ discarded.
    pub intents: Option<intents::UserIntents>,
    /// Some only if saved and changed.
    pub memory: Option<JsStr>,
    pub segments: Vec<(u8, JsStr)>,
    pub seg_settings: Option<Val>,
    pub inter_shard: Option<JsStr>,
    pub console_log: Vec<String>,
    pub console_results: Vec<String>,
    /// (room | "" | "map", visual)
    pub visuals: Vec<(Box<str>, Arc<str>)>,
    pub used_time: u32,
    pub used_clean_ms: f64,
    pub used_dirty_ms: u32,
}
