//! Closing an input stream without taking the program down with it
//! (TDD §15.4, INVARIANT 1).
//!
//! Reported from using the window:
//!
//! > *"if you try changing the input it often just crashed for me when i set
//! > it to no input briefly to try and change it back"*
//!
//! The core dump said exactly what it was: `SIGXCPU` on the thread named
//! `fontelle-input`, inside `snd_pcm_close` → `pw_stream_destroy` →
//! `malloc_trim`. The capture thread runs at real-time priority, and the
//! kernel gives a real-time thread a budget of CPU time (`RLIMIT_RTTIME`,
//! 200 ms as rtkit sets it) that it may burn without blocking. Reading a
//! period at a time never comes near it. Closing the stream does: PipeWire's
//! teardown trims the whole heap, and on a process that has scanned a
//! thousand plugins that is more than the budget — so the kernel kills the
//! **process**. Not a panic, not a message, just a DAW that vanished.
//!
//! So the rule these tests hold: **the capture thread never closes what it
//! captured from.** It hands the stream back and exits, and the stream is
//! closed on a thread with no budget to blow.
//!
//! The second thing here is smaller and was found by the same report: a
//! device dropped without `stop_input` left the monitor ring saying a stream
//! was open, and the idle gate kept the graph running for it forever.

use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use fontelle_engine::{AudioDevice, InputMonitor, drop_off_thread};

/// Something whose drop writes down which thread it happened on.
struct Witness(Arc<Mutex<Option<ThreadId>>>);

impl Drop for Witness {
    fn drop(&mut self) {
        *self.0.lock().unwrap() = Some(std::thread::current().id());
    }
}

#[test]
fn a_value_dropped_off_thread_is_dropped_on_a_thread_of_its_own() {
    let seen = Arc::new(Mutex::new(None));
    let witness = Witness(Arc::clone(&seen));

    let closer = drop_off_thread(witness);
    closer.join().expect("the closing thread must finish");

    let dropped_on = seen.lock().unwrap().expect("the drop must have happened");
    assert_ne!(
        dropped_on,
        std::thread::current().id(),
        "the whole point is that the caller does not pay for the drop"
    );
}

#[test]
fn the_closing_thread_is_not_the_capture_thread() {
    // The capture thread is named so a core dump can name it; the thread
    // that closes must not be it, or the budget it blows is the same one.
    let seen = Arc::new(Mutex::new(None));
    let name = Arc::new(Mutex::new(None));
    struct Named {
        name: Arc<Mutex<Option<String>>>,
        _witness: Witness,
    }
    impl Drop for Named {
        fn drop(&mut self) {
            *self.name.lock().unwrap() = std::thread::current().name().map(str::to_string);
        }
    }
    let named = Named {
        name: Arc::clone(&name),
        _witness: Witness(Arc::clone(&seen)),
    };

    drop_off_thread(named).join().unwrap();

    let dropped_on = name.lock().unwrap().clone();
    assert_ne!(
        dropped_on.as_deref(),
        Some("fontelle-input"),
        "closed on the capture thread is the crash this exists to prevent"
    );
}

#[test]
fn a_device_that_goes_takes_the_monitor_with_it() {
    let monitor = Arc::new(InputMonitor::new(4_096));
    let device = AudioDevice::default_host().with_monitor(Arc::clone(&monitor));
    // What the input stream does the moment it opens.
    monitor.open(48_000, 2);
    assert!(monitor.is_live());

    drop(device);

    assert!(
        !monitor.is_live(),
        "a monitor that says a stream is open after its device is gone keeps \
         the graph awake for a microphone nobody can hear"
    );
}

/// The real thing, on a machine that has one. Opened and closed a few times
/// in a row, the way choosing another input and choosing back does it —
/// each close has to come back promptly and leave the process standing.
///
/// A machine with no PipeWire source has nothing to open and passes; this is
/// a smoke test of the path, not the proof (the proof is the two above and
/// the core dump the module note quotes).
#[cfg(target_os = "linux")]
#[test]
fn a_pipewire_input_survives_being_opened_and_closed_over_and_over() {
    use std::time::{Duration, Instant};
    let sources = fontelle_engine::pipewire_sources();
    let Some(source) = sources.first() else {
        return;
    };
    let monitor = Arc::new(InputMonitor::new(96_000 * 2));
    for _ in 0..3 {
        let (writer, _reader) = fontelle_engine::input_capture_channel(96_000);
        let Ok((input, _rate, _channels)) = fontelle_engine::PipeWireInput::open(
            &source.node,
            source.channels,
            48_000,
            writer,
            Some(Arc::clone(&monitor)),
        ) else {
            // A source PipeWire lists but will not open — busy, gone, or a
            // sandbox — is not this test's subject.
            return;
        };
        std::thread::sleep(Duration::from_millis(100));
        let started = Instant::now();
        drop(input);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "closing an input waited {:?}; the capture thread hands the stream \
             back within a period",
            started.elapsed()
        );
    }
}
