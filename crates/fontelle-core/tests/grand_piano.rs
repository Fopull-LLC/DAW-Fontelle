//! The Grand Piano a new project opens on — the claims that make it a piano.
//!
//! > *"the grand piano sound doesnt sound realistic at all right now"* — Ty,
//! > 2026-09-07; *"... use the osc sampling feature ... if you can find a
//! > grand piano one shot"* — 2026-09-16; *"the default piano is also
//! > extremely clicky sounding and not very realistic ... make the default
//! > grand piano preset for flopsynth just sound a lot more natural and
//! > professional and clean like a real grand piano"* — 2026-10-19.
//!
//! The row plays **the Salamander C5 itself**, in stereo: every key it was
//! recorded at, three strengths of each, the left microphone on one
//! oscillator and the right on another (`fontelle_core::factory_samples`).
//! The September rows put a body round a mono recording instead — a noise
//! hammer, a noise damper, a modelled string — and the October report heard
//! that body as clicks: the hammer's burst started at full level on the
//! note's first sample, a transient eighty decibels over the room before
//! it, and the damper's burst did the same on every release. The recording
//! already holds its own hammer, and its own ring when it is long enough to.
//! So what this file holds is that the piano is the recording, played
//! right: nothing synthesised beside it, every key near its own recording,
//! the strength choosing the recording and the touch the level, a held note
//! singing on, a release that is a damper, and nothing, anywhere, a click.

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

/// The row is the recording and nothing else: the left microphone's set on
/// one oscillator panned hard left, the right's on another panned hard
/// right, and every other layer silent with nothing to lift it.
#[test]
fn it_is_the_stereo_recording_and_nothing_else() {
    let patch = grand_piano();
    let set_on = |layer: usize| match &patch.layers[layer].source {
        Source::Synth(osc) => match osc.source {
            SynthSource::Sample(at) => patch.samples.get(usize::from(at)).and_then(|s| s.factory),
            _ => None,
        },
        _ => None,
    };
    assert_eq!(set_on(0), Some(FactorySampleSet::GrandLeft));
    assert_eq!(set_on(1), Some(FactorySampleSet::GrandRight));
    assert!(patch.layers[0].gain_db > SILENT_DB && patch.layers[1].gain_db > SILENT_DB);
    for layer in 2..patch.layers.len() {
        assert!(
            patch.layers[layer].gain_db <= SILENT_DB,
            "layer {layer} sounds"
        );
        assert!(
            !patch.mod_matrix.routes.iter().any(|route| matches!(
                route.destination,
                fontelle_core::ModDest::LayerGain(at) if usize::from(at) == layer
            )),
            "a route lifts layer {layer}"
        );
    }
    // A project file names the piano rather than carrying it.
    let data = patch.to_data(&Default::default()).unwrap();
    let text = serde_json::to_string(&data).unwrap();
    assert!(text.len() < 40_000, "{} bytes", text.len());
}

