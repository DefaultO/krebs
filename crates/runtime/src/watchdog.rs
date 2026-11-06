//! Watchdog thread: per worker slot a deadline stack, thread clock and IsolateHandle; CPU deadlines via
//! request_interrupt + terminate_execution, wall hard timeout (ARCHITECTURE.md §6.10). Owner: WP08.
//!
//! Mirrors isolated-vm's `RunWithTimeout` (run_with_timeout.h:17-169): every timed `compileAndRun` pushes
//! its own deadline. When one passes, an interrupt on the running thread renders the current 10-frame
//! stack (`RenderSingleStack`) and terminates execution (uncatchable: no `catch`/`finally` runs). The
//! boundary owning a fired deadline decrements the in-flight count, cancels termination when it reaches
//! 0 (`terminate_depth`) and throws `Script execution timed out.`; every other boundary lets the
//! termination unwind through it. Until the owner returns, the watchdog terminates again every
//! `watchdogPollUs` (ivm's "v8 just doesn't get the hint" loop). The hard timeout terminates and keeps
//! terminating until the run ends. With nothing armed the watchdog parks (`begin`/`push` wake it);
//! otherwise it sleeps on the high-resolution timer in naps of at most `NAP_NS`.
//!
//! A slot belongs to one OS thread (a pool worker or any other thread running VMs) and is leaked, so
//! the interrupt callback can hold a plain `&'static` to it.

use crate::cpu::{self, Clock};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use util::sys::ThreadClock;

pub(crate) struct Slot {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    thread: Option<std::thread::ThreadId>,
    clock: Option<ThreadClock>,
    /// The isolate running on this thread and its context, between `begin` and `end` (the context is
    /// created and dropped by the worker under the isolate's lock).
    handle: Option<v8::IsolateHandle>,
    context: Option<v8::Global<v8::Context>>,
    /// Bumped by `begin` and `end`, so a late interrupt can tell it belongs to an earlier run.
    run_gen: u64,
    interrupt_gen: Option<u64>,
    hard_at: Option<u64>,
    hard_fired: bool,
    entries: Vec<Entry>,
    next_id: u64,
    /// Fired deadlines whose termination was issued and not yet consumed by their boundary.
    depth: u32,
}

struct Entry {
    id: u64,
    clock: Clock,
    at: u64,
    fired: bool,
    /// The interrupt ran for it: termination issued and `stack` captured.
    armed: bool,
    stack: String,
}

static SLOTS: Mutex<Vec<&'static Slot>> = Mutex::new(Vec::new());
static POLL_US: AtomicU64 = AtomicU64::new(500);
static WATCHDOG: OnceLock<std::thread::Thread> = OnceLock::new();

/// How often a termination still in flight is repeated (`runtime.watchdogPollUs`).
pub(crate) fn set_poll_us(us: u64) {
    POLL_US.store(us.max(50), Relaxed);
}

/// Sleeps are about 1 ms coarse (Windows' high-resolution timer included) and wake later still on a
/// loaded machine, so the last stretch before a deadline is polled continuously.
const SPIN_NS: u64 = 3_000_000;

/// This thread's slot (created and registered on first use; starts the watchdog thread once).
pub(crate) fn slot() -> &'static Slot {
    thread_local! {
        static SLOT: &'static Slot = new_slot();
    }
    SLOT.with(|s| *s)
}

fn new_slot() -> &'static Slot {
    // ponytail: one leaked slot per thread that ever runs a VM (bounded by the pool + test threads).
    let s: &'static Slot = Box::leak(Box::new(Slot { state: Mutex::new(State::default()) }));
    lock(&SLOTS).push(s);
    WATCHDOG.get_or_init(|| std::thread::Builder::new().name("watchdog".into()).spawn(watch).expect("spawn watchdog").thread().clone());
    s
}

