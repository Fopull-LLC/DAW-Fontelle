//! INVARIANT 1 for the meter on the audio thread: working out a block's bar
//! and its beats, playing them and counting in allocates nothing — across
//! signature changes, groupings and a bottom that does not divide a whole
//! note into whole ticks.
//!
//! Its own binary, because installing [`RtGuardAllocator`] is a process-wide
//! decision and every `tests/*.rs` file is a process of its own.

use std::sync::Arc;

use fontelle_engine::{
    AudioNode, BLOCK_SIZE, Metronome, MetronomeNode, PrepareContext, ProcessContext,
    RtGuardAllocator, Transport, TransportReader, TransportState,
};
use fontelle_types::{CompiledTimeline, Meter, MeterChange, Signature};

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

#[test]
fn the_clock_and_the_click_allocate_nothing() {
    let timeline = CompiledTimeline {
        meter: Meter::with_song(
            &Signature::new(4, 4),
            &[
                MeterChange::new(1, 7, 8).with_groups(vec![3, 2, 2]),
                MeterChange::new(2, 5, 7),
                MeterChange::new(3, 13, 16),
            ],
        ),
        ..Default::default()
    };
    let metronome = Arc::new(Metronome::new());
    metronome.set_on(true);
    let mut node = MetronomeNode::new(Arc::clone(&metronome));
    node.prepare(&PrepareContext {
        sample_rate: 48_000.0,
        max_block_size: BLOCK_SIZE as u32,
    });
    let transport = Transport::new();
    transport.seek(96_000);
    transport.set_count_in(48_000);
    transport.set_state(TransportState::Recording);
    let mut reader = TransportReader::new();
    let mut buffer = vec![0.0f32; BLOCK_SIZE];

    fontelle_engine::mark_current_thread_rt();
    // The count-in, then six bars of changing signature.
    for _ in 0..(48_000 * 7 / BLOCK_SIZE) {
        let step = reader.next_step(&transport, &timeline, BLOCK_SIZE, BLOCK_SIZE, false);
        let frames = step.frames;
        let mut ctx = ProcessContext {
            inputs: &[],
            outputs: &mut [&mut buffer[..frames]],
            all_events: &[],
            live_events: &[],
            audio: &[],
            node: Default::default(),
            transport: step.snapshot,
            sample_range: step.range.clone(),
        };
        node.process(&mut ctx);
    }
    fontelle_engine::unmark_current_thread_rt();
}
