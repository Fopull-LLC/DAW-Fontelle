//! The Grand Piano a new project opens on — the claims that make it a piano.
//!
//! > *"the grand piano sound doesnt sound realistic at all right now"* — Ty,
//! > 2026-09-07; *"still doesnt sound much like a grand piano its sounding
//! > kind of like a mix between a clav and a electric piano"* — 2026-09-08;
//! > *"the grand piano still sounds just a lot like a basic synth wave and
//! > not a actual grand piano ... maybe you could use the osc sampling
//! > feature to make the piano sound more realistic if you can find a grand
//! > piano one shot to use."* — 2026-09-16.
//!
//! Four reports. The first three rounds voiced a table, then a stiff
//! string, against a sampled grand's measurements, and each round measured
//! right and was heard as a synth. The fourth is answered the way Ty asked:
//! the row **plays a recording of a grand** — forty-six of them, a Yamaha
//! C5 at two strengths (`fontelle_core::factory_samples`), crossfaded by
//! velocity through the same sample oscillator a dropped file plays through.
//! So what this file holds is no longer "does it measure like a piano" but
//! "is it the piano, played right": the recordings are what sounds, every
//! key lands on one near its own pitch, a soft touch is the soft recording
//! and a hard one the hard, the level follows velocity without a dip where
//! the two cross, a release is a damper and not a cut, and a project file
//! names the piano rather than carrying it.

use fontelle_core::factory_samples::FactorySampleSet;
use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{NoteTrigger, Patch, PrepareContext, SILENT_DB, SampleStore, Sampler, Source};
use fontelle_dsp::SynthSource;

const SR: f32 = 48_000.0;

fn grand_piano() -> Patch {
    let row = FACTORY
        .iter()
        .find(|row| row.name == "Grand Piano")
        .expect("the bank has a Grand Piano");
    (row.build)()
}

fn sampler_for(patch: Patch) -> Sampler {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler
}

fn render_for(sampler: &mut Sampler, seconds: f32, out: &mut Vec<f32>) {
    let store = SampleStore::new();
    let total = (SR * seconds) as usize;
    let mut done = 0usize;
    while done < total {
        let frames = 512.min(total - done);
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
        for (a, b) in left.iter().zip(&right) {
            out.push((a + b) * 0.5);
        }
        done += frames;
    }
}

/// One held note, `seconds` long.
fn render(patch: Patch, key: u8, velocity: u8, seconds: f32) -> Vec<f32> {
    let mut sampler = sampler_for(patch);
    sampler.trigger(NoteTrigger::new(key, velocity));
    let mut out = Vec::new();
    render_for(&mut sampler, seconds, &mut out);
    out
}

/// A note held for `held` seconds, then released and left for `after`.
fn render_released(patch: Patch, key: u8, velocity: u8, held: f32, after: f32) -> Vec<f32> {
    let mut sampler = sampler_for(patch);
    sampler.trigger(NoteTrigger::new(key, velocity));
    let mut out = Vec::new();
    render_for(&mut sampler, held, &mut out);
    sampler.release_all();
    render_for(&mut sampler, after, &mut out);
    out
}

/// One held note, left and right.
fn render_stereo(patch: Patch, key: u8, velocity: u8, seconds: f32) -> (Vec<f32>, Vec<f32>) {
    let store = SampleStore::new();
    let mut sampler = sampler_for(patch);
    sampler.trigger(NoteTrigger::new(key, velocity));
    let total = (SR * seconds) as usize;
    let (mut left_out, mut right_out) = (Vec::new(), Vec::new());
    let mut done = 0usize;
    while done < total {
        let frames = 512.min(total - done);
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
        left_out.extend_from_slice(&left);
        right_out.extend_from_slice(&right);
        done += frames;
    }
    (left_out, right_out)
}

