//! The real `fontelle-asio` program, run as Fontelle runs it, against its
//! test clock — a pretend driver that calls once per buffer on a timer and
//! can be told to misbehave. No ASIO driver, no sound: this runs on any
//! machine, CI's Windows runner included, and is what proves the process
//! boundary (spawn, shared block, doorbells, the text lines, every way of
//! ending) works end to end.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use fontelle_audio_ipc::{
    Bells, ClientSide, HelperEvent, HelperLine, Layout, Region, RunArgs, State, Wake, channel_name,
};

const HELPER: &str = env!("CARGO_BIN_EXE_fontelle-asio");
const CLOCK: &str = "Fontelle test clock";

static NEXT: AtomicU32 = AtomicU32::new(1);

struct Running {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    region: Region,
    bells: Bells,
}

impl Running {
    fn next_line(&self, within: Duration) -> Option<HelperLine> {
        let until = Instant::now() + within;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => {
                    if let Some(parsed) = HelperLine::parse(&line) {
                        return Some(parsed);
                    }
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn line_where(&self, within: Duration, want: impl Fn(&HelperLine) -> bool) -> HelperLine {
        let until = Instant::now() + within;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.next_line(left) {
                Some(line) if want(&line) => return line,
                Some(_) => continue,
                None => panic!("no such line from the helper within {within:?}"),
            }
        }
    }

    fn wait_exit(&mut self, within: Duration) -> Option<i32> {
        let until = Instant::now() + within;
        while Instant::now() < until {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        None
    }
}

fn spawn(rate: u32, frames: Option<u32>, env: &[(&str, &str)]) -> Running {
    let channel = channel_name(std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let region = Region::create(&channel, Layout::new(8192, 0, 2).unwrap()).unwrap();
    let bells = Bells::create(&channel).unwrap();
    let args = RunArgs {
        driver: CLOCK.to_string(),
        channel,
        rate,
        frames,
        inputs: 0,
        outputs: 2,
        parent: std::process::id(),
    };
    let mut command = Command::new(HELPER);
    command
        .args(args.to_args())
        .env("FONTELLE_ASIO_TEST_CLOCK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    for (k, v) in env {
        command.env(k, v);
    }
    let mut child = command.spawn().expect("the helper runs");
    let stdin = child.stdin.take();
    let stdout = child.stdout.take().unwrap();
    let (tx, lines) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Running {
        child,
        stdin,
        lines,
        region,
        bells,
    }
}

/// Renders `cycles` buffers (a sine), calling `slow` on each to decide how
/// long to dawdle first. Returns the frames each cycle had.
fn serve(run: &Running, cycles: usize, slow: impl Fn(usize) -> Duration) -> Vec<usize> {
    let shared = run.region.shared();
    let mut client = ClientSide::new(shared, &*run.bells.request, &*run.bells.reply);
    let mut frames = Vec::new();
    let mut phase = 0.0f32;
    while frames.len() < cycles {
        match client.wait(Duration::from_secs(5)) {
            Wake::Cycle(cycle) => {
                let started = Instant::now();
                std::thread::sleep(slow(frames.len()));
                let (_, out) = client.buffers(cycle);
                for frame in out.chunks_mut(2) {
                    phase += 440.0 / 48_000.0;
                    let s = (phase * std::f32::consts::TAU).sin() * 0.1;
                    frame[0] = s;
                    frame[1] = s;
                }
                frames.push(cycle.frames);
                client.answer(cycle, started.elapsed().as_nanos() as u32);
            }
            Wake::Stopped => panic!("the helper stopped after {} cycles", frames.len()),
            Wake::Idle => panic!("the helper went quiet after {} cycles", frames.len()),
        }
    }
    frames
}

fn hello_and_ready(run: &Running) -> fontelle_audio_ipc::ReadyInfo {
    assert_eq!(
        run.next_line(Duration::from_secs(10)),
        Some(HelperLine::Hello { version: 1 })
    );
    match run.line_where(Duration::from_secs(10), |l| {
        matches!(l, HelperLine::Ready(_) | HelperLine::Error(_))
    }) {
        HelperLine::Ready(ready) => ready,
        other => panic!("{other:?}"),
    }
}

#[test]
fn it_says_its_protocol_and_lists_its_drivers() {
    let out = Command::new(HELPER).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).lines().next(),
        Some("fontelle-asio\t1")
    );

    let out = Command::new(HELPER)
        .arg("list")
        .env("FONTELLE_ASIO_TEST_CLOCK", "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<_> = text.lines().filter_map(HelperLine::parse).collect();
    assert_eq!(lines[0], HelperLine::Hello { version: 1 });
    assert!(lines.contains(&HelperLine::Driver {
        name: CLOCK.to_string()
    }));

    // Without the switch the clock is not offered: it is for tests.
    let out = Command::new(HELPER)
        .arg("list")
        .env_remove("FONTELLE_ASIO_TEST_CLOCK")
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains(CLOCK));

    let out = Command::new(HELPER).arg("dance").output().unwrap();
    assert!(!out.status.success(), "an unknown command is an error");
}

#[test]
fn a_run_exchanges_every_buffer_on_time_and_stops_when_told() {
    let mut run = spawn(48_000, Some(256), &[]);
    let ready = hello_and_ready(&run);
    assert_eq!(ready.driver, CLOCK);
    assert_eq!(ready.frames, 256);
    assert_eq!(ready.rate, 48_000);
    assert_eq!(ready.outputs, 2);
    assert_eq!(ready.format, "Float32LSB");
    let shared = run.region.shared();
    let h = shared.header();
    assert_eq!(h.buffer_frames.load(Ordering::Relaxed), 256);
    assert_eq!(h.sample_rate.load(Ordering::Relaxed), 48_000);
    assert_eq!(h.helper_pid.load(Ordering::Relaxed), run.child.id());

    let mut handoffs = Vec::new();
    let frames = serve(&run, 300, |_| Duration::ZERO);
    assert!(frames.iter().all(|f| *f == 256), "{frames:?}");
    for _ in 0..50 {
        let f = serve(&run, 1, |_| Duration::ZERO);
        assert_eq!(f, [256]);
        handoffs.push(h.handoff_last_ns.load(Ordering::Relaxed));
    }
    handoffs.sort_unstable();
    let counters = shared.counters();
    eprintln!(
        "handoff across processes (helper's measure, excluding rendering): median {} ns, \
         worst of 50 {} ns, worst since start {} ns",
        handoffs[25], handoffs[49], counters.handoff_max_ns
    );
    // Nothing is claimed about lateness here: the test runs at the lowest
    // priority on a shared machine, not on an audio thread. The handoff's
    // cost is printed; lateness has a test of its own.
    eprintln!("{counters:?}");
    assert!(counters.cycles >= 350);

    writeln!(run.stdin.as_mut().unwrap(), "stop").unwrap();
    run.line_where(Duration::from_secs(5), |l| matches!(l, HelperLine::Bye(_)));
    assert_eq!(run.region.shared().state(), Some(State::HelperStopped));
    assert_eq!(run.wait_exit(Duration::from_secs(5)), Some(0));
}

#[test]
fn a_fontelle_that_vanishes_takes_the_helper_with_it() {
    let mut run = spawn(48_000, Some(128), &[]);
    hello_and_ready(&run);
    serve(&run, 20, |_| Duration::ZERO);
    // Fontelle's end of the pipe closes — what a crash does — with no word
    // in the shared block.
    run.stdin = None;
    assert_eq!(run.wait_exit(Duration::from_secs(5)), Some(0));
}

#[test]
fn a_fontelle_that_closes_the_block_stops_the_helper() {
    let mut run = spawn(48_000, Some(128), &[]);
    hello_and_ready(&run);
    serve(&run, 20, |_| Duration::ZERO);
    ClientSide::new(run.region.shared(), &*run.bells.request, &*run.bells.reply).close();
    run.line_where(Duration::from_secs(5), |l| matches!(l, HelperLine::Bye(_)));
    assert_eq!(run.wait_exit(Duration::from_secs(5)), Some(0));
}

#[test]
fn a_late_fontelle_is_counted_as_late_and_not_as_the_device() {
    // Buffers long enough (21 ms) that a busy test machine's own jitter is
    // not what is being counted.
    let mut run = spawn(48_000, Some(1024), &[]);
    hello_and_ready(&run);
    // Three buffers take 60 ms each.
    serve(&run, 50, |n| {
        if n % 15 == 5 {
            Duration::from_millis(60)
        } else {
            Duration::ZERO
        }
    });
    let counters = run.region.shared().counters();
    // At least the three slow ones; a loaded machine may add more, and may
    // make the clock itself late, so neither count is held to exactly.
    assert!(counters.late_replies >= 3, "{counters:?}");
    eprintln!("{counters:?}");
    run.stdin = None;
    run.wait_exit(Duration::from_secs(5));
}

#[test]
fn a_device_that_skips_buffers_is_counted_as_the_device() {
    let mut run = spawn(
        48_000,
        Some(1024),
        &[("FONTELLE_ASIO_TEST_CLOCK_SKIP_EVERY", "10")],
    );
    hello_and_ready(&run);
    serve(&run, 45, |_| Duration::ZERO);
    let counters = run.region.shared().counters();
    // Four skips of two buffers each — at least one each counted, since a
    // busy machine can make the call before a skip late and the gap shorter.
    assert!(counters.device_xruns >= 4, "{counters:?}");
    eprintln!("{counters:?}");
    run.stdin = None;
    run.wait_exit(Duration::from_secs(5));
}

#[test]
fn a_driver_reset_is_done_by_the_helper_and_said_and_the_new_size_just_arrives() {
    let mut run = spawn(
        48_000,
        Some(1024),
        &[
            ("FONTELLE_ASIO_TEST_CLOCK_RESET_AFTER", "10"),
            ("FONTELLE_ASIO_TEST_CLOCK_RESET_FRAMES", "2048"),
        ],
    );
    hello_and_ready(&run);
    let frames = serve(&run, 25, |_| Duration::ZERO);
    assert_eq!(frames[0], 1024);
    assert_eq!(*frames.last().unwrap(), 2048, "{frames:?}");
    run.line_where(Duration::from_secs(5), |l| {
        *l == HelperLine::Event(HelperEvent::Reset)
    });
    run.line_where(Duration::from_secs(5), |l| {
        *l == HelperLine::Event(HelperEvent::BufferChanged(2048))
    });
    let shared = run.region.shared();
    assert_eq!(shared.header().buffer_frames.load(Ordering::Relaxed), 2048);
    assert_eq!(shared.counters().resets, 1);
    // That the pause around a reset is not counted as the device's is
    // `GapDetector::reset`'s test (fontelle-audio-ipc); timing it here
    // would be timing the test machine.
    run.stdin = None;
    run.wait_exit(Duration::from_secs(5));
}

#[test]
fn a_device_that_stops_calling_is_said_and_the_helper_stops() {
    let mut run = spawn(
        48_000,
        Some(256),
        &[("FONTELLE_ASIO_TEST_CLOCK_STOP_AFTER", "40")],
    );
    hello_and_ready(&run);
    // Fewer than the 40 the clock gives: a busy machine can make two
    // buffers arrive as one.
    serve(&run, 30, |_| Duration::ZERO);
    let mut client = ClientSide::new(run.region.shared(), &*run.bells.request, &*run.bells.reply);
    let started = Instant::now();
    loop {
        match client.wait(Duration::from_secs(10)) {
            Wake::Stopped => break,
            Wake::Idle => panic!("never stopped"),
            Wake::Cycle(c) => client.answer(c, 0),
        }
    }
    assert!(started.elapsed() < Duration::from_secs(6));
    match run.line_where(Duration::from_secs(2), |l| {
        matches!(l, HelperLine::Error(_))
    }) {
        HelperLine::Error(words) => assert!(words.contains("stopped"), "{words}"),
        _ => unreachable!(),
    }
    run.line_where(Duration::from_secs(2), |l| matches!(l, HelperLine::Bye(_)));
    assert_eq!(run.wait_exit(Duration::from_secs(5)), Some(0));
}

#[test]
fn a_rate_the_driver_will_not_take_is_refused_in_words() {
    let mut run = spawn(12_345, Some(256), &[]);
    assert_eq!(
        run.next_line(Duration::from_secs(10)),
        Some(HelperLine::Hello { version: 1 })
    );
    match run.line_where(Duration::from_secs(10), |l| {
        matches!(l, HelperLine::Ready(_) | HelperLine::Error(_))
    }) {
        HelperLine::Error(words) => {
            assert!(words.contains("cannot run at 12.3 kHz"), "{words}");
            assert!(words.contains("48 kHz"), "offers what it can: {words}");
        }
        other => panic!("{other:?}"),
    }
    let code = run.wait_exit(Duration::from_secs(5));
    assert!(matches!(code, Some(c) if c != 0), "{code:?}");
    assert_eq!(run.region.shared().state(), Some(State::HelperStopped));
}
