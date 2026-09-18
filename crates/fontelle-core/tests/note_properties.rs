//! The four per-note properties that were stored, drawn, editable — and heard
//! by nothing: fine pitch, release, and the two free modulation values.
//!
//! `Note` has carried all five of §16.5's properties since the document format
//! was written. Pan arrived at a voice; these four were dropped between the
//! score and the sampler, which meant the piano roll's property lane invited
//! you to draw four curves that changed no sound. This file is the seam they
//! cross, and each test measures the *audible* thing rather than the field:
//!
//! - **fine pitch** as pitch, by counting cycles;
//! - **release** as the length of the tail a note-off leaves behind;
//! - **mod X and mod Y** as two independent modulation sources a patch routes
//!   like any other, because "free" means the patch decides what they do.
//!
//! Two of these have a compatibility rule that is worth stating out loud,
//! since both defaults are `0` and every note ever written carries them:
//! `fine_pitch: 0` is the note as written, and `release: 0` is *the patch's
//! own* release rather than the shortest one. A property whose default
//! silently rewrote every existing project would not be a property worth
//! having.

use fontelle_core::{
    Curve, FilterSlot, Layer, LoopMode, ModDest, ModRoute, ModSource, NoteTrigger, Patch,
    PlaybackConfig, PrepareContext, SampleBuffer, SampleStore, Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

const SR: f32 = 48_000.0;

/// How many samples one cycle of the fixture takes at its root key.
const ROOT_PERIOD: f32 = 240.0;

/// A patch over a looping ramp rooted at key 60 — the same fixture the glide
/// tests use, and for the same reason: a ramp's pitch is a thing you count.
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
        envelopes: vec![flat(0.05), flat(0.05)],
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

/// Straight to full, stays there, and falls over `release_s`.
fn flat(release_s: f32) -> EnvelopeConfig {
    EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s,
        curve: EnvelopeCurve::Linear,
        ..Default::default()
    }
}

/// The dominant period of `out`, in samples, by counting upward crossings of
/// the ramp's midpoint — once per cycle however the wrap gets interpolated.
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

fn ready(patch: Patch) -> Sampler {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });
    sampler
}

// ---------------------------------------------------------------- fine pitch

#[test]
fn fine_pitch_moves_a_note_off_its_own_key() {
    // 1200 cents is an octave, and an octave up is half the period. Cents
    // rather than some private unit because that is what `Note::fine_pitch`
    // says it is, and it is the unit the rest of the pitch path already adds
    // in (layer tuning and the mod matrix's pitch destination are both cents).
    let mut store = SampleStore::new();
    let mut sampler = ready(ramp_patch(&mut store));

    sampler.trigger(NoteTrigger::new(60, 100).with_fine_pitch(1200));
    let out = render(&mut sampler, &store, 4096);

    let measured = period(&out[1024..]);
    let expected = ROOT_PERIOD / 2.0;
    assert!(
        (measured / expected - 1.0).abs() < 0.05,
        "+1200 cents should be an octave up (~{expected} samples), measured {measured}"
    );
}

#[test]
fn fine_pitch_goes_down_as_well_as_up() {
    let mut store = SampleStore::new();
    let mut sampler = ready(ramp_patch(&mut store));

    sampler.trigger(NoteTrigger::new(72, 100).with_fine_pitch(-1200));
    let out = render(&mut sampler, &store, 4096);

    let measured = period(&out[1024..]);
    assert!(
        (measured / ROOT_PERIOD - 1.0).abs() < 0.05,
        "key 72 less an octave is the root (~{ROOT_PERIOD} samples), measured {measured}"
    );
}

#[test]
fn a_fine_pitch_of_zero_is_the_note_as_written() {
    // The default every note in every existing project carries.
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let mut plain = ready(patch.clone());
    let mut zeroed = ready(patch);

    plain.trigger(NoteTrigger::new(64, 100));
    zeroed.trigger(NoteTrigger::new(64, 100).with_fine_pitch(0));

    let a = render(&mut plain, &store, 4096);
    let b = render(&mut zeroed, &store, 4096);
    assert_eq!(
        a, b,
        "fine pitch 0 should be sample-for-sample the plain note"
    );
}

