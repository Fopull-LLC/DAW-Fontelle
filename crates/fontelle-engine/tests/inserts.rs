//! `EffectNode`: one insert, in the graph, with a live end.
//!
//! Two claims, and the second is the one with architecture behind it.
//!
//! 1. An insert **processes the bus in place**, so a chain is a run of nodes
//!    on the same pair of buffers and the order they were scheduled in is the
//!    order the sound goes through them.
//! 2. Its parameters can change **while it is running**. A knob on an EQ is a
//!    control somebody drags, and the fix a fader needed applies here for the
//!    same reason: rebuilding the `CompiledGraph` to move one number
//!    deserialises every channel's patch and reloads every soundfont, sixty
//!    times a second. So an effect gets a live end, exactly as a track's fader
//!    did — see `EffectControls`.
//!
//! What makes this one harder than the fader is size: a fader is four scalars
//! and fits in atomics, and an `EqConfig` is eight bands. The mechanism is the
//! same triple buffer the compiled timeline crosses on.

use fontelle_engine::{AudioNode, EffectNode, PrepareContext, effect_channel};
use fontelle_types::{BandChannel, BandType, EffectConfig, EffectKind, EqBand, EqConfig};

mod common;
use common::process;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;

fn a_bell(freq_hz: f32, gain_db: f32) -> EqConfig {
    let mut eq = EqConfig::new();
    eq.bands[0] = EqBand {
        band_type: BandType::Bell,
        freq_hz,
        gain_db,
        q: 1.0,
        enabled: true,
        solo: false,
        channel: BandChannel::Stereo,
    };
    eq
}

fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Runs `node` over a sine long enough to settle, returning the last block's
/// peak on the left.
fn through(node: &mut EffectNode, freq: f32) -> f32 {
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let mut last = 0.0;
    for block in 0..8 {
        let start = block * BLOCK;
        let mut left = sine(freq, FRAMES)[start..start + BLOCK].to_vec();
        let mut right = left.clone();
        // The tone has to be continuous across blocks, so each block is the
        // slice of one long sine rather than eight copies of the same block.
        process(node, &mut [&mut left, &mut right]);
        last = peak(&left);
    }
    last
}

const FRAMES: usize = BLOCK * 8;

#[test]
fn an_insert_processes_the_bus_it_is_given_in_place() {
    // In place, because that is what makes a chain a chain: three inserts
    // scheduled on one bus are three effects in series, and the scheduler does
    // not have to find a spare buffer per slot.
    let mut node = EffectNode::new(EffectConfig::Eq(a_bell(1_000.0, 12.0)));
    let lifted = through(&mut node, 1_000.0);
    assert!(
        lifted > 3.0,
        "a +12 dB bell should lift a unit sine well past 1.0; got {lifted}"
    );
}

#[test]
fn an_insert_that_has_been_bypassed_is_a_wire() {
    let mut node = EffectNode::new(EffectConfig::Eq(a_bell(1_000.0, 12.0)));
    node.set_bypassed(true);
    let level = through(&mut node, 1_000.0);
    assert!(
        (level - 1.0).abs() < 0.01,
        "a bypassed insert passes the signal through; got {level}"
    );
}

#[test]
fn an_effect_reports_what_kind_it_is() {
    let node = EffectNode::new(EffectConfig::Eq(EqConfig::new()));
    assert_eq!(node.kind(), EffectKind::Eq);
}

// ------------------------------------------------------------- the live end

#[test]
fn a_knob_moves_the_sound_without_the_graph_being_rebuilt() {
    // The whole reason `EffectControls` exists. The node is built once, and
    // then something off the audio thread changes what it does — no new graph,
    // no patch deserialised, no soundfont reloaded.
    let (mut controls, source) = effect_channel(EffectConfig::Eq(a_bell(1_000.0, 0.0)));
    let mut node = EffectNode::new(EffectConfig::Eq(a_bell(1_000.0, 0.0))).with_controls(source);

    let flat = through(&mut node, 1_000.0);
    assert!((flat - 1.0).abs() < 0.01, "flat to begin with; got {flat}");

    controls.publish(EffectConfig::Eq(a_bell(1_000.0, 12.0)));
    let lifted = through(&mut node, 1_000.0);
    assert!(
        lifted > 3.0,
        "and the same node, unrebuilt, now lifts; got {lifted}"
    );
}

#[test]
fn a_node_with_no_live_end_keeps_the_config_it_was_built_with() {
    // An offline render builds its nodes and never touches them again. It must
    // not need a control surface to make a sound.
    let mut node = EffectNode::new(EffectConfig::Eq(a_bell(1_000.0, 12.0)));
    assert!(through(&mut node, 1_000.0) > 3.0);
}

#[test]
fn publishing_nothing_leaves_the_effect_where_it_was() {
    let (_controls, source) = effect_channel(EffectConfig::Eq(a_bell(1_000.0, 12.0)));
    let mut node = EffectNode::new(EffectConfig::Eq(EqConfig::new())).with_controls(source);
    // The channel's initial value is what the node reads on its first block:
    // one source of truth, not two that agree until they don't.
    assert!(through(&mut node, 1_000.0) > 3.0);
}

#[test]
fn the_live_end_can_bypass_as_well_as_tune() {
    // Bypass is a control like any other and gets dragged past just as often.
    let (mut controls, source) = effect_channel(EffectConfig::Eq(a_bell(1_000.0, 12.0)));
    let mut node = EffectNode::new(EffectConfig::Eq(EqConfig::new())).with_controls(source);
    assert!(through(&mut node, 1_000.0) > 3.0);

    controls.set_bypassed(true);
    let level = through(&mut node, 1_000.0);
    assert!(
        (level - 1.0).abs() < 0.01,
        "switched out from the live end; got {level}"
    );
}

