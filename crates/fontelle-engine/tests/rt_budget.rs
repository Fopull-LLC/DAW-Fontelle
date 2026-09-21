//! The real-time thread's CPU budget, and what happens when it is spent.
//!
//! Four core dumps from one afternoon of use (2026-09-20, 17:17 to 17:48)
//! were all `SIGXCPU` on `cpal_alsa_out`, inside a transport reset. The
//! budget is `RLIMIT_RTTIME`: the CPU time a real-time thread may spend
//! **without blocking** before the kernel signals it. `audio_thread_priority`
//! sets the soft limit to **one block's worth** — 128 frames at 48 kHz is
//! 2.7 ms — so a callback that spent three kernel ticks on a loop seam's
//! reset and a heavy block was, by the signal's default disposition, the
//! whole program dumped. Not a crash of ours: a budget nobody had read.
//!
//! Two things fix it, and both are held here. The soft limit is **widened
//! to most of rtkit's hard limit** (200 ms), which no honest callback comes
//! near; and `SIGXCPU` has a **handler** that demotes the registered
//! real-time threads to ordinary scheduling rather than ending the process
//! — an overload is then a glitch, which is what an overload is in every
//! other DAW, and the callback promotes itself again once it has behaved.

#![cfg(target_os = "linux")]

use fontelle_engine::rt_budget;

/// A hard limit to work under, the way rtkit leaves the process: 200 ms.
const HARD_US: u64 = 200_000;

fn rttime() -> (u64, u64) {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_RTTIME, &mut limit) },
        0
    );
    (limit.rlim_cur, limit.rlim_max)
}

#[test]
fn the_soft_budget_is_widened_to_most_of_the_hard_limit() {
    // The crate's own setting: one block's worth, under a 200 ms hard limit.
    let tight = libc::rlimit {
        rlim_cur: 2_667,
        rlim_max: HARD_US,
    };
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_RTTIME, &tight) }, 0);

    rt_budget::widen_budget();

    let (soft, hard) = rttime();
    assert_eq!(hard, HARD_US, "the hard limit is rtkit's and is left alone");
    assert!(
        (HARD_US * 3 / 4..HARD_US).contains(&soft),
        "the soft limit is most of the hard one, short of it so the watchdog \
         fires before the kernel's kill: {soft}"
    );
    // Widening twice is idempotent. (A process with no hard limit at all —
    // no rtkit — is left alone; a hard limit once lowered cannot be raised
    // back without privilege, so that half is not staged here.)
    rt_budget::widen_budget();
    assert_eq!(rttime().0, soft);
}

#[test]
fn the_watchdog_turns_a_spent_budget_into_a_demotion_rather_than_an_exit() {
    // Arm it, on this thread — the way the output callback does once it is
    // promoted. This thread is not real-time in a test, so the demotion
    // itself is a no-op; what is held is that the signal is *survived*
    // (its default disposition is a core dump) and that the callback is told.
    rt_budget::arm_current_thread();
    let before = rt_budget::demotions();

    unsafe { libc::raise(libc::SIGXCPU) };

    assert_eq!(
        rt_budget::demotions(),
        before + 1,
        "the handler ran, and counted the demotion for every callback to see"
    );
    // The thread is on ordinary scheduling afterwards, whatever it was.
    let policy = unsafe { libc::sched_getscheduler(0) } & !libc::SCHED_RESET_ON_FORK;
    assert_eq!(policy, libc::SCHED_OTHER);
}

/// The demotion itself, on a thread rtkit really promoted — skipped where
/// there is no rtkit to ask (CI). What is held: after the signal, the
/// promoted thread is on ordinary scheduling, and the process is here to
/// say so.
#[test]
fn a_promoted_thread_is_demoted_by_the_signal() {
    let (tell, hear) = std::sync::mpsc::channel::<Option<i32>>();
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let worker = std::thread::spawn(move || {
        let promoted = audio_thread_priority::promote_current_thread_to_real_time(128, 48_000);
        let tid = unsafe { libc::syscall(libc::SYS_gettid) } as i32;
        if promoted.is_err() {
            tell.send(None).unwrap();
            return;
        }
        rt_budget::widen_budget();
        rt_budget::arm_current_thread();
        tell.send(Some(tid)).unwrap();
        let _ = wait.recv();
        rt_budget::disarm_current_thread();
    });
    let Some(tid) = hear.recv().unwrap() else {
        eprintln!("no rtkit here; the demotion is not staged");
        let _ = release.send(());
        worker.join().unwrap();
        return;
    };
    // rtkit sets the policy with `SCHED_RESET_ON_FORK`, which reads back
    // in the same word.
    let policy = unsafe { libc::sched_getscheduler(tid) } & !libc::SCHED_RESET_ON_FORK;
    assert!(
        policy == libc::SCHED_RR || policy == libc::SCHED_FIFO,
        "rtkit made it real-time: {policy}"
    );

    unsafe { libc::kill(libc::getpid(), libc::SIGXCPU) };
    std::thread::sleep(std::time::Duration::from_millis(50));

    let policy = unsafe { libc::sched_getscheduler(tid) } & !libc::SCHED_RESET_ON_FORK;
    assert_eq!(policy, libc::SCHED_OTHER, "demoted by the watchdog");
    let _ = release.send(());
    worker.join().unwrap();
}
