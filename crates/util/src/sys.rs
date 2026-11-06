//! OS services std lacks, through direct FFI (no crates): per-thread CPU time,
//! physical core count, OS randomness and a Ctrl-C hook. Windows and Linux.

#[cfg(not(any(all(windows, target_arch = "x86_64"), target_os = "linux")))]
compile_error!("util::sys supports x86_64 Windows and Linux");

/// CPU time consumed by the calling thread, in ns.
pub fn thread_cpu_ns() -> u64 {
    imp::thread_cpu_ns()
}

/// A thread's CPU clock that other threads can read (the watchdog polls workers
/// with it). Create it on the thread to be measured.
pub struct ThreadClock(imp::Clock);

impl ThreadClock {
    pub fn current() -> ThreadClock {
        ThreadClock(imp::Clock::current())
    }

    /// CPU time the thread has consumed, in ns (same scale as `thread_cpu_ns`).
    /// Linux: 0 once the thread has exited; Windows keeps the final value.
    pub fn cpu_ns(&self) -> u64 {
        self.0.cpu_ns()
    }
}

/// Physical cores (SMT siblings counted once); falls back to logical CPUs.
pub fn physical_cores() -> usize {
    imp::physical_cores().unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
}

/// Physical memory the process occupies (resident set / working set), in bytes.
pub fn process_memory() -> u64 {
    imp::process_memory()
}

/// Fills `buf` from the OS CSPRNG. Panics if the OS refuses (nothing sane to do then).
pub fn os_random(buf: &mut [u8]) {
    imp::os_random(buf)
}

/// Runs `f` on its own thread whenever Ctrl-C / Ctrl-Break / console close / logoff / shutdown
/// (Windows) or SIGINT / SIGTERM (Linux) arrives. Installing again replaces the callback.
/// `f` only has to ask for a stop: on a console close the process stays up until main exits,
/// for as long as Windows allows, which is about 5 s.
pub fn on_ctrl_c(f: impl Fn() + Send + Sync + 'static) {
    *CTRL_C.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
    imp::install_ctrl_c();
}

type Callback = Box<dyn Fn() + Send + Sync>;
static CTRL_C: std::sync::Mutex<Option<Callback>> = std::sync::Mutex::new(None);

