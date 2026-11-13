//! Core natives: cpuTime, compileAndRun, halt, heapStats, now (virtual Date), base64ToBuffer, shardName,
//! profile, setupList; system-isolate objectRoom (ARCHITECTURE.md §6.7). Owner: WP08.
//!
//! Natives find the run they belong to through a thread-local `RunCtx`: a VM runs JS only while locked
//! by the thread that installed the context, and the system isolate's `__store` runs on that thread
//! inside the same job.

use crate::cpu::{Clock, RunMeter};
use crate::natives_world::{set, WorldAccess};
use crate::watchdog::{self, Boundary};
use engine::world::{RoomIdx, UserIdx};
use std::alloc::Layout;
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

/// Per-VM state shared with V8 callbacks: the heap-limit flag (near-heap-limit callback data) and
/// isolated-vm's `LimitedAllocator` bookkeeping (allocator_nortti.cc). Refcounted: the Vm, the
/// isolate's array buffer allocator and every host-made buffer hold it, because V8 frees buffers on
/// its own threads, possibly after the Vm is gone.
pub(crate) struct VmFlags {
    pub heap_hit: AtomicBool,
    /// ivm `did_adjust_heap_limit`: the near-heap-limit callback raised V8's limit.
    adjusted: AtomicBool,
    pub handle: OnceLock<v8::IsolateHandle>,
    /// ivm `extra_allocated_memory`: bytes of this isolate's array buffers (`externally_allocated_size`).
    pub extra: AtomicUsize,
    /// ivm `memory_limit` (bytes) and `misc_memory_size` (`heap_size_limit` at creation minus it).
    limit: usize,
    misc: AtomicUsize,
    /// The isolate once created (the allocation check reads its heap), and the check's cache.
    iso: AtomicPtr<c_void>,
    v8_heap: AtomicUsize,
    next_check: AtomicUsize,
    /// The JS side stopped taking over room docs (`docsOff`, doc_reuse.rs).
    pub docs_off: AtomicBool,
}

impl VmFlags {
    pub fn new(limit: usize) -> Arc<VmFlags> {
        Arc::new(VmFlags {
            heap_hit: AtomicBool::new(false),
            adjusted: AtomicBool::new(false),
            handle: OnceLock::new(),
            extra: AtomicUsize::new(0),
            limit,
            misc: AtomicUsize::new(0),
            iso: AtomicPtr::new(std::ptr::null_mut()),
            v8_heap: AtomicUsize::new(4 << 20),
            next_check: AtomicUsize::new(1 << 20),
            docs_off: AtomicBool::new(false),
        })
    }

    /// The isolate exists: termination and heap statistics become available.
    pub fn created(&self, iso: &mut v8::OwnedIsolate, heap_size_limit: usize) {
        let _ = self.handle.set(iso.thread_safe_handle());
        self.misc.store(heap_size_limit.saturating_sub(self.limit), Relaxed);
        // SAFETY: UnsafeRawIsolatePtr is a repr(transparent) raw pointer.
        let raw = unsafe { std::mem::transmute::<v8::UnsafeRawIsolatePtr, *mut c_void>(iso.as_raw_isolate_ptr()) };
        self.iso.store(raw, Relaxed);
    }

