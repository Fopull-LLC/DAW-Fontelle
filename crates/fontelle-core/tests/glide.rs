//! Pitch that moves while a note is sounding: portamento, and slide notes.
//!
//! Reported from using the window: *"I want you to expand the functionality of
//! the piano roll to encompass things like slide and portamento notes that
//! function similarly to FL Studio."*
//!
//! Both are the same machinery seen from two ends, which is why they are one
//! file:
//!
//! - **Portamento** is the *instrument's* setting. `VoiceConfig::glide_time_s`
//!   has been on the patch — and on the instrument editor's panel — since the
//!   patch format was written, and was read by nothing. In `Mono` or `Legato`
//!   a new note now starts at the old note's pitch and slides to its own.
//! - **A slide note** is the *score's*. It does not start a voice; it bends
//!   the one already sounding to its own pitch, over its own length. That is
//!   FL Studio's, and it is what makes a bass line with one glide in it a
//!   thing you draw rather than a thing you automate.
//!
//! The glide is measured here as **pitch**, not as loudness: a route that
//! quietly did nothing would still pass a level check.

use fontelle_core::{
    FilterSlot, Layer, LoopMode, NoteTrigger, Patch, PlaybackConfig, PrepareContext, RetriggerMode,
    SampleBuffer, SampleStore, Sampler, Source, VoiceConfig,
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

#[test]
fn with_no_glide_a_note_is_at_its_own_pitch_from_the_first_sample() {
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });

    sampler.trigger(NoteTrigger::new(72, 100));
    let out = render(&mut sampler, &store, 4096);
    // An octave above the root is half the period.
    let expected = ROOT_PERIOD / 2.0;
    let measured = period(&out[..2048]);
    assert!(
        (measured / expected - 1.0).abs() < 0.05,
        "expected ~{expected} samples per cycle, measured {measured}"
    );
}

#[test]
fn a_slide_bends_a_sounding_note_to_a_new_pitch() {
    // The whole feature: one voice, two pitches, and the second one arrived at
    // rather than restarted.
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });

    sampler.trigger(NoteTrigger::new(60, 100));
    let before = render(&mut sampler, &store, 2048);

    // Slide up an octave over a tenth of a second.
    sampler.slide(72, 0.1, 0);
    let during = render(&mut sampler, &store, 4800);
    let after = render(&mut sampler, &store, 2048);

    let root = period(&before);
    let landed = period(&after);
    assert!(
        (landed / (root / 2.0) - 1.0).abs() < 0.06,
        "it should end an octave up: started at {root}, ended at {landed}"
    );
    // And it got there gradually rather than jumping: the middle of the slide
    // is between the two.
    let middle = period(&during[during.len() / 2 - 512..during.len() / 2 + 512]);
    assert!(
        middle < root * 0.98 && middle > landed * 1.02,
        "the middle of the slide should be between {root} and {landed}, was {middle}"
    );
}

#[test]
fn a_slide_does_not_start_a_second_voice() {
    // What makes it a *slide* rather than a note: the voice that was sounding
    // is the voice that ends up at the new pitch.
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });

    sampler.trigger(NoteTrigger::new(60, 100));
    render(&mut sampler, &store, 256);
    assert_eq!(sampler.active_voices(), 1);

    sampler.slide(67, 0.05, 0);
    render(&mut sampler, &store, 256);
    assert_eq!(sampler.active_voices(), 1, "a slide is not a note-on");
}

#[test]
fn a_slide_with_nothing_sounding_does_nothing_rather_than_starting_a_note() {
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });

    sampler.slide(67, 0.05, 0);
    render(&mut sampler, &store, 256);
    assert_eq!(sampler.active_voices(), 0);
}

#[test]
fn the_note_off_that_ends_a_slid_voice_is_the_one_for_the_key_it_started_on() {
    // The score says "note 60, then slide to 67, then note-off 60". The voice
    // has to still answer to 60, or every slid note hangs.
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });

    sampler.trigger(NoteTrigger::new(60, 100));
    sampler.slide(67, 0.01, 0);
    render(&mut sampler, &store, 1024);
    sampler.note_off(60, 0);
    render(&mut sampler, &store, 48_000);
    assert_eq!(sampler.active_voices(), 0, "a slid note still ends");
}

