//! `chitin bench`, `CHITIN_NATIVE_PROFILE=<µs>`: a sampling profiler for the whole process (Windows).
//!
//! A thread wakes every <µs>, and for each other thread of the process that used CPU since its last look
//! (QueryThreadCycleTime) suspends it, reads its instruction pointer and resumes it; nothing is allocated while a
//! thread is suspended (it may hold the heap lock). The report symbolizes the addresses through dbghelp and the
//! PDBs next to the binaries: self samples per function, the innermost non-inlined frame. Addresses outside any
//! module are V8's generated code. Only the measured ticks are sampled.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;

type Handle = *mut c_void;

#[repr(C)]
struct ThreadEntry32 {
    size: u32,
    usage: u32,
    thread_id: u32,
    owner_process_id: u32,
    base_pri: i32,
    delta_pri: i32,
    flags: u32,
}

/// x64 `CONTEXT`: 1232 bytes, 16-aligned; `ContextFlags` at 0x30, `Rip` at 0xF8.
#[repr(C, align(16))]
struct Context([u8; 1232]);

#[repr(C)]
struct SymbolInfo {
    size_of_struct: u32,
    type_index: u32,
    reserved: [u64; 2],
    index: u32,
    size: u32,
    mod_base: u64,
    flags: u32,
    value: u64,
    address: u64,
    register: u32,
    scope: u32,
    tag: u32,
    name_len: u32,
    max_name_len: u32,
    name: [u8; 1024],
}

const TH32CS_SNAPTHREAD: u32 = 0x4;
const THREAD_ACCESS: u32 = 0x0002 | 0x0008 | 0x0040; // SUSPEND_RESUME | GET_CONTEXT | QUERY_INFORMATION
const CONTEXT_CONTROL: u32 = 0x0010_0001;

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcessId() -> u32;
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentProcess() -> Handle;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
    fn Thread32First(snap: Handle, entry: *mut ThreadEntry32) -> i32;
    fn Thread32Next(snap: Handle, entry: *mut ThreadEntry32) -> i32;
    fn OpenThread(access: u32, inherit: i32, id: u32) -> Handle;
    fn CloseHandle(h: Handle) -> i32;
    fn SuspendThread(h: Handle) -> u32;
    fn ResumeThread(h: Handle) -> u32;
    fn GetThreadContext(h: Handle, ctx: *mut Context) -> i32;
    fn QueryThreadCycleTime(h: Handle, cycles: *mut u64) -> i32;
}

#[link(name = "dbghelp")]
extern "system" {
    fn SymSetOptions(options: u32) -> u32;
    fn SymInitialize(process: Handle, path: *const u8, invade: i32) -> i32;
    fn SymFromAddr(process: Handle, addr: u64, displacement: *mut u64, symbol: *mut SymbolInfo) -> i32;
    fn SymGetModuleBase64(process: Handle, addr: u64) -> u64;
}

/// `CHITIN_NATIVE_PROFILE_P3`: sample only while rooms are processed (`rooms_phase`).
static P3_ONLY: AtomicBool = AtomicBool::new(false);
static GATE: AtomicBool = AtomicBool::new(true);

/// The tick loop marks the start and end of P3.
pub fn rooms_phase(on: bool) {
    if P3_ONLY.load(Relaxed) {
        GATE.store(on, Relaxed);
    }
}

pub struct Sampler {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<(u64, HashMap<u64, u64>)>,
}

/// Starts sampling when `CHITIN_NATIVE_PROFILE` holds an interval in µs.
pub fn start_from_env() -> Option<Sampler> {
    let us: u64 = std::env::var("CHITIN_NATIVE_PROFILE").ok()?.parse().ok()?;
    if std::env::var_os("CHITIN_NATIVE_PROFILE_P3").is_some() {
        P3_ONLY.store(true, Relaxed);
        GATE.store(false, Relaxed);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || sample(Duration::from_micros(us.max(50)), &flag));
    Some(Sampler { stop, thread })
}

