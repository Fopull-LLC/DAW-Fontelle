//! Hearing yourself through the track you are recording on (TDD §15.4).
//!
//! Reported from using the window:
//!
//! > *"please also make it so i can monitor my inputs so it should work like
//! > fl, tracks are already automatically routed to master so i should be able
//! > to hear routed input playing even when song isnt playing or im not
//! > recording and i should be able to hear it because of it routing my input
//! > track to master"*
//!
//! # Why this needs a second ring
//!
//! [`fontelle_engine::InputWriter`]'s ring goes from the input callback to the
//! **disk** thread: that is what a take is. Monitoring goes from the input
//! callback to the **output** callback, and the two cannot share one ring
//! because they are drained at different rates by different threads, and
//! whichever drained first would steal the other's samples.
//!
//! So an [`InputMonitor`] is a second ring with atomics on both ends — the
//! shape [`fontelle_engine::SpectrumTap`] already uses, for the same reason:
//! two threads, no lock, no allocation (INVARIANT 1). One writer (the input
//! callback), one reader ([`MonitorNode`], on the audio thread).
//!
//! # The two clocks
//!
//! The input device and the output device are two crystals, and nothing keeps
//! them in step. So the node **primes** — it holds a block of slack before it
//! plays anything, which is the latency monitoring costs — and it says so
//! through `latency_samples`. Drift in the empty direction re-primes; drift in
//! the full direction is caught rather than left to fill, because a ring that
//! sits full drops every block forever and glitches on every one of them.

use std::sync::Arc;

use fontelle_engine::{
    AudioNode, IdleGate, InputMonitor, MonitorNode, PrepareContext, ProcessContext,
    TransportSnapshot, TransportState,
};

const RATE: f32 = 48_000.0;
const BLOCK: u32 = 512;

/// A monitor big enough for a second of stereo at 48 kHz, already open at
/// `rate` with `channels` channels — which is what a device does when its
/// stream starts.
fn open(rate: u32, channels: u16) -> Arc<InputMonitor> {
    let monitor = Arc::new(InputMonitor::new(96_000));
    monitor.open(rate, channels);
    monitor
}

fn node(monitor: &Arc<InputMonitor>) -> MonitorNode {
    let mut node = MonitorNode::new(Arc::clone(monitor));
    node.prepare(&PrepareContext {
        sample_rate: RATE,
        max_block_size: BLOCK,
    });
    node
}

/// Renders `frames` through `node` in blocks of `block`, over a **stopped**
/// transport — the whole point is that monitoring does not wait for play.
///
/// `bed` is what the buses already hold when the node runs, so that "adds
/// into" can be told from "overwrites".
fn render(node: &mut MonitorNode, frames: usize, block: usize, bed: f32) -> (Vec<f32>, Vec<f32>) {
    let mut left_out = Vec::with_capacity(frames);
    let mut right_out = Vec::with_capacity(frames);
    let mut at = 0i64;
    while left_out.len() < frames {
        let n = block.min(frames - left_out.len());
        let mut left = vec![bed; n];
        let mut right = vec![bed; n];
        {
            let mut outputs: [&mut [f32]; 2] = [&mut left, &mut right];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Stopped,
                    position_sample: at,
                    bpm: 120.0,
                    ..Default::default()
                },
                sample_range: at..at + n as i64,
            };
            node.process(&mut ctx);
        }
        left_out.extend_from_slice(&left);
        right_out.extend_from_slice(&right);
        at += n as i64;
    }
    (left_out, right_out)
}

/// A ramp whose frame *n* holds *n*, so where a sample came from is readable
/// off its value.
fn counting(frames: usize) -> Vec<f32> {
    (0..frames).map(|i| i as f32).collect()
}

/// How big a block a real capture device hands over at a time.
///
/// **Not the graph's.** The output stream is pinned to `BLOCK_SIZE`; an input
/// stream takes whatever its device offers, and on ALSA and PipeWire that is
/// routinely a thousand frames or more. Feeding a test in one enormous `write`
/// is what hid that difference, so every fixture here arrives in blocks.
const DEVICE_BLOCK: usize = 256;

/// Writes `samples` the way a device does: one block at a time.
fn feed(monitor: &Arc<InputMonitor>, samples: &[f32], block_frames: usize, channels: usize) {
    for chunk in samples.chunks(block_frames * channels) {
        monitor.write(chunk);
    }
}

