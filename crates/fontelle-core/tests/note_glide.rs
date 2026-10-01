//! **One note's own glide**: a slide of a note path
//! (`docs/note-paths-plan.md`), addressed to the note it bends.
//!
//! `Sampler::slide` is the FL slide note, and it bends *every* voice in its
//! context to one key — a chord under it moves as a block. Ty: *"you could
//! now have multiple notes sliding at the same time in a chord for example
//! sliding to different notes."* `Sampler::glide_note` names its note.

use fontelle_core::{
    FilterSlot, Layer, LoopMode, NoteTrigger, Patch, PlaybackConfig, PrepareContext, SampleBuffer,
    SampleStore, Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

const SR: f32 = 48_000.0;

/// How many samples one cycle of the fixture takes at its root key.
const ROOT_PERIOD: f32 = 240.0;

/// A patch over a looping **ramp**, rooted at key 60.
///
/// A ramp rather than a sine because measuring pitch means counting how fast
/// the waveform repeats, and a ramp falls off a cliff exactly once per cycle —
/// which makes the count a comparison rather than a Fourier transform.
fn ramp_patch(store: &mut SampleStore) -> Patch {
    let cycle = ROOT_PERIOD as usize;
    let cycles = 400;
    let data: Vec<f32> = (0..cycle * cycles)
        .map(|i| (i % cycle) as f32 / cycle as f32)
        .collect();
    let asset = store.insert(SampleBuffer {
        data: std::sync::Arc::from(data),
        sample_rate: SR as u32,
    });
    Patch {
        layers: vec![Layer {
            source: Source::Sample { file: asset },
            key_range: (0, 127),
            vel_range: (0, 127),
            root_key: 60,
            fine_tune_cents: 0.0,
            playback: PlaybackConfig {
                loop_mode: LoopMode::Forward,
                interpolation: Some(Interpolation::Draft),
                loop_start: 0.0,
                loop_end: (cycle * cycles) as f64,
                end_offset: (cycle * cycles) as f64,
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
        ..Default::default()
    }
}

fn off() -> FilterSlot {
    FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
        ..Default::default()
    }
}

/// Straight to full and stays there: the pitch is what is being measured, and
/// an envelope shaping the level would only make the cliffs harder to find.
fn flat() -> EnvelopeConfig {
    EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.005,
        curve: EnvelopeCurve::Linear,
        ..Default::default()
    }
}

/// The dominant period of `out`, in samples, by counting **upward crossings of
/// the ramp's midpoint**.
///
/// Not by looking for the cliff, which was the obvious way and is wrong: the
/// interpolator smooths the wrap across whichever output samples straddle it,
/// so at some playback rates the fall is spread over two samples and no single
/// step is a cliff at all. A crossing of the middle happens exactly once per
/// cycle however the wrap is filtered.
fn period(out: &[f32]) -> f32 {
    let mut crossings = Vec::new();
    for (index, pair) in out.windows(2).enumerate() {
        if pair[0] < 0.5 && pair[1] >= 0.5 {
            crossings.push(index as f32);
        }
    }
    assert!(crossings.len() >= 2, "no cycles in {} samples", out.len());
    (crossings[crossings.len() - 1] - crossings[0]) / (crossings.len() - 1) as f32
}

/// Renders `frames` from `sampler` in `BLOCK` chunks, as the engine does.
fn render(sampler: &mut Sampler, store: &SampleStore, frames: usize) -> Vec<f32> {
    const BLOCK: usize = 128;
    let mut out = Vec::with_capacity(frames);
    let mut scratch = vec![0.0f32; BLOCK];
    let mut left = frames;
    while left > 0 {
        let n = left.min(BLOCK);
        scratch[..n].fill(0.0);
        sampler.render(store, &mut [&mut scratch[..n]]);
        out.extend_from_slice(&scratch[..n]);
        left -= n;
    }
    out
}

fn prepared(store: &mut SampleStore) -> Sampler {
    let mut sampler = Sampler::new(ramp_patch(store));
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });
    sampler
}

/// Where the voice started on `key` is sounding now, as a key.
fn pitch_of(sampler: &Sampler, key: u8) -> f32 {
    sampler
        .sounding_pitches()
        .find(|&(started, _)| started == key)
        .map(|(_, pitch)| pitch)
        .unwrap_or_else(|| panic!("nothing sounding on {key}"))
}

