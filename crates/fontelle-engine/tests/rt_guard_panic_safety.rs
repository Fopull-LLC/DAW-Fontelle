//! Its own binary (every file under `tests/` is), so it's safe to install its
//! own `#[global_allocator]` here without affecting any other test suite.
//!
//! This reproduces, silently and without any audio hardware, the exact crash
//! Ty hit running `fontelle-app --play-sf2`: an allocation on the RT-tagged
//! thread aborted the whole process (SIGABRT) instead of producing a single
//! clean panic. Root cause: `RtGuardAllocator::alloc`'s own violation-report
//! `panic!(...)` formats a message, which allocates, which re-enters `alloc`
//! while the thread is still tagged RT, which panics *again* mid-unwind —
//! Rust aborts on a double panic.

#[global_allocator]
static ALLOCATOR: fontelle_engine::RtGuardAllocator = fontelle_engine::RtGuardAllocator;

#[test]
fn allocating_on_the_rt_thread_panics_cleanly_instead_of_aborting() {
    fontelle_engine::mark_current_thread_rt();

    let result = std::panic::catch_unwind(|| {
        let leaked: Vec<u8> = Vec::with_capacity(64);
        std::hint::black_box(leaked);
    });

    assert!(
        result.is_err(),
        "allocating on the RT-tagged thread must panic (and be catchable, not abort the process)"
    );
}
