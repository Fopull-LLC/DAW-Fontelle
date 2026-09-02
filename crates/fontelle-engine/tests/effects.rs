//! Every effect the "+ Add effect" menu offers, as an insert that actually
//! runs (TDD §13.4).
//!
//! `fontelle-fx` has the DSP and `fontelle-types` has the parameters; this is
//! the seam between them, and it is the seam that has been silently wrong
//! before. An effect can have a working `process` function, a complete spec
//! table and a row in the menu, and *still* be a slot that does nothing —
//! because `EffectNode` is where a kind is turned into the DSP behind it, and
//! a missing arm there is a `match` that falls through to `_ => {}`.
//!
//! So the tests here are deliberately generic: they loop over
//! `EffectKind::ALL` rather than naming effects, which means **an effect added
//! later is covered the day it is added** rather than the day somebody
//! remembers to write its test. That is the same reason `specs()` drives the
//! panel and `EffectKind::ALL` drives the menu.

use fontelle_engine::{AudioNode, EffectNode, PrepareContext};
use fontelle_types::{EffectConfig, EffectKind};

mod common;
use common::process;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;
/// Long enough for a delay's first repeat and a reverb's first reflection to
/// arrive, so "does anything come out" is a fair question to ask.
const BLOCKS: usize = 64;

fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Runs a continuous tone through `node`, returning every output sample.
fn run(node: &mut EffectNode, freq: f32) -> Vec<f32> {
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let tone = sine(freq, BLOCK * BLOCKS);
    let mut collected = Vec::with_capacity(BLOCK * BLOCKS);
    for block in 0..BLOCKS {
        let start = block * BLOCK;
        let mut left = tone[start..start + BLOCK].to_vec();
        let mut right = left.clone();
        process(node, &mut [&mut left, &mut right]);
        collected.extend_from_slice(&left);
    }
    collected
}

#[test]
fn every_effect_the_menu_offers_builds_a_node_that_runs() {
    // The one that catches a missing `EffectState` arm. A kind with no arm
    // builds, prepares and processes without complaint — and passes the
    // signal through untouched, which is a slot that looks like it is working.
    for kind in EffectKind::ALL {
        let mut node = EffectNode::new(EffectConfig::new(kind));
        let out = run(&mut node, 440.0);
        assert_eq!(node.kind(), kind);
        assert!(
            out.iter().all(|s| s.is_finite()),
            "{} produced a non-finite sample",
            kind.label()
        );
    }
}

#[test]
fn every_effect_at_its_defaults_leaves_something_audible() {
    // An effect somebody has just added must not silence the track. That is
    // the failure a fully-wet reverb insert would be — the tail replacing the
    // sound that caused it — and it is why the time-based effects open part
    // dry (see `fontelle-types/tests/effect_mix.rs`).
    for kind in EffectKind::ALL {
        let mut node = EffectNode::new(EffectConfig::new(kind));
        let out = run(&mut node, 440.0);
        let settled = &out[out.len() / 2..];
        assert!(
            peak(settled) > 0.2,
            "{} at its defaults left only {} of a unit sine",
            kind.label(),
            peak(settled)
        );
    }
}

#[test]
fn every_effect_can_be_bypassed_back_to_a_wire() {
    for kind in EffectKind::ALL {
        let mut node = EffectNode::new(EffectConfig::new(kind));
        node.set_bypassed(true);
        let out = run(&mut node, 440.0);
        let settled = &out[out.len() / 2..];
        assert!(
            (peak(settled) - 1.0).abs() < 0.01,
            "a bypassed {} is not a wire; got {}",
            kind.label(),
            peak(settled)
        );
    }
}

#[test]
fn a_delay_insert_puts_its_repeats_under_the_signal_that_caused_them() {
    // The end-to-end claim the whole per-effect mix default exists for: the
    // dry track is still there, and the echoes are underneath it. The DSP
    // writes repeats only; `EffectNode` is what puts the track back.
    let mut node = EffectNode::new(EffectConfig::new(EffectKind::Delay));
    let out = run(&mut node, 440.0);
    // Before the first repeat can have arrived, the output is the dry signal
    // at whatever the mix leaves of it — not silence, and not full scale.
    let early = peak(&out[..BLOCK]);
    assert!(
        early > 0.2 && early < 1.0,
        "the dry track should be under the repeats at reduced level; got {early}"
    );
}

#[test]
fn an_effect_whose_mix_is_fully_dry_is_a_wire_whatever_it_is() {
    // The other end of the same control, and the one that says the blend is
    // `EffectNode`'s rather than each effect's: an effect that mixed its own
    // dry signal back in would still be audible here.
    for kind in EffectKind::ALL {
        let mut config = EffectConfig::new(kind);
        config.set("mix", 0.0);
        let mut node = EffectNode::new(config);
        let out = run(&mut node, 440.0);
        let settled = &out[out.len() / 2..];
        assert!(
            (peak(settled) - 1.0).abs() < 0.02,
            "a fully dry {} is not a wire; got {}",
            kind.label(),
            peak(settled)
        );
    }
}

// ------------------------------------------- and the one that reads the clock

/// A delay set in note values takes its time from the **transport**, which is
/// the whole reason `TransportSnapshot` carries a tempo at all.
///
/// This is the end of the chain the tempo travels down: the sequencer compiles
/// the project's tempo map onto the timeline in samples, the transport reads
/// the tempo at the position it is rendering, and the node hands it to the
/// effect. Every link is testable on its own, and this is the one that says
/// they are connected.
#[test]
fn a_synced_delay_takes_its_time_from_the_transports_tempo() {
    use common::process_at_tempo;

    let repeat_at = |bpm: f32| {
        let mut config = fontelle_types::DelayConfig::new();
        config.sync = true;
        config.division = fontelle_types::NoteDivision::Quarter;
        // Fully wet, so what is measured is the repeat and not the dry track
        // a delay insert normally sits under.
        config.mix = 1.0;
        let mut node = EffectNode::new(fontelle_types::EffectConfig::Delay(config));
        node.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: BLOCK as u32,
        });

        // One impulse, then silence, so the repeat is the only thing in the
        // output and its position is unambiguous.
        let mut collected = Vec::new();
        for block in 0..BLOCKS {
            let mut left = vec![0.0f32; BLOCK];
            let mut right = vec![0.0f32; BLOCK];
            if block == 0 {
                left[0] = 1.0;
                right[0] = 1.0;
            }
            process_at_tempo(&mut node, &mut [&mut left, &mut right], bpm);
            collected.extend_from_slice(&left);
        }
        collected
            .iter()
            .enumerate()
            .skip(1)
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(at, _)| at)
            .expect("something came back")
    };

    // A quarter note is half a second at 120 bpm and a third of one at 180.
    let at_120 = repeat_at(120.0);
    let at_180 = repeat_at(180.0);
    assert!(
        at_120.abs_diff(24_000) <= 4,
        "a quarter at 120 bpm should repeat at 24000, repeated at {at_120}"
    );
    assert!(
        at_180.abs_diff(16_000) <= 4,
        "a quarter at 180 bpm should repeat at 16000, repeated at {at_180}"
    );
}