#[test]
fn a_glide_bends_its_note_to_a_new_pitch_audibly() {
    let mut store = SampleStore::new();
    let mut sampler = prepared(&mut store);
    sampler.trigger(NoteTrigger::new(60, 100));
    let before = render(&mut sampler, &store, 2048);

    sampler.glide_note(60, 0, 12.0, 0.1);
    let during = render(&mut sampler, &store, 4800);
    let after = render(&mut sampler, &store, 2048);

    let root = period(&before);
    let landed = period(&after);
    assert!(
        (landed / (root / 2.0) - 1.0).abs() < 0.06,
        "an octave up: started at {root}, ended at {landed}"
    );
    let middle = period(&during[during.len() / 2 - 512..during.len() / 2 + 512]);
    assert!(
        middle < root * 0.98 && middle > landed * 1.02,
        "gradual: the middle should be between {root} and {landed}, was {middle}"
    );
    assert_eq!(sampler.active_voices(), 1, "a glide is not a note-on");
}

#[test]
fn a_chord_slides_apart() {
    let mut store = SampleStore::new();
    let mut sampler = prepared(&mut store);
    sampler.trigger(NoteTrigger::new(60, 100));
    sampler.trigger(NoteTrigger::new(64, 100));

    sampler.glide_note(60, 0, 5.0, 0.01);
    sampler.glide_note(64, 0, -2.0, 0.01);
    render(&mut sampler, &store, 4800);

    assert!((pitch_of(&sampler, 60) - 65.0).abs() < 1e-3);
    assert!((pitch_of(&sampler, 64) - 62.0).abs() < 1e-3);
}

#[test]
fn a_glide_leaves_the_other_notes_of_its_context_alone() {
    let mut store = SampleStore::new();
    let mut sampler = prepared(&mut store);
    sampler.trigger(NoteTrigger::new(60, 100));
    sampler.trigger(NoteTrigger::new(67, 100));
    sampler.glide_note(60, 0, 12.0, 0.0);
    render(&mut sampler, &store, 256);
    assert!((pitch_of(&sampler, 67) - 67.0).abs() < 1e-3);
}

#[test]
fn glides_chain_from_wherever_the_last_one_left_the_note() {
    // Up two, then down to one below: the second measures from the note's
    // own key, not from where the first landed, and starts from there.
    let mut store = SampleStore::new();
    let mut sampler = prepared(&mut store);
    sampler.trigger(NoteTrigger::new(60, 100));
    sampler.glide_note(60, 0, 2.0, 0.0);
    render(&mut sampler, &store, 256);
    sampler.glide_note(60, 0, -1.0, 0.1);
    render(&mut sampler, &store, 128);
    let early = pitch_of(&sampler, 60);
    assert!(early < 62.0 && early > 61.0, "leaves from 62, was {early}");
    render(&mut sampler, &store, 9600);
    assert!((pitch_of(&sampler, 60) - 59.0).abs() < 1e-3);
}

#[test]
fn a_glide_with_nothing_sounding_on_its_key_does_nothing() {
    let mut store = SampleStore::new();
    let mut sampler = prepared(&mut store);
    sampler.trigger(NoteTrigger::new(64, 100));
    sampler.glide_note(60, 0, 5.0, 0.0);
    render(&mut sampler, &store, 256);
    assert_eq!(sampler.active_voices(), 1);
    assert!((pitch_of(&sampler, 64) - 64.0).abs() < 1e-3);
}

#[test]
fn a_glided_note_still_ends_on_the_key_it_started_on() {
    let mut store = SampleStore::new();
    let mut sampler = prepared(&mut store);
    sampler.trigger(NoteTrigger::new(60, 100));
    sampler.glide_note(60, 0, 7.0, 0.01);
    render(&mut sampler, &store, 1024);
    sampler.note_off(60, 0);
    render(&mut sampler, &store, 48_000);
    assert_eq!(sampler.active_voices(), 0, "a glided note still ends");
}

#[test]
fn a_glide_names_its_context_too() {
    // Two clips on one channel can both hold a 60; the glide of one is not
    // the other's.
    let mut store = SampleStore::new();
    let mut sampler = prepared(&mut store);
    sampler.trigger(NoteTrigger {
        voice_context: 1,
        ..NoteTrigger::new(60, 100)
    });
    sampler.glide_note(60, 2, 5.0, 0.0);
    render(&mut sampler, &store, 256);
    assert!((pitch_of(&sampler, 60) - 60.0).abs() < 1e-3);
}
