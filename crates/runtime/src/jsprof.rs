//! JS profiler for `chitin bench`, `CHITIN_JS_PROFILE=<µs>` or `CHITIN_JS_PROFILE=cpu:<µs>`.
//!
//! `<µs>`: a thread requests an interrupt on every running player VM that often; the interrupt records the current
//! JS stack. Interrupts run at V8's stack checks (function entry, loop back edges), so the time since the previous
//! check lands on the frame that reached the next one: self time is smeared onto the next function called.
//! `cpu:<µs>`: V8's own sampling profiler through an inspector session per VM (`Profiler.start` / `Profiler.stop`),
//! accurate per function. Its sampler suspends the thread that started it, so run the bench with
//! `--runner-threads 1`; every VM samples that thread, and only the running one finds JS frames.
//! Both report per-function self/total samples (`<runtime>` lines mapped back to their js/ file), GC pauses and
//! per-space heap sizes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use util::json::Json;

struct Slot {
    run: Mutex<Option<(std::thread::ThreadId, v8::IsolateHandle, v8::Global<v8::Context>)>>,
}

#[derive(Default)]
struct Counts {
    samples: u64,
    /// key → (self samples, samples with the key anywhere on the stack)
    by_line: HashMap<String, (u64, u64)>,
    by_function: HashMap<String, (u64, u64)>,
}

static SLOTS: Mutex<Vec<&'static Slot>> = Mutex::new(Vec::new());
static COUNTS: Mutex<Option<Counts>> = Mutex::new(None);
static BUNDLE: OnceLock<Vec<(usize, String)>> = OnceLock::new();

#[derive(Clone, Copy)]
enum Mode {
    Interrupts(u64),
    Cpu(u64),
}

fn mode() -> Option<Mode> {
    static MODE: OnceLock<Option<Mode>> = OnceLock::new();
    *MODE.get_or_init(|| {
        let v = std::env::var("CHITIN_JS_PROFILE").ok()?;
        Some(match v.strip_prefix("cpu") {
            Some(us) => Mode::Cpu(us.trim_start_matches(':').parse().unwrap_or(250).max(50)),
            None => Mode::Interrupts(v.parse().unwrap_or(200).max(20)),
        })
    })
}

fn interval() -> Option<u64> {
    match mode()? {
        Mode::Interrupts(us) => Some(us),
        Mode::Cpu(_) => None,
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The player bundle text, for mapping `<runtime>` lines to files (start line of each `__chitinDefine`).
pub(crate) fn set_bundle(src: &str) {
    if mode().is_none() {
        return;
    }
    let mut files = vec![(1, "host/prelude.js".to_string())];
    for (i, line) in src.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("__chitinDefine(\"") {
            files.push((i + 2, rest.split('"').next().unwrap_or("").to_string()));
        }
    }
    let _ = BUNDLE.set(files);
}

fn slot() -> &'static Slot {
    thread_local!(static SLOT: &'static Slot = {
        let s: &'static Slot = Box::leak(Box::new(Slot { run: Mutex::new(None) }));
        lock(&SLOTS).push(s);
        s
    });
    SLOT.with(|s| *s)
}

/// A player run starts on this thread (no-op unless profiling).
pub(crate) fn begin(handle: v8::IsolateHandle, context: v8::Global<v8::Context>) {
    let Some(us) = interval() else { return };
    static SAMPLER: OnceLock<()> = OnceLock::new();
    SAMPLER.get_or_init(|| {
        *lock(&COUNTS) = Some(Counts::default());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_micros(us));
            for s in lock(&SLOTS).iter() {
                if let Some((_, h, _)) = lock(&s.run).as_ref() {
                    h.request_interrupt(on_interrupt, *s as *const Slot as *mut c_void);
                }
            }
        });
    });
    *lock(&slot().run) = Some((std::thread::current().id(), handle, context));
}

pub(crate) fn end() {
    if interval().is_some() {
        *lock(&slot().run) = None;
    }
}

