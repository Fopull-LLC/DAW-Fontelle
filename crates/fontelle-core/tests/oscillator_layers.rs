//! `Source::Oscillator` layers, which the patch format has named since it was
//! written and the renderer skipped.
//!
//! It matters now because it is what a **blank instrument** is made of. Adding
//! a channel used to need a soundfont chosen in the browser first, so the
//! button did nothing until you went and found one; a new channel now comes up
//! playing three oscillators, which is a sound you can hear the moment the
//! channel exists.
//!
//! Everything here is measured off the rendered block rather than off a field:
//! an oscillator layer that is *stored* and not *sounded* is exactly the state
//! this file exists to end.

use fontelle_core::{
    FilterSlot, Layer, LoopMode, NoteTrigger, Patch, PlaybackConfig, PrepareContext, SampleStore,
    Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, OscKind, SvfMode};

const SR: f32 = 48_000.0;

fn off() -> FilterSlot {
    FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
    }
}

fn flat() -> EnvelopeConfig {
    EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.01,
        curve: EnvelopeCurve::Linear,
    }
}

/// One oscillator layer at `kind`, rooted at middle C so a note plays its own
/// pitch.
fn osc_patch(kind: OscKind) -> Patch {
    Patch {
        layers: vec![Layer {
            source: Source::Oscillator(kind),
            key_range: (0, 127),
            vel_range: (0, 127),
            root_key: 60,
            fine_tune_cents: 0.0,
            playback: PlaybackConfig {
                loop_mode: LoopMode::Off,
                ..PlaybackConfig::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [off(), off()],
        envelopes: vec![flat(), flat()],
        lfos: Vec::new(),
        mod_matrix: Default::default(),
        voice_config: VoiceConfig::default(),
    }
}

/// Renders `frames` of one held note, mono, and hands back the buffer.
fn play(patch: Patch, key: u8, frames: usize) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: frames as u32,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let mut buffer = vec![0.0f32; frames];
    let mut out: Vec<&mut [f32]> = vec![&mut buffer];
    sampler.render(&store, &mut out);
    buffer
}

fn rms(block: &[f32]) -> f32 {
    (block.iter().map(|s| s * s).sum::<f32>() / block.len().max(1) as f32).sqrt()
}

/// How many times the block crosses zero going upwards — the cheapest honest
/// frequency count for a waveform with one cycle per period.
fn rising_zero_crossings(block: &[f32]) -> usize {
    block
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count()
}

// ------------------------------------------------------- it makes a sound ---

#[test]
fn an_oscillator_layer_is_heard() {
    for kind in [
        OscKind::Sine,
        OscKind::Saw,
        OscKind::Square,
        OscKind::Triangle,
        OscKind::Noise,
    ] {
        let block = play(osc_patch(kind), 69, 4_800);
        assert!(
            rms(&block) > 0.05,
            "{kind:?} has to make a sound, got an RMS of {}",
            rms(&block)
        );
    }
}

/// A440 is A440: the note's own pitch, not the root key's.
#[test]
fn an_oscillator_plays_the_note_it_was_given() {
    // A tenth of a second at A440 is 44 cycles; at A220, 22.
    let a440 = rising_zero_crossings(&play(osc_patch(OscKind::Sine), 69, 4_800));
    let a220 = rising_zero_crossings(&play(osc_patch(OscKind::Sine), 57, 4_800));
    assert!(
        (a440 as i32 - 44).abs() <= 1,
        "key 69 is 440 Hz, counted {a440} cycles in a tenth of a second"
    );
    assert!(
        (a220 as i32 - 22).abs() <= 1,
        "key 57 is 220 Hz, counted {a220} cycles"
    );
}

/// `root_key` transposes an oscillator the way it transposes a sample: a root
/// an octave up plays an octave down.
#[test]
fn the_root_key_transposes_an_oscillator() {
    let mut patch = osc_patch(OscKind::Sine);
    patch.layers[0].root_key = 72;
    let counted = rising_zero_crossings(&play(patch, 69, 4_800));
    assert!(
        (counted as i32 - 22).abs() <= 1,
        "a root an octave above middle C plays key 69 at 220 Hz, counted {counted}"
    );
}

/// And `fine_tune_cents` detunes it, which is what makes two oscillators on
/// one note sound like two.
#[test]
fn cents_detune_an_oscillator() {
    let mut patch = osc_patch(OscKind::Sine);
    // An octave up in cents, so the count is unambiguous.
    patch.layers[0].fine_tune_cents = 1200.0;
    let counted = rising_zero_crossings(&play(patch, 69, 4_800));
    assert!(
        (counted as i32 - 88).abs() <= 1,
        "1200 cents up is 880 Hz, counted {counted}"
    );
}

/// A layer's own level applies to an oscillator as it does to a sample —
/// which is what the three level knobs on the panel turn.
#[test]
fn a_layers_gain_applies_to_an_oscillator() {
    let loud = rms(&play(osc_patch(OscKind::Sine), 69, 4_800));
    let mut quiet_patch = osc_patch(OscKind::Sine);
    quiet_patch.layers[0].gain_db = -20.0;
    let quiet = rms(&play(quiet_patch, 69, 4_800));
    let ratio = loud / quiet.max(f32::MIN_POSITIVE);
    assert!(
        (ratio - 10.0).abs() < 0.5,
        "-20 dB is a tenth of the amplitude, got a ratio of {ratio}"
    );
}

/// An oscillator has no end to run off, so the note lasts as long as its
/// envelope says and not one block longer.
#[test]
fn an_oscillator_note_ends_with_its_envelope_and_not_before() {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(osc_patch(OscKind::Saw));
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(60, 100));
    let mut buffer = vec![0.0f32; 512];
    for _ in 0..20 {
        let mut out: Vec<&mut [f32]> = vec![&mut buffer];
        sampler.render(&store, &mut out);
    }
    assert_eq!(
        sampler.active_voices(),
        1,
        "a held oscillator note is still sounding ten thousand samples later"
    );
    sampler.note_off(60, 0);
    for _ in 0..20 {
        let mut out: Vec<&mut [f32]> = vec![&mut buffer];
        sampler.render(&store, &mut out);
    }
    assert_eq!(
        sampler.active_voices(),
        0,
        "and it is gone once the release has run"
    );
}

// ------------------------------------------------- the blank instrument ---

/// What a channel with no soundfont on it plays: three oscillators, and a
/// sound the moment the channel exists.
#[test]
fn the_basic_synth_is_three_oscillators_and_is_audible() {
    let patch = Patch::basic_synth();
    assert_eq!(patch.layers.len(), 3, "three oscillators");
    assert!(
        patch
            .layers
            .iter()
            .all(|l| matches!(l.source, Source::Oscillator(_))),
        "and all three of them are oscillators"
    );
    assert!(
        patch.layers.iter().all(|l| l.key_range == (0, 127)),
        "every key plays it — it is not a sampled instrument with holes in it"
    );
    let block = play(Patch::basic_synth(), 60, 4_800);
    assert!(
        rms(&block) > 0.02,
        "a blank instrument makes a sound when you play it, got {}",
        rms(&block)
    );
}

/// It does not clip on a chord. Three oscillators at full level summed across
/// a two-handed chord is how a default patch earns a reputation.
#[test]
fn the_basic_synth_leaves_room_for_a_chord() {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(Patch::basic_synth());
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 4_800,
    });
    for key in [48, 60, 64, 67] {
        sampler.trigger(NoteTrigger::new(key, 127));
    }
    let mut buffer = vec![0.0f32; 4_800];
    let mut out: Vec<&mut [f32]> = vec![&mut buffer];
    sampler.render(&store, &mut out);
    let peak = buffer.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    assert!(
        peak <= 1.0,
        "a four-note chord on the default patch stays inside full scale, peaked at {peak}"
    );
}