/// Wakes the watchdog to re-plan (a new deadline may be the nearest one).
fn wake() {
    if let Some(t) = WATCHDOG.get() {
        t.unpark();
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Longest sleep while something is armed. `park_timeout` is `WaitOnAddress` on Windows (system tick,
/// up to 15.6 ms late: deadlines overshot by 10 ms on a quiet machine), so the watchdog sleeps with
/// `std::thread::sleep` (a high-resolution waitable timer), which `wake` cannot cut short: a deadline
/// armed meanwhile is seen within this nap. Pushed deadlines are at least 30 ms away (MOD-TIMEOUT
/// floor), hard timeouts seconds.
const NAP_NS: u64 = 5_000_000;

/// Parks while nothing is armed; otherwise sleeps until the nearest deadline is `SPIN_NS` away (at
/// most `NAP_NS` at a time), then polls without sleeping until it passes. CPU deadlines cannot pass
/// sooner than their remaining CPU time in wall time.
fn watch() {
    raise_priority();
    loop {
        let slots = lock(&SLOTS).clone();
        let next = slots.into_iter().filter_map(Slot::poll).min();
        match next {
            None => std::thread::park(),
            Some(ns) if ns > SPIN_NS => std::thread::sleep(Duration::from_nanos((ns - SPIN_NS).min(NAP_NS))),
            // Busy: `yield_now` (SwitchToThread) may give the processor to a ready thread for a whole
            // quantum, which overshot deadlines by ms on a loaded machine.
            Some(_) => std::hint::spin_loop(),
        }
    }
}

/// The watchdog must run on time on a saturated machine (runner threads = cores).
fn raise_priority() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentThread() -> *mut c_void;
            fn SetThreadPriority(thread: *mut c_void, priority: i32) -> i32;
        }
        const THREAD_PRIORITY_HIGHEST: i32 = 2;
        // SAFETY: pseudo handle of the calling thread; failure only leaves the default priority.
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };
    }
    // ponytail: Linux keeps the default priority (raising it needs CAP_SYS_NICE).
}

/// What a `compileAndRun` boundary does after its script returned.
pub(crate) enum Boundary {
    /// Its deadline did not take effect: return the result or let the exception/termination unwind.
    Pass,
    /// Its deadline terminated the script and termination was cancelled: throw the timeout error with
    /// this rendered stack (isolated-vm's `RuntimeGenericError("Script execution timed out.", stack)`).
    TimedOut(String),
}

impl Slot {
    /// A run of `handle`'s isolate starts on this thread; `hard_ms` is the wall hard timeout.
    pub fn begin(&self, handle: v8::IsolateHandle, context: v8::Global<v8::Context>, hard_ms: u64) {
        let mut st = lock(&self.state);
        if st.clock.is_none() {
            st.thread = Some(std::thread::current().id());
            st.clock = Some(ThreadClock::current());
        }
        st.handle = Some(handle);
        st.context = Some(context);
        st.run_gen += 1;
        st.interrupt_gen = None;
        st.hard_at = Some(cpu::wall_ns().saturating_add(hard_ms.saturating_mul(1_000_000)));
        st.hard_fired = false;
        st.entries.clear();
        st.depth = 0;
        drop(st);
        wake();
    }

    /// The run is over: the watchdog stops touching the isolate. Returns whether the hard timeout fired.
    pub fn end(&self) -> bool {
        let mut st = lock(&self.state);
        st.handle = None;
        st.context = None;
        st.run_gen += 1;
        st.interrupt_gen = None;
        st.hard_at = None;
        st.entries.clear();
        st.depth = 0;
        st.hard_fired
    }

    /// Arms a deadline `timeout_ms` from now on `clock`; pair with `pop`.
    pub fn push(&self, clock: Clock, timeout_ms: f64) -> u64 {
        let at = cpu::now_ns(clock).saturating_add((timeout_ms * 1e6) as u64);
        let mut st = lock(&self.state);
        st.next_id += 1;
        let id = st.next_id;
        st.entries.push(Entry { id, clock, at, fired: false, armed: false, stack: String::new() });
        drop(st);
        wake();
        id
    }

