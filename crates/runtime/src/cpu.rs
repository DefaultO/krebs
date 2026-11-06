//! CPU meters (Thread, Wall, Fixed) over util::sys (ARCHITECTURE.md §6.10, D11). Owner: WP08.
//!
//! One meter serves `cpuTime()` (hence `getUsed()`), the billed numbers the JS computes from it and the
//! `compileAndRun` deadlines (02 CPU-CLOCK: never mix clocks). Like isolated-vm's `isolate.cpuTime`
//! (executor.cc:59-106) the value is per VM: the time of its earlier runs plus the running one.

use crate::Meter;
use std::sync::OnceLock;
use std::time::Instant;

/// The clock a deadline is measured on; the watchdog reads both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Clock {
    ThreadCpu,
    Wall,
}

impl Meter {
    /// `Fixed` counts calls, which a `while(true){}` never makes, so its deadlines run on thread CPU.
    // ponytail: Fixed runs bill ms_per_tick but time out on thread CPU; a call-counting deadline would
    // need an interrupt per cpuTime() call.
    pub(crate) fn deadline_clock(&self) -> Clock {
        match self {
            Meter::Wall => Clock::Wall,
            Meter::Thread | Meter::Fixed { .. } => Clock::ThreadCpu,
        }
    }
}

/// Wall ns since the first call in this process (a common epoch for every thread).
pub(crate) fn wall_ns() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Now on `clock` for the calling thread.
pub(crate) fn now_ns(clock: Clock) -> u64 {
    match clock {
        Clock::ThreadCpu => util::sys::thread_cpu_ns(),
        Clock::Wall => wall_ns(),
    }
}

/// `cpuTime()` of one run.
pub(crate) struct RunMeter {
    meter: Meter,
    base_ms: f64,
    t0: u64,
    calls: u64,
}

impl RunMeter {
    /// `base_ms`: the VM's total from earlier runs (0 for a new VM).
    pub fn start(meter: Meter, base_ms: f64) -> RunMeter {
        let t0 = match meter {
            Meter::Thread => util::sys::thread_cpu_ns(),
            Meter::Wall => wall_ns(),
            Meter::Fixed { .. } => 0,
        };
        RunMeter { meter, base_ms, t0, calls: 0 }
    }

    /// The `cpuTime()` native: ms as f64. `Fixed` advances 0.01 ms per call.
    pub fn now_ms(&mut self) -> f64 {
        if let Meter::Fixed { .. } = self.meter {
            self.calls += 1;
        }
        self.total_ms()
    }

    /// The VM's total so far without counting a call; the next run continues from it.
    pub fn total_ms(&self) -> f64 {
        let ns = |now: u64| now.saturating_sub(self.t0) as f64 / 1e6;
        self.base_ms
            + match self.meter {
                Meter::Thread => ns(util::sys::thread_cpu_ns()),
                Meter::Wall => ns(wall_ns()),
                Meter::Fixed { .. } => self.calls as f64 * 0.01,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_counts_calls_and_continues_from_base() {
        let mut m = RunMeter::start(Meter::Fixed { ms_per_tick: 3.0 }, 1.0);
        assert_eq!(m.now_ms(), 1.0 + 0.01);
        assert_eq!(m.now_ms(), 1.0 + 0.02);
        assert_eq!(m.total_ms(), 1.0 + 0.02);
    }

    #[test]
    fn thread_meter_ignores_sleep_wall_does_not() {
        let mut t = RunMeter::start(Meter::Thread, 0.0);
        let mut w = RunMeter::start(Meter::Wall, 0.0);
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(t.now_ms() < 10.0, "thread meter counted sleep");
        assert!(w.now_ms() >= 29.0, "wall meter missed sleep");
        assert_eq!(Meter::Wall.deadline_clock(), Clock::Wall);
        assert_eq!(Meter::Fixed { ms_per_tick: 1.0 }.deadline_clock(), Clock::ThreadCpu);
    }
}
