//! An insert's **external key**: another track's bus, read by this insert's
//! detector (`docs/effects-catalogue.md` §2.1, TDD §13.4).
//!
//! The document names the edge and `fontelle-app`'s compiler orders it; this
//! is the mechanism in between — a [`KeyTapNode`] on the source track's bus and
//! an [`EffectNode`] that reads what it left. Both ends are the audio thread
//! in one pass, which is what makes a `KeyTap` a plain shared block rather
//! than the ring the analyser needs.
//!
//! The measurement throughout is **ducking**: a quiet signal through a
//! compressor set to squash, and a loud key. Without the key nothing happens,
//! because the signal is nowhere near the threshold; with it, the signal is
//! pulled down by something it does not contain, which is the whole definition
//! of a sidechain.

use fontelle_engine::{AudioNode, EffectNode, KeyTap, KeyTapNode, PrepareContext};
use fontelle_types::{CompressorConfig, EffectConfig};

mod common;
use common::process;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// A compressor that squashes anything over −40 dB, so a loud key is
/// unmistakable and a quiet signal on its own is untouched.
fn squasher() -> EffectConfig {
    EffectConfig::Compressor(CompressorConfig {
        threshold_db: -40.0,
        ratio: 20.0,
        attack_ms: 0.1,
        release_ms: 50.0,
        knee_db: 0.0,
        ..CompressorConfig::new()
    })
}

fn prepared(node: &mut EffectNode) {
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
}

/// A block of `level`, in both channels.
fn block(level: f32) -> (Vec<f32>, Vec<f32>) {
    (vec![level; BLOCK], vec![level; BLOCK])
}

// ------------------------------------------------------------------- the tap

#[test]
fn the_tap_carries_the_loudest_side_of_the_block_it_was_given() {
    // The loudest, not the sum and not the average: a stereo-linked detector
    // asks how loud the key is, and a kick panned hard left must not read six
    // decibels quieter than the same kick in the middle.
    let tap = KeyTap::new(BLOCK);
    let mut left = vec![0.25f32; BLOCK];
    let mut right = vec![0.75f32; BLOCK];
    tap.write(&[&mut left, &mut right]);
    let mut out = vec![0.0; BLOCK];
    assert_eq!(tap.read_into(&mut out), BLOCK);
    assert!(out.iter().all(|s| (*s - 0.75).abs() < 1e-6), "{:?}", &out[..4]);
}

#[test]
fn a_tap_nobody_has_filled_reads_silence() {
    // Which is the right answer rather than a defensive one: a compressor
    // keyed to a track that is not playing should not be compressing.
    let tap = KeyTap::new(BLOCK);
    let mut out = vec![1.0; BLOCK];
    assert_eq!(tap.read_into(&mut out), 0);
    assert_eq!(peak(&out), 0.0);
}

#[test]
fn a_short_block_does_not_leave_the_long_one_behind_it() {
    // The device is under no obligation to deliver a full block, and a key
    // that kept the tail of a longer one would duck on audio that has already
    // been and gone.
    let tap = KeyTap::new(BLOCK);
    let mut long = vec![1.0f32; BLOCK];
    let mut long_r = long.clone();
    tap.write(&[&mut long, &mut long_r]);
    let mut short = vec![0.0f32; 32];
    let mut short_r = short.clone();
    tap.write(&[&mut short, &mut short_r]);
    let mut out = vec![9.0; BLOCK];
    assert_eq!(tap.read_into(&mut out), 32);
    assert_eq!(peak(&out), 0.0, "the long block survived the short one");
}

#[test]
fn the_tap_node_leaves_the_track_it_listens_to_alone() {
    // A key takes a copy, the way a send does. One that altered its source
    // would be a sidechain you could hear on the wrong channel.
    let tap = std::sync::Arc::new(KeyTap::new(BLOCK));
    let mut node = KeyTapNode::new(std::sync::Arc::clone(&tap));
    let (mut left, mut right) = block(0.5);
    process(&mut node, &mut [&mut left, &mut right]);
    assert!(left.iter().all(|s| *s == 0.5));
    assert!(right.iter().all(|s| *s == 0.5));
    assert_eq!(tap.frames(), BLOCK);
}

// ---------------------------------------------------------------- the ducking