    /// Disarms deadline `id`. If it terminated the script, `cancel` runs (under the slot lock, so the
    /// watchdog cannot terminate again in between) once no other fired deadline and no other reason
    /// (`other_termination`: halt, heap, hard timeout) still needs the termination.
    pub fn pop(&self, id: u64, other_termination: bool, cancel: impl FnOnce()) -> Boundary {
        let mut st = lock(&self.state);
        let Some(i) = st.entries.iter().rposition(|e| e.id == id) else { return Boundary::Pass };
        let e = st.entries.remove(i);
        // Fired but the interrupt never ran: the script finished first (ivm: did_finish wins).
        if !e.armed {
            return Boundary::Pass;
        }
        st.depth -= 1;
        if st.depth > 0 || other_termination || st.hard_fired {
            return Boundary::Pass;
        }
        cancel();
        Boundary::TimedOut(e.stack)
    }

    /// Fires what is due; returns the ns until this slot next needs a look (None: nothing armed).
    fn poll(&'static self) -> Option<u64> {
        let mut st = lock(&self.state);
        let handle = st.handle.clone()?;
        let wall = cpu::wall_ns();
        let again = POLL_US.load(Relaxed) * 1000;
        if st.hard_fired || st.hard_at.is_some_and(|h| wall >= h) {
            st.hard_fired = true;
            handle.terminate_execution();
            return Some(again);
        }
        let cpu_now = match &st.clock {
            Some(c) if st.entries.iter().any(|e| !e.fired && e.clock == Clock::ThreadCpu) => c.cpu_ns(),
            _ => 0,
        };
        let mut next = st.hard_at.map(|h| h - wall);
        for e in st.entries.iter_mut().filter(|e| !e.fired) {
            let now = if e.clock == Clock::ThreadCpu { cpu_now } else { wall };
            if now >= e.at {
                e.fired = true;
            } else {
                next = Some(next.map_or(e.at - now, |n| n.min(e.at - now)));
            }
        }
        // Also re-requested when a pending interrupt was consumed without arming (one queued in an
        // earlier run of this isolate fires first and finds nothing to do).
        if st.interrupt_gen.is_none() && st.entries.iter().any(|e| e.fired && !e.armed) {
            st.interrupt_gen = Some(st.run_gen);
            handle.request_interrupt(on_interrupt, self as *const Slot as *mut c_void);
        }
        if st.depth > 0 {
            handle.terminate_execution();
        }
        if st.depth > 0 || st.entries.iter().any(|e| e.fired) {
            next = Some(next.map_or(again, |n| n.min(again)));
        }
        next
    }
}

/// Runs on the isolate's thread at its next interrupt check (ivm `TimeoutRunner::Run`).
unsafe extern "C" fn on_interrupt(mut iso: v8::UnsafeRawIsolatePtr, data: *mut c_void) {
    // SAFETY: `data` is a leaked `&'static Slot`.
    let slot = unsafe { &*(data as *const Slot) };
    let mut st = lock(&slot.state);
    // An interrupt queued during an earlier run of this isolate can fire after it migrated: it must
    // not consume the pending interrupt of the run now using the slot on its old thread.
    if st.thread != Some(std::thread::current().id()) {
        return;
    }
    let current = st.interrupt_gen.take() == Some(st.run_gen);
    let n = st.entries.iter().filter(|e| e.fired && !e.armed).count() as u32;
    if !current || n == 0 {
        return; // a late interrupt of an earlier run, or every fired script already returned
    }
    // SAFETY: V8 passes the isolate the interrupt runs on; we are on its thread holding its lock.
    let isolate = unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut iso) };
    let Some(context) = st.context.clone() else { return };
    let stack = render_stack(isolate, &context);
    for e in st.entries.iter_mut().filter(|e| e.fired && !e.armed) {
        e.armed = true;
        e.stack = stack.clone();
    }
    st.depth += n;
    isolate.terminate_execution();
}

