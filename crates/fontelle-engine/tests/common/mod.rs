//! Shared scaffolding for the engine's integration tests.

use fontelle_engine::{AudioNode, ProcessContext, TransportSnapshot, TransportState};

/// Runs one block of `channels` through `node`, with a stopped-clock transport
/// and no events — everything a node that only touches audio needs, and
/// nothing it does not.
pub fn process<'a>(node: &mut dyn AudioNode, channels: &'a mut [&'a mut [f32]]) {
    let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    let mut ctx = ProcessContext {
        inputs: &[],
        outputs: channels,
        all_events: &[],
        live_events: &[],
        node: fontelle_types::NodeId::default(),
        transport: TransportSnapshot {
            state: TransportState::Playing,
            position_sample: 0,
        },
        sample_range: 0..frames as i64,
    };
    node.process(&mut ctx);
}
