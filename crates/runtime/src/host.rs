//! mirror: reference/driver/lib/runtime/user-vm.js
//! parity: DIVERGENT(VM-CODE-VERSION-RESET, VM-MULTI-RUNNER, VM-DISPOSE-ERRORS)
//!
//! Divergences: VM-CODE-VERSION-RESET, any change of the code version (a monotonic counter) recreates the
//! VM, as 02 §2.4 recommends, not only a strictly newer timestamp; VM-MULTI-RUNNER, one heap per user
//! follows it across worker threads instead of one VM per runner process; VM-DISPOSE-ERRORS, heap exhaustion
//! resets in the same tick with the §6.11 text instead of a disposed isolate failing the next create.
//!
//! Isolate lifecycle: creation from the startup snapshot, `_init(natives)`, global resets, system isolates
//! per worker, snapshot build (ARCHITECTURE.md §6.3, §6.5). Owner: WP08.
//!
//! Bundle contract (§6.5, §7.1; the header of `js/host/prelude.js`). The player script `<runtime>` is the
//! prelude's source followed by `__chitinDefine("<path>", function(module, exports, require) {\n<source>\n});`
//! for every FILES entry except `host/*` and `cli/*`, in FILES order, then the generated `common/constants.js`
//! (`module.exports = <engine::constants::constants_json()>`) and the JS-mod game slots as `mods/<name>.js`.
//! Top-level evaluation only registers modules (the prelude's module table); only that is snapshotted. Per VM
//! the host calls `global._init(natives)`; it returns the `__tick` function (or defines `global.__tick`), and
//! the host deletes both globals before player code runs. The system script is the same format over lodash,
//! `common/system.js`, `engine/utils.js`, `host/system.js` and the constants; the host defines the global
//! `objectRoom(id)`, calls `_initSystem(natives)` and then `global.__store(outMessage, userId)`.

use crate::cpu::RunMeter;
use crate::doc_reuse::DocCache;
use crate::marshal::{self, failed};
use crate::natives::{self, with_run, RunCtx, RunGuard, VmFlags};
use crate::natives_world::WorldAccess;
use crate::watchdog;
use util::json::Json;
use crate::{Bundle, Runtime};
use engine::runner::{ResetReason, RunInput, RunOutput, RunStatus};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::Instant;

pub(crate) const HALTED: &str = "CPU halted";
pub(crate) const HEAP: &str = "Isolate has exhausted v8 heap space";
pub(crate) const HARD_TIMEOUT: &str = "Script execution timed out ungracefully, restarting virtual machine";
pub(crate) const NO_MEMORY: &str = "Script execution has been terminated: unable to allocate memory, restarting virtual machine";

// ---- snapshots ----

/// The bundle text: `files["host/prelude.js"]`, then one `__chitinDefine` per kept file (FILES order), the
/// constants and `mods` (JS-mod game slots, `mods/<name>.js`), exactly as tools/oracle/api/bundle.js builds it.
fn bundle_source(files: &[(&str, &str)], keep: impl Fn(&str) -> bool, mods: &[(&str, &str)]) -> String {
    let mut s = String::from(files.iter().find(|(n, _)| *n == "host/prelude.js").map_or("", |(_, src)| *src));
    s.push('\n');
    let mut add = |name: &str, src: &str| {
        s.push_str("__chitinDefine(");
        util::json::write_quoted(&mut s, name);
        s.push_str(", function(module, exports, require) {\n");
        s.push_str(src);
        s.push_str("\n});\n");
    };
    for (name, src) in files.iter().filter(|(n, _)| keep(n)) {
        if *name == "vendor/lodash.js" {
            add(name, &blank_doc_comments(src));
        } else {
            add(name, src);
        }
    }
    add("common/constants.js", &format!("module.exports = {};", engine::constants::constants_json()));
    for (name, src) in mods {
        add(&format!("mods/{name}.js"), src);
    }
    s
}