/// Renders `frames` while a device goes on delivering, which is the only state
/// monitoring is ever really in.
///
/// Tops the ring up before each block, so it neither runs dry nor drifts full
/// — exactly what a device that is keeping up does, and what lets a value
/// assertion below be exact.
#[allow(clippy::too_many_arguments)]
fn render_fed(
    node: &mut MonitorNode,
    monitor: &Arc<InputMonitor>,
    source: &[f32],
    channels: usize,
    frames: usize,
    block: usize,
    bed: f32,
) -> (Vec<f32>, Vec<f32>) {
    let mut left_out = Vec::with_capacity(frames);
    let mut right_out = Vec::with_capacity(frames);
    let mut fed = 0usize;
    let mut at = 0i64;
    while left_out.len() < frames {
        // Enough in hand for this block and the node's own slack.
        while monitor.available_frames() < node.prime_frames() + block && fed < source.len() {
            let end = (fed + DEVICE_BLOCK * channels).min(source.len());
            monitor.write(&source[fed..end]);
            fed = end;
        }
        let n = block.min(frames - left_out.len());
        let mut left = vec![bed; n];
        let mut right = vec![bed; n];
        {
            let mut outputs: [&mut [f32]; 2] = [&mut left, &mut right];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Stopped,
                    position_sample: at,
                    bpm: 120.0,
                    ..Default::default()
                },
                sample_range: at..at + n as i64,
            };
            node.process(&mut ctx);
        }
        left_out.extend_from_slice(&left);
        right_out.extend_from_slice(&right);
        at += n as i64;
    }
    (left_out, right_out)
}

// ------------------------------------------------------------- the ring ---

#[test]
fn what_the_input_wrote_comes_back_out_in_the_order_it_went_in() {
    let monitor = open(48_000, 1);
    let block = counting(64);
    assert_eq!(monitor.write(&block), 64);
    let mut out = vec![0.0f32; 64];
    assert_eq!(monitor.read(&mut out), 64);
    assert_eq!(out, block);
}

#[test]
fn reading_twice_does_not_hand_the_same_samples_back_again() {
    let monitor = open(48_000, 1);
    monitor.write(&[1.0, 2.0, 3.0]);
    let mut out = vec![0.0f32; 3];
    assert_eq!(monitor.read(&mut out), 3);
    let mut again = vec![0.0f32; 3];
    assert_eq!(monitor.read(&mut again), 0, "the ring replayed itself");
}

#[test]
fn a_ring_nobody_empties_drops_what_it_cannot_hold_and_says_how_much() {
    // Written from the input callback, so it may never block and never grow
    // (INVARIANT 1). The honest answer is to lose what does not fit and count
    // it.
    let monitor = Arc::new(InputMonitor::new(16));
    monitor.open(48_000, 1);
    let written = monitor.write(&vec![0.5f32; 64]);
    assert!(written < 64, "it claimed to hold more than it has");
    assert_eq!(monitor.dropped(), 64 - written);
}

#[test]
fn a_monitor_nobody_opened_is_not_live() {
    let monitor = Arc::new(InputMonitor::new(1024));
    assert!(!monitor.is_live());
    monitor.open(48_000, 2);
    assert!(monitor.is_live());
    monitor.close();
    assert!(
        !monitor.is_live(),
        "closing the stream left the monitor live"
    );
}

#[test]
fn closing_the_stream_throws_away_what_was_still_in_the_ring() {
    // Or unplugging a microphone and choosing another would play the tail of
    // the first one through the second.
    let monitor = open(48_000, 1);
    monitor.write(&counting(64));
    monitor.close();
    assert_eq!(monitor.available(), 0);
}

// ------------------------------------------------------------- the node ---

#[test]
fn a_monitor_that_is_not_live_is_silence() {
    let monitor = Arc::new(InputMonitor::new(96_000));
    let mut node = node(&monitor);
    let (left, right) = render(&mut node, 2048, 128, 0.0);
    assert!(
        left.iter().all(|s| *s == 0.0),
        "a closed monitor made sound"
    );
    assert!(right.iter().all(|s| *s == 0.0));
}

