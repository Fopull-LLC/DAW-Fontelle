//! Playing through ASIO by way of the separate `fontelle-asio` program —
//! here against its test clock, which stands in for a driver, so it runs on
//! any machine: the real program spawned, the real shared block, the real
//! audio callback with the allocator that panics on an allocation in it
//! (INVARIANT 1).
//!
//! One test, run in order, because the helper is found and told about its
//! test clock through the environment, which is the process's.

// Where the shared block exists: Windows, and Linux for development.
#![cfg(any(windows, target_os = "linux"))]

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fontelle_engine::{
    ASIO_HELPER_HOST, AudioOutput, BufferPool, CompiledGraph, MixerTrackNode, OutputChoice,
    RtGuardAllocator, ScheduledNode, Transport, output_device_names, output_host_names,
};
use fontelle_types::{CompiledTimeline, NodeId};
use slotmap::Key;

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

const CLOCK: &str = "Fontelle test clock";

fn silent_graph() -> CompiledGraph {
    CompiledGraph {
        schedule: vec![ScheduledNode {
            id: NodeId::null(),
            node: Box::new(MixerTrackNode::new()),
            input_buffers: vec![0, 1],
            output_buffers: vec![0, 1],
        }],
        buffer_pool: BufferPool::with_capacity(2, fontelle_engine::BLOCK_SIZE),
    }
}

/// The helper program, built into this test's own target folder: `cargo
/// test -p fontelle-engine` on its own does not build another package's
/// program. (Fontelle never links it: it is GPL, and a program apart.)
fn helper_program() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().unwrap().parent().unwrap().to_path_buf();
    let name = if cfg!(windows) {
        "fontelle-asio.exe"
    } else {
        "fontelle-asio"
    };
    if let Ok(cargo) = std::env::var("CARGO") {
        let up = profile.parent().unwrap();
        // `target/<triple>/debug` when built for a named target.
        let triple = up
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| n.matches('-').count() >= 2)
            .map(str::to_string);
        let target_dir = if triple.is_some() {
            up.parent().unwrap()
        } else {
            up
        };
        let mut build = Command::new(cargo);
        build
            .args([
                "build",
                "-q",
                "-p",
                "fontelle-asio-host",
                "--bin",
                "fontelle-asio",
            ])
            .env("CARGO_TARGET_DIR", target_dir);
        if let Some(triple) = &triple {
            build.args(["--target", triple]);
        }
        if profile.file_name().is_some_and(|n| n == "release") {
            build.arg("--release");
        }
        let built = build.status();
        assert!(
            built.is_ok_and(|s| s.success()) || profile.join(name).exists(),
            "fontelle-asio would not build"
        );
    }
    let path = profile.join(name);
    assert!(path.exists(), "{} missing", path.display());
    path
}

fn set_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
    // SAFETY: this test binary has one test, and nothing else in it reads
    // the environment while this runs.
    unsafe { std::env::set_var(key, value) };
}

fn remove_env(key: &str) {
    // SAFETY: as `set_env`.
    unsafe { std::env::remove_var(key) };
}