/// The piano with one of its layers taken out, so what that layer adds can
/// be measured as the difference.
fn without(layer: usize) -> Patch {
    let mut patch = grand_piano();
    patch.layers[layer].gain_db = SILENT_DB;
    patch.mod_matrix.routes.retain(|route| {
        !matches!(route.destination, fontelle_core::ModDest::LayerGain(at) if usize::from(at) == layer)
    });
    patch
}

/// Which layer plays the string, and which the noise.
const STRING: usize = 2;
const NOISE: usize = 4;

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

fn level_db(samples: &[f32], from: f32, seconds: f32) -> f32 {
    let start = ((SR * from) as usize).min(samples.len());
    let end = samples.len().min(start + (SR * seconds) as usize);
    db(rms(&samples[start..end]))
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

/// Seconds from the loudest ten milliseconds to the first one 30 dB under it.
fn t30(samples: &[f32]) -> f32 {
    let envelope: Vec<f32> = samples.chunks(480).map(rms).collect();
    let (loudest_at, loudest) = envelope
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .expect("a rendered note has samples");
    envelope[loudest_at..]
        .iter()
        .position(|level| *level < loudest * 0.0316)
        .unwrap_or(envelope.len() - loudest_at) as f32
        * 0.01
}

/// How **bright** the note is: the energy in partials 4 to 16 against the
/// energy in partials 1 to 3, in decibels.
///
/// A tilt rather than a spectral centroid, and the difference matters. A
/// centroid is a magnitude-weighted mean, so on a spectrum whose fundamental
/// stands well over everything else — which is what a piano's is, and what
/// `the_fundamental_is_the_strongest_partial` insists on — it is pinned near
/// the fundamental and barely moves however the upper partials change.
/// Measured: a strike an ear hears as far brighter than its own ring a second
/// later read a fifth of an octave apart on a centroid, which is nothing.
/// What the ear follows is how much is up there at all, and that is this.
fn brightness(samples: &[f32], key: u8, from: f32, seconds: f32) -> f32 {
    let f0 = 440.0 * 2f32.powf((key as f32 - 69.0) / 12.0);
    let start = ((SR * from) as usize).min(samples.len());
    let samples = &samples[start..samples.len().min(start + (SR * seconds) as usize)];
    let mut low = 0.0f32;
    let mut high = 0.0f32;
    for h in 1..=16 {
        let hz = f0 * h as f32;
        if hz > 16_000.0 {
            break;
        }
        let power = partial_power(samples, hz, h);
        if h <= 3 { low += power } else { high += power }
    }
    10.0 * (high / low.max(1e-18)).max(1e-12).log10()
}

/// The power of the partial nearest harmonic `h` of the note, at `hz` —
/// **searched for**, not read at `hz`.
///
/// A piano's partials are not at the harmonics: a stiff string's nth sits
/// at `n·f0·sqrt(1 + B·n²)`, which at middle C is 21 cents sharp by the
/// fifth and a semitone by the sixteenth. A reading taken *at* `n·f0` over a
/// third of a second has bins three hertz wide, so from the fifth partial
/// up it reads the gap between partials and calls it silence — which is what
/// this file did while the piano was a table (exactly harmonic, so nothing
/// was missed) and what made a stretched fifth read −70 dB the day the
/// piano became a string. The sampled reference is stretched too; it had
/// been misread the same way.
///
/// The search widens with the harmonic, as the stretch does: ±10 cents on
/// the fundamental, ±120 by the sixteenth.
fn partial_power(samples: &[f32], hz: f32, h: usize) -> f32 {
    let n = samples.len() as f32;
    let power_at = |hz: f32| {
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, sample) in samples.iter().enumerate() {
            let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
            let phase = std::f32::consts::TAU * hz * i as f32 / SR;
            re += sample * window * phase.cos();
            im -= sample * window * phase.sin();
        }
        (re * re + im * im) / (n * n)
    };
    let span = 10.0 + 7.0 * h as f32;
    let mut best = 0.0f32;
    let mut cents = 0.0f32;
    while cents <= span {
        best = best.max(power_at(hz * 2f32.powf(cents / 1200.0)));
        cents += 3.0;
    }
    best
}
/// The row plays the sampled grand: a soft recording and a hard one on two
/// oscillators, and nothing synthesised beside them. The rest of this file
/// is what "played right" means.
#[test]
fn it_is_the_sampled_grand_not_a_synth() {
    let patch = grand_piano();
    let set_on = |layer: usize| match &patch.layers[layer].source {
        Source::Synth(osc) => match osc.source {
            SynthSource::Sample(at) => patch.samples.get(usize::from(at)).and_then(|s| s.factory),
            _ => None,
        },
        _ => None,
    };
    assert_eq!(set_on(0), Some(FactorySampleSet::GrandSoft));
    assert_eq!(set_on(1), Some(FactorySampleSet::GrandHard));
    assert!(patch.layers[0].gain_db > SILENT_DB && patch.layers[1].gain_db > SILENT_DB);
    // What sits beside the recordings is the instrument's mechanics, not a
    // second tone: a string that sings under them and the noise of the
    // hammer and the damper. Each is held to its job below.
    assert!(matches!(
        &patch.layers[STRING].source,
        Source::Synth(osc) if osc.source == SynthSource::String
    ));
    assert!(matches!(
        &patch.layers[NOISE].source,
        Source::Synth(osc) if osc.source == SynthSource::Noise
    ));
    // And a project file names the piano rather than carrying it.
    let data = patch.to_data(&Default::default()).unwrap();
    let text = serde_json::to_string(&data).unwrap();
    assert!(text.len() < 40_000, "{} bytes", text.len());
}

