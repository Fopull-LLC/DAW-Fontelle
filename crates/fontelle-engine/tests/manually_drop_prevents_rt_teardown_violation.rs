//! Confirms the fix for the second real-hardware crash: `cpal` drops the audio
//! callback closure — and everything it owns — *on the audio thread itself*
//! during stream teardown (`AudioDevice::stop`; the panic named thread
//! `cpal_alsa_out`, not the caller's thread). That thread is still tagged RT
//! and never gets un-tagged before teardown, so any heap-owning field being
//! dropped there was a second, independent INVARIANT 1 violation — this time
//! from `Patch`/`SampleStore` teardown, not steady-state rendering.
//!
//! The fix: `AudioDevice` wraps the graph (and the RT-priority handle) in
//! `std::mem::ManuallyDrop` inside the callback closure, so the implicit drop
//! cpal triggers when it tears down the closure does nothing — no
//! deallocation happens on that thread at all. This is a real, if deliberate,
//! leak: the memory is reclaimed only because `--play-sf2` exits the whole
//! process shortly after `stop()`. A proper deferred-drop ("trash bin" — hand
//! the old graph to a channel a non-RT thread actually frees) is the correct
//! fix for a long-running DAW process and isn't built yet; see PROGRESS.md.
//!
//! This test proves the mechanism itself (`ManuallyDrop` suppresses the drop)
//! without needing real `cpal`/hardware: build the same kind of heap-owning
//! state, tag the thread RT, wrap it, let the wrapper go out of scope, and
//! confirm nothing panics.

use std::mem::ManuallyDrop;
use std::sync::Arc;

use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, SampleStore,
    Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, Interpolation, SvfMode};
use fontelle_engine::RtGuardAllocator;

#[global_allocator]
static ALLOCATOR: RtGuardAllocator = RtGuardAllocator;

fn heap_owning_patch() -> Patch {
    let disabled_filter = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
    };
    let env = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.0,
    };
    // Several layers, several distinct sample buffers -- matches the shape
    // that actually crashed (Square.sf2 imports as 7 layers/7 store entries),
    // not just the single-layer case the first regression test already covers.
    let mut store = SampleStore::new();
    let layers = (0..7)
        .map(|i| {
            let asset = store.insert(SampleBuffer {
                data: Arc::from(vec![0.1; 1_000 + i * 137]),
                sample_rate: 44_100,
            });
            Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Off,
                    interpolation: Interpolation::Normal,
                    ..PlaybackConfig::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }
        })
        .collect();

    // `store` (and the AssetIds `layers` reference) drop here — fine, this
    // test only needs `Patch`/`Layer` to be genuinely heap-owning, not a
    // live, renderable sampler.
    Patch {
        layers,
        filters: [disabled_filter, disabled_filter],
        envelopes: vec![env, env],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
    }
}

#[test]
fn dropping_owned_state_directly_while_rt_tagged_panics() {
    // Establishes the baseline this test is guarding against: without the
    // ManuallyDrop wrapper, tearing down RT-owned state on an RT-tagged
    // thread really does violate INVARIANT 1 -- so the fix below is proven
    // against a real failure mode, not a strawman.
    let patch = heap_owning_patch();
    let sampler = Sampler::new(patch);

    fontelle_engine::mark_current_thread_rt();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop(sampler);
    }));
    fontelle_engine::unmark_current_thread_rt();

    assert!(
        result.is_err(),
        "dropping heap-owning state on an RT-tagged thread should panic (this is the bug)"
    );
}

#[test]
fn manually_drop_suppresses_the_teardown_violation() {
    // Construction (allocating the voice pool, etc.) is real off-RT setup —
    // in the real `AudioDevice`, this all happens before the stream (and
    // its RT-tagged callback thread) exists at all. Only the *drop*, at
    // teardown, is the RT-thread problem being tested here.
    let patch = heap_owning_patch();
    let sampler = ManuallyDrop::new(Sampler::new(patch));

    fontelle_engine::mark_current_thread_rt();
    {
        // Simulates the closure going out of scope on the RT thread during
        // stream teardown: a `ManuallyDrop`-wrapped value's implicit
        // end-of-scope drop does nothing, unlike an explicit `drop()` call
        // (which the compiler correctly refuses to compile for a
        // `ManuallyDrop` — see the sibling test for the real, unwrapped
        // behaviour this is being compared against).
        let _sampler = sampler;
    }
    fontelle_engine::unmark_current_thread_rt();

    // Reaching this line at all is the assertion -- a real violation would
    // have panicked when `_sampler` went out of scope above.
}
