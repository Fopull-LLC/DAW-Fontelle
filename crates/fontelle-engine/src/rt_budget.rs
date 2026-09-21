//! The real-time thread's CPU budget, and the watchdog that spends it
//! gracefully.
//!
//! A thread rtkit has made real-time lives under `RLIMIT_RTTIME`: the CPU
//! time it may use **without blocking** before the kernel sends `SIGXCPU`,
//! and — at the hard limit, 200 ms as rtkit sets it — `SIGKILL`.
//! `audio_thread_priority` sets the *soft* limit to one block's worth
//! (128 frames at 48 kHz: 2.7 ms), and `SIGXCPU`'s default disposition is
//! a core dump. So a callback that spent three ticks on a loop seam's
//! reset and a heavy block ended the whole program: four dumps in one
//! afternoon of use (2026-09-20), each in a reset, none a fault of ours.
//! The old story about `malloc_trim` on the input thread (`pipewire.rs`)
//! was this too — the trim only had to cross the same three ticks.
//!
//! Two things, both cheap:
//!
//! - [`widen_budget`] raises the soft limit to most of the hard one. An
//!   honest callback never comes near 150 ms; a runaway one still meets
//!   the watchdog before the kernel's kill.
//! - [`arm_current_thread`] installs a `SIGXCPU` handler that **demotes**
//!   every registered real-time thread to ordinary scheduling. An overload
//!   is then a glitch, which is what an overload is in every other DAW.
//!   The signal is process-directed, so the handler cannot know which
//!   thread overran and demotes them all; each callback watches
//!   [`demotions`] and promotes itself again once it has behaved.
//!
//! Everything the handler does is a syscall or an atomic — the only things
//! a signal handler may do.

use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

/// How many real-time threads the watchdog can hold: the output callback,
/// the capture thread, and room for a device that spawns its own.
const SLOTS: usize = 8;

/// The kernel thread ids of the promoted threads; zero is an empty slot.
static THREADS: [AtomicI32; SLOTS] = [const { AtomicI32::new(0) }; SLOTS];
/// How many times the handler has fired — a count rather than a flag, so
/// every promoted thread sees each demotion whichever of them reads first.
static DEMOTIONS: AtomicU64 = AtomicU64::new(0);
static ARMED: std::sync::Once = std::sync::Once::new();

/// Widens the soft `RLIMIT_RTTIME` to three quarters of the hard limit —
/// leaving the last quarter for the watchdog to fire in before the kernel's
/// own kill. A process with no hard limit (no rtkit) is left as it is.
pub fn widen_budget() {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: a plain query into a struct we own.
    if unsafe { libc::getrlimit(libc::RLIMIT_RTTIME, &mut limit) } != 0 {
        return;
    }
    if limit.rlim_max == libc::RLIM_INFINITY {
        return;
    }
    let wanted = limit.rlim_max / 4 * 3;
    if limit.rlim_cur == wanted {
        return;
    }
    limit.rlim_cur = wanted;
    // SAFETY: raising a soft limit to below its hard limit needs no privilege.
    unsafe { libc::setrlimit(libc::RLIMIT_RTTIME, &limit) };
}

/// Registers the current thread with the watchdog and installs the
/// `SIGXCPU` handler the first time. Called once a thread has been promoted.
pub fn arm_current_thread() {
    // SAFETY: gettid has no preconditions.
    let tid = unsafe { libc::syscall(libc::SYS_gettid) } as i32;
    if !THREADS
        .iter()
        .any(|slot| slot.load(Ordering::Relaxed) == tid)
    {
        for slot in &THREADS {
            if slot
                .compare_exchange(0, tid, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }
    ARMED.call_once(|| {
        // SAFETY: `sigaction` with a handler that only makes syscalls and
        // touches atomics; `SA_RESTART` so a blocking read on another thread
        // is not broken by a signal meant for the audio thread.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_sigxcpu as *const () as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(libc::SIGXCPU, &action, std::ptr::null_mut());
        }
    });
}

/// Forgets the current thread — a callback thread on its way out.
pub fn disarm_current_thread() {
    // SAFETY: gettid has no preconditions.
    let tid = unsafe { libc::syscall(libc::SYS_gettid) } as i32;
    for slot in &THREADS {
        let _ = slot.compare_exchange(tid, 0, Ordering::AcqRel, Ordering::Relaxed);
    }
}

/// How many times the watchdog has fired. A callback that sees the count
/// move is on ordinary scheduling and may promote itself again.
pub fn demotions() -> u64 {
    DEMOTIONS.load(Ordering::Acquire)
}

extern "C" fn on_sigxcpu(_signal: libc::c_int) {
    // Every registered thread, since the signal does not say which one
    // overran. Demoting a thread that is already ordinary is a no-op.
    let param = libc::sched_param { sched_priority: 0 };
    for slot in &THREADS {
        let tid = slot.load(Ordering::Relaxed);
        if tid != 0 {
            // With `SCHED_RESET_ON_FORK` kept: rtkit set it, and the kernel
            // refuses an unprivileged change that would clear it (EPERM,
            // measured — the demotion silently did nothing without it).
            // SAFETY: a syscall on a thread id; async-signal-safe.
            unsafe {
                libc::sched_setscheduler(tid, libc::SCHED_OTHER | libc::SCHED_RESET_ON_FORK, &param)
            };
        }
    }
    DEMOTIONS.fetch_add(1, Ordering::AcqRel);
}