    /// `used_heap_size`, read only on the thread whose run owns this VM (None otherwise: before the
    /// isolate exists, during disposal, or off its thread).
    fn used_heap(&self) -> Option<usize> {
        let p = self.iso.load(Relaxed);
        let mine = with_run(|c| std::ptr::eq(c.flags, self)).unwrap_or(false);
        // SAFETY: this thread runs the isolate, which is alive.
        let mut raw = unsafe { std::mem::transmute::<*mut c_void, v8::UnsafeRawIsolatePtr>(p) };
        (mine && !p.is_null()).then(|| unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut raw) }.get_heap_statistics().used_heap_size())
    }

    /// ivm's memory-limit rule (environment.cc:99-105,186-199): heap in use plus array buffers above the
    /// limit plus `misc`.
    fn over_limit(&self) -> bool {
        self.used_heap().is_some_and(|used| used + self.extra.load(Relaxed) > self.limit + self.misc.load(Relaxed))
    }

    /// Heap exhaustion: the run ends in `Reset(Heap)` (section 6.11).
    fn exhausted(&self) {
        self.heap_hit.store(true, Relaxed);
        if let Some(h) = self.handle.get() {
            h.terminate_execution();
        }
    }

    /// ivm `LimitedAllocator::Check` without its LowMemoryNotification: a refusal makes V8 collect
    /// garbage and ask again (`Heap::AllocateExternalBackingStore`), and the retry re-reads the heap.
    fn check(&self, len: usize) -> bool {
        let extra = self.extra.load(Relaxed);
        let cap = self.limit + self.misc.load(Relaxed);
        let mut heap = self.v8_heap.load(Relaxed);
        if heap + extra + len > self.next_check.load(Relaxed) {
            if let Some(used) = self.used_heap() {
                heap = used;
                self.v8_heap.store(heap, Relaxed);
            }
            if heap + extra + len > cap {
                return false;
            }
            self.next_check.store(heap + extra + len + (1 << 20), Relaxed);
        }
        heap + extra + len <= cap
    }
}

fn buffer_layout(len: usize) -> Layout {
    Layout::from_size_align(len.max(1), 16).expect("array buffer layout")
}

/// ivm `LimitedAllocator::Allocate`: refused past the memory limit (V8 throws `RangeError: Array buffer
/// allocation failed`), except a buffer of at most 64 bytes, which V8 cannot do without: it is given and
/// the VM ends (here as heap exhaustion).
unsafe fn allocate_buffer(f: &VmFlags, len: usize, zeroed: bool) -> *mut c_void {
    if !f.check(len) {
        if len > 64 {
            return std::ptr::null_mut();
        }
        f.exhausted();
    }
    // SAFETY: non-zero size (buffer_layout).
    let p = unsafe { if zeroed { std::alloc::alloc_zeroed(buffer_layout(len)) } else { std::alloc::alloc(buffer_layout(len)) } };
    if !p.is_null() {
        f.extra.fetch_add(len, Relaxed);
    }
    p.cast()
}

unsafe extern "C" fn ab_allocate(f: &VmFlags, len: usize) -> *mut c_void {
    // SAFETY: see allocate_buffer.
    unsafe { allocate_buffer(f, len, true) }
}

unsafe extern "C" fn ab_allocate_uninitialized(f: &VmFlags, len: usize) -> *mut c_void {
    // SAFETY: see allocate_buffer.
    unsafe { allocate_buffer(f, len, false) }
}

unsafe extern "C" fn ab_free(f: &VmFlags, data: *mut c_void, len: usize) {
    f.extra.fetch_sub(len, Relaxed);
    let _ = f.next_check.update(Relaxed, Relaxed, |n| n.saturating_sub(len));
    // SAFETY: allocated by allocate_buffer with this length.
    unsafe { std::alloc::dealloc(data.cast(), buffer_layout(len)) };
}

unsafe extern "C" fn ab_drop(f: *const VmFlags) {
    // SAFETY: the count `array_buffer_allocator` gave the allocator.
    drop(unsafe { Arc::from_raw(f) });
}

static ALLOCATOR: v8::RustAllocatorVtable<VmFlags> = v8::RustAllocatorVtable {
    allocate: ab_allocate,
    allocate_uninitialized: ab_allocate_uninitialized,
    free: ab_free,
    drop: ab_drop,
};

/// A player isolate's array buffer allocator (ivm `LimitedAllocator`), counting into `flags`.
pub(crate) fn array_buffer_allocator(flags: &Arc<VmFlags>) -> v8::UniqueRef<v8::Allocator> {
    // SAFETY: the allocator owns one count of `flags`, released by `ab_drop`.
    unsafe { v8::new_rust_allocator(Arc::into_raw(flags.clone()), &ALLOCATOR) }
}

unsafe extern "C" fn delete_counted(data: *mut c_void, len: usize, flags: *mut c_void) {
    // SAFETY: the count `uint8_array` took; the bytes are its boxed slice.
    let f = unsafe { Arc::from_raw(flags as *const VmFlags) };
    f.extra.fetch_sub(len, Relaxed);
    drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(data as *mut u8, len)) });
}

