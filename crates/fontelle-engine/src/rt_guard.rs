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

/// INVARIANT 1 enforcement (TDD §20.4): in debug/test builds, panics if the RT
/// thread allocates, reallocates, or deallocates. Release builds fall straight
/// through to the system allocator with no overhead. Install it as the app
/// binary's `#[global_allocator]` — see `fontelle-app/src/main.rs`.
pub struct RtGuardAllocator;

#[cfg(debug_assertions)]
fn assert_not_rt(op: &str, layout: Layout) {
    if current_thread_is_rt() {
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
