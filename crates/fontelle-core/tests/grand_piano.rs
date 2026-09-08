//! The Grand Piano a new project opens on — the claims that make it a piano
//! rather than a synth with a long decay.
//!
//! > *"the grand piano sound doesnt sound realistic at all right now"* — Ty,
//! > 2026-09-07
//!
//! A sampled grand is the soundfont player's job and this row does not
//! pretend otherwise (`presets.rs` says so over the row). What it can be held
//! to is the *physics* a listener hears: a bass string rings for tens of
//! seconds and a treble one for one, a harder strike is a brighter one, and a
//! held key is a note dying rather than a plateau. The first of those was
//! missing entirely — one decay for the whole keyboard — and is the tell that
//! made it a synth.

use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{NoteTrigger, Patch, PrepareContext, SampleStore, Sampler};

const SR: f32 = 48_000.0;

fn grand_piano() -> Patch {
    let row = FACTORY
        .iter()
        .find(|row| row.name == "Grand Piano")
        .expect("the bank has a Grand Piano");
    (row.build)()
}

/// One held note, `seconds` long.
fn render(patch: Patch, key: u8, velocity: u8, seconds: f32) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, velocity));
    let total = (SR * seconds) as usize;
    let mut out = Vec::with_capacity(total);
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
    out
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
    let mut low = 0.0f32;
    let mut high = 0.0f32;
    for h in 1..=16 {
        let hz = f0 * h as f32;
        if hz > 16_000.0 {
            break;
        }
        let power = power_at(hz);
        if h <= 3 { low += power } else { high += power }
    }
    10.0 * (high / low.max(1e-18)).max(1e-12).log10()
}
/// Every partial's level relative to the fundamental's, in decibels, measured
/// over `seconds` from `from`.
///
/// The fundamental as the reference rather than full scale, because what is
/// being asked about is the *balance* between a note's partials, which is
/// what an ear hears as the difference between a piano and a clavinet — and
/// it has to mean the same thing at the strike and three seconds later, when
/// the note is thirty decibels quieter.
fn partials(samples: &[f32], key: u8, from: f32, seconds: f32) -> Vec<f32> {
    let f0 = 440.0 * 2f32.powf((key as f32 - 69.0) / 12.0);
    let start = ((SR * from) as usize).min(samples.len());
    let samples = &samples[start..samples.len().min(start + (SR * seconds) as usize)];
    let n = samples.len() as f32;
    let mut levels = Vec::new();
    for h in 1..=8 {
        let hz = f0 * h as f32;
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (i, sample) in samples.iter().enumerate() {
            let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
            let phase = std::f32::consts::TAU * hz * i as f32 / SR;
            re += sample * window * phase.cos();
            im -= sample * window * phase.sin();
        }
        levels.push(20.0 * ((re * re + im * im).sqrt() / n).max(1e-9).log10());
    }
    let fundamental = levels[0];
    levels.iter().map(|level| level - fundamental).collect()
}

/// How fast the note is falling, in decibels per second, between two moments.
fn fall_db_per_s(samples: &[f32], from: f32, to: f32) -> f32 {
    let level = |at: f32| {
        let start = ((SR * at) as usize).min(samples.len().saturating_sub(1));
        let end = samples.len().min(start + (SR * 0.1) as usize);
        rms(&samples[start..end]).max(1e-9)
    };
    let (a, b) = (level(from), level(to));
    20.0 * (a / b).log10() / (to - from)
}

/// A piano is not hollow: its fundamental is the strongest thing in a struck
/// note, and it stays that way.
///
/// This is the one that named the fault. Measured, before it was fixed: the
/// second partial stood **3.8 dB above** the fundamental at the strike, which
/// is a nasal, hollow spectrum — a clavinet's, and Ty heard it as one.
#[test]
fn the_fundamental_is_the_strongest_partial() {
    for velocity in [40u8, 100, 127] {
        let out = render(grand_piano(), 60, velocity, 2.0);
        let strike = partials(&out, 60, 0.0, 0.15);
        for (index, level) in strike.iter().enumerate().skip(1) {
            assert!(
                *level < 1.0,
                "at velocity {velocity}, partial {} is {level:.1} dB against the \
                 fundamental — a piano is not hollow",
                index + 1
            );
        }
    }
}

/// And no partial has a hole blown in it on the way down.
///
/// The octave layer used to sit on the string's own second partial, four
/// cents sharp, and the two beat: measured, the second partial was **30 dB
/// under** the fundamental a second in while the third and fourth sat at
/// -21, which is a notch no string has and an ear hears as a wobble.
#[test]
fn the_partials_fall_together_rather_than_one_dropping_out() {
    let out = render(grand_piano(), 60, 100, 4.0);
    for at in [0.5f32, 1.0, 2.0] {
        let levels = partials(&out, 60, at, 0.3);
        // A string's partials roll *off*: each one may be as far under its
        // predecessor as it likes, but it may not sit below the one above
        // it. That asymmetry is the difference between a spectrum and a
        // notch, and it is what a beat against a second oscillator makes.
        for index in 1..5 {
            assert!(
                levels[index] > levels[index + 1] - 6.0,
                "{at} s in, partial {} is {:.1} dB while partial {} above it is \
                 {:.1} — a hole, not a roll-off: {levels:.1?}",
                index + 1,
                levels[index],
                index + 2,
                levels[index + 1]
            );
        }
    }
}