/// A `Uint8Array` over `bytes` (no copy) for a buffer the host makes (base64ToBuffer, binary modules),
/// counted for the running VM as the allocator counts vanilla's `Buffer.from` (ENV-BINARY-BUFFER).
pub(crate) fn uint8_array<'s>(scope: &mut v8::PinScope<'s, '_>, bytes: Vec<u8>) -> Option<v8::Local<'s, v8::Uint8Array>> {
    let len = bytes.len();
    let flags = with_run(|c| c.flags).filter(|f| !f.is_null());
    let store = match flags {
        Some(f) => {
            // SAFETY: `f` is the running VM's Arc (RunCtx); the deleter releases the count taken here.
            let f = unsafe {
                Arc::increment_strong_count(f);
                Arc::from_raw(f)
            };
            f.extra.fetch_add(len, Relaxed);
            let data = Box::into_raw(bytes.into_boxed_slice()) as *mut u8 as *mut c_void;
            // SAFETY: `data` lives until `delete_counted`.
            unsafe { v8::ArrayBuffer::new_backing_store_from_ptr(data, len, delete_counted, Arc::into_raw(f) as *mut c_void) }
        }
        None => v8::ArrayBuffer::new_backing_store_from_vec(bytes),
    }
    .make_shared();
    let buf = v8::ArrayBuffer::with_backing_store(scope, &store);
    v8::Uint8Array::new(scope, buf, 0, len)
}

/// Everything natives need about the current run.
pub(crate) struct RunCtx {
    world: *const (dyn WorldAccess + 'static),
    pub user: UserIdx,
    /// Sorted (INT-STORE routing).
    pub visible: Vec<RoomIdx>,
    pub vnow: i64,
    pub nominal_tick_ms: u32,
    pub started: Instant,
    pub meter: RunMeter,
    pub clock: Clock,
    pub halted: bool,
    /// `heap_size_limit` at creation (ivm `initial_heap_size_limit`, the `adjustment` base).
    pub heap_initial: usize,
    /// ENV-CODEGEN: longest eval/Function source allowed (isolated-vm: memory limit / 8).
    pub codegen_limit: usize,
    pub flags: *const VmFlags,
    /// Promises rejected without a handler (ENV-MICROTASKS), with their reasons.
    pub rejections: Vec<(v8::Global<v8::Promise>, v8::Global<v8::Value>)>,
    /// The run's room blobs, as `__tick`'s `blobs` numbers them (the `blob` native).
    pub blobs: Vec<Option<engine::world::RoomBlob>>,
}

impl RunCtx {
    pub fn new(world: &dyn WorldAccess, user: UserIdx, visible: Vec<RoomIdx>, vnow: i64, nominal_tick_ms: u32, meter: RunMeter, clock: Clock) -> RunCtx {
        // SAFETY: only the lifetime is erased; the context is uninstalled (RunGuard) before the run that
        // borrowed `world` returns.
        let world = unsafe { std::mem::transmute::<*const (dyn WorldAccess + '_), *const (dyn WorldAccess + 'static)>(world) };
        RunCtx {
            world,
            user,
            visible,
            vnow,
            nominal_tick_ms,
            started: Instant::now(),
            meter,
            clock,
            halted: false,
            heap_initial: 0,
            codegen_limit: usize::MAX,
            flags: std::ptr::null(),
            rejections: Vec::new(),
            blobs: Vec::new(),
        }
    }

    pub fn world(&self) -> &dyn WorldAccess {
        // SAFETY: see `new`.
        unsafe { &*self.world }
    }

    /// The world for the length of one native call (the path finder's callbacks need a `'static` borrow).
    pub fn world_static(&self) -> &'static dyn WorldAccess {
        // SAFETY: see `new`; the caller does not keep it past the native call.
        unsafe { &*self.world }
    }

