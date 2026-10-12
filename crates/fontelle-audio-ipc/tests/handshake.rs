//! The per-buffer handshake as a state machine, with the helper and Fontelle
//! as two threads over one block — the same code the two processes run,
//! minus the process boundary (`crates/fontelle-asio-host/tests/` crosses
//! it).

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use fontelle_audio_ipc::{
    Bells, ClientSide, Cycle, Doorbell, Exchange, HelperSide, Layout, PollBell, Region, State,
    Wake, channel_name,
};

static NEXT: AtomicU32 = AtomicU32::new(100);

/// The bells to test with: the platform's own, and the polling fallback.
fn bell_kinds() -> Vec<(&'static str, Bells)> {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    vec![
        (
            "native",
            Bells::create(&channel_name(std::process::id(), n)).expect("bells"),
        ),
        (
            "poll",
            Bells {
                request: Box::new(PollBell),
                reply: Box::new(PollBell),
            },
        ),
    ]
}

const LONG: Duration = Duration::from_secs(5);

#[test]
fn every_buffer_goes_round_once_and_comes_back_rendered() {
    for (kind, bells) in bell_kinds() {
        let region = Region::heap(Layout::new(256, 1, 2).unwrap());
        let shared = region.shared();
        shared.set_state(State::Running);
        let request: &dyn Doorbell = &*bells.request;
        let reply: &dyn Doorbell = &*bells.reply;

        std::thread::scope(|s| {
            s.spawn(|| {
                let mut client = ClientSide::new(shared, request, reply);
                let mut seen = Vec::new();
                loop {
                    match client.wait(LONG) {
                        Wake::Cycle(cycle) => {
                            let (input, output) = client.buffers(cycle);
                            assert_eq!(input.len(), cycle.frames);
                            assert_eq!(output.len(), cycle.frames * 2);
                            // Output = input + the cycle number, both channels.
                            for (i, frame) in output.chunks_mut(2).enumerate() {
                                frame[0] = input[i] + cycle.seq as f32;
                                frame[1] = -frame[0];
                            }
                            seen.push(cycle.seq);
                            client.answer(cycle, 1_000);
                        }
                        Wake::Stopped => break,
                        Wake::Idle => panic!("{kind}: the helper went quiet"),
                    }
                }
                assert_eq!(seen, (1..=50).collect::<Vec<u32>>(), "{kind}");
            });

            let mut helper = HelperSide::new(shared, request, reply);
            for n in 1..=50u32 {
                let frames = if n % 2 == 0 { 64 } else { 256 };
                let mut got = Vec::new();
                let how = helper.exchange(
                    frames,
                    LONG,
                    |input| {
                        assert_eq!(input.len(), frames);
                        for (i, s) in input.iter_mut().enumerate() {
                            *s = i as f32;
                        }
                    },
                    |output| got.extend_from_slice(output),
                );
                assert_eq!(how, Exchange::Answered, "{kind} cycle {n}");
                assert_eq!(got.len(), frames * 2);
                for (i, frame) in got.chunks(2).enumerate() {
                    assert_eq!(frame[0], i as f32 + n as f32, "{kind} cycle {n}");
                    assert_eq!(frame[1], -frame[0]);
                }
            }
            helper.stop();
        });

        let counters = shared.counters();
        assert_eq!(counters.cycles, 50, "{kind}");
        assert_eq!(counters.late_replies, 0, "{kind}");
        assert_eq!(counters.device_xruns, 0, "{kind}");
        assert!(counters.handoff_max_ns >= counters.handoff_last_ns);
        assert_eq!(shared.state(), Some(State::HelperStopped));
    }
}

#[test]
fn consecutive_cycles_use_alternate_slots() {
    let region = Region::heap(Layout::new(16, 0, 1).unwrap());
    let shared = region.shared();
    let bells = Bells {
        request: Box::new(PollBell),
        reply: Box::new(PollBell),
    };
    std::thread::scope(|s| {
        s.spawn(|| {
            let mut client = ClientSide::new(shared, &*bells.request, &*bells.reply);
            for _ in 0..4 {
                let Wake::Cycle(cycle) = client.wait(LONG) else {
                    panic!("no cycle")
                };
                let (_, output) = client.buffers(cycle);
                output.fill(cycle.seq as f32);
                client.answer(cycle, 0);
            }
        });
        let mut helper = HelperSide::new(shared, &*bells.request, &*bells.reply);
        for _ in 0..4 {
            assert_eq!(
                helper.exchange(16, LONG, |_| {}, |_| {}),
                Exchange::Answered
            );
        }
    });
    // SAFETY: both sides are done.
    unsafe {
        assert!(shared.output_slot(0)[..16].iter().all(|s| *s == 4.0));
        assert!(shared.output_slot(1)[..16].iter().all(|s| *s == 3.0));
    }
}