#[test]
fn fine_pitch_is_per_note_not_per_channel() {
    // Two notes sounding together, detuned apart — which is the whole reason
    // it rides on the note-on rather than being a parameter of the sampler.
    let mut store = SampleStore::new();
    // All three patches out of the one store: a sampler renders against the
    // store it was built from, and three separate stores would be three sets
    // of handles that resolve to nothing here.
    let (together, first, second) = (
        ramp_patch(&mut store),
        ramp_patch(&mut store),
        ramp_patch(&mut store),
    );
    let mut sampler = ready(together);

    sampler.trigger(NoteTrigger::new(60, 100).in_context(1));
    sampler.trigger(
        NoteTrigger::new(60, 100)
            .in_context(2)
            .with_fine_pitch(1200),
    );
    let both = render(&mut sampler, &store, 4096);

    // Against the two notes rendered on their own: what comes out of one
    // sampler is what comes out of two, which is only true if each voice kept
    // its own detune. Sample-for-sample rather than by ear, because "they
    // sound different" is what a leak between voices looks like too.
    let mut alone = ready(first);
    alone.trigger(NoteTrigger::new(60, 100));
    let plain = render(&mut alone, &store, 4096);

    let mut detuned = ready(second);
    detuned.trigger(NoteTrigger::new(60, 100).with_fine_pitch(1200));
    let up = render(&mut detuned, &store, 4096);

    assert!(
        (period(&plain[512..]) / period(&up[512..]) - 2.0).abs() < 0.1,
        "the fixture: one note at the root and one an octave above it"
    );
    for (index, sample) in both.iter().enumerate() {
        let sum = plain[index] + up[index];
        assert!(
            (sample - sum).abs() < 1e-4,
            "at sample {index}: two voices together should be their sum, \
             {sample} against {sum}"
        );
    }
}

// ------------------------------------------------------------------- release

/// How many samples after a note-off the voice keeps making sound.
fn tail(sampler: &mut Sampler, store: &SampleStore, key: u8, release: u8) -> usize {
    sampler.trigger(NoteTrigger::new(key, 100).with_release(release));
    render(sampler, store, 1024);
    sampler.note_off(key, 0);
    let out = render(sampler, store, (SR * 2.0) as usize);
    out.iter()
        .rposition(|s| s.abs() > 1e-4)
        .map_or(0, |i| i + 1)
}

#[test]
fn a_higher_release_leaves_a_longer_tail() {
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let short = tail(&mut ready(patch.clone()), &store, 60, 0);
    let long = tail(&mut ready(patch), &store, 60, 127);

    assert!(
        long > short * 2,
        "release 127 should ring on well past release 0: {long} samples against {short}"
    );
}

#[test]
fn a_release_of_zero_is_the_patchs_own_release() {
    // The compatibility rule, asserted rather than assumed: `release: 0` is
    // the default on every note ever written, and it has to mean "as the
    // instrument says" or opening an old project would change how it sounds.
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);

    let mut plain = ready(patch.clone());
    plain.trigger(NoteTrigger::new(60, 100));
    render(&mut plain, &store, 1024);
    plain.note_off(60, 0);
    let untouched = render(&mut plain, &store, (SR * 2.0) as usize);
    let untouched = untouched
        .iter()
        .rposition(|s| s.abs() > 1e-4)
        .map_or(0, |i| i + 1);

    let zeroed = tail(&mut ready(patch), &store, 60, 0);
    assert_eq!(
        zeroed, untouched,
        "release 0 should be exactly the patch's own 50ms release"
    );
    // And that is the patch's 0.05s, not some other number.
    let expected = (SR * 0.05) as usize;
    assert!(
        (zeroed as i64 - expected as i64).abs() < 256,
        "expected about {expected} samples of tail, got {zeroed}"
    );
}

#[test]
fn release_is_per_note_so_two_notes_can_ring_for_different_lengths() {
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let mut sampler = ready(patch);

    sampler.trigger(NoteTrigger::new(60, 100).in_context(1).with_release(0));
    sampler.trigger(NoteTrigger::new(67, 100).in_context(2).with_release(127));
    render(&mut sampler, &store, 1024);
    sampler.note_off(60, 1);
    sampler.note_off(67, 2);

    // Well past the short note's tail (the patch's own 50ms) and well short
    // of the long one's (four times it).
    render(&mut sampler, &store, (SR * 0.1) as usize);
    assert_eq!(
        sampler.active_voices(),
        1,
        "the short note should be gone and the long one still ringing"
    );
}