    /// Halt or heap exhaustion: the run ends in a reset, so no boundary may cancel termination.
    pub fn terminating(&self) -> bool {
        // SAFETY: `flags` is the running VM's Arc, alive for the whole run.
        self.halted || (!self.flags.is_null() && unsafe { &*self.flags }.heap_hit.load(Relaxed))
    }
}

thread_local! {
    static RUN: Cell<*mut RunCtx> = const { Cell::new(std::ptr::null_mut()) };
}

/// Installs a run context on this thread for the guard's lifetime.
pub(crate) struct RunGuard {
    ctx: *mut RunCtx,
    prev: *mut RunCtx,
}

impl RunGuard {
    pub fn enter(ctx: RunCtx) -> RunGuard {
        let ctx = Box::into_raw(Box::new(ctx));
        let prev = RUN.with(|r| r.replace(ctx));
        RunGuard { ctx, prev }
    }

    pub fn finish(self) -> RunCtx {
        let me = std::mem::ManuallyDrop::new(self);
        RUN.with(|r| r.set(me.prev));
        // SAFETY: created by Box::into_raw in `enter`; no longer reachable through RUN.
        *unsafe { Box::from_raw(me.ctx) }
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        RUN.with(|r| r.set(self.prev));
        // SAFETY: as in `finish`.
        drop(unsafe { Box::from_raw(self.ctx) });
    }
}

/// Runs `f` on the current run context. `f` must not run JS (natives would re-enter the context).
pub(crate) fn with_run<R>(f: impl FnOnce(&mut RunCtx) -> R) -> Option<R> {
    let p = RUN.with(|r| r.get());
    // SAFETY: non-null only while a RunGuard owns the box; access is single-threaded and not re-entrant
    // because `f` runs no JS.
    (!p.is_null()).then(|| f(unsafe { &mut *p }))
}

pub(crate) fn set_fn(scope: &mut v8::PinScope, o: v8::Local<v8::Object>, name: &str, cb: impl v8::MapFnTo<v8::FunctionCallback>) {
    let f = v8::Function::builder(cb).build(scope).unwrap();
    let key = v8::String::new(scope, name).unwrap();
    f.set_name(key);
    o.set(scope, key.into(), f.into());
}

/// The object handed to `_init(natives)` in a player VM.
pub(crate) fn player_natives<'s>(scope: &mut v8::PinScope<'s, '_>, shard: &str, profile: &str, setup_list: bool) -> v8::Local<'s, v8::Object> {
    let o = v8::Object::new(scope);
    set_fn(scope, o, "cpuTime", cpu_time);
    set_fn(scope, o, "compileAndRun", compile_and_run);
    set_fn(scope, o, "halt", halt);
    set_fn(scope, o, "heapStats", heap_stats);
    set_fn(scope, o, "now", now);
    set_fn(scope, o, "base64ToBuffer", base64_to_buffer);
    set_fn(scope, o, "mark", mark);
    set_fn(scope, o, "docsOff", docs_off);
    set_fn(scope, o, "blob", blob);
    crate::natives_world::install(scope, o);
    crate::natives_market::install(scope, o);
    for (k, v) in [("shardName", shard), ("profile", profile)] {
        let s = v8::String::new(scope, v).unwrap();
        set(scope, o, k, s.into());
    }
    let setup_list = v8::Boolean::new(scope, setup_list);
    set(scope, o, "setupList", setup_list.into());
    o
}

/// The system isolate's natives (`_init` argument, and `objectRoom` as a global).
pub(crate) fn system_natives<'s>(scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Object> {
    let o = v8::Object::new(scope);
    set_fn(scope, o, "objectRoom", object_room);
    o
}

/// `cpuTime()` → ms of the configured meter (vanilla `Number(isolate.cpuTime) / 1e6`).
fn cpu_time(scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    if let Some(ms) = with_run(|c| c.meter.now_ms()) {
        rv.set(v8::Number::new(scope, ms).into());
    }
}

