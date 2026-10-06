//! Analyze Musically's preview player (`docs/analyze-musically-plan.md`
//! §3.10): the study's audio — as recorded, or with its edits — played from
//! a cursor, over a range, looped, with a playhead the window can read and
//! an A/B switch, through a node on the master that never reaches for
//! memory on the audio thread (`study_player_no_allocation.rs`).

use std::sync::Arc;

use fontelle_engine::{AudioNode, PrepareContext, StudyAudio, StudyPlayer, StudyPlayerNode};

mod common;
use common::process;

const SR: f32 = 48_000.0;
const BLOCK: usize = 256;

/// A ramp, so where a sample came from is its value.
fn ramp(frames: usize, scale: f32) -> Arc<[f32]> {
    (0..frames)
        .map(|i| i as f32 * scale)
        .collect::<Vec<_>>()
        .into()
}

fn audio(frames: usize) -> StudyAudio {
    StudyAudio {
        channels: 1,
        sample_rate: 48_000,
        original: ramp(frames, 1e-5),
        edited: ramp(frames, -1e-5),
    }
}

fn node(player: &Arc<StudyPlayer>) -> StudyPlayerNode {
    let mut node = StudyPlayerNode::new(Arc::clone(player));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    node
}

/// One block out of the node: its left and right.
fn block(node: &mut StudyPlayerNode) -> (Vec<f32>, Vec<f32>) {
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    process(node, &mut [&mut left, &mut right]);
    (left, right)
}

#[test]
fn silent_until_asked_and_with_nothing_loaded() {
    let player = StudyPlayer::new();
    let mut node = node(&player);
    player.play(0, None, false);
    let (l, r) = block(&mut node);
    assert!(
        l.iter().chain(&r).all(|v| *v == 0.0),
        "nothing loaded is silence"
    );
    player.submit(audio(48_000));
    player.stop();
    for _ in 0..4 {
        let (l, _) = block(&mut node);
        assert!(l.iter().all(|v| *v == 0.0), "stopped is silence");
    }
}

/// Space from the cursor: the edited audio from that frame, on both sides,
/// once its few milliseconds of fade-in are past; the playhead follows.
#[test]
fn plays_the_edited_audio_from_the_cursor() {
    let player = StudyPlayer::new();
    player.submit(audio(96_000));
    let mut node = node(&player);
    player.play(10_000, None, false);
    let mut heard = Vec::new();
    for _ in 0..8 {
        let (l, r) = block(&mut node);
        assert_eq!(l, r, "mono reaches both sides alike");
        heard.extend(l);
    }
    assert!(player.playing());
    // Past the fade-in, each sample is the edited ramp at its frame.
    for (k, v) in heard.iter().enumerate().skip(512) {
        let expected = -((10_000 + k) as f32) * 1e-5;
        assert!((v - expected).abs() < 1e-6, "sample {k}: {v} vs {expected}");
    }
    assert_eq!(player.position(), 10_000 + 8 * BLOCK as u64);
}

/// B: the original, at the same place, without a jump in time.
#[test]
fn a_b_switches_to_the_original_in_place() {
    let player = StudyPlayer::new();
    player.submit(audio(96_000));
    let mut node = node(&player);
    player.play(0, None, false);
    for _ in 0..4 {
        block(&mut node);
    }
    player.set_original(true);
    assert!(player.original());
    for _ in 0..2 {
        block(&mut node);
    }
    let at = player.position();
    let (l, _) = block(&mut node);
    let expected = at as f32 * 1e-5;
    assert!((l[0] - expected).abs() < 1e-6, "{} vs {expected}", l[0]);
}

/// Enter on a selection: it plays the range and stops at its end.
#[test]
fn a_range_plays_once_and_stops() {
    let player = StudyPlayer::new();
    player.submit(audio(96_000));
    let mut node = node(&player);
    player.play(1_000, Some(2_000), false);
    let mut heard = Vec::new();
    for _ in 0..10 {
        heard.extend(block(&mut node).0);
    }
    assert!(!player.playing(), "it stopped at the end of the range");
    let after: usize = heard
        .iter()
        .skip(1_000 + 400)
        .filter(|v| **v != 0.0)
        .count();
    assert_eq!(after, 0, "silence after the range (and its fade)");
    assert!(player.position() <= 2_000 + 256);
}

/// A region set: Space loops it, the playhead never leaving it.
#[test]
fn a_loop_comes_back_round_and_the_playhead_stays_in_it() {
    let player = StudyPlayer::new();
    player.submit(audio(96_000));
    let mut node = node(&player);
    player.play(5_000, Some(5_600), true);
    let mut wrapped = false;
    let mut last = 0;
    for _ in 0..20 {
        block(&mut node);
        let at = player.position();
        assert!((5_000..5_600).contains(&at), "{at}");
        wrapped |= at < last;
        last = at;
    }
    assert!(wrapped, "it came back round");
    assert!(player.playing());
}

/// A click on the ruler while it plays: it carries on from there.
#[test]
fn seeking_while_playing_jumps() {
    let player = StudyPlayer::new();
    player.submit(audio(96_000));
    let mut node = node(&player);
    player.play(0, None, false);
    block(&mut node);
    player.play(50_000, None, false);
    block(&mut node);
    let at = player.position();
    assert!((50_000..50_000 + 2 * BLOCK as u64).contains(&at), "{at}");
}

/// Audio at another rate than the engine's is read at its own speed.
#[test]
fn another_rate_is_played_at_its_own_speed() {
    let player = StudyPlayer::new();
    player.submit(StudyAudio {
        sample_rate: 24_000,
        ..audio(48_000)
    });
    let mut node = node(&player);
    player.play(0, None, false);
    for _ in 0..8 {
        block(&mut node);
    }
    assert_eq!(player.position(), 4 * BLOCK as u64, "half a frame a sample");
}

