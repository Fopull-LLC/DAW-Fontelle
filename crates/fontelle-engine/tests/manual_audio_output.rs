//! Not run by `cargo test` (or CI) — it opens the real default output device
//! and plays audible sound. This is the actual M0 gate (TDD §22): "audio
//! callback → compiled graph → one sampler voice reading a real SF2 zone →
//! mixer track → device out." Everything up to "device out" has an automated
//! test elsewhere (`fontelle-core`, `fontelle-assets`, `fontelle-engine::graph`);
//! this is the one step that can only be confirmed by a human with speakers.
//!
//! Run it deliberately:
//! ```text
//! cargo test -p fontelle-engine --test manual_audio_output -- --ignored --nocapture
//! ```
//! You should hear roughly one second of a plain tone (a triangle-ish wave
//! built from a synthetic sample buffer, not yet a real SF2 file — pass
//! `FONTELLE_TEST_SF2=/path/to/file.sf2` to import a real one instead and
//! hear *that* play, which is the literal M0 claim).

use std::sync::Arc;
use std::time::Duration;

use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, SampleStore,
    Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, Interpolation, SvfMode};
use fontelle_engine::{AudioDevice, BufferPool, CompiledGraph, SamplerNode, ScheduledNode};
use fontelle_types::NodeId;
use slotmap::Key;

const SAMPLE_RATE: u32 = 48_000;

fn synthetic_patch(store: &mut SampleStore) -> Patch {
    // A short, audible, non-silent tone: a naive band-limited-ish triangle
    // wave baked into a sample buffer and looped, rather than a pure sine, so
    // it's obviously audible over speaker rolloff/room noise.
    let cycle = 200;
    let data: Vec<f32> = (0..cycle)
        .map(|i| {
            let t = i as f32 / cycle as f32;
            4.0 * (t - 0.5).abs() - 1.0
        })
        .collect();
    let asset = store.insert(SampleBuffer {
        data: Arc::from(data),
        sample_rate: SAMPLE_RATE,
    });

    let disabled_filter = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
    };
    let env = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.05,
        sustain_level: 0.6,
        release_s: 0.2,
    };

    Patch {
        layers: vec![Layer {
            source: Source::Sample { file: asset },
            key_range: (0, 127),
            vel_range: (0, 127),
            root_key: 60,
            fine_tune_cents: 0.0,
            playback: PlaybackConfig {
                loop_mode: LoopMode::Forward,
                loop_start: 0.0,
                loop_end: cycle as f64,
                end_offset: cycle as f64,
                interpolation: Interpolation::Normal,
                ..PlaybackConfig::default()
            },
            gain_db: -6.0,
            pan: 0.0,
        }],
        filters: [disabled_filter, disabled_filter],
        envelopes: vec![env, env],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
    }
}

#[test]
#[ignore = "opens the real audio device and plays audible sound; run deliberately"]
fn plays_a_note_through_the_real_output_device() {
    let mut store = SampleStore::new();

    let patch = if let Ok(path) = std::env::var("FONTELLE_TEST_SF2") {
        fontelle_assets::import_sf2(std::path::Path::new(&path), &mut store)
            .expect("FONTELLE_TEST_SF2 must point at a valid SF2 file")
    } else {
        synthetic_patch(&mut store)
    };

    let mut sampler = Sampler::new(patch);
    sampler.prepare(&fontelle_core::PrepareContext {
        sample_rate: SAMPLE_RATE as f32,
        max_block_size: fontelle_engine::BLOCK_SIZE as u32,
    });
    sampler.note_on(60, 100, 0);

    let node = SamplerNode::new(sampler, Arc::new(store));
    let graph = CompiledGraph {
        schedule: vec![ScheduledNode {
            id: NodeId::null(),
            node: Box::new(node),
            input_buffers: Vec::new(),
            output_buffers: vec![0],
        }],
        buffer_pool: BufferPool::with_capacity(1, fontelle_engine::BLOCK_SIZE),
    };

    let mut device = AudioDevice::default_host();
    eprintln!("output device: {:?}", device.default_output_name());
    device
        .start_output_stream(graph, SAMPLE_RATE)
        .expect("failed to open the default output device");

    std::thread::sleep(Duration::from_millis(1200));
    device.stop();
}