// ------------------------------------------------------------------ plumbing

// ------------------------------------------------------------- dry and wet ---
//
// Asked for from using the mixer: *"i should have a knob to adjust the sound
// of the dry sound (before the plugin) and the wet sound (after the plugin
// processes the dry sound) blending"*. The parameter is
// `fontelle_types::EffectConfig::mix` and the blend is here, because this is
// the one place that has both signals: the bus as it arrived, and the bus
// after the effect has had it.
//
// The dry copy is taken in `prepare`-sized scratch, so no block allocates
// (INVARIANT 1) — `no_allocation_during_render.rs` is what holds that.

/// The peak of one settled block of `freq` through `node`, and the peak of the
/// same tone with nothing done to it.
fn wet_and_dry(mix: f32, freq: f32, gain_db: f32) -> (f32, f32) {
    let mut eq = a_bell(freq, gain_db);
    eq.mix = mix;
    let mut node = EffectNode::new(EffectConfig::Eq(eq));
    let wet = through(&mut node, freq);
    let dry = peak(&sine(freq, BLOCK));
    (wet, dry)
}

#[test]
fn an_insert_mixed_dry_is_a_wire() {
    // The other end of the bypass: bypass switches the effect *out*, and a mix
    // of zero leaves it running and inaudible. Both have to be exact, because
    // a wire that is only nearly a wire is a mix that changes when you touch a
    // control you meant to leave alone.
    let (out, dry) = wet_and_dry(0.0, 1_000.0, 12.0);
    assert!(
        (out - dry).abs() < 1e-4,
        "a dry insert changed the signal: {out} against {dry}"
    );
}

#[test]
fn an_insert_mixed_wet_is_the_effect_it_always_was() {
    let (out, dry) = wet_and_dry(1.0, 1_000.0, 12.0);
    let lift = 20.0 * (out / dry).log10();
    assert!(
        (lift - 12.0).abs() < 0.5,
        "a fully wet insert must be the effect: {lift} dB of lift"
    );
}

#[test]
fn half_way_is_half_way_between_the_two() {
    // At the bell's own centre the filter's phase shift is zero, so the two
    // signals add as their peaks do — which is what makes this checkable
    // against arithmetic rather than against a previous run.
    let (wet, dry) = wet_and_dry(1.0, 1_000.0, 12.0);
    let (half, _) = wet_and_dry(0.5, 1_000.0, 12.0);
    let expected = (wet + dry) / 2.0;
    assert!(
        (half - expected).abs() < expected * 0.02,
        "expected about {expected}, got {half}"
    );
}

#[test]
fn the_mix_moves_while_the_effect_is_running() {
    // The same claim the knobs make, for the same reason: this is a control
    // somebody drags, and rebuilding the graph to move it would reload every
    // soundfont in the project sixty times a second.
    let mut eq = a_bell(1_000.0, 12.0);
    eq.mix = 1.0;
    let (mut controls, source) = effect_channel(EffectConfig::Eq(eq));
    let mut node = EffectNode::new(EffectConfig::Eq(eq)).with_controls(source);
    let loud = through(&mut node, 1_000.0);

    eq.mix = 0.0;
    controls.publish(EffectConfig::Eq(eq));
    let dry = through(&mut node, 1_000.0);
    assert!(
        dry < loud * 0.4,
        "the mix did not reach the running effect: {dry} against {loud}"
    );
}

#[test]
fn a_bypassed_insert_is_a_wire_whatever_its_mix_says() {
    let mut eq = a_bell(1_000.0, 12.0);
    eq.mix = 0.5;
    let mut node = EffectNode::new(EffectConfig::Eq(eq));
    node.set_bypassed(true);
    let out = through(&mut node, 1_000.0);
    let dry = peak(&sine(1_000.0, BLOCK));
    assert!((out - dry).abs() < 1e-4, "a bypassed insert is not a wire");
}

#[test]
fn a_reset_clears_the_effects_tail() {
    let mut node = EffectNode::new(EffectConfig::Eq({
        let mut eq = EqConfig::new();
        eq.bands[0] = EqBand {
            band_type: BandType::LowPass12,
            freq_hz: 200.0,
            gain_db: 0.0,
            q: 1.0,
            enabled: true,
            solo: false,
            channel: BandChannel::Stereo,
        };
        eq
    }));
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });

    let mut loud = vec![1.0f32; BLOCK];
    let mut other = vec![1.0f32; BLOCK];
    process(&mut node, &mut [&mut loud, &mut other]);
    node.reset();

    let mut quiet = vec![0.0f32; BLOCK];
    let mut other = vec![0.0f32; BLOCK];
    process(&mut node, &mut [&mut quiet, &mut other]);
    assert!(
        quiet.iter().all(|s| s.abs() < 1e-9),
        "a reset effect fed silence makes silence"
    );
}

#[test]
fn an_effect_adds_no_latency() {
    // None of §13.4's effects but the limiter has any, and the limiter is not
    // an insert. Worth asserting rather than assuming: latency that nothing
    // compensates is a phase error nobody sees coming (plan item 16).
    let node = EffectNode::new(EffectConfig::Eq(EqConfig::new()));
    assert_eq!(node.latency_samples(), 0);
}