/// lodash.js with every JSDoc block (a `/**` line, lines starting with `*`, a `*/` line) blanked, line breaks kept:
/// a third of the file, and every VM's heap holds the bundle source. `@license` blocks stay. Only the text of
/// `_.runInContext.toString()` (which encloses them) changes; line and column numbers do not.
fn blank_doc_comments(src: &str) -> String {
    let lines: Vec<&str> = src.split('\n').collect();
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "/**" {
            let end = lines[i + 1..].iter().position(|l| !l.trim_start().starts_with('*') || l.trim() == "*/");
            if let Some(n) = end.filter(|&n| lines[i + 1 + n].trim() == "*/") {
                let block = &lines[i..=i + 1 + n];
                if !block.iter().any(|l| l.contains("@license")) {
                    out.extend(std::iter::repeat("").take(block.len()));
                    i += block.len();
                    continue;
                }
            }
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}

fn is_player_file(name: &str) -> bool {
    !name.starts_with("cli/") && !name.starts_with("host/")
}

fn is_system_file(name: &str) -> bool {
    matches!(name, "vendor/lodash.js" | "common/system.js" | "engine/utils.js" | "host/system.js")
}

/// The startup blob: the player script in the default context and the system script in context
/// `SYSTEM_CONTEXT`. One blob for every isolate of the process: V8 shares the read-only heap of the first
/// isolate's snapshot process-wide (and promotes snapshot objects into it), so isolates from two different
/// custom blobs must never be alive at once (V8's built-in snapshot stays compatible).
pub(crate) struct Snapshots {
    /// Leaked: every isolate creation borrows it without a copy.
    pub blob: &'static [u8],
}

const SYSTEM_CONTEXT: usize = 0;

pub(crate) fn snapshots(bundle: &Bundle) -> Snapshots {
    let mods: Vec<(&str, &str)> = bundle.mod_game_js.iter().map(|(n, s)| (&**n, &**s)).collect();
    let player = bundle_source(bundle.files, is_player_file, &mods);
    crate::jsprof::set_bundle(&player);
    let system = bundle_source(bundle.files, is_system_file, &[]);
    Snapshots { blob: cached(&player, &system) }
}

/// One blob per distinct bundle per process, built here and never read from disk: a snapshot is code, V8 only checks
/// its version, and the data folder can come from anyone. The lock also keeps SnapshotCreators from running
/// concurrently, which V8 does not survive.
fn cached(player: &str, system: &str) -> &'static [u8] {
    static BLOBS: Mutex<Vec<([u8; 20], &'static [u8])>> = Mutex::new(Vec::new());
    let mut key = Vec::with_capacity(player.len() + system.len() + 64);
    for part in [v8::V8::get_version(), player, system] {
        key.extend_from_slice(part.as_bytes());
        key.push(0);
    }
    let hash = util::sha1::sha1(&key);
    let mut blobs = BLOBS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, b)) = blobs.iter().find(|(h, _)| *h == hash) {
        return b;
    }
    let blob = build_snapshot(player, system).unwrap_or_else(|e| panic!("runtime snapshot: {e}"));
    let blob: &'static [u8] = Box::leak(blob.into_boxed_slice());
    blobs.push((hash, blob));
    blob
}

/// Evaluates `script` as the script `<runtime>` in the scope's context.
fn evaluate(scope: &mut v8::PinScope, script: &str) -> Result<(), String> {
    v8::tc_scope!(let tc, scope);
    let code = v8::String::new(tc, script).ok_or("<runtime>: source too large")?;
    let origin_name = v8::String::new(tc, "<runtime>").unwrap();
    let origin = v8::ScriptOrigin::new(tc, origin_name.into(), 0, 0, false, -1, None, false, false, false, None);
    if v8::Script::compile(tc, code, Some(&origin)).and_then(|s| s.run(tc)).is_none() {
        let e = tc.exception().map(|e| natives::error_text(tc, e)).unwrap_or_default();
        return Err(format!("<runtime>: {e}"));
    }
    Ok(())
}

/// Player bundle (incl. the mod game slots) in the default context, system bundle in context `SYSTEM_CONTEXT`.
fn build_snapshot(player: &str, system: &str) -> Result<Vec<u8>, String> {
    let mut creator = v8::Isolate::snapshot_creator(None, None);
    let result = {
        v8::scope!(let scope, &mut creator);
        let player_context = v8::Context::new(scope, Default::default());
        let r = evaluate(&mut v8::ContextScope::new(scope, player_context), player);
        let system_context = v8::Context::new(scope, Default::default());
        let r = r.and(evaluate(&mut v8::ContextScope::new(scope, system_context), system));
        // create_blob needs the default context even when a bundle failed (the creator must not be dropped).
        scope.set_default_context(player_context);
        let index = scope.add_context(system_context);
        debug_assert_eq!(index, SYSTEM_CONTEXT);
        r
    };
    let blob = creator.create_blob(v8::FunctionCodeHandling::Keep).ok_or("create_blob failed")?;
    result.map(|_| blob.to_vec())
}