/// Every key plays a recording near its own pitch: the nearest one, never
/// more than two semitones away, transposed the rest — so nothing is a
/// chipmunk and nothing is a zone from the wrong octave.
#[test]
fn every_key_lands_on_a_recording_near_its_own_pitch() {
    let patch = grand_piano();
    for sample in &patch.samples {
        for key in 0..=127u8 {
            let zone = sample.zone_for(key).expect("a zone for every key");
            let away = (i16::from(zone.root_key) - i16::from(key)).abs();
            let stretch = if (21..=108).contains(&key) { 2 } else { 24 };
            assert!(
                away <= stretch,
                "{}: key {key} plays the recording of {}, {away} semitones off",
                sample.name,
                zone.root_key
            );
        }
    }
    // And what comes out is at the key's pitch, where the fundamental is
    // the strongest thing in the note (it is not, in the bass: a short
    // soundboard does not radiate 65 Hz, and the C5's C2 has more second
    // partial than first).
    for key in [60u8, 72, 84, 96] {
        let note = render(patch.clone(), key, 100, 0.6);
        let f0 = 440.0 * 2f32.powf((f32::from(key) - 69.0) / 12.0);
        let at = |hz: f32| 10.0 * partial_power(&note[..], hz, 1).max(1e-18).log10();
        let own = at(f0);
        for semis in [-2.0f32, -1.0, 1.0, 2.0] {
            let other = at(f0 * 2f32.powf(semis / 12.0));
            assert!(
                own > other + 10.0,
                "key {key}: {own:.1} dB at its own pitch, {other:.1} dB {semis} semitones off"
            );
        }
    }
}

/// A soft touch is the soft recording and a hard one the hard: the strike
/// is darker at velocity 20 than at 120 by more than a level knob could
/// make it, and the level rises with velocity **all the way up** — the two
/// recordings cross in the middle without the dip two gains summed in
/// decibels would leave.
#[test]
fn velocity_crossfades_the_soft_recording_into_the_hard_one() {
    let patch = grand_piano();
    let soft = render(patch.clone(), 60, 20, 0.5);
    let hard = render(patch.clone(), 60, 120, 0.5);
    let (a, b) = (
        brightness(&soft, 60, 0.0, 0.3),
        brightness(&hard, 60, 0.0, 0.3),
    );
    assert!(
        b > a + 5.0,
        "a hard strike should be brighter than a soft one: {b:.1} against {a:.1} dB"
    );
    let levels: Vec<f32> = [8u8, 32, 56, 80, 104, 127]
        .iter()
        .map(|v| level_db(&render(patch.clone(), 60, *v, 0.4), 0.0, 0.3))
        .collect();
    for pair in levels.windows(2) {
        assert!(
            pair[1] > pair[0] + 1.0,
            "the level should rise with velocity, not dip where the recordings cross: {levels:?}"
        );
    }
    // From the lightest touch a sequencer sends to the hardest: a real
    // piano's pp to ff is thirty-odd decibels, a sampled library's forty.
    let span = levels[levels.len() - 1] - levels[0];
    assert!(
        (28.0..=50.0).contains(&span),
        "a piano's pp to ff is thirty-odd decibels, not {span:.1}: {levels:?}"
    );
}

