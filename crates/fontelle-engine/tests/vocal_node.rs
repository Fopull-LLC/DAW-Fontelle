//! The vocal track's audio node: phrases sung ahead of time, played at the
//! right sample.
//!
//! Nothing is sung on the audio thread. The app renders each phrase through
//! a singer engine, off the audio thread, and publishes a table of where
//! each phrase's audio starts in the song; the node only reads it — at the
//! device's rate whatever the engine's, from wherever the playhead is,
//! crossfading where one phrase's consonant runs under the last one's tail.

use std::sync::Arc;

use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, SungPhrase, TransportSnapshot, TransportState,
    VocalNode, VocalPlayer, VocalTable,
};

const SR: f32 = 48_000.0;

/// A centred channel's level on each side: the channel pan law, as every
/// instrument node places its sound.
fn centre() -> f32 {
    fontelle_types::PanLaw::Minus3Db.gains(0.0).0
}

fn constant(start: f64, rate: u32, seconds: f64, value: f32) -> SungPhrase {
    let frames = (seconds * rate as f64) as usize;
    SungPhrase {
        start,
        sample_rate: rate,
        samples: vec![value; frames].into(),
    }
}

fn sine(start: f64, rate: u32, seconds: f64, hz: f64) -> SungPhrase {
    let frames = (seconds * rate as f64) as usize;
    SungPhrase {
        start,
        sample_rate: rate,
        samples: (0..frames)
            .map(|i| (std::f64::consts::TAU * hz * i as f64 / rate as f64).sin() as f32 * 0.5)
            .collect::<Vec<_>>()
            .into(),
    }
}

fn node_with(phrases: Vec<SungPhrase>) -> (Arc<VocalPlayer>, VocalNode) {
    let player = VocalPlayer::new();
    player.publish(VocalTable::new(phrases));
    let mut node = VocalNode::new(Arc::clone(&player));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 4096,
    });
    (player, node)
}

/// Renders `frames` song samples from `from` in blocks of `block`, left
/// channel.
fn render(
    node: &mut VocalNode,
    from: i64,
    frames: usize,
    block: usize,
    state: TransportState,
) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames);
    let mut at = from;
    while out.len() < frames {
        let n = block.min(frames - out.len());
        let mut left = vec![0.0f32; n];
        let mut right = vec![0.0f32; n];
        {
            let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
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
                    ..Default::default()
                },
                sample_range: at..at + n as i64,
            };
            node.process(&mut ctx);
        }
        out.extend_from_slice(&left);
        at += n as i64;
    }
    out
}

fn play(node: &mut VocalNode, from: i64, frames: usize) -> Vec<f32> {
    render(node, from, frames, 256, TransportState::Playing)
}

#[test]
fn a_phrase_sounds_from_its_start_and_not_before() {
    let (_player, mut node) = node_with(vec![constant(1.0, 48_000, 0.5, 0.5)]);
    let out = play(&mut node, 47_000, 2_000);
    assert!(out[..1_000].iter().all(|s| *s == 0.0), "nothing before 1 s");
    // Fades in over a few milliseconds rather than clicking, then holds.
    assert!(out[1_000 + 960] > 0.3, "sounding 20 ms in: {}", out[1_960]);
    assert!(out[1_000] < 0.2, "not at full level on its first sample");
}

#[test]
fn a_phrase_at_another_rate_plays_at_its_own_pitch_and_length() {
    // A 440 Hz tone sung at 24 kHz, heard through a 48 kHz device.
    let (_player, mut node) = node_with(vec![sine(0.0, 24_000, 1.0, 440.0)]);
    let out = play(&mut node, 0, 48_000);
    let crossings = out[4_800..43_200]
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    // 0.8 s of 440 Hz: 352 upward crossings.
    assert!((crossings as i32 - 352).abs() <= 2, "{crossings} crossings");
    // It ends where a second of audio ends, not half a second in.
    let late: f32 = out[40_000..47_000].iter().map(|s| s.abs()).sum();
    assert!(late > 100.0, "still sounding near the end of its second");
}

/// Space mid-phrase: the node finds its place from the playhead alone, so
/// starting anywhere and in any block size gives the same samples.
#[test]
fn starting_mid_phrase_lands_on_the_same_samples() {
    let phrases = || vec![sine(0.5, 24_000, 2.0, 220.0), sine(2.6, 24_000, 1.0, 330.0)];
    let (_a, mut whole) = node_with(phrases());
    let reference = play(&mut whole, 0, 48_000 * 4);
    for (from, block) in [(60_000i64, 64usize), (100_003, 1_000), (125_000, 333)] {
        let (_b, mut node) = node_with(phrases());
        let part = render(&mut node, from, 20_000, block, TransportState::Playing);
        let expected = &reference[from as usize..from as usize + 20_000];
        for (i, (a, b)) in part.iter().zip(expected).enumerate() {
            assert!(
                (a - b).abs() < 1e-5,
                "from {from} by {block}: sample {i} {a} vs {b}"
            );
        }
    }
}