unsafe extern "C" {
    fn v8__Locker__CONSTRUCT(buf: *mut [usize; 2], isolate: *mut std::ffi::c_void);
    fn v8__Unlocker__CONSTRUCT(buf: *mut [usize; 1], isolate: *mut std::ffi::c_void);
}

/// `iso.lock()`, keeping the V8 interrupts requested while the isolate was unlocked.
///
/// A top-level `v8::Locker` frees the thread's isolate state when dropped, and the next one starts it
/// afresh (`ThreadManager::RestoreThread` → `InitThread`), clearing the interrupt flags: background
/// compile jobs that finished in between never get their code installed, and the finished Maglev jobs
/// stay queued with their zones, ~10 MB per VM. A thread that once took the lock under an `Unlocker`
/// is archived lazily instead: its next Locker restores the state as it was, flags included, and its
/// drop archives it again. So a Locker and an Unlocker go first, never destroyed (their destructors
/// would undo that); on a thread already archived they cost one more uncontended lock and unlock.
fn lock(iso: &v8::SharedIsolate, raw: *mut std::ffi::c_void) -> v8::Locker<'_> {
    // SAFETY: `raw` is `iso`'s isolate, alive, and not locked by this thread. Both objects are plain
    // flags and the isolate pointer, unreferenced once constructed.
    unsafe {
        let (mut locker, mut unlocker) = ([0usize; 2], [0usize; 1]);
        v8__Locker__CONSTRUCT(&mut locker, raw);
        v8__Unlocker__CONSTRUCT(&mut unlocker, raw);
    }
    iso.lock()
}

/// An isolate's address for `lock`, kept as an integer so its owner stays Send.
fn raw_ptr(iso: &mut v8::Isolate) -> usize {
    // SAFETY: UnsafeRawIsolatePtr is a repr(transparent) raw pointer.
    unsafe { std::mem::transmute::<v8::UnsafeRawIsolatePtr, usize>(iso.as_raw_isolate_ptr()) }
}

// ---- player VMs ----

/// One player's isolate and what the host remembers about it.
pub(crate) struct Vm {
    /// `chitin bench` CHITIN_JS_PROFILE=cpu:…: the VM's inspector session (dropped under the isolate lock, see Drop).
    cpu_profile: Option<Box<crate::jsprof::CpuProfile>>,
    context: v8::Global<v8::Context>,
    tick: v8::Global<v8::Function>,
    iso: v8::SharedIsolate,
    /// `iso`'s address, for `lock`.
    raw: usize,
    flags: Arc<VmFlags>,
    /// `CodeBundle.version` the VM evaluated; any other version recreates it.
    code_version: Option<u64>,
    /// `memory_version` of the Memory string the JS side holds (None: unknown, send it again).
    memory_version: Option<u64>,
    /// The room docs the JS side holds (None once it stopped taking them over).
    docs: Option<DocCache>,
    cpu_ms: f64,
    heap_initial: usize,
    pub heap_used: usize,
    /// Physical size of the heap after the last run (used + garbage + free pages).
    pub heap_physical: usize,
    pub heap_limit: usize,
    pub last_used: Instant,
    thread: ThreadId,
}

fn get_fn<'s>(scope: &mut v8::PinScope<'s, '_>, o: v8::Local<v8::Object>, name: &str) -> Option<v8::Local<'s, v8::Function>> {
    let k = v8::String::new(scope, name).unwrap();
    o.get(scope, k.into()).and_then(|v| v8::Local::<v8::Function>::try_from(v).ok())
}

fn delete(scope: &mut v8::PinScope, o: v8::Local<v8::Object>, name: &str) {
    let k = v8::String::new(scope, name).unwrap();
    o.delete(scope, k.into());
}

/// VM-HEAP-LIMIT: `new ivm.Isolate({memoryLimit: 256 + staticTerrainDataSize/1024/1024})` reads the
/// limit into a `size_t` of MB (isolate_handle.cc:76-84, truncated).
fn memory_limit_mb(heap_mb: usize, terrain_mb: f64) -> usize {
    (heap_mb as f64 + terrain_mb) as usize
}