#[test]
fn what_the_input_wrote_is_what_the_bus_hears() {
    // The whole feature: samples that arrived on the input callback, on the
    // bus, with the transport stopped.
    let monitor = open(48_000, 1);
    let mut node = node(&monitor);
    let source = counting(8192);
    let (left, right) = render_fed(&mut node, &monitor, &source, 1, 2048, 128, 0.0);

    assert_eq!(
        &left[..8],
        &counting(8)[..],
        "the take started somewhere else"
    );
    assert_eq!(left, right, "a mono input has to arrive on both sides");
    assert_eq!(left[2047], 2047.0);
}

#[test]
fn it_adds_into_its_bus_rather_than_replacing_what_is_there() {
    // It is a source among sources on a track's bus, like the sampler and the
    // clip player: a node that overwrote would silence the instrument on the
    // same track.
    let monitor = open(48_000, 1);
    let mut node = node(&monitor);
    let (left, _) = render_fed(&mut node, &monitor, &vec![0.25f32; 8192], 1, 1024, 128, 1.0);
    assert!(
        left.iter().all(|s| (*s - 1.25).abs() < 1e-6),
        "the bus it was scheduled on was overwritten"
    );
}

#[test]
fn a_stereo_input_keeps_its_sides() {
    // Interleaved, exactly as the device delivered it: left 1.0, right -1.0.
    let monitor = open(48_000, 2);
    let mut interleaved = Vec::new();
    for _ in 0..8192 {
        interleaved.push(1.0f32);
        interleaved.push(-1.0f32);
    }
    let mut node = node(&monitor);
    let (left, right) = render_fed(&mut node, &monitor, &interleaved, 2, 1024, 128, 0.0);
    assert!(left.iter().all(|s| (*s - 1.0).abs() < 1e-6));
    assert!(right.iter().all(|s| (*s + 1.0).abs() < 1e-6));
}

#[test]
fn an_input_running_at_another_rate_is_read_at_the_ratio_between_them() {
    // The same rule an imported file follows, and the same bug if it is
    // missed: a 24 kHz input read one frame per frame plays an octave sharp.
    let monitor = open(24_000, 1);
    let mut node = node(&monitor);
    let (left, _) = render_fed(&mut node, &monitor, &counting(8192), 1, 1024, 128, 0.0);
    // Half a frame of input per frame of output, so a thousand frames of
    // output have consumed about five hundred of input.
    assert!(
        (left[1000] - 500.0).abs() < 2.0,
        "a 24 kHz input read as {} at frame 1000",
        left[1000]
    );
}

#[test]
fn the_same_input_renders_the_same_at_every_block_size() {
    // This project has shipped this bug once already, audible as bitcrushing:
    // the device is under no obligation to deliver a particular block size.
    let source = counting(16_384);
    let reference = {
        let monitor = open(48_000, 1);
        let mut node = node(&monitor);
        render_fed(&mut node, &monitor, &source, 1, 2048, 512, 0.0).0
    };
    for block in [1usize, 7, 85, 128, 512] {
        let monitor = open(48_000, 1);
        let mut node = node(&monitor);
        let (left, _) = render_fed(&mut node, &monitor, &source, 1, 2048, block, 0.0);
        assert_eq!(left, reference, "blocks of {block} rendered something else");
    }
}

#[test]
fn a_monitor_that_runs_dry_goes_quiet_rather_than_repeating_itself() {
    // An input callback that stalls is a hole, not a loop: the last block
    // played again is a stutter nobody can mistake for their own voice.
    let monitor = open(48_000, 1);
    feed(&monitor, &vec![0.5f32; 2048], DEVICE_BLOCK, 1);
    let mut node = node(&monitor);
    let (left, _) = render(&mut node, 8192, 128, 0.0);
    let tail = &left[6000..];
    assert!(
        tail.iter().all(|s| *s == 0.0),
        "the node kept playing after the input ran out"
    );
}

#[test]
fn it_holds_a_block_of_slack_and_says_how_much() {
    // Two clocks that nothing keeps in step, so the ring has to have something
    // in it before the first sample is played or it underruns on the first
    // block. That slack is latency, and latency that is not reported is
    // latency nothing can ever compensate for.
    let monitor = open(48_000, 1);
    let node = node(&monitor);
    assert!(node.latency_samples() >= BLOCK);
    assert_eq!(node.latency_samples() as usize, node.prime_frames());
}