#[test]
fn a_late_answer_is_silence_and_counted_and_the_next_cycle_is_on_time() {
    for (kind, bells) in bell_kinds() {
        let region = Region::heap(Layout::new(64, 0, 2).unwrap());
        let shared = region.shared();
        let request: &dyn Doorbell = &*bells.request;
        let reply: &dyn Doorbell = &*bells.reply;
        std::thread::scope(|s| {
            s.spawn(|| {
                let mut client = ClientSide::new(shared, request, reply);
                // The first cycle: far too slow.
                let Wake::Cycle(first) = client.wait(LONG) else {
                    panic!("{kind}: no first cycle")
                };
                std::thread::sleep(Duration::from_millis(80));
                client.answer(first, 80_000_000);
                // Then on time, for the newest cycle whatever was missed.
                loop {
                    match client.wait(LONG) {
                        Wake::Cycle(cycle) => {
                            client.buffers(cycle).1.fill(1.0);
                            client.answer(cycle, 10);
                        }
                        Wake::Stopped => break,
                        Wake::Idle => panic!("{kind}: quiet"),
                    }
                }
            });

            let mut helper = HelperSide::new(shared, request, reply);
            let mut took = false;
            let how = helper.exchange(64, Duration::from_millis(5), |_| {}, |_| took = true);
            assert_eq!(how, Exchange::Late, "{kind}");
            assert!(!took, "{kind}: a late cycle hands nothing over");
            // Give the slow answer time to land, then carry on.
            std::thread::sleep(Duration::from_millis(120));
            for _ in 0..5 {
                let mut out = Vec::new();
                let how = helper.exchange(64, LONG, |_| {}, |o| out.extend_from_slice(o));
                assert_eq!(how, Exchange::Answered, "{kind}");
                assert!(
                    out.iter().all(|s| *s == 1.0),
                    "{kind}: this cycle's own output"
                );
            }
            helper.stop();
        });
        let counters = shared.counters();
        assert_eq!(counters.cycles, 6, "{kind}");
        assert_eq!(counters.late_replies, 1, "{kind}");
    }
}

#[test]
fn a_client_that_closes_stops_the_helper_and_a_helper_that_stops_wakes_the_client() {
    for (kind, bells) in bell_kinds() {
        let region = Region::heap(Layout::new(64, 0, 2).unwrap());
        let shared = region.shared();
        let request: &dyn Doorbell = &*bells.request;
        let reply: &dyn Doorbell = &*bells.reply;

        // Fontelle closes while the helper is waiting on it.
        std::thread::scope(|s| {
            s.spawn(|| {
                let mut client = ClientSide::new(shared, request, reply);
                let Wake::Cycle(_) = client.wait(LONG) else {
                    panic!("{kind}")
                };
                client.close();
            });
            let mut helper = HelperSide::new(shared, request, reply);
            let started = std::time::Instant::now();
            let how = helper.exchange(64, LONG, |_| {}, |_| {});
            assert_eq!(how, Exchange::ClientGone, "{kind}");
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{kind}: not the deadline"
            );
        });
        assert_eq!(shared.state(), Some(State::ClientClosed));

        // A client that starts after the helper rang still gets the cycle
        // nobody answered — Fontelle's thread may well start after the
        // driver's first buffer.
        shared.set_state(State::Running);
        let mut late_client = ClientSide::new(shared, request, reply);
        let Wake::Cycle(pending) = late_client.wait(Duration::ZERO) else {
            panic!("{kind}: the unanswered cycle")
        };
        late_client.answer(pending, 0);
        // The helper stops while Fontelle is waiting on it.
        std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(20));
                HelperSide::new(shared, request, reply).stop();
            });
            let mut client = ClientSide::new(shared, request, reply);
            assert_eq!(client.wait(LONG), Wake::Stopped, "{kind}");
        });

        shared.set_state(State::Running);
        let mut client = ClientSide::new(shared, request, reply);
        // Nobody rings: the client's wait comes back idle at the timeout.
        let started = std::time::Instant::now();
        assert_eq!(client.wait(Duration::from_millis(30)), Wake::Idle, "{kind}");
        assert!(started.elapsed() >= Duration::from_millis(25), "{kind}");
    }
}

#[test]
fn a_cycle_names_its_frames() {
    let c = Cycle {
        seq: 3,
        frames: 128,
    };
    assert_eq!(c.frames, 128);
}