/// isolated-vm `IsolateCtor` resource constraints (environment.cc:359-370): old space = the limit,
/// young space = 2^(min(4, mb / 128) + 10) KB. They decide V8's `heap_size_limit` (heapStats).
///
/// That young space is 4 MB, semi-spaces of 2 MB at most, and a bot that allocates 6 MB a tick scavenges five times a tick:
/// what a tick builds outlives two scavenges, is promoted, and dies in old space, so V8 marks the whole heap several
/// times a second. `semi_mb` sizes the young generation to hold a tick. It changes `heap_size_limit`, so it is off
/// for the game server's parity and on in a match. On the pointer-compression build 16 MB pays: on a late-game bot, 12.90
/// against 15.25 ms a tick, 4 interleaved rounds on a busy machine, with far less spread, for about 40 MB per VM
/// (`chitin bench`). On the stock build it was 8 to 10% faster alone and 2% slower on a busy machine.
fn create_params(blob: &'static [u8], mb: usize, semi_mb: usize) -> v8::CreateParams {
    // Atomics.wait sleeps, which no CPU meter sees, so a bot could sit out every tick until the hard timeout.
    let params = v8::CreateParams::default().snapshot_blob(v8::StartupData::from(blob)).set_max_old_generation_size_in_bytes(mb << 20).allow_atomics_wait(false);
    if semi_mb == 0 {
        let young_kb = 2f64.powf((mb as f64 / 128.0).min(4.0) + 10.0) as usize;
        return params.set_max_young_generation_size_in_bytes(young_kb * 1024);
    }
    // two semi-spaces and a new large object space of the same size
    let young = (semi_mb * YOUNG_SEMI_SPACES) << 20;
    params.set_initial_young_generation_size_in_bytes(young).set_max_young_generation_size_in_bytes(young)
}

/// V8's young generation in semi-spaces: `Heap::YoungGenerationSizeFromSemiSpaceSize`.
const YOUNG_SEMI_SPACES: usize = 3;

/// Runs in a player context before anything else does. The memory limit lives in the array buffer allocator
/// (natives.rs), and V8 allocates a resizable buffer without it, so `new ArrayBuffer(n, {maxByteLength})` or `resize` would
/// let a bot commit memory until the host runs out, and shrinking one unmaps memory the path finder may be reading. So
/// `ArrayBuffer` here passes on the length alone, and a shared buffer has no use in a VM of one thread. `transfer` copies
/// through the allocator, but it is one more way to change a buffer under a search. V8 has all of these, so a bot gets less
/// here than on a modern Node: docs/spec/14 counts one mention of shared memory in 500 repos and no use of resizable buffers.
///
/// Strict, so `f.arguments` cannot read the real constructor out of the trap. V8 keeps it out of stack frames too (tests:
/// a_bot_cannot_get_the_real_array_buffer_back). Shared memory from WebAssembly still has a shared buffer, whose prototype
/// loses `constructor`, or that would be another way to the options.
///
/// The trap runs for the life of the VM, after the bot has changed whatever it likes, so nothing here may read through
/// Object.prototype, Array.prototype or Function.prototype. The handler has no prototype: `Object.prototype.get` would be a
/// trap, handed the real constructor. `args[0]` of an empty list would read `Array.prototype[0]`. An array literal defines
/// its element rather than setting it, and `construct` is held here and called directly, never looked up or `.call`ed.
const LOCK_BUFFERS: &str = r#"(function (G) {
    'use strict';
    var Shared = G.SharedArrayBuffer, proto = G.ArrayBuffer.prototype, construct = G.Reflect.construct;
    G.ArrayBuffer = proto.constructor = new G.Proxy(G.ArrayBuffer, {
        __proto__: null,
        construct: function (target, args, newTarget) {
            return construct(target, args.length > 0 ? [args[0]] : [], newTarget);
        }
    });
    delete proto.resize;
    delete proto.transfer;
    delete proto.transferToFixedLength;
    delete Shared.prototype.grow;
    delete Shared.prototype.constructor;
    delete G.SharedArrayBuffer;
    delete G.Atomics.waitAsync;
})(this)"#;

