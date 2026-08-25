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
//! You should hear about 3.5 seconds: a root/third/fifth run in quarter
//! notes, then all three held together as a chord. By default that's a
//! triangle-ish wave built from a synthetic sample buffer; pass
//! `FONTELLE_TEST_SF2=/path/to/file.sf2` to import a real soundfont instead
//! and hear *that* play, which is the literal M0 claim.
//!
//! **What to listen for:** the chord. Three simultaneous voices is the case
//! that exposed the voice-mixing bug where each new voice re-applied its
//! envelope to the ones already mixed into the shared buffer — audibly, held
//! notes ducking every time another note started. If the chord swells
//! smoothly and the earlier notes don't dip as it arrives, that path is
//! healthy.

use std::sync::Arc;
use std::time::Duration;

use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, SampleStore,
    Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_engine::{
    AudioDevice, BufferPool, CompiledGraph, MixerTrackNode, SamplerNode, ScheduledNode,
};
use fontelle_types::{CompiledTimeline, EventPayload, NodeId, TimedEvent};
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
        curve: EnvelopeCurve::Linear,
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

    // Notes come from a real event timeline, not a direct `sampler.note_on()`
    // — the point of listening to this is to hear the path that actually
    // ships, event cursor and all. A three-note run then the triad held
    // together, mirroring `fontelle-app`'s demo phrase; the chord is the part
    // worth listening closely to, since simultaneous voices are what the
    // voice-mixing bug (see PROGRESS.md) made sound wrong.
    let timeline = arpeggio_then_chord(60);

    let graph = CompiledGraph {
        schedule: vec![
            ScheduledNode {
                id: NodeId::null(),
                node: Box::new(SamplerNode::new(sampler, Arc::new(store))),
                input_buffers: Vec::new(),
                output_buffers: vec![0, 1],
            },
            ScheduledNode {
                id: NodeId::null(),
                node: Box::new(MixerTrackNode::new()),
                input_buffers: vec![0, 1],
                output_buffers: vec![0, 1],
            },
        ],
        buffer_pool: BufferPool::with_capacity(2, fontelle_engine::BLOCK_SIZE),
    };

    let mut device = AudioDevice::default_host();
    eprintln!("output device: {:?}", device.default_output_name());
    eprintln!("playing {} events", timeline.events.len());
    device
        .start_output_stream(graph, timeline, SAMPLE_RATE)
        .expect("failed to open the default output device");

    std::thread::sleep(Duration::from_millis(3500));
    device.stop();
}

/// Root/third/fifth as quarter notes, then all three held together — built
/// directly as sample-timestamped events, since `fontelle-engine` can't
/// depend on `fontelle-sequencer` (TDD §4.1). `fontelle-app`'s
/// `demo_song` produces the equivalent phrase through the real document →
/// sequencer path; this is the same music, one layer lower.
fn arpeggio_then_chord(root: u8) -> CompiledTimeline {
    let quarter = SAMPLE_RATE as i64 / 2; // 0.5s at 120bpm
    let mut events = Vec::new();
    let mut push = |sample: i64, key: u8, on: bool| {
        events.push(TimedEvent {
            sample,
            target: NodeId::null(),
            payload: if on {
                EventPayload::NoteOn {
                    key,
                    velocity: 100,
                    voice_context: 0,
                }
            } else {
                EventPayload::NoteOff {
                    key,
                    voice_context: 0,
                }
            },
        });
    };

    for (step, interval) in [0u8, 4, 7].iter().enumerate() {
        let start = step as i64 * quarter;
        push(start, root + interval, true);
        push(start + quarter, root + interval, false);
    }
    let chord_start = 3 * quarter;
    for interval in [0u8, 4, 7] {
        push(chord_start, root + interval, true);
        push(chord_start + quarter * 3, root + interval, false);
    }

    events.sort_by_key(|e| e.sample);
    CompiledTimeline {
        events,
        index: Vec::new(),
    }
}
