//! Persistent worker thread pool for data-parallel loops (the tick pipeline).
//!
//! `for_each(n, f)` runs `f(0..n)` on the caller plus the workers and returns
//! when every item is done. `f` may borrow from the caller's stack: the job
//! holds a lifetime-erased pointer to it, which is sound because the caller
//! does not return (not even by unwinding) until no worker can touch the job.
//!
//! Protocol, all on one atomic `state` word (epoch | CLOSED | count):
//! 1. The caller writes the job, then publishes a new epoch with count 0.
//! 2. A worker joins by CAS-incrementing count while the epoch matches and
//!    CLOSED is clear, and only then reads the job.
//! 3. Everyone claims batches from the `next` counter until it runs past n.
//! 4. The caller sets CLOSED (so nobody new can join) and waits for count 0.
//!    Only then does it return and may later write the next job.
//!
//! `for_each_dyn`/`map_dyn` claim one item at a time in index order, so a list
//! sorted by descending cost is scheduled longest-first (LPT).

use std::any::Any;
use std::cell::UnsafeCell;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long an idle worker (or a waiting caller) spins before sleeping.
/// Back-to-back calls in a tick then never pay a thread wake-up.
const SPIN: Duration = Duration::from_micros(100);

const CLOSED: u64 = 1 << 31;
const COUNT: u64 = CLOSED - 1;

pub struct Pool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    /// Serialises callers. A call that finds it taken (a concurrent caller, or
    /// a nested call from inside a job) runs its items inline instead.
    call: Mutex<()>,
}

/// Keeps hot atomics on separate cache lines.
#[repr(align(128))]
struct Padded<T>(T);

struct Shared {
    /// Epoch (high 32 bits) | CLOSED | number of workers inside the job.
    state: Padded<AtomicU64>,
    /// Next unclaimed item of the current job.
    next: Padded<AtomicUsize>,
    job: UnsafeCell<Option<Job>>,
    panic: Mutex<Option<Box<dyn Any + Send>>>,
    sleepers: AtomicUsize,
    shutdown: AtomicBool,
    lock: Mutex<()>,
    wake: Condvar,
    done: Condvar,
}

// SAFETY: every field but `job` is Sync. `job` is written only by the caller
// holding `Pool::call`, while the state is CLOSED with count 0 (step 1/4 of
// the protocol), so no worker is reading it; workers read it only after
// joining an open epoch, which happens-after that write (Release publish,
// Acquire CAS) and before the caller can write again (it waits for count 0).
// The pointed-to closure is `Sync`, so calling it from many threads is fine.
unsafe impl Sync for Shared {}
// SAFETY: as above; the raw closure pointer is only dereferenced under the protocol.
unsafe impl Send for Shared {}