/// VM-BOOT: isolate from the snapshot, `_init(natives)`, bootstrap globals deleted. Vanilla copies the
/// whole terrain buffer into every VM (`_setStaticTerrainData`, user-vm.js:81-84) through the limited
/// allocator; with per-room copies (J1) its `staticTerrainDataSize` bytes are charged up front instead,
/// for `externally_allocated_size` and the array buffer limit alike.
fn create_vm(rt: &Runtime, terrain_mb: f64) -> Result<Vm, String> {
    let memory_mb = memory_limit_mb(rt.cfg.heap_mb, terrain_mb);
    // ivm `memory_limit` (bytes): the ENV-CODEGEN cap is a eighth of it.
    let heap_limit = memory_mb << 20;
    let flags = VmFlags::new(heap_limit);
    flags.extra.store((terrain_mb * 1048576.0) as usize, Relaxed);
    let params = create_params(rt.snapshots.blob, memory_mb, rt.cfg.semi_space_mb).array_buffer_allocator(natives::array_buffer_allocator(&flags));
    let mut iso = v8::Isolate::new(params);
    crate::jsprof::watch_gc(&mut iso);
    iso.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
    iso.set_promise_reject_callback(natives::on_promise_reject);
    iso.set_modify_code_generation_from_strings_callback(natives::on_codegen);
    let heap_initial = iso.get_heap_statistics().heap_size_limit();
    flags.created(&mut iso, heap_initial);
    let data = Arc::as_ptr(&flags) as *mut std::ffi::c_void;
    iso.add_near_heap_limit_callback(natives::on_near_heap_limit, data);
    iso.add_gc_epilogue_callback(natives::on_mark_sweep_compact, data, v8::GCType::kGCTypeMarkSweepCompact);
    with_run(|c| {
        c.heap_initial = heap_initial;
        c.codegen_limit = heap_limit / 8;
        c.flags = Arc::as_ptr(&flags);
    });
    let profile = match rt.cfg.profile {
        engine::policy::Profile::Private => "private",
        engine::policy::Profile::Mmo => "mmo",
    };
    let booted = (|| {
        v8::scope!(let scope, &mut iso);
        let context = v8::Context::new(scope, Default::default());
        context.set_allow_generation_from_strings(false);
        let scope = &mut v8::ContextScope::new(scope, context);
        evaluate(scope, LOCK_BUFFERS)?;
        v8::tc_scope!(let tc, scope);
        let global = context.global(tc);
        let init = get_fn(tc, global, "_init").ok_or("the runtime bundle defines no _init")?;
        let nat = natives::player_natives(tc, &rt.cfg.shard_name, profile, rt.cfg.setup_list);
        let ret = init.call(tc, global.into(), &[nat.into()]);
        let Some(ret) = ret else {
            return Err(tc.exception().map(|e| natives::error_text(tc, e)).unwrap_or_else(|| "_init terminated".into()));
        };
        let tick = v8::Local::<v8::Function>::try_from(ret).ok().or_else(|| get_fn(tc, global, "__tick")).ok_or("_init gave no __tick")?;
        delete(tc, global, "_init");
        delete(tc, global, "__tick");
        Ok((v8::Global::new(tc, context), v8::Global::new(tc, tick)))
    })();
    let (context, tick) = booted?;
    let raw = raw_ptr(&mut iso);
    // SAFETY: everything attached to the isolate is Send: the callbacks are plain fns and their data is
    // the VmFlags (atomics, an IsolateHandle), shared by the Vm and the isolate's allocator.
    let iso = unsafe { iso.try_into_shared() }.map_err(|e| {
        let msg = e.to_string();
        drop(e.into_isolate());
        msg
    })?;
    Ok(Vm {
        cpu_profile: None,
        context,
        tick,
        iso,
        raw,
        flags,
        code_version: None,
        memory_version: None,
        docs: Some(DocCache::default()),
        cpu_ms: 0.0,
        heap_initial,
        heap_used: 0,
        heap_physical: 0,
        heap_limit,
        last_used: Instant::now(),
        thread: std::thread::current().id(),
    })
}

/// The worker's system isolate: lodash + `common/system.js` + `engine/utils.js` + `host/system.js`.
pub(crate) struct SysVm {
    context: v8::Global<v8::Context>,
    store: v8::Global<v8::Function>,
    iso: v8::SharedIsolate,
    raw: usize,
}

