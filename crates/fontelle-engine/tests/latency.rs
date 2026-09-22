//! Delay compensation: lining up paths that do not cost the same (TDD §5.5).
//!
//! An insert that looks ahead delays the track it is on. Two tracks summing
//! into one bus, one of them delayed, is a mix where the drums arrive at two
//! different times — and it is the kind of error nobody hears as an error:
//! it sounds like a loose player, or a smeared transient.
//!
//! [`DelayNode`] is the aligner. It is not an effect — there is no feedback,
//! no mix, and nothing to set: it is a fixed number of samples of nothing,
//! put on the path that arrives early so both arrive together.

use fontelle_engine::{AudioNode, DelayNode, PrepareContext};

mod common;
use common::process;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn node(samples: u32) -> DelayNode {
    let mut node = DelayNode::new(samples);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    node
}

/// One impulse in, one impulse out, `samples` later.
#[test]
fn a_delay_node_holds_the_signal_back_by_exactly_what_it_was_told() {
    let mut node = node(10);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    left[0] = 1.0;
    right[0] = 1.0;
    process(&mut node, &mut [&mut left, &mut right]);

    let hit = left.iter().position(|s| *s > 0.5);
    assert_eq!(hit, Some(10), "{hit:?}");
    assert_eq!(right.iter().position(|s| *s > 0.5), Some(10));
    assert_eq!(
        left.iter().filter(|s| s.abs() > 1e-9).count(),
        1,
        "one impulse in, one out"
    );
}

/// Zero is a wire, and costs nothing: the common case is a track that needs
/// no compensation at all.
#[test]
fn a_delay_of_nothing_is_a_wire() {
    let mut node = node(0);
    let mut left = vec![0.0f32; BLOCK];
    left[3] = 1.0;
    let mut right = vec![0.0f32; BLOCK];
    process(&mut node, &mut [&mut left, &mut right]);
    assert_eq!(left.iter().position(|s| *s > 0.5), Some(3));
}

/// The whole point is a delay longer than a block: five milliseconds is 240
/// samples and a block is 128, so what it holds has to survive two block
/// boundaries.
#[test]
fn a_delay_longer_than_a_block_carries_across_blocks() {
    let mut node = node(200);
    let mut blocks: Vec<Vec<f32>> = (0..4).map(|_| vec![0.0f32; BLOCK]).collect();
    blocks[0][5] = 1.0;

    let mut hits = Vec::new();
    for (index, block) in blocks.iter_mut().enumerate() {
        let mut right = vec![0.0f32; BLOCK];
        process(&mut node, &mut [block, &mut right]);
        if let Some(at) = block.iter().position(|s| *s > 0.5) {
            hits.push(index * BLOCK + at);
        }
    }
    assert_eq!(hits, vec![205], "one impulse, 200 samples late");
}

/// It reports what it costs, like every other node — a compensator that lied
/// about its own latency would be compensated for in turn.
#[test]
fn a_delay_node_reports_its_own_latency() {
    assert_eq!(node(240).latency_samples(), 240);
    assert_eq!(node(0).latency_samples(), 0);
}

/// A stop empties it. What it was holding belongs to the music that was
/// playing, and playing it over whatever comes next is the same mistake a
/// reverb tail across a seek would be.
#[test]
fn a_reset_empties_the_line() {
    let mut node = node(50);
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    left[0] = 1.0;
    process(&mut node, &mut [&mut left, &mut right]);
    assert!(left.iter().any(|s| *s > 0.5));

    node.reset();
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    process(&mut node, &mut [&mut left, &mut right]);
    assert!(
        left.iter().all(|s| s.abs() < 1e-9),
        "the line still held the impulse"
    );
}

// --- the corrector, whose latency is a function of its settings ------------
//
// `docs/tune-plan.md` §9.5. The gate's look-ahead is a knob; this effect's
// latency is a **function of the range and the mode** (§3.8), which is the
// first insert whose dry line cannot be sized from one constant.

use fontelle_engine::{EffectNode, insert_latency_samples, max_insert_latency_samples};
use fontelle_types::{EffectConfig, TuneConfig, TuneMode, TuneRange};

fn tune_node(config: TuneConfig) -> EffectNode {
    let mut node = EffectNode::new(EffectConfig::Tune(config));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    node
}

/// The graph and the node have to agree, or every track on the mix is a few
/// milliseconds out from the one beside it.
#[test]
fn a_tune_reports_the_latency_the_document_says_it_costs() {
    for range in TuneRange::ALL {
        for mode in TuneMode::ALL {
            let config = TuneConfig {
                range,
                mode,
                ..TuneConfig::new()
            };
            let expected = config.latency_samples(SR);
            assert_eq!(
                insert_latency_samples(&EffectConfig::Tune(config), SR, 120.0, 4),
                expected,
                "the document's answer for {range:?} in {mode:?}"
            );
            assert_eq!(
                tune_node(config).latency_samples(),
                expected,
                "the node's answer for {range:?} in {mode:?}"
            );
        }
    }
}