/// `blob(i)`: the whole text of the run's room blob `i` (§6.6), for a room the VM parses whole: one it did not hold in
/// its last run, or any room once it stopped taking docs over. Put together on the first call for that room and shared
/// by every VM after it; undefined when `i` names no room.
fn blob(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let i = args.get(0).uint32_value(scope).unwrap_or(u32::MAX) as usize;
    let text = with_run(|c| c.blobs.get(i).and_then(Option::as_ref).map(|b| b.text().clone())).flatten();
    if let Some(text) = text {
        debug_assert!(text.is_ascii(), "room blobs are ASCII (§3.4)");
        rv.set(crate::marshal::external_latin1(scope, &text).into());
    }
}

/// `mark(i)`: bench timing of the JS tick's sub-stages (`timing::mark`).
fn mark(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue<v8::Value>) {
    let sub = args.get(0).int32_value(scope).unwrap_or(-1);
    let allocated = crate::timing::BENCH.load(std::sync::atomic::Ordering::Relaxed).then(|| scope.get_heap_statistics().total_allocated_bytes());
    crate::timing::mark(sub, allocated);
}

/// `compileAndRun(code, filename, timeoutMs, kind)`: `isolate.compileScriptSync(code, {filename})
/// .runSync(context, {timeout})` as runtime-driver.js does it, with isolated-vm's timeout behaviour
/// (see watchdog.rs). Compile errors and script exceptions propagate unchanged; `'console'` and
/// `'sandbox'` runs get no deadline (only the hard timeout). The timeout is read as runSync's
/// `ReadOption<int32_t>(options, "timeout", 0)` into a `uint32_t` (script_handle.cc:57,90): undefined
/// or null is none, a non-Int32 number throws `` TypeError: `timeout` must be a 32-bit number `` after
/// the compile, 0 is none and a negative value wraps.
fn compile_and_run(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(code) = args.get(0).to_string(scope) else { return };
    let filename = args.get(1);
    let kind = args.get(3).to_rust_string_lossy(scope);
    let origin = v8::ScriptOrigin::new(scope, filename, 0, 0, false, -1, None, false, false, false, None);
    let Some(script) = v8::Script::compile(scope, code, Some(&origin)) else { return };
    let timeout = args.get(2);
    let timeout_ms = if timeout.is_null_or_undefined() {
        0
    } else if timeout.is_int32() {
        timeout.int32_value(scope).unwrap_or(0) as u32
    } else {
        let msg = v8::String::new(scope, "`timeout` must be a 32-bit number").unwrap();
        let e = v8::Exception::type_error(scope, msg);
        scope.throw_exception(e);
        return;
    };
    let timed = timeout_ms != 0 && kind != "console" && kind != "sandbox";
    let clock = with_run(|c| c.clock).unwrap_or(Clock::ThreadCpu);
    let slot = watchdog::slot();
    let id = timed.then(|| slot.push(clock, timeout_ms as f64));
    let result = script.run(scope);
    if let Some(id) = id {
        let others = with_run(|c| c.terminating()).unwrap_or(false);
        if let Boundary::TimedOut(stack) = slot.pop(id, others, || {
            scope.cancel_terminate_execution();
        }) {
            // ivm RuntimeErrorWithConstructor::ConstructError: Error(message), then `stack` set to
            // message + rendered frames with a plain [[Set]] (no "Error: " prefix).
            const MSG: &str = "Script execution timed out.";
            let msg = v8::String::new(scope, MSG).unwrap();
            let err = v8::Exception::error(scope, msg);
            if let Ok(o) = v8::Local::<v8::Object>::try_from(err) {
                let s = v8::String::new(scope, &format!("{MSG}{stack}")).unwrap();
                set(scope, o, "stack", s.into());
            }
            scope.throw_exception(err);
            return;
        }
    }
    if let Some(v) = result {
        rv.set(v);
    }
}

/// `docsOff()`: the VM takes no more room docs over from its previous run (doc_reuse.rs).
fn docs_off(_scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue<v8::Value>) {
    if let Some(f) = with_run(|c| c.flags).filter(|f| !f.is_null()) {
        // SAFETY: the running VM's flags (RunCtx).
        unsafe { &*f }.docs_off.store(true, Relaxed);
    }
}

