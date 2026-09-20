//! **A legato note that follows a note it touches still has to sound.**
//!
//! Reported from using the window: *"notes that are legato and start and end
//! next to another note makes that note not play if there was one before it
//! next to it."*
//!
//! Measured, and the report is exact. In `RetriggerMode::Legato` a new note
//! takes over the voice already sounding in its context rather than stacking
//! on top of it — and `Voice::legato_to` deliberately does not touch the
//! envelopes, because *not* restarting them is the whole difference between
//! legato and a retrigger.
//!
//! But the voice it takes over may already have been **let go of**. Two notes
//! that touch put a note-off and a note-on on the same sample, and the
//! sequencer orders the off first on purpose (`fontelle_sequencer::sort_events`
//! — see its `rank`). So by the time the second note arrives, the envelope of
//! the first is in its *release* stage, and a take-over that leaves the
//! envelopes alone inherits it: the voice is held again, its pitch is right,
//! and its amplitude is on its way to zero. The note is there, and silent —
//! which is the same shape of failure `rank` was written to fix, one layer
//! further down.
//!
//! The level is what is measured here, not the pitch: a route that quietly
//! left the envelope in release would pass every pitch check there is.

use fontelle_core::{
    FilterSlot, Layer, LoopMode, NoteTrigger, Patch, PlaybackConfig, PrepareContext, RetriggerMode,
    SampleBuffer, SampleStore, Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

const SR: f32 = 48_000.0;

/// A patch over a **constant** sample, so the output level is the envelope and
/// nothing else. A waveform would make every reading depend on where in its
/// cycle the window fell.
fn dc_patch(store: &mut SampleStore) -> Patch {
    let frames = 480_000;
    let asset = store.insert(SampleBuffer {
        data: std::sync::Arc::from(vec![1.0f32; frames]),
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
                loop_end: frames as f64,
                end_offset: frames as f64,
                ..PlaybackConfig::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [off(), off()],
        envelopes: vec![sustaining(), sustaining()],
        lfos: Vec::new(),
        mod_matrix: Default::default(),
        voice_config: VoiceConfig {
            retrigger: RetriggerMode::Legato,
            ..VoiceConfig::default()
        },
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

/// Straight up, holds, and lets go over 50 ms — a short release on purpose,
/// so a note left in it is *silent* rather than merely quieter, which is what
/// the report describes.
fn sustaining() -> EnvelopeConfig {
    EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.05,
        curve: EnvelopeCurve::Linear,
        ..Default::default()
    }
}

fn ready(patch: Patch) -> Sampler {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });
    sampler
}

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

fn rms(out: &[f32]) -> f32 {
    (out.iter().map(|s| s * s).sum::<f32>() / out.len().max(1) as f32).sqrt()
}

/// A quarter of a second in, well past a 50 ms release: whatever is left here
/// is what the note is actually doing.
const SETTLED: usize = 12_000;

#[test]
fn a_note_that_touches_the_one_before_it_still_sounds() {
    let mut store = SampleStore::new();
    let mut sampler = ready(dc_patch(&mut store));

    // The first note, alone, is the reference.
    sampler.trigger(NoteTrigger::new(60, 100));
    render(&mut sampler, &store, 2_000);
    let first = rms(&render(&mut sampler, &store, SETTLED));
    assert!(first > 0.1, "the fixture is silent at {first}");

    // The seam: the two land on the same sample, off first, as the sequencer
    // orders them.
    sampler.note_off(60, 0);
    sampler.trigger(NoteTrigger::new(62, 100));
    render(&mut sampler, &store, 2_000);
    let second = rms(&render(&mut sampler, &store, SETTLED));

    assert!(
        second > first * 0.9,
        "the second note of a legato pair is at {second} against the first's {first}"
    );
}

#[test]
fn a_run_of_touching_notes_all_sound() {
    // *"if there was one before it next to it"* — the third note in a line is
    // the one somebody notices, because by then the pattern is obvious.
    let mut store = SampleStore::new();
    let mut sampler = ready(dc_patch(&mut store));

    let mut levels = Vec::new();
    let mut previous: Option<u8> = None;
    for key in [60u8, 62, 64, 65, 67] {
        // Each note lets go exactly where the next begins, which is what the
        // Legato tool writes.
        if let Some(last) = previous {
            sampler.note_off(last, 0);
        }
        sampler.trigger(NoteTrigger::new(key, 100));
        previous = Some(key);
        render(&mut sampler, &store, 2_000);
        levels.push(rms(&render(&mut sampler, &store, SETTLED)));
    }
    let quietest = levels.iter().copied().fold(f32::MAX, f32::min);
    let loudest = levels.iter().copied().fold(0.0f32, f32::max);
    assert!(
        quietest > loudest * 0.9,
        "notes in a legato run are at {levels:?}"
    );
}

#[test]
fn a_legato_take_over_of_a_held_note_still_does_not_restart_the_envelope() {
    // The other half of the claim, and the reason the fix cannot simply be
    // "retrigger the envelope every time". Legato *means* the envelope carries
    // on: a note arriving while the last one is still held must not re-attack.
    let mut store = SampleStore::new();
    let mut patch = dc_patch(&mut store);
    // A slow attack makes a restart obvious: a re-attacked note is near zero
    // just after it starts, a carried-over one is already at full.
    for env in &mut patch.envelopes {
        env.attack_s = 0.5;
    }
    let mut sampler = ready(patch);

    sampler.trigger(NoteTrigger::new(60, 100));
    render(&mut sampler, &store, 30_000);
    let before = rms(&render(&mut sampler, &store, 512));

    // No note-off: the first key is still down.
    sampler.trigger(NoteTrigger::new(64, 100));
    let after = rms(&render(&mut sampler, &store, 512));
    assert!(
        after > before * 0.9,
        "a legato take-over re-attacked: {before} then {after}"
    );
}

/// The same claim against the **real bank**, because the fixture above proves
/// the mechanism and the report was about presets somebody actually loaded.
///
/// Twenty-one of Flopsynth's factory presets go through the builder's `mono`,
/// which is `RetriggerMode::Legato` — every one of them had this.
#[test]
fn every_legato_preset_in_the_bank_plays_its_second_touching_note() {
    use fontelle_core::flopsynth::presets::FACTORY;
    let store = SampleStore::new();
    let mut checked = 0;
    for row in FACTORY {
        let patch = (row.build)();
        if patch.voice_config.retrigger != RetriggerMode::Legato {
            continue;
        }
        checked += 1;
        let mut sampler = ready(patch);
        sampler.trigger(NoteTrigger::new(48, 100));
        render(&mut sampler, &store, 2_000);
        let first = rms(&render(&mut sampler, &store, 4_000));

        sampler.note_off(48, 0);
        sampler.trigger(NoteTrigger::new(50, 100));
        render(&mut sampler, &store, 2_000);
        let second = rms(&render(&mut sampler, &store, 4_000));

        assert!(
            second > first * 0.25,
            "{}: the second touching note is {second} against the first's {first}",
            row.name
        );
    }
    assert!(checked >= 15, "only {checked} legato presets were checked");
}

// ------------------------------------------------- glide_legato_only ---
//
// *"`VoiceConfig::glide_legato_only` is written, saved, exposed on the
// instrument panel as `patch/voice/legato` and automatable — and read by
// nothing in the audio path."*
//
// It is the knob that says **portamento only between notes that overlap**,
// which is how every mono synth with a legato switch on it behaves and what
// all twenty-one of Flopsynth's `mono` presets ask for. Unread, a phrase of
// separate notes slid between every one of them.

/// Renders and returns the dominant period in samples, by counting upward
/// crossings of the ramp's midpoint — the pitch, which is what a glide moves.
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

/// A ramp, so the pitch can be counted. The DC fixture above cannot show one.
fn ramp_patch(store: &mut SampleStore, legato_only: bool) -> Patch {
    const CYCLE: usize = 240;
    let cycles = 2_000;
    let data: Vec<f32> = (0..CYCLE * cycles)
        .map(|i| (i % CYCLE) as f32 / CYCLE as f32)
        .collect();
    let asset = store.insert(SampleBuffer {
        data: std::sync::Arc::from(data),
        sample_rate: SR as u32,
    });
    let mut patch = dc_patch(&mut SampleStore::new());
    patch.layers[0].source = Source::Sample { file: asset };
    patch.layers[0].playback.loop_end = (CYCLE * cycles) as f64;
    patch.layers[0].playback.end_offset = (CYCLE * cycles) as f64;
    patch.voice_config.glide_time_s = 0.25;
    patch.voice_config.glide_mode = if legato_only {
        fontelle_core::GlideMode::Legato
    } else {
        fontelle_core::GlideMode::Notes
    };
    patch
}

#[test]
fn with_legato_only_set_a_note_over_a_released_key_does_not_glide() {
    let mut store = SampleStore::new();
    let mut sampler = ready(ramp_patch(&mut store, true));

    sampler.trigger(NoteTrigger::new(60, 100));
    let root = period(&render(&mut sampler, &store, 4_096));

    // The seam the Legato tool writes: let go, then the next note.
    sampler.note_off(60, 0);
    sampler.trigger(NoteTrigger::new(72, 100));
    let just_after = period(&render(&mut sampler, &store, 1_024));
    assert!(
        (just_after / (root / 2.0) - 1.0).abs() < 0.08,
        "a separate note must start at its own pitch: wanted {}, got {just_after}",
        root / 2.0
    );
}

#[test]
fn with_legato_only_set_a_note_over_a_held_key_still_glides() {
    // The other half: this is the setting's whole purpose, so it must not
    // simply switch portamento off.
    let mut store = SampleStore::new();
    let mut sampler = ready(ramp_patch(&mut store, true));

    sampler.trigger(NoteTrigger::new(60, 100));
    let root = period(&render(&mut sampler, &store, 4_096));

    // No note-off: the first key is still down, so this one is legato.
    sampler.trigger(NoteTrigger::new(72, 100));
    let just_after = period(&render(&mut sampler, &store, 1_024));
    assert!(
        just_after > root * 0.8,
        "an overlapping note must slide from the last one: {root} then {just_after}"
    );
}

#[test]
fn with_legato_only_clear_every_note_glides_as_it_always_did() {
    let mut store = SampleStore::new();
    let mut sampler = ready(ramp_patch(&mut store, false));

    sampler.trigger(NoteTrigger::new(60, 100));
    let root = period(&render(&mut sampler, &store, 4_096));

    sampler.note_off(60, 0);
    sampler.trigger(NoteTrigger::new(72, 100));
    let just_after = period(&render(&mut sampler, &store, 1_024));
    assert!(
        just_after > root * 0.8,
        "with the switch off a separate note still slides: {root} then {just_after}"
    );
}