fn run_ctrl_c() {
    if let Some(f) = &*CTRL_C.lock().unwrap_or_else(|e| e.into_inner()) {
        f();
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::sync::{Once, OnceLock};

    type Handle = *mut c_void;
    type Bool = i32;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentThread() -> Handle;
        fn GetCurrentProcess() -> Handle;
        fn DuplicateHandle(sp: Handle, s: Handle, tp: Handle, t: *mut Handle, access: u32, inherit: Bool, options: u32) -> Bool;
        fn CloseHandle(h: Handle) -> Bool;
        fn QueryThreadCycleTime(h: Handle, cycles: *mut u64) -> Bool;
        fn QueryPerformanceCounter(c: *mut i64) -> Bool;
        fn QueryPerformanceFrequency(f: *mut i64) -> Bool;
        fn GetLogicalProcessorInformationEx(rel: u32, buf: *mut u8, len: *mut u32) -> Bool;
        fn SetConsoleCtrlHandler(h: Option<unsafe extern "system" fn(u32) -> Bool>, add: Bool) -> Bool;
        fn K32GetProcessMemoryInfo(p: Handle, counters: *mut MemoryCounters, cb: u32) -> Bool;
    }

    /// PROCESS_MEMORY_COUNTERS_EX: cb, PageFaultCount, then 9 SIZE_Ts ending in PrivateUsage.
    #[repr(C)]
    struct MemoryCounters {
        cb: u32,
        faults: u32,
        sizes: [usize; 9],
    }
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(alg: Handle, buf: *mut u8, len: u32, flags: u32) -> i32;
    }

    const DUPLICATE_SAME_ACCESS: u32 = 2;
    const RELATION_PROCESSOR_CORE: u32 = 0;
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 2;

    fn cycles(h: Handle) -> u64 {
        let mut c = 0;
        // SAFETY: h is a valid thread handle (pseudo or duplicated) and c a valid out pointer.
        unsafe { QueryThreadCycleTime(h, &mut c) };
        c
    }

    fn qpc() -> i64 {
        let mut c = 0;
        // SAFETY: valid out pointer; QPC cannot fail on XP and later.
        unsafe { QueryPerformanceCounter(&mut c) };
        c
    }

    /// ns per thread cycle. GetThreadTimes is 15.6 ms coarse; thread cycles are TSC
    /// ticks (the kernel charges them with rdtsc at context switches), so the TSC rate is
    /// measured against QPC once. Both are wall clocks, but a preemption between the two
    /// reads of a pair would skew every later reading by up to (slice / 20 ms), so a pair
    /// counts only when the QPC reads around its rdtsc are under 10 µs apart.
    fn ns_per_cycle() -> f64 {
        static RATE: OnceLock<f64> = OnceLock::new();
        *RATE.get_or_init(|| {
            let mut freq = 0;
            // SAFETY: valid out pointer.
            unsafe { QueryPerformanceFrequency(&mut freq) };
            let pair = || loop {
                let q0 = qpc();
                // SAFETY: rdtsc exists on every x86_64 CPU.
                let t = unsafe { std::arch::x86_64::_rdtsc() };
                let q1 = qpc();
                if (q1 - q0) * 100_000 < freq {
                    return (t, q0 + (q1 - q0) / 2);
                }
            };
            let (t0, q0) = pair();
            std::thread::sleep(std::time::Duration::from_millis(20));
            let (t1, q1) = pair();
            (q1 - q0) as f64 * 1e9 / freq as f64 / (t1 - t0) as f64
        })
    }

    pub fn thread_cpu_ns() -> u64 {
        let rate = ns_per_cycle();
        // SAFETY: pseudo handle of the current thread.
        (cycles(unsafe { GetCurrentThread() }) as f64 * rate) as u64
    }

    /// A real (duplicated) handle: the pseudo handle means "the calling thread" everywhere.
    pub struct Clock(Handle);
    // SAFETY: a thread handle may be used from any thread; QueryThreadCycleTime is thread-safe.
    unsafe impl Send for Clock {}
    // SAFETY: as above; the handle is only read.
    unsafe impl Sync for Clock {}

    impl Clock {
        pub fn current() -> Clock {
            let mut h = std::ptr::null_mut();
            // SAFETY: all handles are valid pseudo handles; h is a valid out pointer.
            let ok = unsafe { DuplicateHandle(GetCurrentProcess(), GetCurrentThread(), GetCurrentProcess(), &mut h, 0, 0, DUPLICATE_SAME_ACCESS) };
            assert!(ok != 0, "DuplicateHandle failed");
            ns_per_cycle(); // calibrate (20 ms) now rather than in the watchdog's first poll
            Clock(h)
        }

        pub fn cpu_ns(&self) -> u64 {
            (cycles(self.0) as f64 * ns_per_cycle()) as u64
        }
    }

    impl Drop for Clock {
        fn drop(&mut self) {
            // SAFETY: we own the duplicated handle.
            unsafe { CloseHandle(self.0) };
        }
    }

    pub fn physical_cores() -> Option<usize> {
        let mut len = 0u32;
        // SAFETY: a null buffer with len 0 asks for the required size.
        unsafe { GetLogicalProcessorInformationEx(RELATION_PROCESSOR_CORE, std::ptr::null_mut(), &mut len) };
        let mut buf = vec![0u8; len as usize];
        // SAFETY: buf has len bytes.
        if len == 0 || unsafe { GetLogicalProcessorInformationEx(RELATION_PROCESSOR_CORE, buf.as_mut_ptr(), &mut len) } == 0 {
            return None;
        }
        // Variable-size records: {Relationship: u32, Size: u32, ...}, one per core.
        let (mut pos, mut cores) = (0usize, 0usize);
        while pos + 8 <= len as usize {
            let size = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into().unwrap()) as usize;
            if size == 0 {
                break;
            }
            cores += 1;
            pos += size;
        }
        (cores > 0).then_some(cores)
    }

    pub fn process_memory() -> u64 {
        let mut m = MemoryCounters { cb: std::mem::size_of::<MemoryCounters>() as u32, faults: 0, sizes: [0; 9] };
        // SAFETY: pseudo process handle; m is a valid PROCESS_MEMORY_COUNTERS_EX of size cb.
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut m, m.cb) };
        m.sizes[1] as u64 // WorkingSetSize
    }

    pub fn os_random(buf: &mut [u8]) {
        for chunk in buf.chunks_mut(u32::MAX as usize) {
            // SAFETY: chunk is a valid writable buffer of the given length.
            let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), chunk.as_mut_ptr(), chunk.len() as u32, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
            assert!(status >= 0, "BCryptGenRandom failed: {status:#x}");
        }
    }

    /// Closing the console window, logging off and shutting down. Windows ends the process as soon as the handler
    /// returns from one of these.
    const CTRL_CLOSE_EVENT: u32 = 2;
    const CTRL_LOGOFF_EVENT: u32 = 5;
    const CTRL_SHUTDOWN_EVENT: u32 = 6;

    unsafe extern "system" fn handler(event: u32) -> Bool {
        // The system runs console handlers on a fresh thread, so a blocking callback is fine.
        super::run_ctrl_c();
        // Returning would cut off whatever the callback started, the world's last save above all, so this thread stays
        // until main exits, which takes it along. Windows itself ends the process after about 5 s.
        if matches!(event, CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        1
    }

    pub fn install_ctrl_c() {
        static ONCE: Once = Once::new();
        // SAFETY: handler is a valid extern "system" fn for the process lifetime.
        ONCE.call_once(|| assert!(unsafe { SetConsoleCtrlHandler(Some(handler), 1) } != 0, "SetConsoleCtrlHandler failed"));
    }

    #[cfg(test)]
    pub fn thread_times_ns() -> u64 {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetThreadTimes(h: Handle, c: *mut u64, e: *mut u64, k: *mut u64, u: *mut u64) -> Bool;
        }
        let (mut c, mut e, mut k, mut u) = (0, 0, 0, 0);
        // SAFETY: FILETIME is two u32s = one little-endian u64; all pointers valid.
        unsafe { GetThreadTimes(GetCurrentThread(), &mut c, &mut e, &mut k, &mut u) };
        (k + u) * 100
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::{c_int, c_long, c_uint, c_ulong, c_void};
    use std::sync::atomic::{AtomicI32, Ordering::Relaxed};
    use std::sync::Once;

    #[repr(C)]
    struct Timespec {
        sec: i64,
        nsec: c_long,
    }

    /// glibc `struct sigaction` (x86_64 and aarch64 share this layout).
    #[repr(C)]
    struct SigAction {
        handler: usize,
        mask: [u64; 16],
        flags: c_int,
        restorer: usize,
    }

    extern "C" {
        fn clock_gettime(clock: c_int, ts: *mut Timespec) -> c_int;
        fn pthread_self() -> c_ulong;
        fn pthread_getcpuclockid(t: c_ulong, clock: *mut c_int) -> c_int;
        fn getrandom(buf: *mut c_void, len: usize, flags: c_uint) -> isize;
        fn sigaction(sig: c_int, act: *const SigAction, old: *mut SigAction) -> c_int;
        fn pipe(fds: *mut c_int) -> c_int;
        fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
        fn write(fd: c_int, buf: *const c_void, n: usize) -> isize;
    }

    const CLOCK_THREAD_CPUTIME_ID: c_int = 3;
    const SIGINT: c_int = 2;
    const SIGTERM: c_int = 15;
    const SA_RESTART: c_int = 0x1000_0000;

    fn read_clock(clock: c_int) -> Option<u64> {
        let mut ts = Timespec { sec: 0, nsec: 0 };
        // SAFETY: valid out pointer.
        (unsafe { clock_gettime(clock, &mut ts) } == 0).then(|| ts.sec as u64 * 1_000_000_000 + ts.nsec as u64)
    }

    pub fn thread_cpu_ns() -> u64 {
        read_clock(CLOCK_THREAD_CPUTIME_ID).expect("CLOCK_THREAD_CPUTIME_ID")
    }

    pub struct Clock(c_int);

    impl Clock {
        pub fn current() -> Clock {
            let mut id = 0;
            // SAFETY: pthread_self is always valid for the calling thread; valid out pointer.
            assert_eq!(unsafe { pthread_getcpuclockid(pthread_self(), &mut id) }, 0, "pthread_getcpuclockid failed");
            Clock(id)
        }

        pub fn cpu_ns(&self) -> u64 {
            read_clock(self.0).unwrap_or(0)
        }
    }

    pub fn physical_cores() -> Option<usize> {
        // One entry per distinct sibling set = one per physical core.
        let mut sets = std::collections::HashSet::new();
        for e in std::fs::read_dir("/sys/devices/system/cpu").ok()?.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("cpu") && name[3..].bytes().all(|b| b.is_ascii_digit()) && name.len() > 3 {
                if let Ok(s) = std::fs::read_to_string(e.path().join("topology/thread_siblings_list")) {
                    sets.insert(s.trim().to_string());
                }
            }
        }
        (!sets.is_empty()).then_some(sets.len())
    }

    pub fn process_memory() -> u64 {
        let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        let kb = status.lines().find_map(|l| l.strip_prefix("VmRSS:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok());
        kb.unwrap_or(0) * 1024
    }

    pub fn os_random(mut buf: &mut [u8]) {
        while !buf.is_empty() {
            // SAFETY: buf is a valid writable buffer of buf.len() bytes.
            let n = unsafe { getrandom(buf.as_mut_ptr().cast(), buf.len(), 0) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "getrandom failed: {e}");
                continue;
            }
            buf = &mut buf[n as usize..];
        }
    }

    static PIPE_W: AtomicI32 = AtomicI32::new(-1);

    extern "C" fn handler(_sig: c_int) {
        // Only async-signal-safe work here: wake the callback thread through the pipe.
        // SAFETY: writing one byte from a valid buffer; a bad fd only makes write fail.
        unsafe { write(PIPE_W.load(Relaxed), [1u8].as_ptr().cast(), 1) };
    }

    pub fn install_ctrl_c() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let mut fds = [0 as c_int; 2];
            // SAFETY: fds holds two ints.
            assert_eq!(unsafe { pipe(fds.as_mut_ptr()) }, 0, "pipe failed");
            PIPE_W.store(fds[1], Relaxed);
            let r = fds[0];
            std::thread::Builder::new()
                .name("ctrl-c".into())
                .spawn(move || loop {
                    let mut b = 0u8;
                    // SAFETY: valid one-byte buffer.
                    if unsafe { read(r, (&mut b as *mut u8).cast(), 1) } == 1 {
                        super::run_ctrl_c();
                    }
                })
                .expect("spawn ctrl-c thread");
            let act = SigAction { handler: handler as extern "C" fn(c_int) as usize, mask: [0; 16], flags: SA_RESTART, restorer: 0 };
            for sig in [SIGINT, SIGTERM] {
                // SAFETY: act is a valid sigaction; old may be null.
                assert_eq!(unsafe { sigaction(sig, &act, std::ptr::null_mut()) }, 0, "sigaction failed");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spin_ms(ms: u64) {
        let t = std::time::Instant::now();
        while t.elapsed().as_millis() < ms as u128 {
            std::hint::black_box(0u64);
        }
    }

    #[test]
    fn thread_cpu_matches_reference() {
        // Reference: GetThreadTimes on Windows (charged per 15.6 ms clock tick, so it is
        // noisy on a loaded machine: up to 3 tries), CLOCK_THREAD_CPUTIME_ID on Linux.
        std::thread::spawn(|| {
            #[cfg(windows)]
            let reference = imp::thread_times_ns;
            #[cfg(not(windows))]
            let reference = thread_cpu_ns;
            let mut log = Vec::new();
            for _ in 0..3 {
                let wall = std::time::Instant::now();
                let (a, r0) = (thread_cpu_ns(), reference());
                spin_ms(1000);
                let (b, r1) = (thread_cpu_ns(), reference());
                let (ours, theirs, wall) = ((b - a) as f64, (r1 - r0) as f64, wall.elapsed().as_nanos() as f64);
                assert!(ours <= wall * 1.01, "thread cpu {ours} ns exceeds wall {wall} ns");
                if theirs > 3e8 && (ours / theirs - 1.0).abs() < 0.05 {
                    return;
                }
                log.push(format!("ours {ours} ns, reference {theirs} ns"));
            }
            panic!("thread cpu off by more than 5%: {log:?}");
        })
        .join()
        .unwrap();
    }

    #[test]
    fn clock_read_from_another_thread() {
        let (tx, rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            tx.send(ThreadClock::current()).unwrap();
            spin_ms(300);
            let own = thread_cpu_ns();
            go_rx.recv().unwrap();
            own
        });
        let clock = rx.recv().unwrap();
        let first = clock.cpu_ns();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let later = clock.cpu_ns();
        go_tx.send(()).unwrap();
        let own = worker.join().unwrap();
        assert!(later > first + 100_000_000, "worker clock did not advance: {first} -> {later}");
        assert!(later <= own + 50_000_000, "watcher {later} ns vs worker's own {own} ns");
        // Sleeping costs no CPU: idle main thread barely moved.
        let me = ThreadClock::current();
        let a = me.cpu_ns();
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(me.cpu_ns() - a < 20_000_000);
    }

    #[test]
    fn cores_and_random() {
        let logical = std::thread::available_parallelism().unwrap().get();
        let p = physical_cores();
        assert!(p >= 1 && p <= logical, "physical {p}, logical {logical}");
        let (mut a, mut b) = ([0u8; 64], [0u8; 64]);
        os_random(&mut a);
        os_random(&mut b);
        assert_ne!(a, b);
        assert!(a.iter().any(|&x| x != 0));
        os_random(&mut []);
    }

    #[test]
    fn ctrl_c_installs() {
        on_ctrl_c(|| {});
        on_ctrl_c(|| {}); // replacing is fine
    }
}