#[test]
fn portamento_starts_a_mono_note_at_the_pitch_of_the_one_before_it() {
    // `VoiceConfig::glide_time_s` has been on the panel since the patch format
    // was written and was read by nothing.
    let mut store = SampleStore::new();
    let mut patch = ramp_patch(&mut store);
    patch.voice_config.retrigger = RetriggerMode::Mono;
    patch.voice_config.glide_time_s = 0.1;
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });

    sampler.trigger(NoteTrigger::new(60, 100));
    let root = period(&render(&mut sampler, &store, 2048));

    sampler.trigger(NoteTrigger::new(72, 100));
    // Right after the second note it is still near the *first* note's pitch.
    let just_after = period(&render(&mut sampler, &store, 512));
    assert!(
        just_after > root * 0.8,
        "a glide starts where the last note was: {root} then {just_after}"
    );

    // And a quarter of a second later it has arrived.
    render(&mut sampler, &store, 12_000);
    let landed = period(&render(&mut sampler, &store, 2048));
    assert!(
        (landed / (root / 2.0) - 1.0).abs() < 0.06,
        "it should arrive an octave up: {landed} against {}",
        root / 2.0
    );
}

#[test]
fn a_patch_with_no_glide_time_jumps_the_way_it_always_did() {
    let mut store = SampleStore::new();
    let mut patch = ramp_patch(&mut store);
    patch.voice_config.retrigger = RetriggerMode::Mono;
    patch.voice_config.glide_time_s = 0.0;
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });

    sampler.trigger(NoteTrigger::new(60, 100));
    let root = period(&render(&mut sampler, &store, 2048));
    sampler.trigger(NoteTrigger::new(72, 100));
    let landed = period(&render(&mut sampler, &store, 2048));
    assert!(
        (landed / (root / 2.0) - 1.0).abs() < 0.06,
        "with no glide the second note is at its own pitch immediately"
    );
}

// --- Phase 3 (`docs/flopsynth-next.md` §4.6): the mode, the curve, and a
// --- per-note glide time.

use fontelle_core::{Curve, GlideCurve, GlideMode, ModDest, ModRoute, ModSource};

/// Where a mono note is between its start and its target, as a fraction of
/// the octave, `frames` into the glide.
fn glide_progress(patch: Patch, store: &SampleStore, frames: usize) -> f32 {
    glide_progress_after(patch, store, frames, false)
}

/// As [`glide_progress`], letting go of the first note before the second
/// when `release_first` — so a poly patch has one voice to measure.
fn glide_progress_after(
    patch: Patch,
    store: &SampleStore,
    frames: usize,
    release_first: bool,
) -> f32 {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });
    sampler.trigger(NoteTrigger::new(60, 100));
    let root = period(&render(&mut sampler, store, 2048));
    if release_first {
        sampler.note_off(60, 0);
        render(&mut sampler, store, 128);
    }
    sampler.trigger(NoteTrigger::new(72, 100));
    if frames > 0 {
        render(&mut sampler, store, frames);
    }
    let now = period(&render(&mut sampler, store, 512));
    // Period halves over the octave; progress in semitones over twelve.
    (root / now).log2()
}

/// The glide's shape (§4.6): linear is a straight line in semitones, fast
/// covers most of the way early, slow late, and exponential is the
/// capacitor's curve — quick off the mark and asymptotic at the end.
#[test]
fn the_glide_curve_shapes_the_way_there() {
    let mut store = SampleStore::new();
    let base = {
        let mut patch = ramp_patch(&mut store);
        patch.voice_config.retrigger = RetriggerMode::Mono;
        patch.voice_config.glide_time_s = 0.2;
        patch
    };
    let halfway = (0.1 * SR) as usize;
    let with = |curve: GlideCurve, frames: usize| {
        let mut patch = base.clone();
        patch.voice_config.glide_curve = curve;
        glide_progress(patch, &store, frames)
    };
    let linear = with(GlideCurve::Linear, halfway);
    assert!(
        (0.4..=0.6).contains(&linear),
        "linear is halfway at half time: {linear:.2}"
    );
    let fast = with(GlideCurve::Fast, halfway);
    assert!(
        fast > 0.68,
        "fast is most of the way at half time: {fast:.2}"
    );
    let slow = with(GlideCurve::Slow, halfway);
    assert!(
        slow < 0.32,
        "slow has hardly started at half time: {slow:.2}"
    );
    let exponential = with(GlideCurve::Exponential, halfway);
    assert!(
        exponential > 0.85,
        "exponential is nearly there at half time: {exponential:.2}"
    );
    // And every one of them arrives.
    for curve in GlideCurve::ALL {
        let landed = with(curve, (0.3 * SR) as usize);
        assert!(
            (landed - 1.0).abs() < 0.06,
            "{curve:?} arrives: {landed:.2}"
        );
    }
    assert_eq!(GlideCurve::default(), GlideCurve::Linear);
}

