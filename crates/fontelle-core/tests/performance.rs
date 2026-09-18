//! The wheels: what a **built-in** instrument does with a mod wheel, a pitch
//! bend and aftertouch (TDD §7.4, §7.5).
//!
//! The hosting pass gave these to plugins — `fontelle_host::HostedProcessor`
//! carries a controller into whatever language the plugin speaks — and left
//! this half open, so a soundfont played from a keyboard heard the notes and
//! none of the hand playing them. `ModSource::Aftertouch`, `ModWheel` and
//! `PitchBend` have been in the matrix since it was written and read as a
//! flat zero, which is what this closes.
//!
//! **They are live, not captured at note-on**, like the channel's own pan:
//! moving a wheel has to move what is already sounding, or it is not a wheel.
//!
//! Two of the three are matrix sources and nothing else — where a wheel goes
//! is the patch's decision, and inventing one would be a mapping nobody
//! asked for. The **bend** is the exception: every keyboard bends pitch, so
//! it is applied to the note's own pitch over a range the patch names
//! (`VoiceConfig::bend_range_semitones`, two semitones by default, which is
//! what SF2 2.04's default pitch-wheel modulator amounts to) — and it is
//! *also* a source, so a patch can send it somewhere else as well.

use fontelle_core::{
    Curve, FilterSlot, Layer, LoopMode, ModDest, ModMatrix, ModRoute, ModSource, NoteTrigger,
    Patch, PlaybackConfig, PrepareContext, SampleBuffer, SampleStore, Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

const SR: f32 = 48_000.0;

/// How many samples one cycle of the fixture takes at its root key.
const ROOT_PERIOD: f32 = 240.0;

/// A looping **ramp** rooted at key 60 — the same fixture the glide tests
/// use, and for the same reason: measuring pitch means counting how fast the
/// waveform repeats, and a ramp crosses its own midpoint exactly once a
/// cycle.
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

/// The dominant period of `out` in samples — see the glide tests, which
/// explain why this counts midpoint crossings rather than looking for the
/// ramp's cliff.
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

fn peak(out: &[f32]) -> f32 {
    out.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn playing(patch: Patch) -> (Sampler, SampleStore) {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });
    (sampler, SampleStore::new())
}

// ------------------------------------------------------------- the bend ---

/// A bend moves what is already sounding, and by the range the patch names —
/// two semitones by default. Up is up: a higher pitch is a shorter period.
#[test]
fn a_pitch_bend_moves_what_is_already_sounding() {
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let (mut sampler, _) = playing(patch);

    sampler.trigger(NoteTrigger::new(60, 100));
    let unbent = period(&render(&mut sampler, &store, 2048));

    sampler.set_pitch_bend(1.0);
    let bent = period(&render(&mut sampler, &store, 2048));
    let expected = ROOT_PERIOD / 2f32.powf(2.0 / 12.0);
    assert!(
        (bent / expected - 1.0).abs() < 0.05,
        "two semitones up from {unbent}: expected ~{expected}, measured {bent}"
    );

    // And down, which is the same control the other way.
    sampler.set_pitch_bend(-1.0);
    let down = period(&render(&mut sampler, &store, 2048));
    let expected = ROOT_PERIOD * 2f32.powf(2.0 / 12.0);
    assert!(
        (down / expected - 1.0).abs() < 0.05,
        "expected ~{expected}, measured {down}"
    );

    // Let go: back where it was, not left wherever it was pushed.
    sampler.set_pitch_bend(0.0);
    let centred = period(&render(&mut sampler, &store, 2048));
    assert!(
        (centred / unbent - 1.0).abs() < 0.02,
        "{centred} vs {unbent}"
    );
}

/// How far a bend goes is the **patch's** own setting, beside the glide and
/// the polyphony: a lead that bends an octave and a pad that bends a tone
/// are the same wheel and different instruments.
#[test]
fn how_far_a_bend_goes_is_the_patchs_own_setting() {
    let mut store = SampleStore::new();
    let mut patch = ramp_patch(&mut store);
    patch.voice_config.bend_range_semitones = 12.0;
    let (mut sampler, _) = playing(patch);

    sampler.trigger(NoteTrigger::new(60, 100));
    let unbent = period(&render(&mut sampler, &store, 2048));
    sampler.set_pitch_bend(1.0);
    let bent = period(&render(&mut sampler, &store, 2048));
    assert!(
        (bent / (unbent / 2.0) - 1.0).abs() < 0.05,
        "a full bend is an octave: {unbent} to {bent}"
    );
}

