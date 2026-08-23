// INVARIANT 1 enforcement (FONTELLE_TDD.md §20.4): only the final binary can set
// the process's global allocator, so it's installed here rather than in
// `fontelle-engine` itself.
#[global_allocator]
static ALLOCATOR: fontelle_engine::RtGuardAllocator = fontelle_engine::RtGuardAllocator;

fn main() {
    // M0 gate (TDD §22): audio callback -> compiled graph -> one sampler voice
    // reading a real SF2 zone -> mixer track -> device out, triggered by a note
    // from a clip on the timeline, at 128 frames / 48kHz, with the allocation
    // assertion above active and passing. Not wired up yet.
    todo!(
        "winit event loop -> fontelle-ui docked panels -> fontelle-engine::AudioDevice \
         -> fontelle-sequencer::compile -> CompiledTimeline over triple_buffer"
    )
}
