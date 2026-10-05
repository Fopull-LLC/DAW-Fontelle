//! The output moved to another device, or another buffer, while the song
//! plays — on the real default device, with the allocator that panics on an
//! allocation in the audio callback (INVARIANT 1).
//!
//! Skipped, saying so, on a machine with no output that opens (CI's Linux
//! runners have no sound card).

use std::sync::Arc;
use std::time::Duration;

use fontelle_engine::{
    AudioOutput, BufferPool, CompiledGraph, MixerTrackNode, OutputChoice, RtGuardAllocator,
    ScheduledNode, Transport,
};
use fontelle_types::{CompiledTimeline, NodeId};
use slotmap::Key;

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

fn silent_graph() -> CompiledGraph {
    CompiledGraph {
        schedule: vec![ScheduledNode {
            id: NodeId::null(),
            node: Box::new(MixerTrackNode::new()),
            input_buffers: vec![0, 1],
            output_buffers: vec![0, 1],
        }],
        buffer_pool: BufferPool::with_capacity(2, fontelle_engine::BLOCK_SIZE),
    }
}

#[test]
fn the_song_carries_on_through_a_change_of_device_and_a_missing_one_falls_back() {
    let transport = Arc::new(Transport::new());
    transport.play();
    let missing = OutputChoice {
        host: None,
        device: Some("No Such Device Anywhere".into()),
        buffer_frames: None,
    };
    let mut output = match AudioOutput::start(
        fontelle_engine::graph_channel(silent_graph()).1,
        fontelle_engine::timeline_channel(CompiledTimeline::empty()).1,
        48_000,
        Arc::clone(&transport),
        None,
        None,
        &missing,
    ) {
        Ok(output) => output,
        Err(e) => {
            eprintln!("skipped: no output opens on this machine ({e})");
            return;
        }
    };
    let status = output.status().cloned().expect("open");
    let why = status
        .fell_back
        .clone()
        .expect("the missing device was said");
    assert!(why.contains("No Such Device Anywhere"), "{why}");
    assert_eq!(
        status.buffer_frames,
        Some(fontelle_engine::DEFAULT_OUTPUT_BUFFER),
        "{status:?}"
    );

    std::thread::sleep(Duration::from_millis(400));
    let before = transport.position_sample();
    assert!(before > 0, "the first stream played");

    let status = output
        .reopen(&OutputChoice {
            buffer_frames: Some(1024),
            ..OutputChoice::default()
        })
        .expect("the default opens again");
    assert_eq!(status.fell_back, None);
    assert_eq!(status.buffer_frames, Some(1024), "{status:?}");
    std::thread::sleep(Duration::from_millis(400));
    let after = transport.position_sample();
    assert!(
        after > before,
        "the second stream carried on from where the first stopped: {before} then {after}"
    );
    assert!(
        after < before + 48_000,
        "and did not start again from somewhere else: {before} then {after}"
    );
    output.close();
    assert!(output.status().is_none());
}
