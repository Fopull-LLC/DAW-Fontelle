//! Offline rendering has to produce byte-for-byte what the device would have
//! played, or it's useless for diagnosing "it sounds wrong" — which is what it
//! was built for.

mod common;

use std::sync::Arc;

use fontelle_app::{SampleLibrary, render_offline, write_wav16};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

use common::{SR, set_number};
use fontelle_model::NumberTarget;

fn synthetic_patch(library: &mut SampleLibrary) -> Patch {
    // A 100-sample sine cycle, looped: a signal with an obvious, checkable
    // shape, unlike a constant.
    let cycle = 100;
    let data: Vec<f32> = (0..cycle)
        .map(|i| (i as f32 / cycle as f32 * std::f32::consts::TAU).sin())
        .collect();
    let asset = library.insert_synthetic(
        "cycle",
        SampleBuffer {
            data: Arc::from(data),
            sample_rate: SR,
        },
    );
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
                interpolation: Some(Interpolation::Normal),
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
    let mut library = SampleLibrary::new();
    let patch = synthetic_patch(&mut library);
    let (_project, mut realised, timeline) =
        common::demo_rig(&patch, &library, fontelle_app::PLAYBACK_QUALITY);
    render_offline(&timeline, &mut realised.graph, total)
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

/// TDD §7.6: playback and render quality are independent, and export defaults
/// to a better kernel than playback does. This checks the setting is actually
/// live all the way through `SamplerNode` and the compiled graph, not just
/// stored on the `Sampler` — a quality that a node quietly drops on the floor
/// is worse than one that was never wired up, because it looks correct.
#[test]
fn the_render_quality_override_reaches_the_graph() {
    let render_at = |quality: Interpolation| {
        let mut library = SampleLibrary::new();
        // Deliberately not `synthetic_patch`: its 100-sample cycle is 0.01
        // cycles per sample, where every kernel agrees to four decimal places.
        // A test of the interpolation setting needs content high enough in the
        // spectrum for the kernel to matter.
        let patch = bright_patch(&mut library);
        let (_project, mut realised, timeline) = common::demo_rig(&patch, &library, quality);
        render_offline(&timeline, &mut realised.graph, 24_000)
    };

    let as_authored = render_at(fontelle_app::PLAYBACK_QUALITY);
    let high = render_at(fontelle_app::RENDER_QUALITY);

    assert!(
        as_authored.iter().any(|s| *s != 0.0),
        "the fixture should make sound"
    );
    // Relative to the signal, not absolute: the demo's velocities and its
    // -12 dB track gain scale everything down, so an absolute threshold would
    // be measuring the fader rather than the kernel.
    let mean_level: f32 =
        as_authored.iter().map(|s| s.abs()).sum::<f32>() / as_authored.len() as f32;
    let mean_difference: f32 = as_authored
        .iter()
        .zip(high.iter())
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / as_authored.len() as f32;
    let relative = mean_difference / mean_level;
    assert!(
        relative > 0.01,
        "the override must measurably change the rendered audio; difference was \
         {relative:.4} of the signal level ({mean_difference} against {mean_level})"
    );
}

/// `synthetic_patch` with its cycle shortened to five samples — 0.2 cycles per
/// sample, ordinary upper-mid content once transposed, and the region where a
/// windowed sinc is measurably more accurate than Hermite.
fn bright_patch(library: &mut SampleLibrary) -> Patch {
    let cycle = 5;
    let cycles = 102;
    let data: Vec<f32> = (0..cycle * cycles)
        .map(|i| (i as f32 / cycle as f32 * std::f32::consts::TAU).sin())
        .collect();
    let len = data.len() as f64;
    let asset = library.insert_synthetic(
        "bright-cycle",
        SampleBuffer {
            data: Arc::from(data),
            sample_rate: SR,
        },
    );
    let mut patch = synthetic_patch(library);
    patch.layers[0].source = Source::Sample { file: asset };
    // Unpinned, so the session quality is what decides — which is the thing
    // under test.
    patch.layers[0].playback.interpolation = None;
    patch.layers[0].playback.loop_end = len;
    patch.layers[0].playback.end_offset = len;
    patch
}

/// And `fontelle-app`'s export path has to actually ask for it.
#[test]
fn the_offline_bounce_renders_at_export_quality() {
    assert_eq!(fontelle_app::PLAYBACK_QUALITY, Interpolation::Normal);
    assert_eq!(fontelle_app::RENDER_QUALITY, Interpolation::High);
    assert_ne!(fontelle_app::PLAYBACK_QUALITY, fontelle_app::RENDER_QUALITY);
}

/// The track fader has to be a fader. It was a constant chosen for the demo
/// phrase's three-voice chord, which leaves a whole arrangement about 20 dB
/// too quiet — different material needs different headroom, and until there is
/// a master limiter the fader is the only place to say so.
#[test]
fn the_track_gain_scales_the_render() {
    let render_at = |gain_db: f32| {
        let mut library = SampleLibrary::new();
        let patch = synthetic_patch(&mut library);
        let mut project = common::demo_with(&patch, &library);
        // The master fader is a document value now, not a parameter threaded
        // into a graph builder — which is the point of the realisation step.
        let master = project.mixer.master.unwrap();
        set_number(
            &mut project,
            NumberTarget::TrackGainDb(master),
            gain_db as f64,
        );
        let (mut realised, timeline) =
            common::realise_at(&project, &library, fontelle_app::PLAYBACK_QUALITY);
        render_offline(&timeline, &mut realised.graph, 24_000)
    };

    let quiet = render_at(-12.0);
    let loud = render_at(-6.0);
    let peak = |pcm: &[f32]| pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));

    let ratio = peak(&loud) / peak(&quiet);
    let expected = 10f32.powf(6.0 / 20.0);
    assert!(
        (ratio - expected).abs() < 0.02,
        "6 dB more fader should be {expected}x the peak, got {ratio}"
    );
}