/// isolated-vm `StackTraceHolder::RenderSingleStack(CurrentStackTrace(isolate, 10))` (stack_trace.cc:149-216):
/// no `Type.` prefix, and an anonymous non-constructor frame prints its bare location and ends the text.
pub(crate) fn render_stack(isolate: &mut v8::Isolate, context: &v8::Global<v8::Context>) -> String {
    v8::callback_scope!(unsafe scope, isolate);
    v8::scope!(let scope, scope);
    let context = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, context);
    let mut out = String::new();
    let Some(trace) = v8::StackTrace::current_stack_trace(scope, 10) else { return out };
    let text = |s: Option<v8::Local<v8::String>>, scope: &v8::PinScope<'_, '_>| {
        s.map(|s| s.to_rust_string_lossy(scope)).unwrap_or_default()
    };
    for i in 0..trace.get_frame_count() {
        let Some(frame) = trace.get_frame(scope, i) else { break };
        out.push_str("\n    at ");
        let name = text(frame.get_function_name(scope), scope);
        let location = |out: &mut String| {
            let script = text(frame.get_script_name_or_source_url(scope), scope);
            if script.is_empty() {
                out.push_str(if frame.is_eval() { "[eval]" } else { "<anonymous>" });
            } else {
                out.push_str(&script);
            }
            out.push_str(&format!(":{}:{}", frame.get_line_number(), frame.get_column()));
        };
        if frame.is_wasm() {
            let script = text(frame.get_script_name(scope), scope);
            let named = !name.is_empty() || !script.is_empty();
            if named {
                if script.is_empty() {
                    out.push_str(&name);
                } else {
                    out.push_str(&script);
                    if !name.is_empty() {
                        out.push('.');
                        out.push_str(&name);
                    }
                }
                out.push_str(" (<WASM>");
            }
            out.push_str(&format!("{}:{}", frame.get_line_number(), frame.get_column()));
            if named {
                out.push(')');
            }
        } else if frame.is_constructor() {
            out.push_str("new ");
            out.push_str(if name.is_empty() { "<anonymous>" } else { &name });
            out.push_str(" (");
            location(&mut out);
            out.push(')');
        } else if !name.is_empty() {
            out.push_str(&name);
            out.push_str(" (");
            location(&mut out);
            out.push(')');
        } else {
            location(&mut out);
            return out;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fired_slot() -> &'static Slot {
        let s: &'static Slot = Box::leak(Box::new(Slot { state: Mutex::new(State::default()) }));
        let mut st = lock(&s.state);
        st.run_gen = 1;
        st.entries.push(Entry { id: 1, clock: Clock::Wall, at: 0, fired: true, armed: false, stack: String::new() });
        drop(st);
        s
    }

    /// A stale interrupt delivered on another thread (the isolate migrated) leaves the slot's pending
    /// interrupt to the run that owns it.
    #[test]
    fn stale_interrupt_on_another_thread_consumes_nothing() {
        let s = fired_slot();
        let other = std::thread::spawn(|| std::thread::current().id()).join().unwrap();
        {
            let mut st = lock(&s.state);
            st.thread = Some(other);
            st.interrupt_gen = Some(1);
        }
        // SAFETY: returns before touching the isolate (wrong thread).
        unsafe { on_interrupt(v8::UnsafeRawIsolatePtr::null(), s as *const Slot as *mut c_void) };
        let st = lock(&s.state);
        assert_eq!((st.interrupt_gen, st.depth, st.entries[0].armed), (Some(1), 0, false));
    }

    /// A fired, unarmed deadline whose interrupt was consumed elsewhere gets a new interrupt.
    #[test]
    fn fired_deadline_without_pending_interrupt_is_requested_again() {
        crate::init_v8();
        let iso = v8::Isolate::new(Default::default());
        let s = fired_slot();
        lock(&s.state).handle = Some(iso.thread_safe_handle());
        s.poll();
        assert_eq!(lock(&s.state).interrupt_gen, Some(1));
        lock(&s.state).handle = None;
    }
}