/// The bass rings long and the treble short — the recording's own decay,
/// which the envelope is not allowed to shorten into a synth's.
#[test]
fn the_bass_rings_long_and_the_treble_rings_short() {
    let patch = grand_piano();
    let bass = t30(&render(patch.clone(), 36, 100, 5.0));
    let middle = t30(&render(patch.clone(), 60, 100, 4.0));
    let top = t30(&render(patch, 96, 100, 2.0));
    assert!(bass > 1.2, "C2 should ring: 30 dB in {bass:.2} s");
    assert!(
        (0.7..=3.0).contains(&middle),
        "middle C: 30 dB in {middle:.2} s, the reference's 1.4"
    );
    assert!(top < 0.6, "C7 should be short: 30 dB in {top:.2} s");
    assert!(bass > middle && middle > top);
}

/// A note that outlives its recording fades; it does not stop. Held longer
/// than any recording is, the tail has no step in it, and it is well down
/// — how far depends on the key, because the modelled string under the
/// recording rings on in the bass the way a real bass string does (the
/// reference's C2 is thirty decibels down in two seconds and audible long
/// after), and is gone at the top.
#[test]
fn a_note_that_outlives_its_recording_fades_rather_than_stops() {
    let patch = grand_piano();
    for (key, seconds, down) in [(21u8, 5.5f32, 25.0f32), (60, 4.0, 40.0), (108, 2.5, 45.0)] {
        let note = render(patch.clone(), key, 110, seconds);
        let peak = note.iter().fold(0f32, |m, s| m.max(s.abs()));
        let from = (SR * 0.5) as usize;
        let biggest_step = note[from..]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0f32, f32::max);
        assert!(
            biggest_step < peak * 0.08,
            "key {key}: a step of {biggest_step:.3} against a peak of {peak:.3} is a cut"
        );
        let end = level_db(&note, seconds - 0.3, 0.3);
        let start = level_db(&note, 0.0, 0.3);
        assert!(
            end < start - down || end < -70.0,
            "key {key}: {end:.1} dB at the end of {seconds} s against {start:.1} at the strike"
        );
    }
}

/// The note starts from rest: the recording begins before the hammer lands
/// and there is no click of a waveform starting mid-cycle.
#[test]
fn the_note_starts_from_rest_without_a_click() {
    let note = render(grand_piano(), 60, 110, 0.3);
    let peak = note.iter().fold(0f32, |m, s| m.max(s.abs()));
    let first = &note[..(SR * 0.0003) as usize];
    let loudest_first = first.iter().fold(0f32, |m, s| m.max(s.abs()));
    assert!(
        loudest_first < peak * 0.05,
        "{loudest_first:.3} in the first third of a millisecond, against a peak of {peak:.3}"
    );
}

/// A release is a damper on the string: the note is gone in a few tenths
/// of a second, and it goes as a fade, not a cut.
#[test]
fn a_release_is_a_damper_not_a_cut() {
    let patch = grand_piano();
    for key in [36u8, 60, 84] {
        let note = render_released(patch.clone(), key, 100, 1.0, 1.0);
        let before = level_db(&note, 0.9, 0.1);
        let after = level_db(&note, 1.4, 0.1);
        assert!(
            after < before - 30.0,
            "key {key}: {after:.1} dB four tenths after the release, {before:.1} before it"
        );
        let release = &note[(SR * 1.0) as usize..(SR * 1.05) as usize];
        let biggest_step = release
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0f32, f32::max);
        let peak = note.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(biggest_step < peak * 0.1, "key {key}: the release clicks");
    }
}