/// A piano's decay has two slopes: the prompt sound goes fast and the
/// aftersound goes slowly. A single straight line down is what an electric
/// piano has, and it is the other half of why this sounded like one.
#[test]
fn the_note_has_a_prompt_sound_over_a_long_aftersound() {
    let out = render(grand_piano(), 60, 100, 12.0);
    let prompt = fall_db_per_s(&out, 0.05, 0.5);
    let after = fall_db_per_s(&out, 2.0, 6.0);
    assert!(
        prompt > after * 2.5,
        "the strike should die faster than the ring: {prompt:.1} dB/s then {after:.1} dB/s"
    );
    assert!(
        after < 4.0,
        "the aftersound should be slow: {after:.1} dB/s takes middle C out in seconds"
    );
}

#[test]
fn the_bass_rings_long_and_the_treble_rings_short() {
    // C2 and C7, both held. A real grand's C2 takes well over ten seconds to
    // fade and its C7 is gone in a couple; the ratio is what the ear keys
    // on, and a synth piano with one decay time for the whole keyboard is
    // wrong at both ends at once.
    let low = t30(&render(grand_piano(), 36, 100, 8.0));
    let high = t30(&render(grand_piano(), 96, 100, 8.0));
    assert!(
        low > 3.0,
        "C2 fell 30 dB in {low:.2} s, which is a harpsichord"
    );
    assert!(
        high < 1.5,
        "C7 took {high:.2} s to fall 30 dB, which is an organ"
    );
    assert!(
        low > high * 3.0,
        "the decay should follow the key: C2 {low:.2} s, C7 {high:.2} s"
    );

    // And middle C, which is the note anybody tries first. A grand's C4 is
    // still audible ten seconds after it is struck; at under two it is an
    // electric piano, which is what this measured when Ty said it sounded
    // like a clavinet.
    let middle = t30(&render(grand_piano(), 60, 100, 20.0));
    assert!(
        middle > 5.0,
        "middle C fell 30 dB in {middle:.2} s, which is not a grand"
    );
}

#[test]
fn a_harder_strike_is_a_brighter_one() {
    // The same key at two velocities. A piano's touch is its whole
    // vocabulary and almost none of it is loudness: a hard blow shortens the
    // hammer's contact and throws energy into modes a soft one never
    // reaches, which is worth well over ten decibels of tilt between a
    // pianissimo and a fortissimo on a real instrument.
    let soft = brightness(&render(grand_piano(), 60, 40, 0.5), 60, 0.0, 0.15);
    let hard = brightness(&render(grand_piano(), 60, 120, 0.5), 60, 0.0, 0.15);
    assert!(
        hard - soft > 8.0,
        "velocity should open the sound: soft {soft:.1} dB, hard {hard:.1} dB of tilt"
    );
}

#[test]
fn the_shine_goes_before_the_note_does() {
    // Middle C, held. A string's upper modes are damped first, so its
    // spectrum darkens as it rings while the fundamental is still going
    // strong: the shine is gone a second in and the note is not. The plain
    // decaying oscillator every synth piano is built from cannot do that at
    // all — one envelope takes the whole spectrum down together.
    let out = render(grand_piano(), 60, 100, 4.0);
    let at_strike = brightness(&out, 60, 0.0, 0.15);
    let later = brightness(&out, 60, 1.0, 0.3);
    assert!(
        at_strike - later > 8.0,
        "the strike should be brighter than the ring: {at_strike:.1} dB then {later:.1} dB"
    );
    assert!(
        t30(&out) > 1.0,
        "middle C should still be ringing a second in"
    );
}

/// A released note lets its voice go, however long the string it was on.
///
/// The bass now decays over *minutes* — a hundred-decibel fall at the bottom
/// of the keyboard is a quarter of an hour, which is the right shape for a
/// held string and the wrong one for a voice pool. What saves it is that a
/// note-off starts the release stage and the release is a tenth of a second:
/// the damper, not the string. This is the assertion that says the two are
/// independent, because the alternative is a piano part that silently runs
/// out of polyphony halfway through a bar.
#[test]
fn a_released_bass_note_frees_its_voice() {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(grand_piano());
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
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
    // Half a second is five times the damper's own hundred milliseconds.
    render_for(&mut sampler, 0.5);
    assert_eq!(
        sampler.active_voices(),
        0,
        "a released note must give its voice back rather than ring on for the \
         minutes its decay would otherwise take"
    );
}
