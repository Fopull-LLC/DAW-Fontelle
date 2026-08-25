//! Offline rendering has to produce byte-for-byte what the device would have
//! played, or it's useless for diagnosing "it sounds wrong" — which is what it
//! was built for.

use std::sync::Arc;

use fontelle_app::{build_graph, demo_song, render_offline, write_wav16};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, SampleStore,
    Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

const SR: u32 = 48_000;

fn synthetic_patch(store: &mut SampleStore) -> Patch {
    // A 100-sample sine cycle, looped: a signal with an obvious, checkable
    // shape, unlike a constant.
    let cycle = 100;
    let data: Vec<f32> = (0..cycle)
        .map(|i| (i as f32 / cycle as f32 * std::f32::consts::TAU).sin())
        .collect();
    let asset = store.insert(SampleBuffer {
        data: Arc::from(data),
        sample_rate: SR,
    });
    let disabled = FilterSlot {
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
        release_s: 0.01,
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
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [disabled, disabled],
        envelopes: vec![env, env],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
    }
}

fn render(total: i64) -> Vec<f32> {
    let song = demo_song(60, 120.0, SR);
    let mut store = SampleStore::new();
    let patch = synthetic_patch(&mut store);
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&fontelle_core::PrepareContext {
        sample_rate: SR as f32,
        max_block_size: fontelle_engine::BLOCK_SIZE as u32,
    });
    let mut graph = build_graph(&song, sampler, Arc::new(store));
    render_offline(&song, &mut graph, total)
}

#[test]
fn renders_interleaved_stereo_of_the_requested_length() {
    let total = 4_800;
    let out = render(total);
    assert_eq!(
        out.len(),
        total as usize * 2,
        "interleaved stereo: two samples per frame"
    );
}

#[test]
fn the_demo_song_renders_audible_non_silent_audio() {
    let out = render(48_000);
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.01, "expected audible output, got peak {peak}");
}

/// The demo's mixer track carries deliberate headroom; if that ever stops
/// being true, the render clips and this catches it before ears do.
#[test]
fn the_demo_song_does_not_clip() {
    let out = render(96_000);
    let over = out.iter().filter(|s| s.abs() > 1.0).count();
    assert_eq!(over, 0, "{over} samples exceed full scale");
}

#[test]
fn both_channels_carry_equal_level_for_a_centred_track() {
    let out = render(48_000);
    let left: f32 = out.iter().step_by(2).map(|s| s * s).sum();
    let right: f32 = out.iter().skip(1).step_by(2).map(|s| s * s).sum();
    assert!(
        (left - right).abs() < left * 1e-4,
        "centred track must be balanced: {left} vs {right}"
    );
}

#[test]
fn wav16_writes_a_well_formed_header_and_reports_clipping() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("fontelle-wav-{}.wav", std::process::id()));

    // Two frames of stereo, one sample deliberately over full scale.
    let samples = [0.0f32, 0.5, -0.5, 2.0];
    let clipped = write_wav16(&path, &samples, 2, 48_000).expect("write");
    assert_eq!(clipped, 1, "the 2.0 sample must be counted as clipped");

    let bytes = std::fs::read(&path).expect("read back");
    std::fs::remove_file(&path).ok();

    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(&bytes[12..16], b"fmt ");
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2, "channels");
    assert_eq!(
        u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
        48_000,
        "sample rate"
    );
    assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16, "bit depth");
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(
        u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize,
        samples.len() * 2,
        "data chunk size = one i16 per sample"
    );
    assert_eq!(bytes.len(), 44 + samples.len() * 2);

    // The clamped sample must land at positive full scale, not wrap negative.
    let last = i16::from_le_bytes([bytes[50], bytes[51]]);
    assert!(last > 32_000, "2.0 must clamp to +full scale, got {last}");
}