/// A bend is a **channel** control, so it moves every note sounding on it —
/// a bent chord is a bent chord — and a note started while the wheel is
/// held arrives already bent.
#[test]
fn a_bend_moves_a_note_started_while_it_is_held() {
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let (mut sampler, _) = playing(patch);

    sampler.set_pitch_bend(1.0);
    sampler.trigger(NoteTrigger::new(60, 100));
    let bent = period(&render(&mut sampler, &store, 2048));
    let expected = ROOT_PERIOD / 2f32.powf(2.0 / 12.0);
    assert!(
        (bent / expected - 1.0).abs() < 0.05,
        "expected ~{expected}, measured {bent}"
    );
}

// ----------------------------------------------------- the other two ---

/// A patch with one route: the source, full depth, onto the layer's gain.
fn routed(store: &mut SampleStore, source: ModSource) -> Patch {
    let mut patch = ramp_patch(store);
    patch.mod_matrix = ModMatrix {
        routes: vec![ModRoute {
            source,
            // Down 96 dB at rest and up at full — `invert` turns the
            // source round and the negative depth points it downward, so
            // the source at zero is silence and at full is unity. A source
            // read as a flat zero, which is what these three were, is a
            // note that never comes back.
            destination: ModDest::LayerGain(0),
            depth: -1.0,
            curve: Curve::Linear,
            via: None,
            invert: true,
            bypass: false,
        }],
    };
    patch
}

#[test]
fn the_mod_wheel_is_a_source_the_matrix_can_read() {
    let mut store = SampleStore::new();
    let patch = routed(&mut store, ModSource::ModWheel);
    let (mut sampler, _) = playing(patch);

    sampler.set_mod_wheel(1.0);
    sampler.trigger(NoteTrigger::new(60, 100));
    let open = peak(&render(&mut sampler, &store, 2048));
    assert!(open > 0.1, "the wheel is up: {open}");

    sampler.set_mod_wheel(0.0);
    let shut = peak(&render(&mut sampler, &store, 2048));
    assert!(
        shut < open / 8.0,
        "the wheel moved what was already sounding: {shut} vs {open}"
    );
}

#[test]
fn aftertouch_is_a_source_the_matrix_can_read() {
    let mut store = SampleStore::new();
    let patch = routed(&mut store, ModSource::Aftertouch);
    let (mut sampler, _) = playing(patch);

    sampler.set_aftertouch(1.0);
    sampler.trigger(NoteTrigger::new(60, 100));
    let leant = peak(&render(&mut sampler, &store, 2048));
    assert!(leant > 0.1, "{leant}");

    sampler.set_aftertouch(0.0);
    let released = peak(&render(&mut sampler, &store, 2048));
    assert!(released < leant / 8.0, "{released} vs {leant}");
}

/// The bend is a source **as well as** a pitch: a patch that routes it at a
/// filter or a level gets it, and the note bends too. It is bipolar, so a
/// centred wheel reads as zero.
#[test]
fn the_bend_is_a_source_as_well_as_a_pitch() {
    let mut store = SampleStore::new();
    let patch = routed(&mut store, ModSource::PitchBend);
    let (mut sampler, _) = playing(patch);

    sampler.set_pitch_bend(1.0);
    sampler.trigger(NoteTrigger::new(60, 100));
    let pushed = peak(&render(&mut sampler, &store, 2048));
    sampler.set_pitch_bend(0.0);
    let centred = peak(&render(&mut sampler, &store, 2048));
    assert!(pushed > centred * 4.0, "pushed {pushed}, centred {centred}");
}

/// Everything a hand is doing is forgotten when the instrument is reset —
/// a transport stop must not leave the next note bent.
#[test]
fn a_reset_lets_go_of_every_wheel() {
    let mut store = SampleStore::new();
    let patch = ramp_patch(&mut store);
    let (mut sampler, _) = playing(patch);

    sampler.trigger(NoteTrigger::new(60, 100));
    let unbent = period(&render(&mut sampler, &store, 2048));
    sampler.set_pitch_bend(1.0);
    sampler.set_mod_wheel(1.0);
    sampler.set_aftertouch(1.0);
    sampler.reset();

    sampler.trigger(NoteTrigger::new(60, 100));
    let after = period(&render(&mut sampler, &store, 2048));
    assert!((after / unbent - 1.0).abs() < 0.02, "{after} vs {unbent}");
}