unsafe extern "C" fn on_interrupt(mut iso: v8::UnsafeRawIsolatePtr, data: *mut c_void) {
    // SAFETY: `data` is a leaked `&'static Slot`.
    let slot = unsafe { &*(data as *const Slot) };
    let context = match lock(&slot.run).as_ref() {
        Some((t, _, c)) if *t == std::thread::current().id() => c.clone(),
        _ => return, // an interrupt left over from an earlier run
    };
    // SAFETY: V8 passes the isolate the interrupt runs on; we are on its thread holding its lock.
    let isolate = unsafe { v8::Isolate::ref_from_raw_isolate_ptr_mut(&mut iso) };
    v8::callback_scope!(unsafe scope, isolate);
    v8::scope!(let scope, scope);
    let context = v8::Local::new(scope, &context);
    let scope = &mut v8::ContextScope::new(scope, context);
    let Some(trace) = v8::StackTrace::current_stack_trace(scope, 64) else { return };
    let mut frames = Vec::new();
    for i in 0..trace.get_frame_count() {
        let Some(f) = trace.get_frame(scope, i) else { break };
        let name = f.get_function_name(scope).map(|s| s.to_rust_string_lossy(scope)).unwrap_or_default();
        let script = f.get_script_name_or_source_url(scope).map(|s| s.to_rust_string_lossy(scope)).unwrap_or_default();
        frames.push((name, script, f.get_line_number()));
    }
    let mut guard = lock(&COUNTS);
    let Some(c) = guard.as_mut() else { return };
    c.samples += 1;
    let mut seen_line = std::collections::HashSet::new();
    let mut seen_fn = std::collections::HashSet::new();
    for (i, (name, script, line)) in frames.iter().enumerate() {
        let (file, l) = locate(script, *line);
        let by_line = format!("{} {file}:{l}", if name.is_empty() { "(anon)" } else { name });
        let by_fn = format!("{} {file}", if name.is_empty() { "(anon)" } else { name });
        if seen_line.insert(by_line.clone()) {
            let e = c.by_line.entry(by_line).or_default();
            e.0 += (i == 0) as u64;
            e.1 += 1;
        }
        if seen_fn.insert(by_fn.clone()) {
            let e = c.by_function.entry(by_fn).or_default();
            e.0 += (i == 0) as u64;
            e.1 += 1;
        }
    }
}

fn locate(script: &str, line: usize) -> (String, usize) {
    if script == "<runtime>" {
        if let Some(files) = BUNDLE.get() {
            if let Some((start, f)) = files.iter().rev().find(|(s, _)| *s <= line) {
                return (f.clone(), line + 1 - start);
            }
        }
    }
    (script.to_string(), line)
}

/// The tables since the last call (None unless profiling).
pub fn report(top: usize) -> Option<String> {
    mode()?;
    let c = lock(&COUNTS).replace(Counts::default())?;
    let n = c.samples.max(1) as f64;
    let take = |a: &std::sync::atomic::AtomicU64| a.swap(0, std::sync::atomic::Ordering::Relaxed);
    let gc = GC_NS.each_ref().map(|a| take(a) as f64 / 1e6);
    let (gcs, runs) = (GC_COUNT.each_ref().map(take), take(&RUNS).max(1) as f64);
    let mut out = format!(
        "JS profile: {} samples; GC pauses {:.1} ms scavenge, {:.1} ms other; per run {:.3} scavenges, {:.3} other GCs\n",
        c.samples, gc[0], gc[1], gcs[0] as f64 / runs, gcs[1] as f64 / runs
    );
    let table = |out: &mut String, title: &str, m: &HashMap<String, (u64, u64)>, by_self: bool| {
        let mut v: Vec<_> = m.iter().collect();
        v.sort_by_key(|(_, (s, t))| std::cmp::Reverse(if by_self { *s } else { *t }));
        out.push_str(&format!("  {title}\n"));
        for (k, (s, t)) in v.into_iter().take(top) {
            out.push_str(&format!("    {:>5.1}% self {:>5.1}% total  {k}\n", *s as f64 * 100.0 / n, *t as f64 * 100.0 / n));
        }
    };
    table(&mut out, "functions by self", &c.by_function, true);
    table(&mut out, "functions by total", &c.by_function, false);
    table(&mut out, "lines by self", &c.by_line, true);
    spaces_report(&mut out);
    Some(out)
}

static GC_NS: [std::sync::atomic::AtomicU64; 2] = [const { std::sync::atomic::AtomicU64::new(0) }; 2];
static GC_COUNT: [std::sync::atomic::AtomicU64; 2] = [const { std::sync::atomic::AtomicU64::new(0) }; 2];
/// Runs since the last report.
static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

thread_local!(static GC_START: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) });

