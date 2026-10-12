//! The real shared memory and doorbells, two mappings of one name in one
//! process — what the helper and Fontelle each hold.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use fontelle_audio_ipc::{
    Bells, ClientSide, Exchange, HelperSide, Layout, Region, State, Wake, channel_name,
};

#[test]
fn names_are_per_process_and_per_exchange() {
    let a = channel_name(10, 1);
    assert_ne!(a, channel_name(10, 2));
    assert_ne!(a, channel_name(11, 1));
    assert!(a.contains("fontelle-audio"), "{a}");
    if cfg!(windows) {
        assert!(a.starts_with("Local\\"), "{a}");
    } else {
        assert!(a.starts_with('/'), "{a}");
    }
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn what_one_mapping_writes_the_other_reads_and_the_name_goes_with_its_maker() {
    let name = channel_name(std::process::id(), 7001);
    let made = Region::create(&name, Layout::new(128, 2, 2).unwrap()).expect("create");
    let opened = Region::open(&name).expect("open");
    assert_eq!(opened.shared().layout(), made.shared().layout());
    made.shared()
        .header()
        .sample_rate
        .store(48_000, Ordering::Relaxed);
    assert_eq!(
        opened.shared().header().sample_rate.load(Ordering::Relaxed),
        48_000
    );
    // SAFETY: one writer, then one reader, in this thread.
    unsafe {
        made.shared().output_slot(1)[5] = 0.25;
        assert_eq!(opened.shared().output_slot(1)[5], 0.25);
    }
    assert!(
        Region::create(&name, Layout::new(128, 2, 2).unwrap()).is_err(),
        "taken"
    );
    drop(opened);
    drop(made);
    assert!(Region::open(&name).is_err(), "gone with its maker");
}

#[cfg(any(windows, target_os = "linux"))]
#[test]
fn the_handshake_runs_across_two_mappings_and_its_cost_is_measured() {
    let name = channel_name(std::process::id(), 7002);
    let made = Region::create(&name, Layout::new(256, 0, 2).unwrap()).unwrap();
    let opened = Region::open(&name).unwrap();
    let client_bells = Bells::create(&name).unwrap();
    let helper_bells = Bells::open(&name).unwrap();
    made.shared().set_state(State::Running);

    const CYCLES: usize = 2_000;
    let mut round_trips = Vec::with_capacity(CYCLES);
    std::thread::scope(|s| {
        s.spawn(|| {
            let mut client =
                ClientSide::new(made.shared(), &*client_bells.request, &*client_bells.reply);
            loop {
                match client.wait(Duration::from_secs(5)) {
                    Wake::Cycle(cycle) => {
                        client.buffers(cycle).1.fill(0.0);
                        client.answer(cycle, 0);
                    }
                    Wake::Stopped => break,
                    Wake::Idle => panic!("quiet"),
                }
            }
        });
        let mut helper = HelperSide::new(
            opened.shared(),
            &*helper_bells.request,
            &*helper_bells.reply,
        );
        for _ in 0..CYCLES {
            let t = Instant::now();
            let how = helper.exchange(256, Duration::from_secs(1), |_| {}, |_| {});
            round_trips.push(t.elapsed());
            assert_eq!(how, Exchange::Answered);
        }
        helper.stop();
    });
    round_trips.sort();
    let median = round_trips[CYCLES / 2];
    let p99 = round_trips[CYCLES * 99 / 100];
    let counters = made.shared().counters();
    eprintln!(
        "handoff across two mappings, {CYCLES} cycles: median {median:?}, p99 {p99:?}, \
         helper's own last {} ns, max {} ns",
        counters.handoff_last_ns, counters.handoff_max_ns
    );
    // Generous: this runs on a loaded CI box at the lowest priority. The
    // number that matters is printed; the bound only catches a design that
    // sleeps instead of waking.
    assert!(median < Duration::from_millis(2), "median {median:?}");
    assert_eq!(counters.cycles, CYCLES as u64);
}
