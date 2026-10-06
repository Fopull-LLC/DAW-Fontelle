//! Analyze Musically as a mixer insert, the engine half
//! (`docs/analyze-musically-plan.md` §6.1): a wire that records what plays
//! through it.

use std::sync::Arc;

use fontelle_engine::{
    AnalyzeCapture, AnalyzeCaptureEvent, AnalyzeCaptureNode, AudioNode, EffectNode, PrepareContext,
    ProcessContext, TransportSnapshot, TransportState,
};
use fontelle_types::{AnalyzeConfig, ArmMode, EffectConfig};

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn insert(config: AnalyzeConfig, capture: &Arc<AnalyzeCapture>) -> EffectNode {
    let mut node =
        EffectNode::new(EffectConfig::Analyze(config)).with_analyze_capture(Arc::clone(capture));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    node
}

fn run(
    node: &mut dyn AudioNode,
    left: &mut [f32],
    right: &mut [f32],
    state: TransportState,
    at: i64,
) {
    let frames = left.len();
    let mut channels: [&mut [f32]; 2] = [left, right];
    let mut ctx = ProcessContext {
        inputs: &[],
        outputs: &mut channels,
        all_events: &[],
        live_events: &[],
        audio: &[],
        node: fontelle_types::NodeId::default(),
        transport: TransportSnapshot {
            state,
            position_sample: at,
            bpm: 120.0,
            ..Default::default()
        },
        sample_range: at..at + frames as i64,
    };
    node.process(&mut ctx);
}

/// A take as the reader sees it.
#[derive(Debug, Default)]
struct Take {
    song_sample: Option<i64>,
    frames: Vec<f32>,
    stopped: Option<u64>,
}

fn drain(capture: &AnalyzeCapture, takes: &mut Vec<Take>) {
    capture.drain(&mut |event| match event {
        AnalyzeCaptureEvent::Started { song_sample } => takes.push(Take {
            song_sample,
            ..Take::default()
        }),
        AnalyzeCaptureEvent::Audio(frames) => takes
            .last_mut()
            .expect("audio before a start")
            .frames
            .extend_from_slice(frames),
        AnalyzeCaptureEvent::Stopped { dropped_frames } => {
            takes.last_mut().expect("a stop before a start").stopped = Some(dropped_frames)
        }
    });
}

/// Deterministic noise, so every block differs.
fn noise(seed: &mut u32, out: &mut [f32]) {
    for sample in out {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        *sample = (*seed as f32 / u32::MAX as f32) * 2.0 - 1.0;
    }
}

fn config(arm: ArmMode) -> AnalyzeConfig {
    AnalyzeConfig {
        arm,
        ..AnalyzeConfig::new()
    }
}

#[test]
fn audio_passes_through_bit_identical() {
    for (arm, armed, bypassed, mix) in [
        (ArmMode::Now, true, false, 1.0),
        (ArmMode::Now, false, false, 1.0),
        (ArmMode::OnInput, true, false, 1.0),
        (ArmMode::Now, true, true, 1.0),
        // A mix turned down blends nothing: a wire with itself is itself,
        // and x * m + x * (1 - m) is not x to the bit.
        (ArmMode::Now, true, false, 0.37),
    ] {
        let capture = Arc::new(AnalyzeCapture::new(48_000));
        capture.arm(armed);
        let mut node = insert(AnalyzeConfig { mix, ..config(arm) }, &capture);
        node.set_bypassed(bypassed);
        let mut seed = 7;
        for block in 0..50 {
            let (mut left, mut right) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
            noise(&mut seed, &mut left);
            noise(&mut seed, &mut right);
            // Some of it out of range and some of it odd, which a wire must
            // carry exactly too.
            left[3] = 4.0;
            right[5] = f32::MIN_POSITIVE / 2.0;
            let (want_left, want_right) = (left.clone(), right.clone());
            run(
                &mut node,
                &mut left,
                &mut right,
                TransportState::Playing,
                (block * BLOCK) as i64,
            );
            let bits = |x: &[f32]| x.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
            assert_eq!(bits(&left), bits(&want_left), "{arm:?} armed {armed}");
            assert_eq!(bits(&right), bits(&want_right), "{arm:?} armed {armed}");
        }
        // And while it was at it, it recorded only when armed and on.
        let mut takes = Vec::new();
        drain(&capture, &mut takes);
        assert_eq!(
            !takes.is_empty(),
            armed && !bypassed,
            "{arm:?} armed {armed}"
        );
    }
}

#[test]
fn on_play_records_exactly_the_played_span() {
    let capture = Arc::new(AnalyzeCapture::new(48_000));
    capture.arm(true);
    let mut node = insert(config(ArmMode::OnPlay), &capture);
    let mut seed = 11;
    let mut played = Vec::new();
    let start = 9_600i64;
    for block in 0..12 {
        let (mut left, mut right) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        noise(&mut seed, &mut left);
        noise(&mut seed, &mut right);
        let rolling = (3..8).contains(&block);
        let at = start + (block as i64 - 3) * BLOCK as i64;
        if rolling {
            for i in 0..BLOCK {
                played.push(left[i]);
                played.push(right[i]);
            }
        }
        let state = if rolling {
            TransportState::Playing
        } else {
            TransportState::Stopped
        };
        run(&mut node, &mut left, &mut right, state, at);
    }
    let mut takes = Vec::new();
    drain(&capture, &mut takes);
    assert_eq!(takes.len(), 1);
    assert_eq!(takes[0].song_sample, Some(start));
    assert_eq!(takes[0].frames.len(), played.len(), "5 blocks, stereo");
    assert_eq!(takes[0].frames, played);
    assert_eq!(takes[0].stopped, Some(0));
    assert!(!capture.is_recording());
}