/// The dry line is sized for the **most** this insert could ever ask for, so
/// that winding the range down and back up again reaches for nothing on the
/// audio thread (INVARIANT 1) — the same rule the gate's line follows.
#[test]
fn the_dry_line_is_sized_for_the_widest_range_and_the_deeper_mode() {
    let widest = TuneConfig {
        range: TuneRange::Low,
        mode: TuneMode::Studio,
        ..TuneConfig::new()
    };
    let most = widest.latency_samples(SR);
    for range in TuneRange::ALL {
        for mode in TuneMode::ALL {
            let config = EffectConfig::Tune(TuneConfig {
                range,
                mode,
                ..TuneConfig::new()
            });
            assert_eq!(
                max_insert_latency_samples(&config, SR),
                most,
                "every tune sizes its line for the same worst case"
            );
        }
    }
    // And the gate's is still its own knob's top, which is what it was.
    let gate = EffectConfig::Gate(fontelle_types::GateConfig::new());
    assert_eq!(
        max_insert_latency_samples(&gate, SR),
        (fontelle_types::MAX_GATE_LOOKAHEAD_MS / 1000.0 * SR).ceil() as u32
    );
    // And an effect with no latency at all asks for no line.
    let eq = EffectConfig::Eq(fontelle_types::EqConfig::new());
    assert_eq!(max_insert_latency_samples(&eq, SR), 0);
}

/// An impulse through a track carrying a tuner comes out exactly as late as
/// the tuner said it would, so a [`DelayNode`] on its sibling lines the two
/// up — which is the whole of §5.5 for this effect.
#[test]
fn a_tune_in_studio_mode_is_padded_like_a_gate_with_look_ahead() {
    let config = TuneConfig {
        range: TuneRange::AltoTenor,
        mode: TuneMode::Studio,
        ..TuneConfig::new()
    };
    let latency = config.latency_samples(SR) as usize;
    let mut node = tune_node(config);

    // An impulse, and enough blocks after it for the answer to come out.
    let total = latency + BLOCK * 4;
    let mut left = vec![0.0f32; total];
    left[10] = 1.0;
    let right = left.clone();
    let mut out_left = Vec::new();
    let mut out_right = Vec::new();
    for block in 0..total / BLOCK {
        let (from, to) = (block * BLOCK, (block + 1) * BLOCK);
        let mut a = left[from..to].to_vec();
        let mut b = right[from..to].to_vec();
        process(&mut node, &mut [&mut a, &mut b]);
        out_left.extend_from_slice(&a);
        out_right.extend_from_slice(&b);
    }
    let peak = out_left
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map(|(i, _)| i)
        .expect("something came out");
    assert!(
        (peak as i64 - (10 + latency) as i64).abs() <= 2,
        "the impulse landed at {peak}, and the insert says it costs {latency}"
    );
    // And a sibling track held back by the same amount arrives with it.
    let mut sibling = DelayNode::new(latency as u32);
    sibling.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let mut a = vec![0.0f32; BLOCK];
    let mut b = vec![0.0f32; BLOCK];
    a[10] = 1.0;
    b[10] = 1.0;
    let mut sibling_out = Vec::new();
    for _ in 0..total / BLOCK {
        process(&mut sibling, &mut [&mut a, &mut b]);
        sibling_out.extend_from_slice(&a);
        a.fill(0.0);
        b.fill(0.0);
    }
    let sibling_peak = sibling_out
        .iter()
        .position(|s| s.abs() > 0.5)
        .expect("the compensator passed the impulse");
    assert_eq!(sibling_peak, 10 + latency);
}

/// The comb test the gate has, for the insert whose latency is thirty times
/// the gate's: a corrector doing nothing at half mix must still be a wire.
#[test]
fn a_tune_at_half_mix_does_not_comb_against_its_own_dry() {
    for range in TuneRange::ALL {
        // `amount` at zero is one of the two ways to make this a wire (§4.7),
        // so the wet path is the input delayed and nothing else.
        let wire = TuneConfig {
            range,
            amount: 0.0,
            ..TuneConfig::new()
        };
        let latency = wire.latency_samples(SR) as usize;
        let total = latency * 3 + BLOCK * 16;
        let input: Vec<f32> = (0..total)
            .map(|i| (std::f32::consts::TAU * 1_000.0 * i as f32 / SR).sin() * 0.5)
            .collect();

        let render = |mix: f32| -> Vec<f32> {
            let mut node = tune_node(TuneConfig { mix, ..wire });
            let mut out = Vec::with_capacity(total);
            for block in 0..total / BLOCK {
                let (from, to) = (block * BLOCK, (block + 1) * BLOCK);
                let mut a = input[from..to].to_vec();
                let mut b = input[from..to].to_vec();
                process(&mut node, &mut [&mut a, &mut b]);
                out.extend_from_slice(&a);
            }
            out
        };
        let wet = render(1.0);
        for mix in [0.5, 0.25, 0.75] {
            let blended = render(mix);
            // The first grains are laid before the tracker has heard a note,
            // at the period the corrector opens on rather than at the one
            // being sung, and where those overlap the first real ones the
            // window sum is not quite one. That is a handful of milliseconds
            // at the top of the very first sound and it is not what this test
            // is about — the comb it *is* about would run for as long as the
            // track does.
            // Past the shifter's own ramp-up, which is as long as the
            // tracker takes to be sure of a note — three of the range's
            // longest periods, so it scales with the latency.
            let start = latency * 2 + BLOCK * 4;
            let error = blended[start..]
                .iter()
                .zip(&wet[start..])
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(
                error < 1e-3,
                "{range:?} at mix {mix} combs against its own dry: worst sample off by {error}"
            );
        }
    }
}