/// The recordings are a real string's, stretched: a stiff string's nth
/// partial sits at `n·f0·sqrt(1 + B·n²)`, sharp of the harmonic and more so
/// for every partial up. This is what three rounds of voicing a table could
/// never give the piano and what the string source was built to, and a
/// recording has it for free. The numbers are the C5's own, read off the
/// recording: a big piano's long strings are less stiff than a small one's,
/// so middle C's twelfth partial is some twenty cents sharp where a baby
/// grand's would be fifty, and its sixth five or six.
#[test]
fn the_partials_are_stretched_the_way_a_stiff_strings_are() {
    let note = render(grand_piano(), 60, 100, 0.6);
    let f0 = 261.63f32;
    let window = &note[..(SR * 0.4) as usize];
    // Where each partial actually is: the sharpest reading within a range.
    let sharpest = |h: usize| {
        let mut best = (0.0f32, f32::MIN);
        let mut cents = -10.0f32;
        while cents <= 80.0 {
            let hz = f0 * h as f32 * 2f32.powf(cents / 1200.0);
            let power = partial_power_at(window, hz);
            if power > best.1 {
                best = (cents, power);
            }
            cents += 2.0;
        }
        best.0
    };
    let sixth = sharpest(6);
    let twelfth = sharpest(12);
    assert!(
        twelfth > 12.0,
        "the twelfth partial should be well sharp of the harmonic: {twelfth:.0} cents"
    );
    assert!(
        twelfth > sixth + 6.0,
        "and sharper than the sixth: {twelfth:.0} against {sixth:.0} cents"
    );
}

/// The power at exactly `hz`, no search.
fn partial_power_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / SR;
        re += sample * window * phase.cos();
        im -= sample * window * phase.sin();
    }
    (re * re + im * im) / (n * n)
}

#[test]
fn a_released_bass_note_frees_its_voice() {
    let store = SampleStore::new();
    let mut sampler = sampler_for(grand_piano());
    sampler.trigger(NoteTrigger::new(24, 100));

    let render_for = |sampler: &mut Sampler, seconds: f32| {
        let mut done = 0usize;
        let total = (SR * seconds) as usize;
        while done < total {
            let frames = 512.min(total - done);
            let mut left = vec![0.0f32; frames];
            let mut right = vec![0.0f32; frames];
            sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
            done += frames;
        }
    };

    render_for(&mut sampler, 0.5);
    assert_eq!(sampler.active_voices(), 1, "the note is still held");

    sampler.release_all();
    // A second is several times the damper's own few hundred milliseconds.
    render_for(&mut sampler, 1.0);
    assert_eq!(
        sampler.active_voices(),
        0,
        "a released note must give its voice back rather than ring on for the \
         minutes its decay would otherwise take"
    );
}

// ---------------------------------------------------------- the mechanics ---
//
// > *"it sounds like a soundfont with reverb right now pretty much. i want
// > you to use the features in the synth to take this sampled sound and then
// > turn it into a really tactile realistic feeling piano instrument
// > preset."* — Ty, 2026-09-16
//
// A soundfont is a recording played back; an instrument is the recording
// plus what a body does around it. What is added, each measured as the
// difference the layer makes: the keyboard across the stereo field, the
// hammer felt on a hard strike, the damper heard on letting go, and the
// strings singing on under the recording.