/// `halt()`: vanilla disposes the isolate (`_halt`, user-vm.js:68-71); here the run is terminated and
/// the host resets the VM with "CPU halted".
fn halt(scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, _rv: v8::ReturnValue<v8::Value>) {
    with_run(|c| c.halted = true);
    scope.terminate_execution();
}

/// `heapStats()` → isolated-vm `getHeapStatisticsSync()` (isolate_handle.cc:346-377): ten fields in this
/// order, `total_available_size`/`heap_size_limit` minus the growth since creation.
/// `externally_allocated_size` is ivm's extra allocated memory: the VM's array buffers (VmFlags), not
/// V8's external memory, which also holds the host's external strings (room blobs, event logs,
/// Memory, code) that vanilla copies into the heap instead.
fn heap_stats(scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let h = scope.get_heap_statistics();
    let initial = with_run(|c| c.heap_initial).filter(|&i| i > 0).unwrap_or(h.heap_size_limit());
    let adjustment = h.heap_size_limit() as f64 - initial as f64;
    // SAFETY: `flags` is the running VM's Arc (see `RunCtx::terminating`).
    let extra = with_run(|c| (!c.flags.is_null()).then(|| unsafe { &*c.flags }.extra.load(Relaxed))).flatten().unwrap_or(h.external_memory());
    let o = v8::Object::new(scope);
    let fields = [
        ("total_heap_size", h.total_heap_size() as f64),
        ("total_heap_size_executable", h.total_heap_size_executable() as f64),
        ("total_physical_size", h.total_physical_size() as f64),
        ("total_available_size", h.total_available_size() as f64 - adjustment),
        ("used_heap_size", h.used_heap_size() as f64),
        ("heap_size_limit", h.heap_size_limit() as f64 - adjustment),
        ("malloced_memory", h.malloced_memory() as f64),
        ("peak_malloced_memory", h.peak_malloced_memory() as f64),
        ("does_zap_garbage", h.does_zap_garbage() as u8 as f64),
        ("externally_allocated_size", extra as f64),
    ];
    for (k, v) in fields {
        let n = v8::Number::new(scope, v);
        set(scope, o, k, n.into());
    }
    rv.set(o.into());
}

/// `now()` → `vclock(T) + min(realMsSinceRunStart, nominalTickMs − 1)` (CLK-SANDBOX-DATE).
fn now(scope: &mut v8::PinScope, _args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(t) = with_run(|c| {
        let real = c.started.elapsed().as_millis() as i64;
        c.vnow + real.min(c.nominal_tick_ms.saturating_sub(1) as i64)
    }) else {
        return;
    };
    rv.set(v8::Number::new(scope, t as f64).into());
}

/// `base64ToBuffer(b64, proto?)` → `Uint8Array` of `Buffer.from(b64, 'base64')` (buffer@4.9.1 +
/// base64-js, ENV-BINARY-BUFFER); `proto`, when an object, becomes its prototype (the bundle's Buffer).
fn base64_to_buffer(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let Some(s) = args.get(0).to_string(scope) else { return };
    let bytes = buffer_from_base64(&s.to_rust_string_lossy(scope));
    let Some(a) = uint8_array(scope, bytes) else { return };
    if args.get(1).is_object() {
        a.set_prototype(scope, args.get(1));
    }
    rv.set(a.into());
}

/// buffer@4.9.1 `base64clean` + the bundled base64-js `toByteArray` (the `getLens` version shipped in
/// @screeps/driver build/runtime.bundle.js): drop everything outside `[+/0-9A-Za-z-_]` (padding
/// included), give up below 2 characters, re-pad to a multiple of 4 and decode, URL-safe `-`/`_`
/// accepted. `placeHoldersLen = 4 - validLen % 4`, so a lone trailing character (3 placeholders)
/// yields no byte.
pub(crate) fn buffer_from_base64(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' | b'-' => 62,
        _ => 63, // '/' | '_'
    };
    let clean: Vec<u8> = s.trim().bytes().filter(|c| c.is_ascii_alphanumeric() || b"+/-_".contains(c)).collect();
    if clean.len() < 2 {
        return Vec::new();
    }
    let padded = clean.len().div_ceil(4) * 4;
    let placeholders = padded - clean.len();
    let mut out = Vec::with_capacity(padded / 4 * 3);
    for q in clean.chunks(4) {
        let n = q.iter().enumerate().fold(0u32, |n, (i, &c)| n | (val(c) as u32) << (18 - 6 * i));
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    }
    out.truncate(padded / 4 * 3 - placeholders);
    out
}