fn create_sys(snapshot: &'static [u8]) -> Result<SysVm, String> {
    let mut iso = v8::Isolate::new(v8::CreateParams::default().snapshot_blob(v8::StartupData::from(snapshot)));
    let booted = (|| {
        v8::scope!(let scope, &mut iso);
        let context = v8::Context::from_snapshot(scope, SYSTEM_CONTEXT, Default::default()).ok_or("no system context in the snapshot")?;
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let tc, scope);
        let global = context.global(tc);
        let nat = natives::system_natives(tc);
        let object_room = get_fn(tc, nat, "objectRoom").unwrap();
        crate::natives_world::set(tc, global, "objectRoom", object_room.into());
        let init = get_fn(tc, global, "_initSystem").ok_or("the system bundle defines no _initSystem")?;
        if init.call(tc, global.into(), &[nat.into()]).is_none() {
            return Err(tc.exception().map(|e| natives::error_text(tc, e)).unwrap_or_default());
        }
        let store = get_fn(tc, global, "__store").ok_or("the system bundle defines no __store")?;
        Ok((v8::Global::new(tc, context), v8::Global::new(tc, store)))
    })();
    let (context, store) = booted?;
    let raw = raw_ptr(&mut iso);
    // SAFETY: no embedder state is attached to the system isolate besides plain-fn natives.
    let iso = unsafe { iso.try_into_shared() }.map_err(|e| {
        let msg = e.to_string();
        drop(e.into_isolate());
        msg
    })?;
    Ok(SysVm { context, store, iso, raw })
}