/// The keyboard lies left to right across the stereo field, as it does
/// from the bench: the bass to the left, the treble to the right, middle C
/// in the middle.
#[test]
fn the_keyboard_lies_left_to_right_across_the_stereo_field() {
    let patch = grand_piano();
    let balance = |key: u8| {
        let (left, right) = render_stereo(patch.clone(), key, 100, 0.5);
        db(rms(&right)) - db(rms(&left))
    };
    let bass = balance(36);
    let middle = balance(60);
    let treble = balance(96);
    assert!(
        bass < -2.0,
        "C2 should lean left: {bass:+.1} dB right of left"
    );
    assert!(treble > 2.0, "C7 should lean right: {treble:+.1} dB");
    assert!(
        middle.abs() < 1.5,
        "middle C in the middle: {middle:+.1} dB"
    );
}

/// The hammer is felt on a hard strike: the noise layer is in the strike
/// of a fortissimo — the recording's hammer lands ten to forty
/// milliseconds in; its first ten are the room before it — and nowhere
/// after, and there is less of it in a soft touch.
#[test]
fn the_hammer_is_felt_on_a_hard_strike_and_not_after_it() {
    let with = render(grand_piano(), 60, 120, 0.4);
    let muted = render(without(NOISE), 60, 120, 0.4);
    let strike = level_db(&with, 0.01, 0.03) - level_db(&muted, 0.01, 0.03);
    let later = level_db(&with, 0.1, 0.2) - level_db(&muted, 0.1, 0.2);
    assert!(
        strike > 1.0,
        "the hammer should add to the strike: {strike:+.2} dB"
    );
    assert!(
        later.abs() < 0.3,
        "and be gone once the string sounds: {later:+.2} dB at 100 ms"
    );
    let soft_with = render(grand_piano(), 60, 30, 0.4);
    let soft_without = render(without(NOISE), 60, 30, 0.4);
    let soft = level_db(&soft_with, 0.01, 0.03) - level_db(&soft_without, 0.01, 0.03);
    assert!(
        soft < strike - 1.0,
        "a soft touch has less hammer in it: {soft:+.2} against {strike:+.2} dB"
    );
}

/// Letting go has the damper's sound: the noise layer is heard in the
/// moment after the release — the felt landing on the string — and not
/// while the note is held.
#[test]
fn letting_go_has_the_dampers_sound() {
    let with = render_released(grand_piano(), 60, 100, 1.0, 0.5);
    let muted = render_released(without(NOISE), 60, 100, 1.0, 0.5);
    let held = level_db(&with, 0.5, 0.4) - level_db(&muted, 0.5, 0.4);
    let release = level_db(&with, 1.0, 0.05) - level_db(&muted, 1.0, 0.05);
    assert!(
        held.abs() < 0.3,
        "no damper while the key is down: {held:+.2} dB"
    );
    assert!(
        release > 1.0,
        "the damper should be heard as the key comes up: {release:+.2} dB"
    );
    // And it is a sound, not a click: no step in it.
    let peak = with.iter().fold(0f32, |m, s| m.max(s.abs()));
    let from = (SR * 0.98) as usize;
    let step = with[from..from + (SR * 0.1) as usize]
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0f32, f32::max);
    assert!(step < peak * 0.1, "the damper clicks: a step of {step:.3}");
}

/// The strings sing on under the recording: a modelled string at the note,
/// well under the recording at the strike, carries the ring — its own
/// stretched partials beating slowly against the recording's, which is what
/// a piano's three strings do to each other.
#[test]
fn the_strings_sing_on_under_the_recording() {
    let with = render(grand_piano(), 60, 100, 2.5);
    let muted = render(without(STRING), 60, 100, 2.5);
    let strike = level_db(&with, 0.0, 0.03) - level_db(&muted, 0.0, 0.03);
    let ring = level_db(&with, 1.8, 0.4) - level_db(&muted, 1.8, 0.4);
    assert!(
        strike.abs() < 0.6,
        "the strike is the recording's: {strike:+.2} dB"
    );
    assert!(
        ring > 0.7,
        "the string should be heard in the ring: {ring:+.2} dB at two seconds"
    );
    // Not a synth under a piano: still well under the recording.
    assert!(ring < 6.0, "{ring:+.2} dB is a second instrument");
}