/// Every key plays a recording a semitone or less from its own pitch, and
/// what comes out is at the key's pitch.
#[test]
fn every_key_lands_on_a_recording_near_its_own_pitch() {
    let patch = grand_piano();
    for sample in &patch.samples {
        for key in 0..=127u8 {
            let zone = sample
                .zone_for_note(key, 100)
                .expect("a zone for every key");
            let away = (i16::from(zone.root_key) - i16::from(key)).abs();
            let stretch = if (21..=108).contains(&key) { 1 } else { 24 };
            assert!(
                away <= stretch,
                "{}: key {key} plays the recording of {}, {away} semitones off",
                sample.name,
                zone.root_key
            );
        }
    }
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

/// The strength chooses the recording — a soft strike is darker than a
/// hard one by more than a level could make it — and the touch sets the
/// level, rising all the way up with no step back where the recordings
/// change, over a real piano's thirty-odd decibels.
#[test]
fn velocity_chooses_the_recording_and_sets_the_level() {
    let patch = grand_piano();
    let soft = render(patch.clone(), 60, 30, 0.5);
    let hard = render(patch.clone(), 60, 120, 0.5);
    let (a, b) = (
        brightness(&soft, 60, 0.0, 0.3),
        brightness(&hard, 60, 0.0, 0.3),
    );
    assert!(
        b > a + 5.0,
        "a hard strike should be brighter than a soft one: {b:.1} against {a:.1} dB"
    );
    let velocities = [8u8, 30, 44, 45, 64, 80, 81, 100, 127];
    let levels: Vec<f32> = velocities
        .iter()
        .map(|v| level_db(&render(patch.clone(), 60, *v, 0.4), 0.0, 0.3))
        .collect();
    for pair in levels.windows(2) {
        assert!(
            pair[1] > pair[0] - 0.5,
            "the level should rise with velocity, not step back where the recordings change: \
             {velocities:?} → {levels:?}"
        );
    }
    let span = levels[levels.len() - 1] - levels[0];
    assert!(
        (28.0..=44.0).contains(&span),
        "a piano's pp to ff is thirty-odd decibels, not {span:.1}: {levels:?}"
    );
}

/// The bass rings long and the treble short — the recording's own decay.
#[test]
fn the_bass_rings_long_and_the_treble_rings_short() {
    let patch = grand_piano();
    let bass = t30(&render(patch.clone(), 36, 100, 6.0));
    let middle = t30(&render(patch.clone(), 60, 100, 4.5));
    let top = t30(&render(patch, 96, 100, 2.0));
    assert!(bass > 2.0, "C2 should ring: 30 dB in {bass:.2} s");
    assert!(
        (0.7..=4.0).contains(&middle),
        "middle C: 30 dB in {middle:.2} s"
    );
    assert!(top < 0.8, "C7 should be short: 30 dB in {top:.2} s");
    assert!(bass > middle && middle > top);
}

/// A held note sings on: middle C is still well up three seconds in, and
/// the bass five — not gone where a short recording was cut, as the
/// September cut's four seconds made it.
#[test]
fn a_held_note_sings_on() {
    let patch = grand_piano();
    for (key, at) in [(60u8, 3.0f32), (36, 4.5)] {
        let note = render(patch.clone(), key, 90, at + 0.5);
        let strike = level_db(&note, 0.0, 0.3);
        let later = level_db(&note, at, 0.4);
        assert!(
            later > strike - 40.0,
            "key {key}: {later:.1} dB at {at} s against {strike:.1} at the strike"
        );
    }
}

/// A note that outlives its recording fades; it does not stop.
#[test]
fn a_note_that_outlives_its_recording_fades_rather_than_stops() {
    let patch = grand_piano();
    for (key, seconds) in [(21u8, 7.0f32), (60, 5.5), (108, 2.5)] {
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
        assert!(
            end < -70.0,
            "key {key}: {end:.1} dB at the end of {seconds} s"
        );
    }
}

/// The sharpest transient anywhere in `stereo` (interleaved), as how far
/// its one-millisecond burst of second difference stands over the forty
/// milliseconds round it, in decibels. A piano's own hammer reads twenty to
/// thirty-five off the silence before a note; the September noise hammer
/// read eighty, its damper forty.
fn sharpest_transient(mono: &[f32]) -> (f32, f32) {
    let d2: Vec<f32> = (2..mono.len())
        .map(|n| mono[n] - 2.0 * mono[n - 1] + mono[n - 2])
        .collect();
    let w = 48;
    let e: Vec<f32> = d2
        .chunks(w)
        .map(|c| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let mut worst = (0.0f32, 0.0f32);
    for i in 0..e.len() {
        let lo = i.saturating_sub(20);
        let hi = (i + 20).min(e.len());
        let mut around: Vec<f32> = (lo..hi)
            .filter(|j| j.abs_diff(i) > 2)
            .map(|j| e[j])
            .collect();
        around.sort_by(f32::total_cmp);
        let median = around
            .get(around.len() / 2)
            .copied()
            .unwrap_or(0.0)
            .max(1e-7);
        let ratio = 20.0 * (e[i] / median).log10();
        if e[i] > 1e-4 && ratio > worst.1 {
            worst = (i as f32 * w as f32 / SR, ratio);
        }
    }
    worst
}

/// A key at a moment: seconds in, the key, its velocity — or 0 for a
/// release.
type Played = (f32, u8, u8);

/// Nothing clicks: not a strike, not a release, not a key struck again
/// while it still sounds, not a chord — Ty's *"extremely clicky"*.
#[test]
fn nothing_clicks() {
    let store = SampleStore::new();
    // (seconds, key, velocity or 0 for a release)
    let passages: [(&str, &[Played]); 4] = [
        (
            "staccato",
            &[
                (0.1, 60, 90),
                (0.25, 60, 0),
                (0.6, 64, 110),
                (0.7, 64, 0),
                (1.0, 67, 40),
                (1.2, 67, 0),
            ],
        ),
        (
            "repeats",
            &[
                (0.1, 60, 100),
                (0.3, 60, 0),
                (0.35, 60, 100),
                (0.6, 60, 100),
                (0.8, 60, 120),
                (1.2, 60, 0),
            ],
        ),
        (
            "chord",
            &[
                (0.1, 48, 80),
                (0.1, 60, 80),
                (0.1, 64, 80),
                (0.1, 67, 80),
                (1.5, 48, 0),
                (1.5, 60, 0),
                (1.5, 64, 0),
                (1.5, 67, 0),
            ],
        ),
        (
            "bass",
            &[(0.1, 33, 127), (1.0, 33, 0), (1.2, 21, 100), (2.2, 21, 0)],
        ),
    ];
    for (name, events) in passages {
        let mut sampler = Sampler::new(grand_piano());
        sampler.prepare(&PrepareContext {
            sample_rate: SR,
            max_block_size: 64,
        });
        let total = (SR * 2.6) as usize;
        let mut out = Vec::with_capacity(total);
        let mut at = 0usize;
        while at < total {
            for (t, key, velocity) in events {
                if (t * SR) as usize == at {
                    if *velocity > 0 {
                        sampler.note_on(*key, *velocity, 0);
                    } else {
                        sampler.note_off(*key, 0);
                    }
                }
            }
            let mut left = [0.0f32; 64];
            let mut right = [0.0f32; 64];
            sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
            out.extend(left.iter().zip(&right).map(|(l, r)| l + r));
            at += 64;
        }
        let (when, ratio) = sharpest_transient(&out);
        assert!(
            ratio < 45.0,
            "{name}: a transient {ratio:.0} dB over its surroundings at {when:.3} s is a click"
        );
    }
}

/// Stereo as the microphones heard it: the two sides are not one signal
/// twice, and not two that cancel; and the keyboard leans from the bass on
/// the left to the treble on the right.
#[test]
fn the_piano_is_heard_in_stereo() {
    let patch = grand_piano();
    let (left, right) = render_stereo(patch.clone(), 60, 100, 1.0);
    let dot: f32 = left.iter().zip(&right).map(|(l, r)| l * r).sum();
    let correlation = dot / (rms(&left) * rms(&right) * left.len() as f32).max(1e-12);
    assert!(
        (0.2..0.97).contains(&correlation),
        "the sides correlate at {correlation:.3}"
    );
    let balance = |key: u8| {
        let (left, right) = render_stereo(patch.clone(), key, 100, 0.5);
        db(rms(&right)) - db(rms(&left))
    };
    let (bass, treble) = (balance(33), balance(96));
    assert!(
        treble > bass + 1.0,
        "the treble should sit right of the bass: {bass:+.1} and {treble:+.1} dB"
    );
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