/// Where a phrase's lead-in runs under the last one's tail, the two meet in
/// an equal-power crossfade: the outgoing gain squared and the incoming
/// gain squared add to one all the way across.
#[test]
fn where_phrases_meet_they_crossfade_at_equal_power() {
    // The first runs 1.0..2.0 s, the second from 1.9 s: 100 ms of overlap.
    let first_alone = || {
        vec![
            constant(1.0, 48_000, 1.0, 1.0),
            constant(1.9, 48_000, 1.0, 0.0),
        ]
    };
    let second_alone = || {
        vec![
            constant(1.0, 48_000, 1.0, 0.0),
            constant(1.9, 48_000, 1.0, 1.0),
        ]
    };
    let (_a, mut a) = node_with(first_alone());
    let (_b, mut b) = node_with(second_alone());
    let out_gain: Vec<f32> = play(&mut a, 48_000, 96_000)
        .iter()
        .map(|s| s / centre())
        .collect();
    let in_gain: Vec<f32> = play(&mut b, 48_000, 96_000)
        .iter()
        .map(|s| s / centre())
        .collect();
    let overlap = (0.9 * 48_000.0) as usize..(1.0 * 48_000.0) as usize;
    for i in overlap.clone().step_by(97) {
        let power = out_gain[i] * out_gain[i] + in_gain[i] * in_gain[i];
        assert!(
            (power - 1.0).abs() < 0.02,
            "at {i}: {} and {}",
            out_gain[i],
            in_gain[i]
        );
    }
    // Before the overlap the first is whole and the second silent.
    let i = (0.5 * 48_000.0) as usize;
    assert!((out_gain[i] - 1.0).abs() < 1e-4);
    assert!(in_gain[i].abs() < 1e-6);
    // Through the overlap one goes down and the other up.
    assert!(out_gain[overlap.start + 100] > out_gain[overlap.end - 100]);
    assert!(in_gain[overlap.start + 100] < in_gain[overlap.end - 100]);
}

#[test]
fn a_stopped_song_or_a_count_in_is_silent() {
    let (_player, mut node) = node_with(vec![constant(0.0, 48_000, 1.0, 0.5)]);
    for state in [TransportState::Stopped, TransportState::CountingIn] {
        let out = render(&mut node, 4_800, 1_000, 256, state);
        assert!(out.iter().all(|s| *s == 0.0), "{state:?} sings nothing");
    }
    // And a render is heard: an export goes through here.
    let out = render(&mut node, 4_800, 1_000, 256, TransportState::Rendering);
    assert!(out.iter().any(|s| *s != 0.0));
}

#[test]
fn a_new_table_is_heard_from_the_next_block_and_the_old_one_comes_back() {
    let (player, mut node) = node_with(vec![constant(0.0, 48_000, 2.0, 0.25)]);
    let before = play(&mut node, 24_000, 256);
    assert!((before[100] - 0.25 * centre()).abs() < 1e-4);
    player.publish(VocalTable::new(vec![constant(0.0, 48_000, 2.0, 0.75)]));
    let after = play(&mut node, 24_256, 256);
    assert!(
        (after[100] - 0.75 * centre()).abs() < 1e-4,
        "{}",
        after[100]
    );
    // The table it replaced is waiting to be freed off the audio thread.
    assert!(player.has_retired());
    player.reclaim();
    assert!(!player.has_retired());
}

#[test]
fn a_node_built_after_a_publish_starts_with_it() {
    let player = VocalPlayer::new();
    player.publish(VocalTable::new(vec![constant(0.0, 48_000, 1.0, 0.5)]));
    // A graph rebuild: a new node, the same player.
    let mut node = VocalNode::new(Arc::clone(&player));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    let out = play(&mut node, 10_000, 256);
    assert!((out[0] - 0.5 * centre()).abs() < 1e-4);
}

#[test]
fn the_channel_level_and_pan_apply() {
    let player = VocalPlayer::new();
    player.publish(VocalTable::new(vec![constant(0.0, 48_000, 1.0, 1.0)]));
    let mut node = VocalNode::new(Arc::clone(&player)).on_channel(-6.0, -1.0);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    let mut left = vec![0.0f32; 128];
    let mut right = vec![0.0f32; 128];
    {
        let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut channels,
            all_events: &[],
            live_events: &[],
            audio: &[],
            node: fontelle_types::NodeId::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: 10_000,
                ..Default::default()
            },
            sample_range: 10_000..10_128,
        };
        node.process(&mut ctx);
    }
    assert!(
        left[64] > 0.4 && left[64] < 0.8,
        "-6 dB hard left: {}",
        left[64]
    );
    assert!(
        right[64].abs() < 1e-3,
        "nothing on the right: {}",
        right[64]
    );
}
