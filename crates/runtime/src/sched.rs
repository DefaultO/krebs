//! Runner-phase scheduling: one shared LPT cursor (M1); home-worker affinity + steal-largest (M5)
//! (ARCHITECTURE.md §6.4). Owner: WP08, then WP19.

use crate::Runtime;
use engine::runner::{RunJob, RunOutput};
use engine::world::World;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Mutex;

/// `runner_threads` worker loops on the pool pop jobs largest-`expected_ms` first (vanilla's
/// `lastUsedDirtyTime` order; ties keep the given order) until none are left. Outputs come back in
/// `jobs` order.
pub(crate) fn run_phase(rt: &Runtime, pool: &util::pool::Pool, w: &World, jobs: &[RunJob]) -> Vec<RunOutput> {
    let order = lpt_order(jobs);
    let next = AtomicUsize::new(0);
    let outs: Vec<Mutex<Option<RunOutput>>> = jobs.iter().map(|_| Mutex::new(None)).collect();
    let loops = rt.cfg.runner_threads.clamp(1, jobs.len().max(1));
    pool.for_each(loops, |_| {
        while let Some(&i) = order.get(next.fetch_add(1, Relaxed)) {
            let t = std::time::Instant::now();
            let input = engine::runner::build_input(w, &jobs[i]);
            crate::timing::add(0, t);
            let out = crate::host::run(rt, w, input);
            crate::timing::add_run(jobs[i].user, t);
            *outs[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(out);
        }
    });
    outs.into_iter().map(|o| o.into_inner().unwrap_or_else(|e| e.into_inner()).expect("every job ran")).collect()
}

fn lpt_order(jobs: &[RunJob]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by(|&a, &b| jobs[b].expected_ms.total_cmp(&jobs[a].expected_ms));
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn largest_first_stable() {
        let job = |user, expected_ms| RunJob { user, expected_ms, limit: 0.0, tick_limit: 0.0, bucket: 0.0 };
        let jobs = [job(0, 1.0), job(1, 5.0), job(2, 1.0), job(3, 9.0)];
        assert_eq!(lpt_order(&jobs), [3, 1, 0, 2]);
    }
}
