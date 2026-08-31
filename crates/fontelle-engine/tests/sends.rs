//! A send: a copy of one track's signal, at a level, into another (TDD §13.2).
//!
//! `MixerTrack::sends` has been in the document since the mixer was written
//! and `Mixer::has_cycle` has counted send edges from the day it was written —
//! and `SendNode` was two lines and a comment. This is the DSP half.
//!
//! # A send is not an output
//!
//! Routing a track's *output* into a reverb sends all of it and nothing stays
//! dry. A send takes a copy and **leaves its source untouched**, which is what
//! a reverb bus is and the one property everything else here depends on.

use std::sync::Arc;

use fontelle_engine::{AudioNode, PrepareContext, ProcessContext, SendControls, SendNode};
use fontelle_engine::{TransportSnapshot, TransportState};
use fontelle_types::PanLaw;

const FRAMES: usize = 64;

fn node(controls: &Arc<SendControls>) -> SendNode {
    let mut node = SendNode::new(Arc::clone(controls), PanLaw::Minus3Db);
    node.prepare(&PrepareContext {
        sample_rate: 48_000.0,
        max_block_size: 512,
    });
    node
}

/// Runs `node` with `source` on its input bus and `dest` already carrying
/// something, and hands back both afterwards.
fn run(node: &mut SendNode, source: [f32; 2], dest: [f32; 2]) -> ([Vec<f32>; 2], [Vec<f32>; 2]) {
    let ins = [vec![source[0]; FRAMES], vec![source[1]; FRAMES]];
    let mut outs = [vec![dest[0]; FRAMES], vec![dest[1]; FRAMES]];
    {
        let inputs: Vec<&[f32]> = ins.iter().map(|c| c.as_slice()).collect();
        let (a, b) = outs.split_at_mut(1);
        let mut outputs: Vec<&mut [f32]> = vec![&mut a[0], &mut b[0]];
        let mut ctx = ProcessContext {
            inputs: &inputs,
            outputs: &mut outputs,
            all_events: &[],
            live_events: &[],
            node: Default::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: 0,
            },
            sample_range: 0..FRAMES as i64,
        };
        node.process(&mut ctx);
    }
    (ins, outs)
}

/// What `db` is as a linear gain.
fn linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[test]
fn a_send_at_unity_adds_the_whole_signal_into_its_target() {
    let controls = Arc::new(SendControls::new(0.0, 0.0, false));
    let mut node = node(&controls);
    let (_, out) = run(&mut node, [0.5, 0.5], [0.0, 0.0]);

    // Centred, on a -3 dB law: about 0.707 of it down each side.
    let (l, r) = PanLaw::Minus3Db.gains(0.0);
    assert!((out[0][0] - 0.5 * l).abs() < 1e-5, "left {}", out[0][0]);
    assert!((out[1][0] - 0.5 * r).abs() < 1e-5, "right {}", out[1][0]);
}

#[test]
fn a_send_leaves_its_source_alone() {
    // The whole difference between a send and an output, and the one thing
    // everything else depends on: the dry path goes on to the master exactly
    // as it was.
    let controls = Arc::new(SendControls::new(0.0, 0.0, false));
    let mut node = node(&controls);
    let (source, _) = run(&mut node, [0.5, -0.25], [0.0, 0.0]);
    assert!(source[0].iter().all(|s| *s == 0.5));
    assert!(source[1].iter().all(|s| *s == -0.25));
}

#[test]
fn a_send_adds_into_its_target_rather_than_writing_over_it() {
    // A reverb bus has as many things arriving at it as were sent there. A
    // node that overwrote would leave only the last one.
    let controls = Arc::new(SendControls::new(0.0, 0.0, false));
    let mut node = node(&controls);
    let (_, out) = run(&mut node, [0.25, 0.25], [0.5, 0.5]);
    assert!(out[0][0] > 0.5, "the bed was written over: {}", out[0][0]);
}

#[test]
fn the_level_is_what_decides_how_much_goes() {
    let quiet = Arc::new(SendControls::new(-20.0, 0.0, false));
    let mut node = node(&quiet);
    let (_, out) = run(&mut node, [1.0, 1.0], [0.0, 0.0]);
    let (l, _) = PanLaw::Minus3Db.gains(0.0);
    assert!((out[0][0] - linear(-20.0) * l).abs() < 1e-5, "{}", out[0][0]);
}