/// Times this isolate's GC pauses (no-op unless profiling).
pub(crate) fn watch_gc(iso: &mut v8::Isolate) {
    if mode().is_none() {
        return;
    }
    unsafe extern "C" fn pro(_: v8::UnsafeRawIsolatePtr, _: v8::GCType, _: v8::GCCallbackFlags, _: *mut c_void) {
        GC_START.with(|s| s.set(Some(std::time::Instant::now())));
    }
    unsafe extern "C" fn epi(_: v8::UnsafeRawIsolatePtr, t: v8::GCType, _: v8::GCCallbackFlags, _: *mut c_void) {
        if let Some(s) = GC_START.with(|s| s.take()) {
            let i = (t != v8::GCType::kGCTypeScavenge) as usize;
            GC_NS[i].fetch_add(s.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
            GC_COUNT[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    iso.add_gc_prologue_callback(pro, std::ptr::null_mut(), v8::GCType::kGCTypeAll);
    iso.add_gc_epilogue_callback(epi, std::ptr::null_mut(), v8::GCType::kGCTypeAll);
}

/// Latest per-space heap sizes of each VM (used, physical), for the report.
static SPACES: Mutex<Option<HashMap<u32, Vec<(&'static str, usize, usize)>>>> = Mutex::new(None);

pub(crate) fn spaces(iso: &mut v8::Isolate, user: u32) {
    if mode().is_none() {
        return;
    }
    let mut v: Vec<_> = (0..iso.number_of_heap_spaces())
        .filter_map(|i| iso.get_heap_space_statistics(i))
        .map(|s| (s.space_name().to_str().unwrap_or("?"), s.space_used_size(), s.physical_space_size()))
        .collect();
    // V8's own memory outside the spaces: malloced now / at peak, and external (array buffers) as reported
    let h = iso.get_heap_statistics();
    v.push(("(malloced: now / peak)", h.malloced_memory(), h.peak_malloced_memory()));
    v.push(("(external)", h.external_memory(), h.external_memory()));
    RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    lock(&SPACES).get_or_insert_with(HashMap::new).insert(user, v);
}

fn spaces_report(out: &mut String) {
    let guard = lock(&SPACES);
    let Some(m) = guard.as_ref() else { return };
    let mut sum: Vec<(&str, usize, usize)> = Vec::new();
    for v in m.values() {
        for &(name, used, phys) in v {
            match sum.iter_mut().find(|s| s.0 == name) {
                Some(s) => {
                    s.1 += used;
                    s.2 += phys;
                }
                None => sum.push((name, used, phys)),
            }
        }
    }
    let n = m.len().max(1) as f64 * 1048576.0;
    out.push_str(&format!("  heap spaces, MB per VM ({} VMs): used / physical\n", m.len()));
    for (name, used, phys) in sum {
        out.push_str(&format!("    {name:<24} {:>6.2} / {:>6.2}\n", used as f64 / n, phys as f64 / n));
    }
}

// ---- cpu mode ----

/// A VM's inspector session with the profiler running. Touched only under the VM's isolate lock.
pub(crate) struct CpuProfile {
    session: v8::inspector::V8InspectorSession,
    _inspector: v8::inspector::V8Inspector,
    responses: Rc<RefCell<Vec<String>>>,
}

// SAFETY: created, used and dropped only while the owning Vm's isolate is locked (host.rs).
unsafe impl Send for CpuProfile {}

struct Client;
impl v8::inspector::V8InspectorClientImpl for Client {}

struct Channel(Rc<RefCell<Vec<String>>>);
impl v8::inspector::ChannelImpl for Channel {
    fn send_response(&self, _call_id: i32, message: v8::UniquePtr<v8::inspector::StringBuffer>) {
        if let Some(m) = message.as_ref() {
            self.0.borrow_mut().push(m.string().to_string());
        }
    }
    fn send_notification(&self, _message: v8::UniquePtr<v8::inspector::StringBuffer>) {}
    fn flush_protocol_notifications(&self) {}
}

impl CpuProfile {
    fn send(&self, message: &str) -> Option<String> {
        self.session.dispatch_protocol_message(v8::inspector::StringView::from(message.as_bytes()));
        self.responses.borrow_mut().pop()
    }
}

/// Starts V8's profiler on this VM (cpu mode only, once per VM), on this thread.
pub(crate) fn cpu_attach(iso: &mut v8::Isolate, context: &v8::Global<v8::Context>, slot: &mut Option<Box<CpuProfile>>) {
    let Some(Mode::Cpu(us)) = mode() else { return };
    if slot.is_some() {
        return;
    }
    let inspector = v8::inspector::V8Inspector::create(iso, v8::inspector::V8InspectorClient::new(Box::new(Client)));
    v8::scope!(let scope, iso);
    let context = v8::Local::new(scope, context);
    inspector.context_created(context, 1, v8::inspector::StringView::empty(), v8::inspector::StringView::empty());
    let responses = Rc::new(RefCell::new(Vec::new()));
    let session = inspector.connect(
        1,
        v8::inspector::Channel::new(Box::new(Channel(responses.clone()))),
        v8::inspector::StringView::empty(),
        v8::inspector::V8InspectorClientTrustLevel::FullyTrusted,
    );
    let p = CpuProfile { session, _inspector: inspector, responses };
    p.send(r#"{"id":1,"method":"Profiler.enable"}"#);
    p.send(&format!(r#"{{"id":2,"method":"Profiler.setSamplingInterval","params":{{"interval":{us}}}}}"#));
    p.send(r#"{"id":3,"method":"Profiler.start"}"#);
    *slot = Some(Box::new(p));
}

/// Adds the VM's samples so far to the report and starts a new profile in the same session (a second inspector on
/// the isolate crashed V8).
pub(crate) fn cpu_collect(iso: &mut v8::Isolate, p: &CpuProfile) {
    v8::scope!(let _scope, iso);
    let text = p.send(r#"{"id":4,"method":"Profiler.stop"}"#);
    p.send(r#"{"id":5,"method":"Profiler.start"}"#);
    let Some(text) = text else { return };
    let Ok(j) = Json::parse(&text) else { return };
    let Some(nodes) = j.get("result").and_then(|r| r.get("profile")).and_then(|p| p.get("nodes")).and_then(Json::as_array) else { return };
    // node id → (key, parent id); keys are "function file:line" with the function's first line.
    let mut info: HashMap<u64, (String, Option<u64>)> = HashMap::new();
    let mut hits: Vec<(u64, u64)> = Vec::new();
    let mut lines: Vec<(String, u64)> = Vec::new();
    for n in nodes {
        let id = n.get("id").and_then(Json::as_f64).unwrap_or(0.0) as u64;
        let cf = n.get("callFrame");
        let name = cf.and_then(|c| c.get("functionName")).and_then(Json::as_str).unwrap_or("");
        let url = cf.and_then(|c| c.get("url")).and_then(Json::as_str).unwrap_or("");
        let line = cf.and_then(|c| c.get("lineNumber")).and_then(Json::as_f64).unwrap_or(-1.0);
        let key = if url.is_empty() && name.starts_with('(') {
            name.to_string() // (program), (garbage collector), (idle), (root)
        } else {
            let (file, l) = locate(url, (line + 1.0).max(0.0) as usize);
            format!("{} {file}:{l}", if name.is_empty() { "(anon)" } else { name })
        };
        info.entry(id).or_insert((String::new(), None)).0 = key;
        for c in n.get("children").and_then(Json::as_array).into_iter().flatten() {
            let c = c.as_f64().unwrap_or(0.0) as u64;
            info.entry(c).or_insert((String::new(), None)).1 = Some(id);
        }
        let h = n.get("hitCount").and_then(Json::as_f64).unwrap_or(0.0) as u64;
        if h > 0 {
            hits.push((id, h));
        }
        for t in n.get("positionTicks").and_then(Json::as_array).into_iter().flatten() {
            let l = t.get("line").and_then(Json::as_f64).unwrap_or(0.0) as usize;
            let ticks = t.get("ticks").and_then(Json::as_f64).unwrap_or(0.0) as u64;
            let (file, l) = locate(url, l);
            lines.push((format!("{} {file}:{l}", if name.is_empty() { "(anon)" } else { name }), ticks));
        }
    }
    let mut guard = lock(&COUNTS);
    let c = guard.get_or_insert_with(Counts::default);
    for (k, ticks) in lines {
        let e = c.by_line.entry(k).or_default();
        e.0 += ticks;
        e.1 += ticks;
    }
    for (id, h) in hits {
        let Some((key, _)) = info.get(&id) else { continue };
        if key == "(idle)" || key == "(program)" || key == "(root)" {
            continue; // another VM's turn on the thread, or host code
        }
        c.samples += h;
        c.by_function.entry(key.clone()).or_default().0 += h;
        let mut seen = std::collections::HashSet::new();
        let mut at = Some(id);
        while let Some(i) = at {
            let Some((k, parent)) = info.get(&i) else { break };
            if seen.insert(k.as_str()) && k != "(root)" {
                c.by_function.entry(k.clone()).or_default().1 += h;
            }
            at = *parent;
        }
    }
}
