use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static IS_RT_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Call once from inside the audio callback (or any worker in the RT pool, §5.4)
/// before it starts touching the graph. Every call the thread makes through the
/// global allocator is checked against this flag from then on.
pub fn mark_current_thread_rt() {
    IS_RT_THREAD.with(|f| f.set(true));
}

pub fn current_thread_is_rt() -> bool {
    IS_RT_THREAD.with(|f| f.get())
}

/// The inverse of `mark_current_thread_rt`. **There is no production call site
/// for this yet** — dropping heap-owning audio-graph state (a `CompiledGraph`,
/// its `Patch`es, their sample buffers) safely on RT-thread teardown needs a
/// deferred-drop / trash-bin mechanism (hand the old graph to a channel a
/// non-RT thread actually drops), which isn't built. This exists so tests can
/// scope the RT-tagged region honestly — clear it before letting RT-owned
/// state fall out of scope, the way real playback never drops it mid-stream.
/// See `PROGRESS.md`.
pub fn unmark_current_thread_rt() {
    IS_RT_THREAD.with(|f| f.set(false));
}

/// INVARIANT 1 enforcement (TDD §20.4): in debug/test builds, panics if the RT
/// thread allocates, reallocates, or deallocates. Release builds fall straight
/// through to the system allocator with no overhead. Install it as the app
/// binary's `#[global_allocator]` — see `fontelle-app/src/main.rs`.
pub struct RtGuardAllocator;

#[cfg(debug_assertions)]
fn assert_not_rt(op: &str, layout: Layout) {
    if current_thread_is_rt() {
        // Panicking allocates (formatting this message, unwinding, an
        // optional backtrace) — all of it would otherwise re-enter this exact
        // check while still flagged RT, panic again mid-unwind, and abort the
        // whole process before anyone sees why. Un-flag first: by this point
        // INVARIANT 1 is already violated and the RT thread is not staying
        // real-time-safe regardless, so it's more useful to let this one
        // report cleanly than to preserve the flag for a report that never
        // arrives.
        IS_RT_THREAD.with(|f| f.set(false));
        panic!(
            "INVARIANT 1 violated: {op} on the RT thread (size={}, align={})",
            layout.size(),
            layout.align()
        );
    }
}

/// Runs `f` with the current thread tagged RT, and **always** clears the tag
/// afterward — including when `f` unwinds.
///
/// This scoping is the point, not a convenience. The audio backend owns the
/// callback thread and does its own work on it *between* our invocations:
/// cpal's ALSA worker, for one, drops its `StreamWorkerContext` (a
/// `Box<[pollfd]>`) on that thread as the worker exits. That deallocation is
/// legitimate and none of our business, but a tag left set after our callback
/// body returns turns it into a spurious INVARIANT 1 violation at stream
/// teardown — which is exactly what it did, and what cost a long time to
/// track down (see `PROGRESS.md`). INVARIANT 1 is about *our* processing, so
/// the tag lives exactly as long as our processing does.
pub fn with_rt_thread<R>(f: impl FnOnce() -> R) -> R {
    struct ClearOnDrop;
    impl Drop for ClearOnDrop {
        fn drop(&mut self) {
            unmark_current_thread_rt();
        }
    }

    // Constructed before the tag is set, so it runs on every exit path —
    // normal return or unwind.
    let _clear = ClearOnDrop;
    mark_current_thread_rt();
    f()
}

unsafe impl GlobalAlloc for RtGuardAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        #[cfg(debug_assertions)]
        assert_not_rt("alloc", layout);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        #[cfg(debug_assertions)]
        assert_not_rt("dealloc", layout);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        #[cfg(debug_assertions)]
        assert_not_rt("realloc", layout);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
