//! Built-in `devtools` mod (PROTO-DEVTOOLS, ARCHITECTURE.md §2.8): `POST /api/dev/control` for bot development
//! loops. Body `{action, n?, ms?, tick?}`:
//!   pause | resume              stop / restart the main loop (env `mainLoopPaused`)
//!   step {n=1} | runFor {n}      run n more ticks while paused (`system.runTicks`)
//!   runUntil {tick}              run while paused until game time reaches `tick`
//!   speed {ms}                   minimal tick duration (env `tickRate`)
//!   status                       nothing; every reply is `{ok: 1, time, paused, tickRate, pendingTicks}`
//! No authentication: enable it (`mods: ["devtools"]`) on development servers only.

use backend::http::{Request, Response};
use backend::mods::{Manifest, Mod, Registry};
use backend::{ApiCtx, Route};
use engine::doc::Val;
use std::sync::atomic::Ordering;
use util::json::Json;

pub struct Devtools;
pub static MOD: Devtools = Devtools;

impl Mod for Devtools {
    fn manifest(&self) -> Manifest {
        Manifest { name: "devtools", version: env!("CARGO_PKG_VERSION"), dependencies: &[] }
    }
    fn register(&self, r: &mut Registry) {
        r.routes.push(Route { method: "POST", path: "/api/dev/control", handler: control });
    }
}

fn control(req: &Request, ctx: &ApiCtx) -> Response {
    let body = req.body();
    let action = body.get("action").and_then(Json::as_str).unwrap_or("status").to_string();
    let num = |k: &str| body.get(k).and_then(Json::as_f64).filter(|n| n.is_finite());
    let ticks = |n: Option<f64>| n.map_or(0, |n| n.max(0.0) as u64);
    let (n, tick, ms) = (num("n"), num("tick"), num("ms"));
    // applied at the next P0 (also while paused); the reply is the world right after it
    let r = backend::call(&ctx.commands, move |w| {
        let run = crate::tick::RUN_TICKS.get();
        let add = |k: u64| {
            if let Some(t) = run {
                t.fetch_add(k, Ordering::Relaxed);
            }
        };
        let set = |w: &mut engine::world::World, key: &str, v: Json| engine::coll::env_set(w, key, &Val::from_trusted_json(&v)).map(|_| ());
        match action.as_str() {
            "pause" => set(w, "mainLoopPaused", Json::Str("1".into()))?,
            "resume" => set(w, "mainLoopPaused", Json::Str("0".into()))?,
            "step" => add(ticks(n).max(1)),
            "runFor" => add(ticks(n)),
            "runUntil" => add(ticks(tick).saturating_sub(w.time)),
            "speed" => match ms {
                Some(ms) if ms >= 0.0 => set(w, "tickRate", Json::Num(ms))?,
                _ => return Err("invalid ms".to_string()),
            },
            "status" => {}
            _ => return Err(format!("unknown action {action}")),
        }
        Ok(Json::Obj(vec![
            ("time".into(), Json::Num(w.time as f64)),
            ("paused".into(), Json::Bool(w.env.paused)),
            ("tickRate".into(), Json::Num(w.env.tick_rate_ms as f64)),
            ("pendingTicks".into(), Json::Num(run.map_or(0, |t| t.load(Ordering::Relaxed)) as f64)),
        ]))
    });
    match r {
        Ok(status) => Response::ok(status),
        Err(e) => Response::error(&e),
    }
}