/// System isolate `objectRoom(id)` → the room name for INT-STORE routing, or undefined (§4.3).
fn object_room(scope: &mut v8::PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue<v8::Value>) {
    let id = args.get(0).to_rust_string_lossy(scope);
    let Some(name) = with_run(|c| {
        let w = c.world();
        w.object_room(&id, c.user, &c.visible).map(|r| w.room_name(r).to_string())
    })
    .flatten() else {
        return;
    };
    rv.set(v8::String::new(scope, &name).unwrap().into());
}

/// Promise reject tracking for ENV-MICROTASKS: rejections without a handler are kept until a handler
/// is attached; whatever is left after the end-of-run checkpoint fails the run.
pub(crate) extern "C" fn on_promise_reject(msg: v8::PromiseRejectMessage) {
    let promise = msg.get_promise();
    v8::callback_scope!(unsafe scope, promise);
    v8::scope!(let scope, scope);
    match msg.get_event() {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
            let value = msg.get_value().unwrap_or_else(|| v8::undefined(scope).into());
            let entry = (v8::Global::new(scope, promise), v8::Global::new(scope, value));
            with_run(|c| c.rejections.push(entry));
        }
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
            let p = v8::Global::new(scope, promise);
            with_run(|c| c.rejections.retain(|(q, _)| *q != p));
        }
        _ => {}
    }
}

/// make.js `error.stack || error` / runtime.js `_.isObject(e) && e.stack || e.toString()` for a thrown
/// value; getters that throw fall back to the plain conversion.
pub(crate) fn error_text(scope: &mut v8::PinScope, v: v8::Local<v8::Value>) -> String {
    v8::tc_scope!(let tc, scope);
    if let Ok(o) = v8::Local::<v8::Object>::try_from(v) {
        let key = v8::String::new(tc, "stack").unwrap();
        if let Some(stack) = o.get(tc, key.into()) {
            if stack.boolean_value(tc) {
                if let Some(s) = stack.to_string(tc) {
                    return s.to_rust_string_lossy(tc);
                }
            }
        }
    }
    v.to_string(tc).map(|s| s.to_rust_string_lossy(tc)).unwrap_or_default()
}

/// Reads a V8 string as `JsStr` without loss (Latin-1 when every unit fits a byte, else UTF-16).
pub(crate) fn js_str(scope: &mut v8::PinScope, s: v8::Local<v8::String>) -> util::jsstr::JsStr {
    let n = s.length();
    if s.contains_only_onebyte() {
        let mut b = vec![0u8; n];
        s.write_one_byte_v2(scope, 0, &mut b, v8::WriteFlags::empty());
        util::jsstr::JsStr::Latin1(b.into())
    } else {
        let mut b = vec![0u16; n];
        s.write_v2(scope, 0, &mut b, v8::WriteFlags::empty());
        util::jsstr::JsStr::Utf16(b.into())
    }
}

/// ENV-CODEGEN (isolated-vm `CodeGenCallback`): with string codegen disabled on the context, `eval` and
/// `Function` ask here; sources longer than memory limit / 8 are refused (V8's EvalError), the rest pass.
pub(crate) fn on_codegen<'s>(_scope: &mut v8::PinScope<'s, '_>, source: v8::Local<'s, v8::Value>, _code_like: bool) -> v8::ModifyCodeGenerationFromStringsResult<'s> {
    let limit = with_run(|c| c.codegen_limit).unwrap_or(usize::MAX);
    let too_long = v8::Local::<v8::String>::try_from(source).is_ok_and(|s| s.length() > limit);
    v8::ModifyCodeGenerationFromStringsResult { codegen_allowed: !too_long, modified_source: None }
}

