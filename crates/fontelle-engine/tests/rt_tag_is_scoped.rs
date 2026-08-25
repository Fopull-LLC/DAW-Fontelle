//! The RT tag must not outlive the callback body it belongs to.
//!
//! Regression test for a real, long-unexplained bug: `AudioDevice`'s callback
//! tagged the thread RT and never cleared it, so when cpal's ALSA worker
//! exited it dropped its own `StreamWorkerContext` — a `Box<[pollfd]>`,
//! `size=16, align=4` — on that still-tagged thread, tripping INVARIANT 1 at
//! stream teardown. It presented as a mysterious per-block allocation for
//! weeks because the panic surfaced near the end of a short run (see
//! `PROGRESS.md`); the fix is to scope the tag to exactly our own processing,
//! which is all INVARIANT 1 ever meant to police.

use fontelle_engine::{current_thread_is_rt, with_rt_thread};

#[test]
fn the_tag_is_set_inside_and_cleared_after() {
    assert!(!current_thread_is_rt(), "starts untagged");

    let seen_inside = with_rt_thread(current_thread_is_rt);

    assert!(seen_inside, "the body must run tagged");
    assert!(
        !current_thread_is_rt(),
        "the tag must not survive the body returning — this is what let cpal's \
         worker-thread teardown trip INVARIANT 1"
    );
}

#[test]
fn the_tag_is_cleared_even_when_the_body_panics() {
    let result = std::panic::catch_unwind(|| {
        with_rt_thread(|| {
            assert!(current_thread_is_rt());
            panic!("deliberate");
        })
    });

    assert!(
        result.is_err(),
        "the panic must propagate, not be swallowed"
    );
    assert!(
        !current_thread_is_rt(),
        "an unwinding body must still clear the tag, or every later allocation \
         on this thread reports a false violation"
    );
}

#[test]
fn the_body_can_return_a_value() {
    assert_eq!(with_rt_thread(|| 6 * 7), 42);
}

#[test]
fn nesting_leaves_the_thread_untagged_at_the_end() {
    with_rt_thread(|| {
        assert!(current_thread_is_rt());
        with_rt_thread(|| assert!(current_thread_is_rt()));
        // The inner scope clearing the tag is acceptable — real callbacks
        // never nest — but the outer scope must still leave it clear.
    });
    assert!(!current_thread_is_rt());
}