fn sample(every: Duration, stop: &AtomicBool) -> (u64, HashMap<u64, u64>) {
    let (pid, me) = unsafe { (GetCurrentProcessId(), GetCurrentThreadId()) };
    // thread id → (handle, cycles at the last look)
    let mut threads: HashMap<u32, (Handle, u64)> = HashMap::new();
    let mut hits: HashMap<u64, u64> = HashMap::new();
    let (mut samples, mut round) = (0u64, 0u64);
    let mut ctx = Box::new(Context([0; 1232]));
    let mut ips = Vec::with_capacity(64);
    while !stop.load(Relaxed) {
        if round % 256 == 0 {
            refresh(&mut threads, pid, me);
        }
        round += 1;
        ips.clear();
        if !GATE.load(Relaxed) {
            std::thread::sleep(every);
            continue;
        }
        for (handle, last) in threads.values_mut() {
            let mut cycles = 0;
            if unsafe { QueryThreadCycleTime(*handle, &mut cycles) } == 0 || cycles == *last {
                continue;
            }
            *last = cycles;
            if unsafe { SuspendThread(*handle) } == u32::MAX {
                continue;
            }
            ctx.0[0x30..0x34].copy_from_slice(&CONTEXT_CONTROL.to_le_bytes());
            let ok = unsafe { GetThreadContext(*handle, &mut *ctx) } != 0;
            unsafe { ResumeThread(*handle) };
            if ok {
                ips.push(u64::from_le_bytes(ctx.0[0xF8..0x100].try_into().unwrap()));
            }
        }
        for &ip in &ips {
            *hits.entry(ip).or_default() += 1;
            samples += 1;
        }
        std::thread::sleep(every);
    }
    for (h, _) in threads.into_values() {
        unsafe { CloseHandle(h) };
    }
    (samples, hits)
}

/// Adds the process's new threads, drops the ones that exited.
fn refresh(threads: &mut HashMap<u32, (Handle, u64)>, pid: u32, me: u32) {
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snap as isize == -1 {
        return;
    }
    let mut seen = Vec::new();
    let mut e = ThreadEntry32 { size: std::mem::size_of::<ThreadEntry32>() as u32, usage: 0, thread_id: 0, owner_process_id: 0, base_pri: 0, delta_pri: 0, flags: 0 };
    let mut more = unsafe { Thread32First(snap, &mut e) } != 0;
    while more {
        if e.owner_process_id == pid && e.thread_id != me {
            seen.push(e.thread_id);
            threads.entry(e.thread_id).or_insert_with(|| {
                let h = unsafe { OpenThread(THREAD_ACCESS, 0, e.thread_id) };
                (h, 0)
            });
        }
        more = unsafe { Thread32Next(snap, &mut e) } != 0;
    }
    unsafe { CloseHandle(snap) };
    threads.retain(|id, (h, _)| {
        let keep = seen.contains(id) && !h.is_null();
        if !keep && !h.is_null() {
            unsafe { CloseHandle(*h) };
        }
        keep
    });
}

impl Sampler {
    /// Stops sampling and returns the report: the `top` functions by self samples.
    pub fn report(self, top: usize) -> String {
        self.stop.store(true, Relaxed);
        let (samples, hits) = self.thread.join().unwrap_or_default();
        let process = unsafe { GetCurrentProcess() };
        unsafe {
            // UNDNAME | DEFERRED_LOADS | LOAD_ANYTHING; the binary's own directory, or its PDB is not found.
            SymSetOptions(0x2 | 0x4 | 0x40);
            let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()));
            let path = std::ffi::CString::new(dir.unwrap_or_default()).unwrap_or_default();
            SymInitialize(process, path.as_ptr() as *const u8, 1);
        }
        let mut by_fn: HashMap<String, u64> = HashMap::new();
        let mut info = Box::new(SymbolInfo {
            size_of_struct: 88,
            type_index: 0,
            reserved: [0; 2],
            index: 0,
            size: 0,
            mod_base: 0,
            flags: 0,
            value: 0,
            address: 0,
            register: 0,
            scope: 0,
            tag: 0,
            name_len: 0,
            max_name_len: 1024,
            name: [0; 1024],
        });
        for (ip, n) in hits {
            let mut disp = 0u64;
            info.size_of_struct = 88;
            info.max_name_len = 1024;
            let name = if unsafe { SymFromAddr(process, ip, &mut disp, &mut *info) } != 0 {
                let len = (info.name_len as usize).min(1023);
                String::from_utf8_lossy(&info.name[..len]).into_owned()
            } else if unsafe { SymGetModuleBase64(process, ip) } != 0 {
                "<module, no symbol>".to_string()
            } else {
                "<V8 generated code>".to_string()
            };
            *by_fn.entry(name).or_default() += n;
        }
        // A thread caught inside a wait system call used CPU since the last look but is idle now.
        let waiting = |name: &str| (name.starts_with("Nt") || name.starts_with("Zw")) && (name.contains("Wait") || name.contains("Delay") || name.contains("Remove"));
        let idle: u64 = by_fn.iter().filter(|(k, _)| waiting(k)).map(|(_, n)| n).sum();
        let busy = samples.saturating_sub(idle).max(1);
        let mut rows: Vec<(String, u64)> = by_fn.into_iter().filter(|(k, _)| !waiting(k)).collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut out = format!("native profile: {busy} busy samples ({idle} in wait calls left out)\n");
        for (name, n) in rows.into_iter().take(top) {
            let short: String = name.chars().take(160).collect();
            out.push_str(&format!("  {:>5.1}%  {short}\n", n as f64 * 100.0 / busy as f64));
        }
        out
    }
}
