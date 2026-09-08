//! What the player makes of a crossfade: two overlapping clips, one sound.
//!
//! `fontelle-types/tests/crossfade.rs` is the curve and
//! `fontelle-sequencer/tests/crossfade.rs` is the length. This is the node
//! applying both, per sample, against the transport's own position — so the
//! blend lands on the same samples whatever the block size.

use std::sync::Arc;

use fontelle_core::{AudioBuffer, AudioStore};
use fontelle_engine::{
    AudioClipNode, AudioNode, PrepareContext, ProcessContext, TransportSnapshot, TransportState,
};
use fontelle_types::{AssetId, AssetKind, AssetRef, AudioClipData, AudioPlacement, ClipId, NodeId};

const RATE: f32 = 48_000.0;

fn ids<K: slotmap::Key>(n: usize) -> Vec<K> {
    let mut map: slotmap::SlotMap<K, ()> = slotmap::SlotMap::with_key();
    (0..n).map(|_| map.insert(())).collect()
}

fn an_id<K: slotmap::Key>() -> K {
    ids(1)[0]
}

/// A mono buffer of `frames` frames all at `level`.
fn constant(frames: usize, level: f32) -> AudioBuffer {
    AudioBuffer {
        data: Arc::from(vec![level; frames]),
        sample_rate: 48_000,
        channels: 1,
    }
}

fn an_asset(id: AssetId) -> AssetRef {
    AssetRef {
        id,
        path: "take.wav".into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    }
}

struct Rig {
    node: AudioClipNode,
    node_id: NodeId,
    asset: AssetId,
    clips: slotmap::SlotMap<ClipId, ()>,
}

fn rig(buffer: AudioBuffer) -> Rig {
    let asset: AssetId = an_id();
    let mut store = AudioStore::new();
    store.insert(asset, buffer);
    let mut node = AudioClipNode::new(Arc::new(store));
    node.prepare(&PrepareContext {
        sample_rate: RATE,
        max_block_size: 1024,
    });
    Rig {
        node,
        node_id: an_id(),
        asset,
        clips: slotmap::SlotMap::with_key(),
    }
}

impl Rig {
    fn placement(
        &mut self,
        start: i64,
        length: i64,
        crossfade_in: i64,
        crossfade_out: i64,
    ) -> AudioPlacement {
        AudioPlacement {
            target: self.node_id,
            clip: self.clips.insert(()),
            range: start..start + length,
            repeat: 0,
            crossfade_in,
            crossfade_out,
            data: AudioClipData::whole(an_asset(self.asset), length, 48_000),
        }
    }

    fn render(
        &mut self,
        placements: &[AudioPlacement],
        from: i64,
        frames: usize,
        block: usize,
    ) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames);
        let mut at = from;
        while out.len() < frames {
            let n = block.min(frames - out.len());
            let mut left = vec![0.0f32; n];
            let mut right = vec![0.0f32; n];
            {
                let mut outputs: [&mut [f32]; 2] = [&mut left, &mut right];
                let mut ctx = ProcessContext {
                    inputs: &[],
                    outputs: &mut outputs,
                    all_events: &[],
                    live_events: &[],
                    audio: placements,
                    node: self.node_id,
                    transport: TransportSnapshot {
                        state: TransportState::Playing,
                        position_sample: at,
                        bpm: 120.0,
                    },
                    sample_range: at..at + n as i64,
                };
                self.node.process(&mut ctx);
            }
            out.extend_from_slice(&left);
            at += n as i64;
        }
        out
    }
}

#[test]
fn two_clips_blend_over_their_overlap_at_equal_power() {
    // A at 0..200 fading out over its last 100; B at 100..300 fading in
    // over its first 100. Both are a constant 1.0.
    let mut r = rig(constant(1000, 1.0));
    let a = r.placement(0, 200, 0, 100);
    let b = r.placement(100, 200, 100, 0);
    let out = r.render(&[a, b], 0, 300, 300);

    // Before the overlap: A alone, untouched.
    for (i, v) in out[..100].iter().enumerate() {
        assert!((v - 1.0).abs() < 1e-6, "frame {i} is {v}");
    }
    // The first sample of the overlap: all A, none of B.
    assert!((out[100] - 1.0).abs() < 1e-4, "frame 100 is {}", out[100]);
    // The middle: both at -3 dB, summing to √2.
    let mid = out[150];
    assert!(
        (mid - std::f32::consts::SQRT_2).abs() < 1e-3,
        "frame 150 is {mid}"
    );
    // After the overlap: B alone.
    for (i, v) in out[200..].iter().enumerate() {
        assert!((v - 1.0).abs() < 1e-6, "frame {} is {v}", i + 200);
    }
    // And the power is constant across the whole blend.
    let a_gain =
        |i: usize| ((1.0 - (i as f32 - 100.0) / 100.0) * std::f32::consts::FRAC_PI_2).sin();
    let b_gain = |i: usize| (((i as f32 - 100.0) / 100.0) * std::f32::consts::FRAC_PI_2).sin();
    for (i, value) in out.iter().enumerate().take(200).skip(100) {
        let wanted = a_gain(i) + b_gain(i);
        assert!(
            (value - wanted).abs() < 1e-3,
            "frame {i} is {value} not {wanted}"
        );
    }
}

#[test]
fn the_blend_lands_on_the_same_samples_whatever_the_block_size() {
    let mut r = rig(constant(1000, 1.0));
    let a = r.placement(0, 200, 0, 100);
    let b = r.placement(100, 200, 100, 0);
    let placements = [a, b];
    let whole = r.render(&placements, 0, 300, 300);
    for block in [1, 7, 85, 128] {
        let split = r.render(&placements, 0, 300, block);
        assert_eq!(split, whole, "blocks of {block} render differently");
    }
}

#[test]
fn a_clips_own_fade_and_the_crossfade_both_apply() {
    // A clip with its own fade-in of 50 frames, placed to crossfade in over
    // 100: the two multiply, so the first 50 frames are quieter than the
    // crossfade alone would make them and the rest are exactly it.
    let mut r = rig(constant(1000, 1.0));
    let mut b = r.placement(100, 200, 100, 0);
    b.data.fade_in = fontelle_types::Fade {
        frames: 50,
        ..fontelle_types::Fade::NONE
    };
    let out = r.render(&[b], 0, 300, 300);
    let crossfade = |i: usize| (((i as f32 - 100.0) / 100.0) * std::f32::consts::FRAC_PI_2).sin();
    assert!(
        out[125] < crossfade(125) - 0.1,
        "frame 125 is {} — the clip's fade was ignored",
        out[125]
    );
    assert!(
        (out[175] - crossfade(175)).abs() < 1e-3,
        "frame 175 is {}",
        out[175]
    );
}