#[test]
fn on_input_starts_at_threshold() {
    let capture = Arc::new(AnalyzeCapture::new(96_000));
    capture.arm(true);
    let release_ms = 50.0;
    let mut node = insert(
        AnalyzeConfig {
            arm: ArmMode::OnInput,
            threshold_db: -20.0,
            release_ms,
            ..AnalyzeConfig::new()
        },
        &capture,
    );
    // Quiet (under -20 dB), then a square wave at half scale from frame 1000
    // for 4800 frames, then quiet again.
    let total = 20_000;
    let loud = 1_000..5_800;
    let signal: Vec<f32> = (0..total)
        .map(|i| {
            if loud.contains(&i) {
                if (i / 50) % 2 == 0 { 0.5 } else { -0.5 }
            } else {
                0.01
            }
        })
        .collect();
    for (block, chunk) in signal.chunks(BLOCK).enumerate() {
        let mut left = chunk.to_vec();
        let mut right = chunk.to_vec();
        // The transport is stopped: On input catches a take hands-free.
        run(
            &mut node,
            &mut left,
            &mut right,
            TransportState::Stopped,
            (block * BLOCK) as i64,
        );
    }
    let mut takes = Vec::new();
    drain(&capture, &mut takes);
    assert_eq!(takes.len(), 1, "one take");
    let take = &takes[0];
    assert_eq!(take.song_sample, None, "a free take");
    // Its first frame is the first one over the threshold.
    assert_eq!(take.frames[0], 0.5);
    let release = (release_ms / 1000.0 * SR) as usize;
    assert_eq!(
        take.frames.len() / 2,
        loud.len() + release,
        "the take runs to the end of the sound and the release after it"
    );
    assert_eq!(take.stopped, Some(0));
}

#[test]
fn a_full_ring_counts_dropped_frames_and_never_blocks() {
    let capacity = 1_000;
    let capture = Arc::new(AnalyzeCapture::new(capacity));
    capture.arm(true);
    let mut node = insert(config(ArmMode::Now), &capture);
    let blocks = 20;
    let started = std::time::Instant::now();
    for block in 0..blocks {
        let (mut left, mut right) = (vec![0.25f32; BLOCK], vec![-0.25f32; BLOCK]);
        run(
            &mut node,
            &mut left,
            &mut right,
            TransportState::Stopped,
            (block * BLOCK) as i64,
        );
    }
    // Nobody drained it: the ring held what it could and the rest was
    // counted, not waited for.
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(capture.dropped_frames(), (blocks * BLOCK - capacity) as u64);
    assert!(capture.is_recording());
    let mut takes = Vec::new();
    drain(&capture, &mut takes);
    assert_eq!(takes[0].frames.len() / 2, capacity);
    // Drained, there is room again.
    let (mut left, mut right) = (vec![0.5f32; BLOCK], vec![0.5f32; BLOCK]);
    run(&mut node, &mut left, &mut right, TransportState::Stopped, 0);
    capture.arm(false);
    let (mut left, mut right) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    run(&mut node, &mut left, &mut right, TransportState::Stopped, 0);
    drain(&capture, &mut takes);
    assert_eq!(takes.len(), 1);
    assert_eq!(takes[0].frames.len() / 2, capacity + BLOCK);
    assert_eq!(
        takes[0].stopped,
        Some((blocks * BLOCK - capacity) as u64),
        "the take says what it lost"
    );
}

#[test]
fn post_fader_records_after_the_fader_and_not_before() {
    let capture = Arc::new(AnalyzeCapture::new(48_000));
    capture.arm(true);
    let mut node = insert(
        AnalyzeConfig {
            arm: ArmMode::Now,
            post_fader: true,
            ..AnalyzeConfig::new()
        },
        &capture,
    );
    let mut post = AnalyzeCaptureNode::new(Arc::clone(&capture));
    post.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let (mut left, mut right) = (vec![0.8f32; BLOCK], vec![0.8f32; BLOCK]);
    run(&mut node, &mut left, &mut right, TransportState::Stopped, 0);
    // The fader, between the two points: half.
    for sample in left.iter_mut().chain(right.iter_mut()) {
        *sample *= 0.5;
    }
    run(&mut post, &mut left, &mut right, TransportState::Stopped, 0);
    assert!(
        left.iter().all(|v| *v == 0.4),
        "the post point changed the bus"
    );
    let mut takes = Vec::new();
    drain(&capture, &mut takes);
    assert_eq!(takes.len(), 1);
    assert_eq!(takes[0].frames.len(), 2 * BLOCK, "recorded once, not twice");
    assert!(
        takes[0].frames.iter().all(|v| *v == 0.4),
        "recorded after the fader"
    );
}

#[test]
fn a_take_carries_on_across_a_graph_rebuild() {
    let capture = Arc::new(AnalyzeCapture::new(48_000));
    capture.arm(true);
    let mut first = insert(config(ArmMode::Now), &capture);
    let (mut left, mut right) = (vec![0.1f32; BLOCK], vec![0.1f32; BLOCK]);
    run(
        &mut first,
        &mut left,
        &mut right,
        TransportState::Stopped,
        0,
    );
    // The rebuilt graph's node, handed the same capture.
    drop(first);
    let mut second = insert(config(ArmMode::Now), &capture);
    let (mut left, mut right) = (vec![0.2f32; BLOCK], vec![0.2f32; BLOCK]);
    run(
        &mut second,
        &mut left,
        &mut right,
        TransportState::Stopped,
        0,
    );
    let mut takes = Vec::new();
    drain(&capture, &mut takes);
    assert_eq!(takes.len(), 1, "one take, not two");
    assert_eq!(takes[0].frames.len(), 4 * BLOCK);
    assert_eq!(takes[0].stopped, None, "still recording");
}
