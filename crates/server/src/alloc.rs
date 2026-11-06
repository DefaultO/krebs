//! Global allocator: the system heap behind a per-thread cache of small blocks.
//!
//! Requests of at most 4 KiB (alignment ≤ 16) are rounded up to a multiple of 16 (of 256 past 512 bytes) and
//! served from that size class's free list on the current thread, falling back to the system heap with the rounded
//! layout; a freed block goes onto the freeing thread's list (up to 64 KiB per class) or back to the heap. Room
//! processing and input building make and drop many short-lived `Rc`/`Vec`/`Arc<str>` blocks; the Windows heap
//! costs tens of nanoseconds per call where a list pop or push costs a few. A thread's lists go back to the heap when it exits.
//! The system heap is the Segment Heap (build.rs, chitin.manifest).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::ptr::{self, null_mut};

const STEP: usize = 16;
const SMALL: usize = 32;
const BIG_STEP: usize = 256;
const CLASSES: usize = SMALL + (4096 - SMALL * STEP) / BIG_STEP;
const LIST_BYTES: usize = 64 << 10;

pub struct Cached;

struct Lists {
    head: [*mut u8; CLASSES],
    len: [u32; CLASSES],
}

/// Cached, Live (flushes on thread exit), Off (being destroyed, or exited: straight to the heap).
#[derive(Clone, Copy, PartialEq)]
enum State {
    Fresh,
    Live,
    Off,
}

thread_local! {
    static LISTS: UnsafeCell<Lists> = const { UnsafeCell::new(Lists { head: [null_mut(); CLASSES], len: [0; CLASSES] }) };
    static STATE: Cell<State> = const { Cell::new(State::Fresh) };
    static GUARD: Guard = const { Guard };
}

struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = STATE.try_with(|s| s.set(State::Off));
        let _ = LISTS.try_with(|l| {
            // SAFETY: this thread's lists; nothing else on this thread touches them while Off.
            let l = unsafe { &mut *l.get() };
            for c in 0..CLASSES {
                let mut p = l.head[c];
                while !p.is_null() {
                    // SAFETY: every listed block came from `System` with the class layout; its first word links on.
                    let next = unsafe { *(p as *mut *mut u8) };
                    unsafe { System.dealloc(p, class_layout(c)) };
                    p = next;
                }
                l.head[c] = null_mut();
                l.len[c] = 0;
            }
        });
    }
}

fn class_of(layout: Layout) -> Option<usize> {
    let n = layout.size();
    if layout.align() > STEP || n > class_size(CLASSES - 1) {
        None
    } else if n <= SMALL * STEP {
        Some(n.saturating_sub(1) / STEP)
    } else {
        Some(SMALL + (n - SMALL * STEP - 1) / BIG_STEP)
    }
}

fn class_size(c: usize) -> usize {
    if c < SMALL {
        (c + 1) * STEP
    } else {
        SMALL * STEP + (c - SMALL + 1) * BIG_STEP
    }
}

fn class_layout(c: usize) -> Layout {
    // SAFETY: class sizes are non-zero multiples of 16, and 16 is a power of two.
    unsafe { Layout::from_size_align_unchecked(class_size(c), STEP) }
}

/// The current thread's lists, or None while the cache is off for it. The first call arms the exit flush;
/// allocations made while arming it bypass nothing, since the state is Live before `GUARD` is touched.
fn lists() -> Option<*mut Lists> {
    let state = STATE.try_with(|s| s.get()).ok()?;
    match state {
        State::Off => None,
        State::Live => LISTS.try_with(|l| l.get()).ok(),
        State::Fresh => {
            STATE.with(|s| s.set(State::Live));
            if GUARD.try_with(|_| ()).is_err() {
                STATE.with(|s| s.set(State::Off));
                return None;
            }
            LISTS.try_with(|l| l.get()).ok()
        }
    }
}

unsafe impl GlobalAlloc for Cached {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let Some(c) = class_of(layout) else { return unsafe { System.alloc(layout) } };
        if let Some(l) = lists() {
            // SAFETY: this thread's lists, not borrowed elsewhere (the allocator does not re-enter itself here).
            let l = unsafe { &mut *l };
            let p = l.head[c];
            if !p.is_null() {
                l.head[c] = unsafe { *(p as *mut *mut u8) };
                l.len[c] -= 1;
                return p;
            }
        }
        unsafe { System.alloc(class_layout(c)) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let Some(c) = class_of(layout) else { return unsafe { System.dealloc(ptr, layout) } };
        if let Some(l) = lists() {
            let l = unsafe { &mut *l };
            if (l.len[c] as usize) < LIST_BYTES / class_size(c) {
                unsafe { *(ptr as *mut *mut u8) = l.head[c] };
                l.head[c] = ptr;
                l.len[c] += 1;
                return;
            }
        }
        unsafe { System.dealloc(ptr, class_layout(c)) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if class_of(layout).is_none() {
            return unsafe { System.alloc_zeroed(layout) };
        }
        let p = unsafe { self.alloc(layout) };
        if !p.is_null() {
            unsafe { ptr::write_bytes(p, 0, layout.size()) };
        }
        p
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller guarantees new_size, rounded up to the alignment, does not overflow isize.
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        match (class_of(layout), class_of(new_layout)) {
            (Some(a), Some(b)) if a == b => ptr,
            (None, None) => unsafe { System.realloc(ptr, layout, new_size) },
            _ => {
                let p = unsafe { self.alloc(new_layout) };
                if !p.is_null() {
                    unsafe { ptr::copy_nonoverlapping(ptr, p, layout.size().min(new_size)) };
                    unsafe { self.dealloc(ptr, layout) };
                }
                p
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // The server's tests run on `Cached` (main.rs), so these exercise it: blocks freed on other threads, threads
    // exiting with full lists, reallocs across classes and to the heap.
    #[test]
    fn blocks_cross_threads_and_survive_exits() {
        let made: Vec<Vec<Box<[u8]>>> = (0..8u8)
            .map(|t| std::thread::spawn(move || (0..4000usize).map(|i| vec![t; 1 + i * 7 % 5000].into_boxed_slice()).collect()).join().unwrap())
            .collect();
        for (t, blocks) in made.into_iter().enumerate() {
            std::thread::spawn(move || {
                for (i, b) in blocks.into_iter().enumerate() {
                    assert_eq!(b.len(), 1 + i * 7 % 5000);
                    assert!(b.iter().all(|&x| x == t as u8));
                }
            })
            .join()
            .unwrap();
        }
        let mut v: Vec<u64> = Vec::new();
        for i in 0..2000u64 {
            v.push(i);
            if i % 7 == 0 {
                v.shrink_to_fit();
            }
        }
        assert!(v.iter().enumerate().all(|(i, &x)| x == i as u64));
        let mut s = String::new();
        for i in 0..300 {
            s.push_str(&i.to_string());
        }
        assert!(s.starts_with("0123456789101112"));
    }
}