/// §4.2 step 4 on this thread's system isolate.
pub(crate) fn store(rt: &Runtime, bytes: &[u8], user_id: &str) -> Result<String, String> {
    let me = std::thread::current().id();
    let taken = rt.sys.lock().unwrap_or_else(|e| e.into_inner()).remove(&me);
    let sys = match taken {
        Some(s) => s,
        None => create_sys(rt.snapshots.blob)?,
    };
    let r = {
        let mut locker = lock(&sys.iso, sys.raw as _);
        v8::scope!(let scope, &mut *locker);
        let context = v8::Local::new(scope, &sys.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let f = v8::Local::new(scope, &sys.store);
        marshal::store(scope, f, bytes, user_id)
    };
    rt.sys.lock().unwrap_or_else(|e| e.into_inner()).insert(me, sys);
    r
}

/// How the player isolate part of a run ended.
enum Player {
    Out(marshal::Captured),
    Error(String),
}

/// One job on this thread: VM (created or reset as needed) → `__tick` → output captured → microtasks
/// drained → system isolate `__store` → RunOutput (§2.3, §4.2).
pub(crate) fn run(rt: &Runtime, world: &dyn WorldAccess, input: RunInput) -> RunOutput {
    let user = input.user;
    let mut vm = rt.vms.lock().unwrap_or_else(|e| e.into_inner()).remove(&user);
    if vm.as_ref().is_some_and(|v| v.code_version.is_some_and(|c| c != input.code.version)) {
        vm = None; // code changed: global reset
        rt.resets.fetch_add(1, Relaxed);
    }
    let meter = rt.cfg.meter;
    let ctx = RunCtx::new(
        world,
        user,
        input.visible.clone(),
        input.vnow,
        input.nominal_tick_ms,
        RunMeter::start(meter, vm.as_ref().map_or(0.0, |v| v.cpu_ms)),
        meter.deadline_clock(),
    );
    let guard = RunGuard::enter(ctx);
    let mut vm = match vm {
        Some(v) => {
            with_run(|c| {
                c.heap_initial = v.heap_initial;
                c.codegen_limit = v.heap_limit / 8;
                c.flags = &*v.flags;
            });
            v
        }
        None => {
            let t = Instant::now();
            let v = create_vm(rt, world.terrain_heap_mb());
            crate::timing::add(1, t);
            match v {
                Ok(v) => v,
                Err(e) => return failed(user, RunStatus::TotalError, e),
            }
        }
    };
    let mut t = Instant::now();
    let me = std::thread::current().id();
    if vm.thread != me {
        vm.thread = me;
        rt.migrations.fetch_add(1, Relaxed);
    }
    let (memory_version, code_version) = (input.memory_version, input.code.version);
    let pass_memory = vm.memory_version != Some(memory_version);
    let pass_code = vm.code_version != Some(code_version);
    let user_id = world.user_id(user).to_string();
    if vm.flags.docs_off.load(Relaxed) {
        vm.docs = None;
    }
    let plans = vm.docs.as_mut().map(|c| c.plan(&input.blobs, &input.blob_rooms));
    with_run(|c| c.blobs = input.blobs.clone());

    let slot = watchdog::slot();
    let (player, hard) = {
        let mut locker = lock(&vm.iso, vm.raw as _);
        slot.begin(vm.iso.thread_safe_handle(), vm.context.clone(), rt.cfg.hard_timeout_ms);
        crate::jsprof::begin(vm.iso.thread_safe_handle(), vm.context.clone());
        crate::jsprof::cpu_attach(&mut locker, &vm.context, &mut vm.cpu_profile);
        let player = (|| {
            v8::scope!(let scope, &mut *locker);
            let context = v8::Local::new(scope, &vm.context);
            let scope = &mut v8::ContextScope::new(scope, context);
            v8::tc_scope!(let tc, scope);
            let args = marshal::tick_args(tc, input, plans, pass_memory, pass_code);
            let tick = v8::Local::new(tc, &vm.tick);
            let recv = v8::undefined(tc).into();
            let out = tick.call(tc, recv, &args);
            t = crate::timing::add(2, t);
            let Some(out) = out else {
                if tc.has_terminated() {
                    return Player::Error("Script execution has been terminated".into());
                }
                return Player::Error(tc.exception().map(|e| natives::error_text(tc, e)).unwrap_or_default());
            };
            let cap = match marshal::capture(tc, out) {
                Ok(c) => c,
                Err(e) => return Player::Error(e),
            };
            // ENV-MICROTASKS: drained after output capture; a rejection still unhandled fails the run.
            tc.perform_microtask_checkpoint();
            let rejected = with_run(|c| c.rejections.first().map(|(_, v)| v.clone())).flatten();
            if let Some(reason) = rejected {
                let reason = v8::Local::new(tc, reason);
                return Player::Error(natives::error_text(tc, reason));
            }
            Player::Out(cap)
        })();
        t = crate::timing::add(3, t);
        let hard = slot.end();
        crate::jsprof::end();
        with_run(|c| c.rejections.clear());
        // Whatever terminated this run must not leak into the next one.
        locker.cancel_terminate_execution();
        let h = locker.get_heap_statistics();
        vm.heap_used = h.used_heap_size();
        vm.heap_physical = h.total_physical_size();
        crate::jsprof::spaces(&mut locker, user);
        (player, hard)
    };
    let ctx = guard.finish();
    vm.cpu_ms = ctx.meter.total_ms();
    vm.last_used = Instant::now();
    if pass_code {
        vm.code_version = Some(code_version);
    }

    let reset = if ctx.halted {
        Some((ResetReason::Halt, HALTED.to_string()))
    } else if vm.flags.heap_hit.load(Relaxed) {
        Some((ResetReason::Heap, HEAP.to_string()))
    } else if hard {
        Some((ResetReason::HardTimeout, HARD_TIMEOUT.to_string()))
    } else if let Player::Error(e) = &player {
        // make.js:285-292 on the failed run's text (`""+error`, the first line of our text): these
        // clear the VM too, an allocation failure with its own message.
        if e.contains("Array buffer allocation failed") {
            Some((ResetReason::Heap, NO_MEMORY.to_string()))
        } else if e.contains("Isolate is disposed") || e.contains("Isolate has exhausted v8 heap space") {
            Some((ResetReason::Heap, e.clone()))
        } else {
            None
        }
    } else {
        None
    };
    if let Some((reason, text)) = reset {
        drop(vm);
        rt.resets.fetch_add(1, Relaxed);
        return failed(user, RunStatus::Reset(reason), text);
    }
    let out = match player {
        Player::Error(e) => failed(user, RunStatus::TotalError, e),
        Player::Out(cap) => {
            // §4.2 step 4 in Rust when the outMessage allows it (store.rs), else on the system isolate, whose
            // objectRoom reads the run context again.
            let object_room = |id: &str| world.object_room(id, user, &ctx.visible).map(|r| world.room_name(r).to_string());
            let native = if rt.native_store { crate::store::store(&cap.bytes, object_room) } else { None };
            let stored = match native {
                Some(j) => Ok(j),
                None => {
                    crate::timing::STORE_FALLBACKS.fetch_add(1, Relaxed);
                    let _guard = RunGuard::enter(RunCtx::new(world, user, ctx.visible.clone(), 0, 0, RunMeter::start(meter, 0.0), meter.deadline_clock()));
                    store(rt, &cap.bytes, &user_id).and_then(|s| Json::parse(&s).map_err(|_| "__store returned invalid JSON".to_string()))
                }
            };
            t = crate::timing::add(4, t);
            let out = match stored {
                Ok(j) => marshal::output(world, user, j, cap, meter),
                Err(e) => failed(user, RunStatus::TotalError, e),
            };
            crate::timing::add(5, t);
            out
        }
    };
    // The JS side keeps the Memory string it last saw; only a saved, unchanged Memory is known to match.
    let kept = matches!(out.status, RunStatus::Done | RunStatus::PartialError) && out.memory.is_none();
    vm.memory_version = kept.then_some(memory_version);
    rt.vms.lock().unwrap_or_else(|e| e.into_inner()).insert(user, vm);
    out
}

/// Disposes a VM outside a run (idle eviction, governor, CLI).
pub(crate) fn dispose(vm: Vm) {
    drop(vm);
}

impl Drop for Vm {
    fn drop(&mut self) {
        if let Some(p) = self.cpu_profile.take() {
            let _locker = self.iso.lock();
            drop(p);
        }
    }
}

/// Hands the VM's V8 CPU profile to the bench report (jsprof cpu mode) and restarts the profiler.
pub(crate) fn collect_cpu_profile(vm: &mut Vm) {
    if let Some(p) = &vm.cpu_profile {
        let mut locker = vm.iso.lock();
        crate::jsprof::cpu_collect(&mut locker, p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_keeps_each_file_and_adds_constants() {
        let files = [("host/prelude.js", "PRELUDE"), ("engine/a.js", "'use strict';\nexports.a = 1"), ("cli/x.js", "nope"), ("host/system.js", "sys")];
        let s = bundle_source(&files, is_player_file, &[("m", "MOD")]);
        assert!(s.starts_with("PRELUDE\n__chitinDefine(\"engine/a.js\", function(module, exports, require) {\n'use strict';\nexports.a = 1\n});\n"));
        assert!(!s.contains("nope") && !s.contains("sys\n") && !s.contains("\"host/prelude.js\""));
        assert!(s.contains("__chitinDefine(\"common/constants.js\", function(module, exports, require) {\nmodule.exports = {"));
        assert!(s.ends_with("__chitinDefine(\"mods/m.js\", function(module, exports, require) {\nMOD\n});\n"));
        assert!(is_system_file("host/system.js") && !is_system_file("engine/game/game.js"));
        assert!(bundle_source(&files, is_system_file, &[]).contains("__chitinDefine(\"host/system.js\""));
    }

    #[test]
    fn lodash_doc_comments_are_blanked_line_for_line() {
        let src = "/**\n * @license\n */\n;(function() {\n  /**\n   * Doc.\n   *\n   * @returns {*}\n   */\n  function a(x) {\n    return x\n      * 2;\n  }\n  /**\n   * Not closed\n  var b;\n}());";
        let out = blank_doc_comments(src);
        assert_eq!(out, "/**\n * @license\n */\n;(function() {\n\n\n\n\n\n  function a(x) {\n    return x\n      * 2;\n  }\n  /**\n   * Not closed\n  var b;\n}());");
        let real = crate::bundle::FILES.iter().find(|(n, _)| *n == "vendor/lodash.js").unwrap().1;
        let blanked = blank_doc_comments(real);
        assert_eq!(blanked.lines().count(), real.lines().count());
        assert!(blanked.len() < real.len() * 2 / 3, "{} of {}", blanked.len(), real.len());
    }

    /// One character above U+00FF makes V8 keep the whole bundle source as a two-byte string, in every VM's heap.
    #[test]
    fn bundles_are_latin1() {
        for keep in [is_player_file as fn(&str) -> bool, is_system_file] {
            let s = bundle_source(crate::bundle::FILES, keep, &[]);
            let bad = s.lines().enumerate().find(|(_, l)| l.chars().any(|c| c as u32 > 0xff));
            assert!(bad.is_none(), "non-Latin-1 text at bundle line {}: {}", bad.unwrap().0 + 1, bad.unwrap().1);
        }
    }
}