#[derive(Clone, Copy)]
struct Job {
    /// Lifetime-erased borrow of the caller's closure.
    f: *const (dyn Fn(usize) + Sync + 'static),
    n: usize,
    batch: usize,
}

thread_local! {
    static WORKER_INDEX: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Index of the calling thread within its pool: 1..threads for pool workers,
/// 0 for any other thread (the caller is participant 0). Lets per-worker state
/// (a system isolate, a watchdog slot) live in a plain array.
pub fn worker_index() -> usize {
    WORKER_INDEX.with(|w| w.get())
}

fn lock(m: &Mutex<()>) -> MutexGuard<'_, ()> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Pool {
    /// A pool running jobs on `threads` threads in total: the caller plus
    /// `threads - 1` workers. `threads <= 1` runs everything inline.
    pub fn new(threads: usize) -> Pool {
        let shared = Arc::new(Shared {
            state: Padded(AtomicU64::new(CLOSED)),
            next: Padded(AtomicUsize::new(0)),
            job: UnsafeCell::new(None),
            panic: Mutex::new(None),
            sleepers: AtomicUsize::new(0),
            shutdown: AtomicBool::new(false),
            lock: Mutex::new(()),
            wake: Condvar::new(),
            done: Condvar::new(),
        });
        let workers = (1..threads.max(1))
            .map(|i| {
                let sh = shared.clone();
                std::thread::Builder::new()
                    .name(format!("pool-{i}"))
                    .spawn(move || {
                        WORKER_INDEX.with(|w| w.set(i));
                        worker(&sh)
                    })
                    .expect("spawn pool worker")
            })
            .collect();
        Pool { shared, workers, call: Mutex::new(()) }
    }

    pub fn threads(&self) -> usize {
        self.workers.len() + 1
    }

    /// Calls `f(i)` for every i in 0..n across the pool and blocks until all
    /// are done. If any call panics, the first panic is re-raised here after
    /// every other thread has stopped using `f`.
    pub fn for_each<F: Fn(usize) + Sync>(&self, n: usize, f: F) {
        self.run_job(n, (n / (self.threads() * 8)).max(1), &f);
    }

    /// `for_each` claiming one item at a time in index order: for few,
    /// uneven items sorted by descending cost.
    pub fn for_each_dyn<F: Fn(usize) + Sync>(&self, n: usize, f: F) {
        self.run_job(n, 1, &f);
    }

    /// `(0..n).map(f).collect()`, computed across the pool.
    pub fn map<T: Send, F: Fn(usize) -> T + Sync>(&self, n: usize, f: F) -> Vec<T> {
        self.map_with(n, false, f)
    }

    /// `map` claiming one item at a time in index order (see `for_each_dyn`).
    pub fn map_dyn<T: Send, F: Fn(usize) -> T + Sync>(&self, n: usize, f: F) -> Vec<T> {
        self.map_with(n, true, f)
    }

    fn map_with<T: Send, F: Fn(usize) -> T + Sync>(&self, n: usize, dyn_: bool, f: F) -> Vec<T> {
        // ponytail: a mutex per slot keeps this safe code; write into
        // MaybeUninit slots if the ~20ns per item ever shows up.
        let slots: Vec<Mutex<Option<T>>> = (0..n).map(|_| Mutex::new(None)).collect();
        let store = |i: usize| *slots[i].lock().unwrap() = Some(f(i));
        if dyn_ {
            self.for_each_dyn(n, store);
        } else {
            self.for_each(n, store);
        }
        slots.into_iter().map(|s| s.into_inner().unwrap().unwrap()).collect()
    }

    fn run_job(&self, n: usize, batch: usize, f: &(dyn Fn(usize) + Sync)) {
        assert!(n <= usize::MAX / 2, "too many items");
        let _guard = match self.call.try_lock() {
            Ok(g) => g,
            Err(TryLockError::Poisoned(e)) => e.into_inner(), // an earlier job panicked
            Err(TryLockError::WouldBlock) => {
                // ponytail: concurrent and nested calls run serially; queue jobs if that ever matters.
                (0..n).for_each(f);
                return;
            }
        };
        if self.workers.is_empty() || n <= 1 {
            (0..n).for_each(f);
            return;
        }
        let sh = &*self.shared;
        // SAFETY: only the lifetime changes. The pointer is dereferenced by
        // workers that joined this job, and we do not return (run() catches
        // panics, the wait below cannot unwind) until all of them have left
        // and CLOSED keeps new ones out; it is cleared before returning.
        let f: *const (dyn Fn(usize) + Sync + '_) = f;
        let f = unsafe { std::mem::transmute::<*const (dyn Fn(usize) + Sync + '_), *const (dyn Fn(usize) + Sync + 'static)>(f) };
        let job = Job { f, n, batch };

        // 1. Publish. The previous job is CLOSED with count 0, so no worker reads `job` now.
        // SAFETY: see `unsafe impl Sync for Shared`.
        unsafe { *sh.job.get() = Some(job) };
        sh.next.0.store(0, Relaxed);
        let epoch = (sh.state.0.load(Relaxed) >> 32).wrapping_add(1) & 0xffff_ffff;
        // SeqCst pairs with the sleepers check in wait_for_job (no lost wake-ups).
        sh.state.0.store(epoch << 32, SeqCst);
        if sh.sleepers.load(SeqCst) > 0 {
            let _g = lock(&sh.lock);
            sh.wake.notify_all();
        }

        // 3. Work alongside the workers.
        run(sh, &job);

        // 4. Close and wait for everyone who joined to leave.
        let s = sh.state.0.fetch_or(CLOSED, AcqRel);
        if s & COUNT != 0 {
            let start = Instant::now();
            while sh.state.0.load(Acquire) & COUNT != 0 {
                if start.elapsed() > SPIN {
                    let mut g = lock(&sh.lock);
                    while sh.state.0.load(Acquire) & COUNT != 0 {
                        g = sh.done.wait(g).unwrap_or_else(|e| e.into_inner());
                    }
                    break;
                }
                std::hint::spin_loop();
            }
        }
        // SAFETY: nobody is inside the job any more; drop the dangling pointer.
        unsafe { *sh.job.get() = None };
        let payload = sh.panic.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(p) = payload {
            panic::resume_unwind(p);
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.shutdown.store(true, SeqCst);
        {
            let _g = lock(&self.shared.lock);
            self.shared.wake.notify_all();
        }
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}

fn worker(sh: &Shared) {
    let mut seen = sh.state.0.load(Acquire) >> 32;
    while let Some(epoch) = wait_for_job(sh, seen) {
        seen = epoch;
        if join(sh, epoch) {
            // SAFETY: we joined the open job `epoch`, so it is published and
            // the caller cannot clear or replace it until we leave.
            let job = unsafe { (*sh.job.get()).unwrap() };
            run(sh, &job);
            leave(sh);
        }
    }
}

/// Waits for an epoch other than `seen`; None on shutdown.
fn wait_for_job(sh: &Shared, seen: u64) -> Option<u64> {
    let start = Instant::now();
    let mut spins = 0u32;
    loop {
        let e = sh.state.0.load(Acquire) >> 32;
        if e != seen {
            return Some(e);
        }
        if sh.shutdown.load(Relaxed) {
            return None;
        }
        spins = spins.wrapping_add(1);
        if spins.is_multiple_of(64) && start.elapsed() > SPIN {
            break;
        }
        std::hint::spin_loop();
    }
    let mut g = lock(&sh.lock);
    sh.sleepers.fetch_add(1, SeqCst);
    let result = loop {
        if sh.shutdown.load(SeqCst) {
            break None;
        }
        let e = sh.state.0.load(SeqCst) >> 32;
        if e != seen {
            break Some(e);
        }
        g = sh.wake.wait(g).unwrap_or_else(|e| e.into_inner());
    };
    sh.sleepers.fetch_sub(1, SeqCst);
    result
}

/// Protocol step 2: enter job `epoch` if it is still open.
fn join(sh: &Shared, epoch: u64) -> bool {
    let mut s = sh.state.0.load(Relaxed);
    loop {
        if s >> 32 != epoch || s & CLOSED != 0 {
            return false;
        }
        // Acquire: synchronises with the publishing store, so `job` is visible.
        match sh.state.0.compare_exchange_weak(s, s + 1, Acquire, Relaxed) {
            Ok(_) => return true,
            Err(x) => s = x,
        }
    }
}

fn leave(sh: &Shared) {
    // Release: our side effects happen-before the caller seeing count 0.
    let prev = sh.state.0.fetch_sub(1, Release);
    if prev & CLOSED != 0 && prev & COUNT == 1 {
        let _g = lock(&sh.lock);
        sh.done.notify_all();
    }
}

/// Protocol step 3: claim and run batches until the items run out. Panics are
/// caught and stored so the protocol always completes.
fn run(sh: &Shared, job: &Job) {
    // SAFETY: callers are inside the job (the caller itself, or a joined
    // worker), so the closure is alive.
    let f = unsafe { &*job.f };
    loop {
        let start = sh.next.0.fetch_add(job.batch, Relaxed);
        if start >= job.n {
            return;
        }
        let end = (start + job.batch).min(job.n);
        if let Err(p) = panic::catch_unwind(AssertUnwindSafe(|| (start..end).for_each(f))) {
            let mut slot = sh.panic.lock().unwrap_or_else(|e| e.into_inner());
            if slot.is_none() {
                *slot = Some(p);
            }
            sh.next.0.store(job.n, Relaxed); // stop handing out work
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU8;

    #[test]
    fn every_item_once() {
        let pool = Pool::new(8);
        for n in [0, 1, 2, 7, 100, 1_000_000] {
            let hits: Vec<AtomicU8> = (0..n).map(|_| AtomicU8::new(0)).collect();
            let sum = AtomicU64::new(0);
            pool.for_each(n, |i| {
                hits[i].fetch_add(1, Relaxed);
                sum.fetch_add(i as u64, Relaxed);
            });
            assert!(hits.iter().all(|h| h.load(Relaxed) == 1));
            assert_eq!(sum.load(Relaxed), (n as u64) * (n as u64).saturating_sub(1) / 2);
        }
    }

    #[test]
    fn uneven_tasks_and_map() {
        let pool = Pool::new(8);
        let base = vec![3u64; 1000]; // borrowed from this stack frame
        let out = pool.map(1000, |i| {
            if i % 97 == 0 {
                std::thread::sleep(Duration::from_millis(2));
            }
            base[i] * i as u64
        });
        assert_eq!(out, (0..1000).map(|i| 3 * i as u64).collect::<Vec<_>>());
    }

    #[test]
    fn panics_propagate_after_completion() {
        let pool = Pool::new(4);
        let running = AtomicUsize::new(0);
        for _ in 0..20 {
            let r = panic::catch_unwind(AssertUnwindSafe(|| {
                pool.for_each(10_000, |i| {
                    running.fetch_add(1, SeqCst);
                    if i == 777 {
                        panic!("boom {i}");
                    }
                    std::hint::black_box(i);
                    running.fetch_sub(1, SeqCst);
                })
            }));
            let msg = r.unwrap_err().downcast::<String>().unwrap();
            assert_eq!(*msg, "boom 777");
            // Only the panicking call is unbalanced: nothing still runs.
            assert_eq!(running.swap(0, SeqCst), 1);
        }
        assert_eq!(pool.map(100, |i| i).len(), 100); // still usable
    }

    #[test]
    fn nested_and_concurrent_calls() {
        let pool = Pool::new(4);
        let total = AtomicUsize::new(0);
        pool.for_each(16, |_| pool.for_each(16, |_| _ = total.fetch_add(1, Relaxed)));
        assert_eq!(total.load(Relaxed), 256);
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..50 {
                        assert_eq!(pool.map(64, |i| i * 2).iter().sum::<usize>(), 64 * 63);
                    }
                });
            }
        });
        assert_eq!(Pool::new(1).map(5, |i| i), vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn dyn_claims_in_index_order() {
        let pool = Pool::new(4);
        for _ in 0..20 {
            // Item i waits until every earlier item has started. With one item per claim
            // in index order this completes; claims out of order would leave all threads
            // waiting on an unclaimed lower item.
            let started = AtomicUsize::new(0);
            let deadline = Instant::now() + Duration::from_secs(10);
            pool.for_each_dyn(200, |i| {
                while started.load(SeqCst) != i {
                    assert!(Instant::now() < deadline, "item {i} claimed out of order");
                    std::hint::spin_loop();
                }
                started.store(i + 1, SeqCst);
            });
            assert_eq!(started.load(SeqCst), 200);
        }
        assert_eq!(pool.map_dyn(1000, |i| i * 3), (0..1000).map(|i| i * 3).collect::<Vec<_>>());
    }

    #[test]
    fn worker_indexes() {
        let pool = Pool::new(4);
        assert_eq!(worker_index(), 0);
        let seen: Vec<AtomicU8> = (0..4).map(|_| AtomicU8::new(0)).collect();
        for _ in 0..50 {
            pool.for_each_dyn(64, |_| {
                seen[worker_index()].store(1, Relaxed);
                std::thread::sleep(Duration::from_micros(200));
            });
        }
        assert_eq!(seen[0].load(Relaxed), 1, "caller is participant 0");
        assert!(seen[1..].iter().any(|s| s.load(Relaxed) == 1), "workers report 1..threads");
    }

    #[test]
    fn idle_workers_wake_up() {
        let pool = Pool::new(4);
        std::thread::sleep(Duration::from_millis(20)); // workers go to sleep
        let ids = pool.map(64, |_| {
            std::thread::sleep(Duration::from_millis(1));
            format!("{:?}", std::thread::current().id())
        });
        let distinct: std::collections::HashSet<_> = ids.into_iter().collect();
        assert!(distinct.len() > 1, "work never left the caller");
    }
}
