use std::sync::Arc;

use fontelle_core::{PrepareContext, SampleStore, Sampler};
use fontelle_engine::{
    AudioDevice, BLOCK_SIZE, BufferPool, CompiledGraph, SamplerNode, ScheduledNode,
};
use fontelle_types::NodeId;
use slotmap::Key;

// INVARIANT 1 enforcement (FONTELLE_TDD.md §20.4): only the final binary can set
// the process's global allocator, so it's installed here rather than in
// `fontelle-engine` itself.
#[global_allocator]
static ALLOCATOR: fontelle_engine::RtGuardAllocator = fontelle_engine::RtGuardAllocator;

const SAMPLE_RATE: u32 = 48_000;

/// The M0 vertical slice (TDD §22), runnable for real: audio callback ->
/// compiled graph -> one sampler voice reading a real SF2 zone -> device out,
/// with the zero-allocation debug assertion above active. Not yet triggered by
/// a clip on a timeline — there's no timeline UI or `fontelle-sequencer` wiring
/// here yet, just a hardcoded note-on. See `PROGRESS.md`.
fn play_sf2(path: &str, key: u8, hold_seconds: u64) {
    let mut store = SampleStore::new();
    let patch = fontelle_assets::import_sf2(std::path::Path::new(path), &mut store)
        .unwrap_or_else(|e| panic!("failed to import {path}: {e}"));

    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SAMPLE_RATE as f32,
        max_block_size: BLOCK_SIZE as u32,
    });
    sampler.note_on(key, 100, 0);

    let node = SamplerNode::new(sampler, Arc::new(store));
    let graph = CompiledGraph {
        schedule: vec![ScheduledNode {
            id: NodeId::null(),
            node: Box::new(node),
            input_buffers: Vec::new(),
            output_buffers: vec![0],
        }],
        buffer_pool: BufferPool::with_capacity(1, BLOCK_SIZE),
    };

    let mut device = AudioDevice::default_host();
    println!(
        "Fontelle: playing key {key} from {path} on {:?}",
        device.default_output_name()
    );
    device
        .start_output_stream(graph, SAMPLE_RATE)
        .unwrap_or_else(|e| panic!("failed to open the default output device: {e}"));

    std::thread::sleep(std::time::Duration::from_secs(hold_seconds));
    device.stop();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--play-sf2") {
        let path = args
            .get(pos + 1)
            .expect("--play-sf2 requires a file path argument");
        play_sf2(path, 60, 2);
        return;
    }

    // The full DAW: docked panels, timeline, transport. Not built yet — see
    // PROGRESS.md for what's real (the audio/sampler path above) versus what
    // this still needs (fontelle-ui windowing, fontelle-sequencer wiring the
    // engine to a real document instead of one hardcoded note-on).
    todo!(
        "winit event loop -> fontelle-ui docked panels -> fontelle-engine::AudioDevice \
         -> fontelle-sequencer::compile -> CompiledTimeline over triple_buffer \
         (run with `--play-sf2 <path>` for the M0 vertical slice instead)"
    )
}