// ------------------------------------------------------------ mod X and mod Y

fn routed(store: &mut SampleStore, source: ModSource) -> Patch {
    let mut patch = ramp_patch(store);
    patch.mod_matrix.routes.push(ModRoute {
        source,
        destination: ModDest::LayerGain(0),
        depth: -1.0,
        curve: Curve::Linear,
        via: None,
        invert: false,
        bypass: false,
    });
    patch
}

/// Peak level of a rendered block.
fn peak(out: &[f32]) -> f32 {
    out.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn mod_x_is_a_modulation_source_a_patch_can_route() {
    // "Free" means the *patch* decides what it does, so the seam being tested
    // is that it arrives at the mod matrix as a source like any other — here
    // routed at full depth to a layer's gain, so more mod X is less level.
    let mut store = SampleStore::new();
    let patch = routed(&mut store, ModSource::NoteModX);

    let mut quiet = ready(patch.clone());
    quiet.trigger(NoteTrigger::new(60, 100).with_mod_x(127));
    let quiet = peak(&render(&mut quiet, &store, 2048));

    let mut loud = ready(patch);
    loud.trigger(NoteTrigger::new(60, 100).with_mod_x(0));
    let loud = peak(&render(&mut loud, &store, 2048));

    assert!(
        quiet < loud * 0.5,
        "mod X routed to gain should be audible: {quiet} against {loud}"
    );
}

#[test]
fn mod_y_is_a_second_and_independent_one() {
    // Two knobs, not one knob wired twice: a patch listening to Y must not
    // hear X.
    let mut store = SampleStore::new();
    let patch = routed(&mut store, ModSource::NoteModY);

    let mut moved = ready(patch.clone());
    moved.trigger(NoteTrigger::new(60, 100).with_mod_y(127));
    let moved = peak(&render(&mut moved, &store, 2048));

    let mut other = ready(patch.clone());
    other.trigger(NoteTrigger::new(60, 100).with_mod_x(127));
    let other = peak(&render(&mut other, &store, 2048));

    let mut plain = ready(patch);
    plain.trigger(NoteTrigger::new(60, 100));
    let plain = peak(&render(&mut plain, &store, 2048));

    assert!(
        moved < plain * 0.5,
        "mod Y should reach the route it is wired to"
    );
    assert!(
        (other - plain).abs() < plain * 0.02,
        "mod X should not reach a route wired to mod Y: {other} against {plain}"
    );
}

#[test]
fn an_unrouted_mod_value_changes_nothing() {
    // The default patch routes neither, and a note drawn with both wide open
    // should sound exactly like one drawn without.
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);

    let mut plain = ready(patch.clone());
    plain.trigger(NoteTrigger::new(60, 100));
    let a = render(&mut plain, &store, 2048);

    let mut wide = ready(patch);
    wide.trigger(NoteTrigger::new(60, 100).with_mod_x(127).with_mod_y(127));
    let b = render(&mut wide, &store, 2048);

    assert_eq!(a, b, "a source nothing is routed from should be inaudible");
}

#[test]
fn a_recycled_voice_does_not_carry_the_last_notes_properties() {
    // The bug this class of field invites, and the one `pan` already had: a
    // voice comes back out of the pool still holding what the previous note
    // set, and a plain note plays as the one before it.
    let mut store = SampleStore::new();
    let mut sampler = ready(ramp_patch(&mut store));

    sampler.trigger(
        NoteTrigger::new(60, 100)
            .with_fine_pitch(1200)
            .with_release(127),
    );
    render(&mut sampler, &store, 512);
    sampler.note_off(60, 0);
    render(&mut sampler, &store, (SR * 2.0) as usize);

    sampler.trigger(NoteTrigger::new(60, 100));
    let out = render(&mut sampler, &store, 4096);
    let measured = period(&out[1024..]);
    assert!(
        (measured / ROOT_PERIOD - 1.0).abs() < 0.05,
        "the second note is a plain key 60 (~{ROOT_PERIOD} samples), measured {measured}"
    );
}