#[test]
fn a_send_at_the_bottom_of_its_travel_sends_nothing() {
    // Where every send starts. A new send that was audible would change the
    // mix the moment it was made.
    let controls = Arc::new(SendControls::new(-60.0, 0.0, false));
    let mut node = node(&controls);
    let (_, out) = run(&mut node, [1.0, 1.0], [0.0, 0.0]);
    assert!(
        out[0].iter().all(|s| s.abs() < 1e-3),
        "a send at the bottom is audible: {}",
        out[0][0]
    );
}

#[test]
fn the_level_can_be_moved_while_it_is_running() {
    // The same handshake a fader has: the atomics are what makes a drag
    // audible before the mouse comes up.
    let controls = Arc::new(SendControls::new(-60.0, 0.0, false));
    let mut node = node(&controls);
    let (_, silent) = run(&mut node, [1.0, 1.0], [0.0, 0.0]);
    assert!(silent[0][0].abs() < 1e-3);

    controls.set_level_db(0.0);
    let (_, loud) = run(&mut node, [1.0, 1.0], [0.0, 0.0]);
    assert!(loud[0][0] > 0.5, "the level did not reach the graph");
}

#[test]
fn a_send_can_be_placed_in_the_field() {
    let left = Arc::new(SendControls::new(0.0, -1.0, false));
    let mut node = node(&left);
    let (_, out) = run(&mut node, [1.0, 1.0], [0.0, 0.0]);
    assert!(
        out[0][0] > out[1][0],
        "hard left should favour the left: {} vs {}",
        out[0][0],
        out[1][0]
    );
}

#[test]
fn a_muted_send_sends_nothing() {
    // Sends are silenced with the track that feeds them, so a solo elsewhere
    // does not leave a reverb ringing from a part nobody can hear.
    let controls = Arc::new(SendControls::new(0.0, 0.0, true));
    let mut node = node(&controls);
    let (_, out) = run(&mut node, [1.0, 1.0], [0.0, 0.0]);
    assert!(out[0].iter().all(|s| *s == 0.0));
    assert!(out[1].iter().all(|s| *s == 0.0));
}

#[test]
fn a_mono_send_does_not_lose_three_decibels_to_a_pan_law() {
    // The same rule `MixerTrackNode` follows: pan only means something with
    // two channels to balance between, and applying the centre gain on a mono
    // bus makes every send quietly -3 dB down.
    let controls = Arc::new(SendControls::new(0.0, 0.0, false));
    let mut node = node(&controls);

    let input = vec![1.0f32; FRAMES];
    let mut output = vec![0.0f32; FRAMES];
    {
        let inputs: Vec<&[f32]> = vec![&input];
        let mut outputs: Vec<&mut [f32]> = vec![&mut output];
        let mut ctx = ProcessContext {
            inputs: &inputs,
            outputs: &mut outputs,
            all_events: &[],
            live_events: &[],
            node: Default::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: 0,
            },
            sample_range: 0..FRAMES as i64,
        };
        node.process(&mut ctx);
    }
    assert!((output[0] - 1.0).abs() < 1e-5, "{}", output[0]);
}

#[test]
fn a_send_with_more_targets_than_sources_writes_only_what_it_has() {
    // A mono track into a stereo bus. Nothing here should index past either
    // side's channel count.
    let controls = Arc::new(SendControls::new(0.0, 0.0, false));
    let mut node = node(&controls);
    let input = vec![1.0f32; FRAMES];
    let mut a = vec![0.0f32; FRAMES];
    let mut b = vec![0.0f32; FRAMES];
    {
        let inputs: Vec<&[f32]> = vec![&input];
        let mut outputs: Vec<&mut [f32]> = vec![&mut a, &mut b];
        let mut ctx = ProcessContext {
            inputs: &inputs,
            outputs: &mut outputs,
            all_events: &[],
            live_events: &[],
            node: Default::default(),
            transport: TransportSnapshot {
                state: TransportState::Playing,
                position_sample: 0,
            },
            sample_range: 0..FRAMES as i64,
        };
        node.process(&mut ctx);
    }
    assert!(a[0] > 0.0);
    assert!(b.iter().all(|s| *s == 0.0), "there was no second source");
}