/// Near-heap-limit callback (ivm NearHeapLimitCallback, environment.cc:221-235). V8 also calls it in
/// the last-resort GC after a refused array buffer, which vanilla answers with a catchable RangeError,
/// so it ends the VM (section 6.11) only past ivm's limit rule; either way V8 gets 32 MB more (ivm:
/// 1 GB), which the next mark-sweep takes back while under the limit.
pub(crate) unsafe extern "C" fn on_near_heap_limit(data: *mut c_void, current: usize, _initial: usize) -> usize {
    // SAFETY: `data` is the VM's flags, alive as long as the isolate (its allocator holds a count).
    let flags = unsafe { &*(data as *const VmFlags) };
    flags.adjusted.store(true, Relaxed);
    if flags.over_limit() {
        flags.exhausted();
    }
    current + 32 * 1024 * 1024
}

/// ivm MarkSweepCompactEpilogue (environment.cc:186-219) without the moderate-pressure hints: over the
/// limit after a GC that collected everything ends the VM; over it after an ordinary one asks for that
/// GC (ivm: critical memory pressure); under it, a raised heap limit goes back down.
pub(crate) unsafe extern "C" fn on_mark_sweep_compact(mut iso: v8::UnsafeRawIsolatePtr, _t: v8::GCType, gc: v8::GCCallbackFlags, data: *mut c_void) {
    // SAFETY: as in on_near_heap_limit; V8 calls GC callbacks on the isolate's thread.
    let flags = unsafe { &*(data as *const VmFlags) };
    let all = v8::GCCallbackFlags::kGCCallbackFlagCollectAllAvailableGarbage.0;
    if flags.over_limit() {
        if gc.0 & (all | v8::GCCallbackFlags::kGCCallbackFlagForced.0) != 0 {
            flags.exhausted();
        } else if let Some(h) = flags.handle.get() {
            h.request_interrupt(collect_all_garbage, data);
        }
    } else if gc.0 & all == 0 && flags.adjusted.load(Relaxed) {
        // SAFETY: the isolate this callback runs for.
        let iso = unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut iso) };
        iso.remove_near_heap_limit_callback(on_near_heap_limit, flags.limit);
        iso.add_near_heap_limit_callback(on_near_heap_limit, data);
        if iso.get_heap_statistics().heap_size_limit() == flags.limit + flags.misc.load(Relaxed) {
            flags.adjusted.store(false, Relaxed);
        }
    }
}

/// ivm `MemoryPressureNotification(kCritical)`: a full GC, whose epilogue decides.
unsafe extern "C" fn collect_all_garbage(mut iso: v8::UnsafeRawIsolatePtr, _data: *mut c_void) {
    // SAFETY: interrupts run on the isolate's thread.
    unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut iso) }.memory_pressure_notification(v8::MemoryPressureLevel::Critical);
}

#[cfg(test)]
mod tests {
    use super::buffer_from_base64 as b;

    /// Oracle vectors: `toByteArray(base64clean(s))` from the official runtime.bundle.js (node).
    #[test]
    fn base64_like_buffer_4_9_1() {
        assert_eq!(b("aGVsbG8="), b"hello");
        assert_eq!(b("aGVsbG8"), b"hello"); // missing padding
        assert_eq!(b(" aGV\nsbG8= "), b"hello"); // junk dropped
        assert_eq!(b("_-8="), [0xff, 0xef]); // URL-safe
        assert_eq!(b("a"), b""); // under 2 characters
        assert_eq!(b("Y"), b"");
        assert_eq!(b("YQ"), b"a");
        assert_eq!(b("YQ=="), b"a");
        assert_eq!(b("YWJj"), b"abc");
        assert_eq!(b("YWJjZA"), b"abcd");
        assert_eq!(b("YQ==YQ=="), [97, 6, 16]); // inner padding is junk too
        // A lone trailing character is dropped (base64-js getLens: 3 placeholders, no byte for them).
        assert_eq!(b("YWJjZ"), b"abc");
        assert_eq!(b("YWJjZ="), b"abc");
        assert_eq!(b("Zm9vYmFyZ"), b"foobar");
    }
}
