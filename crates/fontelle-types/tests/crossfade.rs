//! The automatic crossfade where two audio clips overlap (TDD §15.2).
//!
//! Reported from using the window: *"when audio clips are overlapping, in
//! addition to this, it should also blend together like a transition the
//! timing based on how long the overlap section is."*
//!
//! A placement carries how far in from each end the blend runs, in song
//! samples, and answers what gain it has at any song sample. **Equal power**
//! — a sine for the way in and a cosine for the way out — because the two
//! clips are two different recordings: a linear blend of uncorrelated
//! material dips three decibels in the middle, and a crossfade you can hear
//! as a dip is not a transition.
//!
//! This is separate from a clip's own fades (`AudioClipData::fade_in`), which
//! are in the *file's* frames and belong to the clip wherever it is put. The
//! crossfade belongs to the **placement** — it is a fact about two clips on
//! one row of the arrangement — and is worked out by the compiler.

use fontelle_types::{AssetId, AssetKind, AssetRef, AudioClipData, AudioPlacement, ClipId, NodeId};

fn a_placement(start: i64, length: i64) -> AudioPlacement {
    let asset = AssetRef {
        id: AssetId::default(),
        path: "take.wav".into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    };
    AudioPlacement {
        target: NodeId::default(),
        clip: ClipId::default(),
        range: start..start + length,
        repeat: 0,
        crossfade_in: 0,
        crossfade_out: 0,
        phase: 0,
        data: AudioClipData::whole(asset, length, 48_000),
    }
}

#[test]
fn a_placement_with_no_crossfade_is_at_full_gain_throughout() {
    let p = a_placement(1000, 500);
    for at in [1000, 1001, 1250, 1499] {
        assert_eq!(p.auto_gain(at), 1.0, "at {at}");
    }
}

#[test]
fn the_way_in_rises_from_silence_over_the_crossfade_and_no_further() {
    let mut p = a_placement(1000, 500);
    p.crossfade_in = 100;
    assert_eq!(p.auto_gain(1000), 0.0, "the first sample is silent");
    let mid = p.auto_gain(1050);
    assert!(
        (mid - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-4,
        "halfway is -3 dB, got {mid}"
    );
    assert!(
        (p.auto_gain(1100) - 1.0).abs() < 1e-6,
        "the end of the fade is full"
    );
    assert_eq!(p.auto_gain(1300), 1.0);
    // And it only ever rises.
    let mut last = 0.0;
    for at in 1000..=1100 {
        let g = p.auto_gain(at);
        assert!(g >= last, "fell at {at}");
        last = g;
    }
}

#[test]
fn the_way_out_falls_to_silence_over_the_crossfade_at_the_end() {
    let mut p = a_placement(1000, 500);
    p.crossfade_out = 100;
    assert_eq!(p.auto_gain(1000), 1.0);
    assert!(
        (p.auto_gain(1400) - 1.0).abs() < 1e-6,
        "the fade starts at full"
    );
    let mid = p.auto_gain(1450);
    assert!(
        (mid - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-4,
        "halfway is -3 dB, got {mid}"
    );
    let last = p.auto_gain(1499);
    assert!(last < 0.02, "the last sample is all but silent, got {last}");
}

#[test]
fn two_clips_blended_over_the_same_stretch_keep_a_constant_power() {
    // The whole reason for the curve: at every sample of the overlap, what
    // the one loses in power the other gains.
    let mut earlier = a_placement(0, 1000);
    earlier.crossfade_out = 300;
    let mut later = a_placement(700, 1000);
    later.crossfade_in = 300;
    for at in 700..1000 {
        let a = earlier.auto_gain(at);
        let b = later.auto_gain(at);
        let power = a * a + b * b;
        assert!((power - 1.0).abs() < 1e-4, "power {power} at {at}");
    }
}

#[test]
fn a_crossfade_longer_than_the_placement_is_clamped_to_it() {
    // A one-beat clip dropped entirely inside a longer one overlaps for its
    // whole length: it fades in over all of itself, and never past.
    let mut p = a_placement(0, 100);
    p.crossfade_in = 5000;
    assert_eq!(p.auto_gain(0), 0.0);
    assert!(p.auto_gain(99) > 0.99);
}

#[test]
fn both_ends_at_once_multiply() {
    let mut p = a_placement(0, 200);
    p.crossfade_in = 200;
    p.crossfade_out = 200;
    // Halfway: -3 dB in and -3 dB out.
    let g = p.auto_gain(100);
    assert!((g - 0.5).abs() < 1e-4, "got {g}");
}

#[test]
fn outside_the_placement_the_gain_is_nothing() {
    let mut p = a_placement(100, 100);
    p.crossfade_in = 10;
    assert_eq!(p.auto_gain(99), 0.0);
    assert_eq!(p.auto_gain(200), 0.0);
}

#[test]
fn a_looped_placement_fades_against_the_whole_block_not_each_pass() {
    // A one-bar loop dragged over the end of another clip fades in once,
    // at the front of the block; a fade at the front of every pass would be
    // a stutter.
    let mut p = a_placement(0, 1000);
    p.repeat = 100;
    p.crossfade_in = 500;
    assert_eq!(p.auto_gain(0), 0.0);
    assert!(
        p.auto_gain(100) > 0.0,
        "the second pass is not silent again"
    );
    assert!(p.auto_gain(100) > p.auto_gain(50));
    assert!((p.auto_gain(600) - 1.0).abs() < 1e-6);
}
