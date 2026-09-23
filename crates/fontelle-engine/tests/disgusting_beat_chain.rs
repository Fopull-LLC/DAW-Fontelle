//! A drawn slope, through the **whole chain**: the timeline's tempo table,
//! the transport's snapshot, the node, and the DSP.
//!
//! Ty, on the first release: *"it seems the diagonal lines on time changing
//! is causing this buzz crunchy sound."* It was. Every half of the chain was
//! right on its own — the DSP resampled to the last bit, the tempo table knew
//! where the song was — and the number that crossed between them was rounded
//! to a whole tick, twenty-five samples wide at 120 bpm, once a block.
//!
//! So this test is at the level the fault lived at: nothing here checks a
//! function, it checks that a straight line drawn on the time lane comes out
//! of the graph as a clean pitch.

use fontelle_engine::{
    AudioNode, EffectNode, PrepareContext, ProcessContext, Transport, TransportReader,
    TransportState, disgusting_beat_channel, timeline_channel,
};
use fontelle_types::{
    CompiledTimeline, CurveShape, DisgustingBeatBank, DisgustingBeatConfig, DisgustingBeatGrid,
    DisgustingBeatLaneKind, DisgustingBeatLength, DisgustingBeatPoint, EffectConfig,
};

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;
/// One bar of 4/4 at 120 bpm.
const BAR: usize = 2 * SR as usize;

fn sine(freq: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f32::consts::TAU * freq * i as f32 / SR).sin())
        .collect()
}

/// The largest kink in a run of samples. A sine's is `A\u{b7}\u{3c9}\u{b2}`, tiny and
/// smooth; a jump in the read position is a step, and a step is enormous by
/// this measure — which is why it is the measure.
fn roughness(samples: &[f32]) -> f32 {
    samples
        .windows(3)
        .map(|w| (w[0] - 2.0 * w[1] + w[2]).abs())
        .fold(0.0f32, f32::max)
}

#[test]
fn a_drawn_slope_comes_out_of_the_graph_as_a_clean_pitch() {
    // Half speed over the first half of the bar: the lane falls a quarter of
    // a lane-length across half of it, so `rate = 1 + dv/dp` is 0.5 and a
    // 1 kHz tone comes out at 500 Hz.
    let mut bank = DisgustingBeatBank::new();
    {
        let lane = bank.scenes[0]
            .lane_mut(DisgustingBeatLaneKind::Time)
            .expect("a time lane");
        lane.length = DisgustingBeatLength::Bar;
        lane.on = true;
        lane.points = vec![
            DisgustingBeatPoint::new(0.0, 0.0, CurveShape::Linear),
            DisgustingBeatPoint::new(0.5, -0.25, CurveShape::Linear),
        ];
        lane.tidy(DisgustingBeatLaneKind::Time);
    }
    let (_live, source) = disgusting_beat_channel(DisgustingBeatGrid::from(&bank));
    let mut node = EffectNode::new(EffectConfig::DisgustingBeat(DisgustingBeatConfig::new()))
        .with_disgusting_beat(source);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });

    // **The real transport**, not a snapshot made up here: the reader is what
    // the audio callback drives and its snapshot is what the node is handed.
    // Building one by hand would have passed all along.
    let (_publisher, mut timeline) = timeline_channel(CompiledTimeline::default());
    let transport = Transport::new();
    transport.set_state(TransportState::Playing);
    let mut reader = TransportReader::new();

    let tone = sine(1000.0, BAR * 2);
    let mut out = Vec::with_capacity(tone.len());
    let mut at = 0usize;
    while at + BLOCK <= tone.len() {
        let step = reader.next_step(&transport, timeline.current(), BLOCK, BLOCK, false);
        let frames = step.frames.min(BLOCK);
        let mut left = tone[at..at + frames].to_vec();
        let mut right = left.clone();
        {
            let (a, b) = (&mut left[..], &mut right[..]);
            let mut channels: [&mut [f32]; 2] = [a, b];
            let mut outputs: Vec<&mut [f32]> = channels.iter_mut().map(|c| &mut **c).collect();
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: Default::default(),
                transport: step.snapshot,
                sample_range: step.range.clone(),
            };
            node.process(&mut ctx);
        }
        out.extend_from_slice(&left);
        at += frames;
    }

    // The second bar, away from both ends of the slope: the first is where
    // the memory was still filling and the last is the wrap, which is a jump
    // by design.
    let window = &out[BAR + BAR / 20..BAR + BAR * 9 / 20];
    let clean = roughness(&sine(500.0, window.len()));
    let got = roughness(window);
    assert!(
        got < clean * 4.0,
        "the slope is not smooth: the biggest kink is {got}, where a clean \
         500 Hz tone's is {clean}. A read position that jumps sounds like a \
         buzz at whatever rate it jumps."
    );
}
