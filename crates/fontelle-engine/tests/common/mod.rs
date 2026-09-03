//! Shared scaffolding for the engine's integration tests.

use fontelle_engine::{AudioNode, ProcessContext, TransportSnapshot, TransportState};

/// Runs one block of `channels` through `node`, with a stopped-clock transport
/// and no events — everything a node that only touches audio needs, and
/// nothing it does not.
pub fn process<'a>(node: &mut dyn AudioNode, channels: &'a mut [&'a mut [f32]]) {
    process_at_tempo(node, channels, fontelle_types::DEFAULT_BPM);
}

/// The same, at a stated tempo — for the nodes that read one. A synced delay
/// is the first; anything with an LFO will be the next.
pub fn process_at_tempo<'a>(
    node: &mut dyn AudioNode,
    channels: &'a mut [&'a mut [f32]],
    bpm: f32,
) {
    let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let mut ctx = ProcessContext {
        inputs: &[],
        outputs: channels,
        all_events: &[],
        live_events: &[],
        audio: &[],
        node: fontelle_types::NodeId::default(),
        transport: TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
            bpm,
        },
        sample_range: 0..frames as i64,
    };
    node.process(&mut ctx);
}