#[test]
fn a_ring_drifting_towards_full_is_caught_rather_than_left_to_fill() {
    // The other direction of the same two clocks. A ring that reaches its end
    // drops every block from then on and glitches on every one of them; one
    // catch-up, once, is a seam nobody hears.
    let monitor = open(48_000, 1);
    let mut node = node(&monitor);
    // Far more than the node will ever be given a chance to drain, and in
    // ordinary device blocks so that the size of one is not itself the thing
    // being read as drift.
    feed(&monitor, &counting(64_000), DEVICE_BLOCK, 1);
    render(&mut node, 4096, 128, 0.0);
    assert!(
        monitor.available_frames() <= node.high_water_frames(),
        "the ring was left holding {} frames against a high water of {}",
        monitor.available_frames(),
        node.high_water_frames()
    );
    assert_eq!(
        monitor.dropped(),
        0,
        "it dropped from the writing end instead"
    );
}

// -------------------------------------------------------- the idle gate ---

#[test]
fn a_stopped_transport_keeps_running_the_graph_while_a_monitor_is_live() {
    // §6.3 says a stopped transport does not process the graph, which is what
    // makes idle cost nothing. A live microphone is exactly the case where
    // "stopped" does not mean "idle" — the same reconciliation a held key
    // already gets.
    let mut gate = IdleGate::new();
    assert!(!gate.is_awake(0), "an idle graph should be asleep");
    gate.set_monitoring(true);
    assert!(gate.is_awake(0), "a live input did not wake the graph");
    // And it stays awake through silence: a microphone in a quiet room is
    // still a microphone, and a gate that measured its way to sleep would
    // swallow the first word.
    gate.observe(0.0);
    assert!(gate.is_awake(0));
    gate.set_monitoring(false);
    gate.observe(0.0);
    assert!(!gate.is_awake(0), "closing the input left the graph awake");
}

#[test]
fn a_transport_stop_does_not_interrupt_what_is_being_played_into_the_room() {
    // The rule `reset_sequenced` exists for, applied to the one source that is
    // neither the timeline's nor a note: a stop, a seek and a loop seam all
    // cut what the *song* started, and a microphone is not the song. Re-priming
    // there would put a hole in the monitor at every loop.
    let monitor = open(48_000, 1);
    feed(&monitor, &vec![0.5f32; 4096], DEVICE_BLOCK, 1);
    let mut node = node(&monitor);
    render(&mut node, 1024, 128, 0.0);

    node.reset_sequenced();
    let (left, _) = render(&mut node, 128, 128, 0.0);
    assert!(
        left.iter().all(|s| (*s - 0.5).abs() < 1e-6),
        "a stop left a hole in the monitor"
    );
}

#[test]
fn a_full_reset_drops_the_frames_in_hand_and_primes_again() {
    // The other half of the pair above, and the difference between them: a
    // device torn down, a panic, a graph replaced. A full reset does **not**
    // mean silence — a microphone that is still open goes on being heard —
    // it means the frames in hand belong to a stream that is gone and the
    // slack has to be rebuilt.
    //
    // Which is observable exactly where it matters: with less than a block of
    // slack left in the ring there is nothing to prime from, so it waits
    // rather than playing on into an underrun.
    let monitor = open(48_000, 1);
    let mut node = node(&monitor);
    // Exactly what the node is willing to hold — more and it would trim the
    // excess as drift, which is a different mechanism and would make the
    // arithmetic below wrong.
    let held = node.high_water_frames();
    feed(&monitor, &vec![0.5f32; held], DEVICE_BLOCK, 1);
    // Drawn down to just under the slack the node needs, so that "it primes
    // again" is the difference between silence and sound. Measured off the
    // node rather than assumed, because how much slack it wants depends on
    // what the device delivers.
    let spend = held - node.prime_frames() + 64;
    let (heard, _) = render(&mut node, spend, 128, 0.0);
    assert!(
        heard.iter().all(|s| (*s - 0.5).abs() < 1e-6),
        "it never primed in the first place"
    );
    assert!(
        monitor.available_frames() < node.prime_frames(),
        "the ring still holds a full prime, so this proves nothing"
    );

    node.reset();
    let (after, _) = render(&mut node, 128, 128, 0.0);
    assert!(
        after.iter().all(|s| *s == 0.0),
        "a full reset carried on from the frames it was holding"
    );
}