/// New edits arrive while it plays: the next blocks are the new audio, at
/// the same place; and a graph rebuilt meanwhile starts with the latest.
#[test]
fn new_audio_is_swapped_in_where_it_plays() {
    let player = StudyPlayer::new();
    player.submit(audio(96_000));
    let mut node = node(&player);
    player.play(0, None, false);
    for _ in 0..4 {
        block(&mut node);
    }
    player.submit(StudyAudio {
        edited: ramp(96_000, 2e-5),
        ..audio(96_000)
    });
    let at = player.position();
    let (l, _) = block(&mut node);
    assert!((l[0] - at as f32 * 2e-5).abs() < 1e-6, "{} at {at}", l[0]);
    // What the swap retired goes back to the window's thread.
    player.reclaim();
    let mut rebuilt = self::node(&player);
    let (l, _) = block(&mut rebuilt);
    let at = player.position() - BLOCK as u64;
    assert!((l[250] - (at + 250) as f32 * 2e-5).abs() < 1e-6);
}

// ------------------------------------------ its own transport (Ty, P5 try)

/// Ty, trying the preview build: *"the playhead inside analyze musically
/// only moves when the arrangement playhead is moving ... they should have
/// no correlation"*. A stopped transport runs no graph unless something
/// says it must (`IdleGate`), and a listen in the window said nothing: the
/// node sat unprocessed, its playhead frozen, until the song rolled. The
/// transport the callback reads now asks the player too.
#[test]
fn a_listen_keeps_a_stopped_transport_awake_until_it_ends() {
    let transport = fontelle_engine::Transport::new();
    let player = StudyPlayer::new();
    transport.wake_for_preview(Arc::clone(&player));
    player.submit(audio(BLOCK * 4));
    let mut node = node(&player);
    let mut gate = fontelle_engine::IdleGate::new();
    assert!(!transport.is_playing());
    assert!(!transport.is_attended(), "nothing to listen to yet");
    gate.set_attended(transport.is_attended());
    assert!(!gate.is_awake(0), "an idle studio costs nothing");

    player.play(0, None, false);
    assert!(transport.is_attended(), "a listen wakes the graph");
    gate.set_attended(transport.is_attended());
    assert!(gate.is_awake(0));
    // The node, run as the callback runs it with the song stopped, sounds
    // and moves its playhead.
    let mut heard = false;
    for _ in 0..8 {
        let (l, _) = stopped_block(&mut node, 0);
        heard |= l.iter().any(|v| *v != 0.0);
    }
    assert!(heard, "it played with the transport stopped");
    assert!(!player.playing(), "it ran to its end");
    assert!(
        !transport.is_attended(),
        "and once it has ended the graph may sleep again"
    );
}

#[test]
fn stopping_a_listen_lets_the_graph_sleep() {
    let transport = fontelle_engine::Transport::new();
    let player = StudyPlayer::new();
    transport.wake_for_preview(Arc::clone(&player));
    player.submit(audio(96_000));
    player.play(0, None, true);
    assert!(transport.is_attended());
    player.stop();
    assert!(!transport.is_attended());
    // A plugin editor's own flag is still its own.
    transport.set_attended(true);
    assert!(transport.is_attended());
}

/// The song's clock is not the study's: a stopped, a rolling and a seeking
/// transport all leave the listen exactly where it would have been.
#[test]
fn the_song_never_moves_the_study_playhead() {
    let player = StudyPlayer::new();
    player.submit(audio(96_000));
    let mut node = node(&player);
    player.play(1_000, None, false);
    let mut positions = Vec::new();
    for (i, song_at) in [0i64, 0, 480_000, 7, 96_000_000, 0].into_iter().enumerate() {
        let (l, _) = if i % 2 == 0 {
            stopped_block(&mut node, song_at)
        } else {
            rolling_block(&mut node, song_at)
        };
        assert!(i == 0 || l.iter().any(|v| *v != 0.0));
        positions.push(player.position());
    }
    for (i, at) in positions.iter().enumerate() {
        assert_eq!(*at, 1_000 + (i as u64 + 1) * BLOCK as u64, "{positions:?}");
    }
    // A transport stop or a seek resets the graph's nodes: the listen goes on.
    node.reset();
    let before = player.position();
    stopped_block(&mut node, 0);
    assert_eq!(player.position(), before + BLOCK as u64);
    assert!(player.playing());
}

fn stopped_block(node: &mut StudyPlayerNode, song_at: i64) -> (Vec<f32>, Vec<f32>) {
    block_with(node, fontelle_engine::TransportState::Stopped, song_at)
}

fn rolling_block(node: &mut StudyPlayerNode, song_at: i64) -> (Vec<f32>, Vec<f32>) {
    block_with(node, fontelle_engine::TransportState::Playing, song_at)
}

fn block_with(
    node: &mut StudyPlayerNode,
    state: fontelle_engine::TransportState,
    song_at: i64,
) -> (Vec<f32>, Vec<f32>) {
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    {
        let mut outputs: [&mut [f32]; 2] = [&mut left, &mut right];
        let mut ctx = fontelle_engine::ProcessContext {
            inputs: &[],
            outputs: &mut outputs,
            all_events: &[],
            live_events: &[],
            audio: &[],
            node: fontelle_types::NodeId::default(),
            transport: fontelle_engine::TransportSnapshot {
                state,
                position_sample: song_at,
                ..Default::default()
            },
            sample_range: song_at..song_at + BLOCK as i64,
        };
        node.process(&mut ctx);
    }
    (left, right)
}