#[test]
fn an_insert_with_no_key_listens_to_the_signal_passing_through_it() {
    // The behaviour every compressor in this program had before keys existed,
    // and the one it has to keep: a quiet signal under the threshold comes
    // out untouched.
    let mut node = EffectNode::new(squasher());
    prepared(&mut node);
    let (mut left, mut right) = block(0.001);
    process(&mut node, &mut [&mut left, &mut right]);
    assert!(
        (peak(&left) - 0.001).abs() < 1e-5,
        "a quiet signal was compressed by its own level: {}",
        peak(&left)
    );
}

#[test]
fn a_loud_key_ducks_a_quiet_signal() {
    // The sidechain, as one measurement: the signal is pulled down by
    // something it does not contain.
    let tap = std::sync::Arc::new(KeyTap::new(BLOCK));
    let mut source = KeyTapNode::new(std::sync::Arc::clone(&tap));
    let mut node = EffectNode::new(squasher()).with_key(std::sync::Arc::clone(&tap));
    prepared(&mut node);

    // The source track runs first — which is what the compiler guarantees.
    let (mut kick_l, mut kick_r) = block(1.0);
    process(&mut source, &mut [&mut kick_l, &mut kick_r]);

    let (mut left, mut right) = block(0.001);
    process(&mut node, &mut [&mut left, &mut right]);
    // The second half of the block, which is past the attack: the first
    // samples of the first block a key arrives in are the ramp, and measuring
    // the peak over the whole thing measures how fast the compressor opened
    // rather than how far it closed.
    let ducked = peak(&left[BLOCK / 2..]);
    // 20:1 at 40 dB over the threshold is 38 dB down, so a −60 dBFS signal
    // comes out near −98.
    assert!(
        ducked < 2e-5,
        "a full-scale key did not duck a −60 dB signal: {ducked}"
    );
}

#[test]
fn the_key_is_this_blocks_signal_and_not_the_last_ones() {
    // Why the order matters. The key falls silent, and the compressor has to
    // notice in the block it happens rather than the one after: a detector
    // reading a stale tap would release a block late, every block, which is a
    // duck permanently out of time with the kick that caused it.
    let tap = std::sync::Arc::new(KeyTap::new(BLOCK));
    let mut source = KeyTapNode::new(std::sync::Arc::clone(&tap));
    let mut node = EffectNode::new(EffectConfig::Compressor(CompressorConfig {
        // Instant, so the block's first samples say what the detector saw.
        attack_ms: 0.01,
        release_ms: 0.01,
        ..match squasher() {
            EffectConfig::Compressor(config) => config,
            _ => unreachable!(),
        }
    }))
    .with_key(std::sync::Arc::clone(&tap));
    prepared(&mut node);

    // A loud block, then a silent one.
    let (mut loud_l, mut loud_r) = block(1.0);
    process(&mut source, &mut [&mut loud_l, &mut loud_r]);
    let (mut left, mut right) = block(0.001);
    process(&mut node, &mut [&mut left, &mut right]);
    assert!(peak(&left) < 0.0002, "the loud key did not duck");

    let (mut quiet_l, mut quiet_r) = block(0.0);
    process(&mut source, &mut [&mut quiet_l, &mut quiet_r]);
    let (mut left, mut right) = block(0.001);
    process(&mut node, &mut [&mut left, &mut right]);
    assert!(
        peak(&left) > 0.0009,
        "the key went silent and the duck stayed: {}",
        peak(&left)
    );
}

#[test]
fn a_bypassed_insert_ignores_its_key() {
    // A bypass is a wire, whatever is arriving on the sidechain.
    let tap = std::sync::Arc::new(KeyTap::new(BLOCK));
    let mut source = KeyTapNode::new(std::sync::Arc::clone(&tap));
    let mut node = EffectNode::new(squasher()).with_key(std::sync::Arc::clone(&tap));
    node.set_bypassed(true);
    prepared(&mut node);
    let (mut kick_l, mut kick_r) = block(1.0);
    process(&mut source, &mut [&mut kick_l, &mut kick_r]);
    let (mut left, mut right) = block(0.001);
    process(&mut node, &mut [&mut left, &mut right]);
    assert!((peak(&left) - 0.001).abs() < 1e-6);
}

#[test]
fn a_stop_leaves_the_key_silent_rather_than_holding_the_last_kick() {
    let tap = std::sync::Arc::new(KeyTap::new(BLOCK));
    let mut source = KeyTapNode::new(std::sync::Arc::clone(&tap));
    let (mut kick_l, mut kick_r) = block(1.0);
    process(&mut source, &mut [&mut kick_l, &mut kick_r]);
    assert_eq!(tap.frames(), BLOCK);
    source.reset();
    assert_eq!(tap.frames(), 0);
}