// -------------------------------------------- the two block sizes differ ---
//
// > *"the audio monitoring sounds very flickery and weird but when i record it
// > it does actually sound clearer in playback."*
//
// The recording was clean and the monitor was not, which says the capture was
// never the problem: the two rings are drained by different threads at
// different rates, and only the monitor's reader had assumed anything about
// how much arrives at once.
//
// It had assumed the **graph's** block. The output stream is pinned to
// `BLOCK_SIZE`; an input stream takes whatever its device offers, and on ALSA
// and PipeWire that is routinely a thousand frames or more. So a thousand
// frames landed every twenty-one milliseconds and the reader, holding two and
// a half milliseconds of slack, ran dry eight times in between — and the
// drift catch-up, sized off the same wrong number, threw a block away every
// time one arrived. Both halves of the flicker, from one assumption.

/// One second of a steady tone, in `channels`-interleaved samples.
fn tone(frames: usize, channels: usize) -> Vec<f32> {
    (0..frames * channels).map(|_| 0.5f32).collect()
}

#[test]
fn the_slack_it_holds_is_measured_against_the_input_not_against_the_graph() {
    // The number that was wrong. A device handing over a thousand frames at a
    // time needs a thousand frames of slack; the graph's own block says
    // nothing about that.
    let monitor = open(48_000, 1);
    let mut node = node(&monitor);
    let small = node.prime_frames();

    feed(&monitor, &tone(4096, 1), 1024, 1);
    render(&mut node, 512, 128, 0.0);
    assert!(
        node.prime_frames() >= 1024,
        "a 1024-frame input still primes on {} frames",
        node.prime_frames()
    );
    assert!(node.prime_frames() > small, "it never adapted at all");
    // And it is reported, because latency nothing reports is latency nothing
    // can compensate for.
    assert_eq!(node.latency_samples() as usize, node.prime_frames());
}

#[test]
fn an_input_arriving_in_big_blocks_plays_without_holes() {
    // The bug itself, at the size it actually happens: 1024 frames in, 128
    // frames out, over and over. Every dropout the old reader produced shows
    // up here as a zero.
    let monitor = open(48_000, 1);
    let mut node = node(&monitor);
    let device_block = 1024usize;

    // Long enough to prime, then twenty cycles of steady state.
    let mut heard: Vec<f32> = Vec::new();
    for cycle in 0..24 {
        feed(&monitor, &tone(device_block, 1), device_block, 1);
        let (left, _) = render(&mut node, device_block, 128, 0.0);
        if cycle >= 4 {
            heard.extend_from_slice(&left);
        }
    }

    assert!(!heard.is_empty());
    let holes = heard.iter().filter(|s| **s == 0.0).count();
    assert_eq!(
        holes,
        0,
        "{holes} of {} frames were silent \u{2014} the monitor is dropping out",
        heard.len()
    );
}

#[test]
fn a_block_bigger_than_the_graphs_is_not_mistaken_for_drift() {
    // The second half of the flicker. The catch-up is for a ring that is
    // genuinely running away, not for one input block being larger than one
    // output block — which is the ordinary case and must never cost a sample.
    let monitor = open(48_000, 1);
    let mut node = node(&monitor);
    let device_block = 2048usize;

    let source = counting(device_block * 6);
    let mut heard: Vec<f32> = Vec::new();
    for cycle in 0..6 {
        let from = cycle * device_block;
        feed(
            &monitor,
            &source[from..from + device_block],
            device_block,
            1,
        );
        let (left, _) = render(&mut node, device_block, 128, 0.0);
        heard.extend_from_slice(&left);
    }

    // Whatever it played, it played **in order and without gaps** — a
    // catch-up that fired would show as the ramp jumping.
    let first = heard
        .iter()
        .position(|s| *s != 0.0)
        .expect("it played nothing");
    let played = &heard[first..];
    for (i, pair) in played.windows(2).enumerate() {
        assert_eq!(
            pair[1] - pair[0],
            1.0,
            "the ramp jumped at frame {i}: {} to {}",
            pair[0],
            pair[1]
        );
    }
}

// ------------------------------------- a plugin editor is somebody (2026-09-05)

/// The window says whether a plugin's own editor is open, and the audio
/// thread reads it off the transport the callback is already driven by:
/// an open editor is a reason to keep the graph awake, since an LV2 editor
/// reaches its plugin only through `run`.
#[test]
fn the_transport_carries_whether_a_plugin_editor_is_open() {
    let transport = fontelle_engine::Transport::new();
    assert!(!transport.is_attended(), "nothing open at the start");
    transport.set_attended(true);
    assert!(transport.is_attended());
    transport.set_attended(false);
    assert!(!transport.is_attended());
}