fn kill(pid: u32) {
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .status();
    #[cfg(not(windows))]
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

fn until(within: Duration, mut what: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + within;
    while Instant::now() < end {
        if what() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    what()
}

#[test]
fn fontelle_plays_through_the_asio_helper_and_carries_on_when_it_goes() {
    // Not there: not offered.
    set_env("FONTELLE_ASIO_HELPER", "/no/such/fontelle-asio");
    assert!(
        !output_host_names().iter().any(|h| h == ASIO_HELPER_HOST),
        "offered with no helper beside Fontelle"
    );

    // There: offered, with its drivers.
    let helper = helper_program();
    set_env("FONTELLE_ASIO_HELPER", &helper);
    set_env("FONTELLE_ASIO_TEST_CLOCK", "1");
    let hosts = output_host_names();
    assert!(hosts.iter().any(|h| h == ASIO_HELPER_HOST), "{hosts:?}");
    assert_eq!(output_device_names(Some(ASIO_HELPER_HOST)), [CLOCK]);

    // Opened, and the song plays through it.
    let transport = Arc::new(Transport::new());
    transport.play();
    let choice = OutputChoice {
        host: Some(ASIO_HELPER_HOST.to_string()),
        device: Some(CLOCK.to_string()),
        buffer_frames: Some(256),
    };
    let mut output = AudioOutput::start(
        fontelle_engine::graph_channel(silent_graph()).1,
        fontelle_engine::timeline_channel(CompiledTimeline::empty()).1,
        48_000,
        Arc::clone(&transport),
        None,
        None,
        &choice,
    )
    .expect("the helper opens its test clock");
    let status = output.status().expect("open");
    assert_eq!(status.host, ASIO_HELPER_HOST);
    assert_eq!(status.device, CLOCK);
    assert_eq!(status.buffer_frames, Some(256));
    assert_eq!(status.latencies, Some((256, 256)));
    assert_eq!(status.fell_back, None);
    let said = status.to_string();
    assert!(said.contains("in 5.3 ms, out 5.3 ms"), "{said}");
    assert!(
        until(Duration::from_secs(3), || transport.position_sample()
            > 4_800),
        "the song moved: {}",
        transport.position_sample()
    );
    assert!(output.helper_pid().is_some());
    assert!(
        until(Duration::from_secs(2), || output
            .stats()
            .health()
            .handoff_ns
            .is_some()),
        "the helper's handoff is read back"
    );

    // The driver resets and comes back with another buffer: the stream
    // carries on and the status says the new size.
    set_env("FONTELLE_ASIO_TEST_CLOCK_RESET_AFTER", "20");
    // The clock prefers 512 until the reset; a preferred size that moved is
    // the person's choice in the driver's panel, so it is taken.
    set_env("FONTELLE_ASIO_TEST_CLOCK_RESET_FRAMES", "1024");
    let status = output.reopen(&choice).expect("opens again");
    assert_eq!(status.buffer_frames, Some(256));
    assert!(
        until(Duration::from_secs(5), || output
            .status()
            .is_some_and(|s| s.buffer_frames == Some(1024))),
        "{:?}",
        output.status()
    );
    let before = transport.position_sample();
    assert!(
        until(Duration::from_secs(3), || transport.position_sample()
            > before + 4_800),
        "still playing after the reset"
    );
    remove_env("FONTELLE_ASIO_TEST_CLOCK_RESET_AFTER");
    remove_env("FONTELLE_ASIO_TEST_CLOCK_RESET_FRAMES");
    assert!(!output.stats().lost(), "a reset is not a loss");

    // The driver's own panel, asked for through the running helper: the
    // test clock has none, and says so — once, as a note, while it plays on.
    assert_eq!(output.open_driver_panel(), Ok(()));
    let mut note = None;
    assert!(
        until(Duration::from_secs(3), || {
            note = note.take().or_else(|| output.take_note());
            note.is_some()
        }),
        "the helper's word about the panel reaches the window"
    );
    let note = note.unwrap();
    assert!(note.contains("no control panel"), "{note}");
    assert_eq!(output.take_note(), None, "said once");
    assert!(!output.stats().lost(), "and it plays on");

    // The helper dies: noticed, said, and the next open does not go back
    // to it on its own.
    let pid = output.helper_pid().unwrap();
    kill(pid);
    assert!(
        until(Duration::from_secs(5), || output.stats().lost()),
        "a dead helper is noticed"
    );
    let why = output.loss_reason().expect("a reason");
    assert!(why.contains("fontelle-asio"), "{why}");
    assert!(
        why.contains("unexpectedly") && !why.contains("panel"),
        "a crash is not blamed on the panel the driver lacked: {why}"
    );
    match output.reopen_after_loss(&choice) {
        Ok(status) => {
            assert_ne!(status.host, ASIO_HELPER_HOST, "{status:?}");
            let fell = status.fell_back.expect("said");
            assert!(fell.contains("fontelle-asio"), "{fell}");
        }
        // CI's Linux runners have no sound card: nothing else opens either.
        Err(e) => assert!(e.0.contains("fontelle-asio"), "{e:?}"),
    }

    // Chosen again on purpose, it is tried again.
    let status = output.reopen(&choice).expect("the helper opens again");
    assert_eq!(status.host, ASIO_HELPER_HOST);
    let pid = output.helper_pid().unwrap();
    output.close();
    assert!(output.status().is_none());
    assert!(output.helper_pid().is_none(), "closed with the stream");
    #[cfg(target_os = "linux")]
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "the helper exited and was reaped"
    );
    #[cfg(not(target_os = "linux"))]
    let _ = pid;
}
