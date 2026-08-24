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