/// The mode (§4.6): `Always` is portamento in a poly patch — every note
/// starts where the last one was — which the old bool could not say; the
/// bool's address keeps reading the same two values it did.
#[test]
fn glide_always_is_portamento_in_a_poly_patch() {
    use fontelle_core::patch_params::{set, value};
    let mut store = SampleStore::new();
    let mut patch = ramp_patch(&mut store);
    patch.voice_config.retrigger = RetriggerMode::Poly;
    patch.voice_config.glide_time_s = 0.2;
    // Poly and the old modes: a second note jumps to its own pitch.
    for mode in [GlideMode::Notes, GlideMode::Legato] {
        patch.voice_config.glide_mode = mode;
        let progress = glide_progress_after(patch.clone(), &store, 0, true);
        assert!(
            progress > 0.9,
            "{mode:?} in poly does not glide: {progress:.2}"
        );
    }
    // Always: it slides from the note before, even in poly — and the
    // first voice is still sounding underneath, released or not.
    patch.voice_config.glide_mode = GlideMode::Always;
    let progress = glide_progress_after(patch.clone(), &store, 0, true);
    assert!(
        progress < 0.3,
        "always glides in poly: {progress:.2} of the way at the start"
    );
    let landed = glide_progress_after(patch.clone(), &store, (0.3 * SR) as usize, true);
    assert!((landed - 1.0).abs() < 0.06, "and arrives: {landed:.2}");

    // The addresses: the old switch reads the same two values, the mode
    // chooser is new, and the two agree.
    assert_eq!(value(&patch, "patch/voice/legato"), Some(0.0));
    assert!(set(&mut patch, "patch/voice/legato", 1.0));
    assert_eq!(patch.voice_config.glide_mode, GlideMode::Legato);
    assert_eq!(value(&patch, "patch/voice/glide_mode"), Some(0.5));
    assert!(set(&mut patch, "patch/voice/glide_mode", 1.0));
    assert_eq!(patch.voice_config.glide_mode, GlideMode::Always);
    assert_eq!(value(&patch, "patch/voice/legato"), Some(0.0));
    assert!(set(&mut patch, "patch/voice/glide_mode", 0.0));
    assert_eq!(patch.voice_config.glide_mode, GlideMode::Notes);
    assert!(set(&mut patch, "patch/voice/glide_curve", 1.0));
    assert_eq!(patch.voice_config.glide_curve, GlideCurve::Slow);
    // The file: a patch written with the bool reads with the mode, and a
    // patch at the old modes writes what it wrote.
    let data = patch.to_data(&Default::default()).unwrap();
    let text = data.body.to_string();
    assert!(text.contains("\"glide_legato_only\":false"), "{text}");
    assert!(!text.contains("glide_always"), "{text}");
    patch.voice_config.glide_mode = GlideMode::Always;
    let data = patch.to_data(&Default::default()).unwrap();
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back.voice_config.glide_mode, GlideMode::Always);
    assert_eq!(back.voice_config.glide_curve, GlideCurve::Slow);
}

/// A per-note glide time (§4.6): `ModDest::GlideTime` is read when the
/// note starts, from the per-note sources — a soft note slides slowly, a
/// hard one snaps.
#[test]
fn the_glide_time_is_a_destination_read_at_the_note() {
    let mut store = SampleStore::new();
    let mut patch = ramp_patch(&mut store);
    patch.voice_config.retrigger = RetriggerMode::Mono;
    patch.voice_config.glide_time_s = 0.0;
    // Velocity adds up to two seconds: at velocity 127 the glide is two
    // seconds long, at 100 about a second and a half.
    patch.mod_matrix.routes.push(ModRoute {
        source: ModSource::Velocity,
        destination: ModDest::GlideTime,
        depth: 1.0,
        curve: Curve::Linear,
        via: None,
        invert: false,
        bypass: false,
    });
    let progress = glide_progress(patch.clone(), &store, (0.2 * SR) as usize);
    assert!(
        (0.05..=0.25).contains(&progress),
        "a fifth of a second into a second and a half: {progress:.2}"
    );
    // Without the route there is no glide time and the note jumps.
    patch.mod_matrix.routes.clear();
    let progress = glide_progress(patch, &store, 0);
    assert!(progress > 0.9, "{progress:.2}");
    assert!(
        fontelle_core::flopsynth::destinations(&fontelle_core::flopsynth::flopsynth_init())
            .iter()
            .any(|(d, _)| *d == ModDest::GlideTime)
    );
}
