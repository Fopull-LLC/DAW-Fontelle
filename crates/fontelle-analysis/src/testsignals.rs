//! Signals the analysis is tested and benchmarked on, synthesised here so no
//! audio file is ever committed. The reference posteriorgrams in
//! `tests/fixtures/` were made from exactly these samples (the
//! `write_fixtures` example writes them out for `make_reference.py`).
//!
//! Everything is computed in `f64` from closed forms, so the samples are the
//! same on every machine to well below what any assertion can see.

use std::f64::consts::TAU;

/// A note the signal was made with: what an ideal transcription returns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpectedNote {
    pub start: f64,
    pub end: f64,
    pub midi: u8,
}

/// A synthesised piece of audio and the notes in it.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub name: &'static str,
    pub sample_rate: u32,
    pub samples: Vec<f32>,
    pub notes: Vec<ExpectedNote>,
}

/// How a voice sounds.
#[derive(Debug, Clone, Copy)]
struct Voice {
    /// Partials above the fundamental, each `1 / k^rolloff` loud.
    partials: usize,
    rolloff: f64,
    gain: f64,
    /// Vibrato depth (cents, peak) and rate, starting `vibrato_delay` s in.
    vibrato_cents: f64,
    vibrato_hz: f64,
    vibrato_delay: f64,
}

fn midi_hz(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

/// Adds one note to `out`, with a 10 ms attack and a 30 ms release inside
/// `[start, end)`.
fn add_note(out: &mut [f64], sample_rate: u32, note: ExpectedNote, voice: Voice) {
    let sr = f64::from(sample_rate);
    let first = (note.start * sr).round() as usize;
    let last = ((note.end * sr).round() as usize).min(out.len());
    let base = midi_hz(f64::from(note.midi));
    let (attack, release) = (0.010, 0.030);
    let length = note.end - note.start;
    let mut phase = 0.0f64;
    for (i, sample) in out.iter_mut().enumerate().take(last).skip(first) {
        let t = (i - first) as f64 / sr;
        let vib = if t > voice.vibrato_delay {
            // Faded in over 100 ms, as a singer's is.
            let fade = ((t - voice.vibrato_delay) / 0.1).min(1.0);
            fade * voice.vibrato_cents * (TAU * voice.vibrato_hz * (t - voice.vibrato_delay)).sin()
        } else {
            0.0
        };
        let f0 = base * 2f64.powf(vib / 1200.0);
        phase += f0 / sr;
        let env = if t < attack {
            0.5 - 0.5 * (std::f64::consts::PI * t / attack).cos()
        } else if t > length - release {
            let r = ((length - t) / release).max(0.0);
            0.5 - 0.5 * (std::f64::consts::PI * r).cos()
        } else {
            1.0
        };
        let mut s = 0.0;
        for k in 1..=voice.partials {
            if f0 * k as f64 >= sr * 0.45 {
                break;
            }
            s += (TAU * phase * k as f64).sin() / (k as f64).powf(voice.rolloff);
        }
        *sample += voice.gain * env * s;
    }
}

fn render(
    name: &'static str,
    sample_rate: u32,
    seconds: f64,
    parts: &[(ExpectedNote, Voice)],
) -> Fixture {
    let mut out = vec![0.0f64; (seconds * f64::from(sample_rate)).round() as usize];
    for &(note, voice) in parts {
        add_note(&mut out, sample_rate, note, voice);
    }
    let mut notes: Vec<ExpectedNote> = parts.iter().map(|(n, _)| *n).collect();
    notes.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.midi.cmp(&b.midi)));
    Fixture {
        name,
        sample_rate,
        samples: out.into_iter().map(|s| s as f32).collect(),
        notes,
    }
}

const PLUCKED: Voice = Voice {
    partials: 6,
    rolloff: 2.0,
    gain: 0.22,
    vibrato_cents: 0.0,
    vibrato_hz: 0.0,
    vibrato_delay: 0.0,
};

const SUNG: Voice = Voice {
    partials: 12,
    rolloff: 1.0,
    gain: 0.18,
    vibrato_cents: 35.0,
    vibrato_hz: 5.5,
    vibrato_delay: 0.15,
};

fn n(start: f64, end: f64, midi: u8) -> ExpectedNote {
    ExpectedNote { start, end, midi }
}

/// C4, E4 and G4 together from 0.5 s to 3.0 s, in 4 s: a soft saw-ish tone.
pub fn c_major_triad(sample_rate: u32) -> Fixture {
    let parts = [60, 64, 67].map(|m| (n(0.5, 3.0, m), PLUCKED));
    render("triad", sample_rate, 4.0, &parts)
}

/// A sung-like line (twelve partials, 5.5 Hz vibrato of ±35 cents fading in
/// after 150 ms) of six notes with short gaps, in 5 s.
pub fn vibrato_melody(sample_rate: u32) -> Fixture {
    let parts = [
        n(0.30, 0.80, 60),
        n(0.85, 1.35, 62),
        n(1.40, 1.90, 64),
        n(1.95, 2.70, 67),
        n(2.75, 3.25, 65),
        n(3.30, 4.40, 64),
    ]
    .map(|note| (note, SUNG));
    render("vibrato", sample_rate, 5.0, &parts)
}

/// Two held chords (C major, then F major, below middle C) under a sung
/// melody an octave and more above, in 5 s.
pub fn melody_and_chords(sample_rate: u32) -> Fixture {
    let chord = Voice {
        gain: 0.12,
        ..PLUCKED
    };
    let mut parts: Vec<(ExpectedNote, Voice)> = Vec::new();
    for m in [48, 52, 55] {
        parts.push((n(0.30, 2.40, m), chord));
    }
    for m in [53, 57, 60] {
        parts.push((n(2.50, 4.60, m), chord));
    }
    for note in [
        n(0.30, 0.80, 72),
        n(0.90, 1.40, 76),
        n(1.50, 2.30, 79),
        n(2.50, 3.00, 77),
        n(3.10, 3.60, 76),
        n(3.70, 4.60, 72),
    ] {
        parts.push((note, SUNG));
    }
    render("mix", sample_rate, 5.0, &parts)
}

/// The three fixtures with reference outputs, at 22 050 Hz.
pub fn reference_fixtures() -> Vec<Fixture> {
    vec![
        c_major_triad(22_050),
        vibrato_melody(22_050),
        melody_and_chords(22_050),
    ]
}

/// A voice whose pitch is any curve: twelve partials falling as `1/k`,
/// 10 ms in and 30 ms out, `cents(t)` (MIDI cents, 6900 = A4) for `t` in
/// seconds from `start`, sounding until `end`, in `seconds` of audio.
pub fn sung_curve(
    sample_rate: u32,
    seconds: f64,
    start: f64,
    end: f64,
    cents: impl Fn(f64) -> f64,
) -> Vec<f32> {
    let sr = f64::from(sample_rate);
    let mut out = vec![0.0f32; (seconds * sr).round() as usize];
    let first = (start * sr).round() as usize;
    let last = ((end * sr).round() as usize).min(out.len());
    let length = end - start;
    let mut phase = 0.0f64;
    for (i, sample) in out.iter_mut().enumerate().take(last).skip(first) {
        let t = (i - first) as f64 / sr;
        let f0 = 440.0 * 2f64.powf((cents(t) - 6900.0) / 1200.0);
        phase += f0 / sr;
        let env = if t < 0.010 {
            0.5 - 0.5 * (std::f64::consts::PI * t / 0.010).cos()
        } else if t > length - 0.030 {
            let r = ((length - t) / 0.030).max(0.0);
            0.5 - 0.5 * (std::f64::consts::PI * r).cos()
        } else {
            1.0
        };
        let mut s = 0.0;
        for k in 1..=12 {
            if f0 * f64::from(k) >= sr * 0.45 {
                break;
            }
            s += (TAU * phase * f64::from(k)).sin() / f64::from(k);
        }
        *sample = (0.2 * env * s) as f32;
    }
    out
}

/// The pitch of [`sung_glide`] `t` seconds into it: G3 up a fifth to D4,
/// exponentially, over 1.5 s.
pub fn sung_glide_cents(t: f64) -> f64 {
    5500.0 + 700.0 * (t / 1.5).clamp(0.0, 1.0)
}

/// A sung glissando, [`sung_glide_cents`], from 0.3 s to 1.8 s of 2.1 s.
pub fn sung_glide(sample_rate: u32) -> Vec<f32> {
    sung_curve(sample_rate, 2.1, 0.3, 1.8, sung_glide_cents)
}

/// The slow part of [`drifting_vibrato_note`] `t` seconds in: A3, rising
/// 60 cents across its 2 s.
pub fn drift_cents(t: f64) -> f64 {
    5700.0 + 30.0 * t
}

/// The vibrato of [`drifting_vibrato_note`]: ±30 cents at 5.5 Hz.
pub fn vibrato_cents(t: f64) -> f64 {
    30.0 * (TAU * 5.5 * t).sin()
}

/// One sung A3, drifting sharp ([`drift_cents`]) with a steady vibrato
/// ([`vibrato_cents`]), from 0.25 s to 2.25 s of 2.5 s.
pub fn drifting_vibrato_note(sample_rate: u32) -> Vec<f32> {
    sung_curve(sample_rate, 2.5, 0.25, 2.25, |t| {
        drift_cents(t) + vibrato_cents(t)
    })
}

/// White noise, uniform, deterministic (a xorshift), at `level` peak.
pub fn noise(sample_rate: u32, seconds: f64, level: f32) -> Vec<f32> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    (0..(seconds * f64::from(sample_rate)).round() as usize)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * level
        })
        .collect()
}

/// Note material in a key, for key detection: `n` notes, mostly from the
/// scale (tonic, fifth and third favoured, as tunes do) with `chromatic`
/// of them any of the twelve at random, a bass note every fourth,
/// durations 0.1 to 1 s. Deterministic in `seed`.
pub fn key_material(
    seed: u64,
    root: u8,
    minor: bool,
    chromatic: f32,
    n: usize,
) -> Vec<crate::key::PitchWeight> {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let steps: [u8; 7] = if minor {
        [0, 2, 3, 5, 7, 8, 10]
    } else {
        [0, 2, 4, 5, 7, 9, 11]
    };
    let favour = [3.0, 1.0, 2.0, 1.2, 2.5, 1.0, 1.0];
    let total: f64 = favour.iter().sum();
    (0..n)
        .map(|i| {
            let pc = if next() < f64::from(chromatic) {
                (next() * 12.0) as u8 % 12
            } else {
                let mut pick = next() * total;
                let mut degree = 0;
                while degree < 6 && pick >= favour[degree] {
                    pick -= favour[degree];
                    degree += 1;
                }
                steps[degree]
            };
            let bass = i % 4 == 0;
            let octave = if bass { 3 } else { 5 };
            crate::key::PitchWeight {
                midi: 12 * octave + (root + pc) % 12,
                seconds: (0.1 + 0.9 * next()) as f32,
                weight: (0.5 + 0.5 * next()) as f32,
            }
        })
        .collect()
}

/// A pure sine melody, C major up and down (C4 to C5), 0.4 s a note with
/// 0.1 s between, in 4.5 s.
pub fn sine_melody(sample_rate: u32) -> Fixture {
    let sine = Voice {
        partials: 1,
        rolloff: 1.0,
        gain: 0.4,
        vibrato_cents: 0.0,
        vibrato_hz: 0.0,
        vibrato_delay: 0.0,
    };
    let keys = [60u8, 62, 64, 65, 67, 69, 71, 72, 71];
    let parts: Vec<(ExpectedNote, Voice)> = keys
        .iter()
        .enumerate()
        .map(|(i, &m)| (n(0.1 + 0.5 * i as f64, 0.5 + 0.5 * i as f64, m), sine))
        .collect();
    render("sine-melody", sample_rate, 4.6, &parts)
}

/// Two bars of a rock beat at 100 BPM, synthesised: a kick (a sine falling
/// from 120 to 45 Hz), a snare (noise and a 190 Hz knock) and closed hats
/// (high noise, 30 ms), in 4.8 s. No notes to find.
pub fn drum_loop(sample_rate: u32) -> Vec<f32> {
    let sr = f64::from(sample_rate);
    let mut out = vec![0.0f64; (4.8 * sr) as usize];
    let noise = noise(sample_rate, 4.8, 1.0);
    let eighth = 60.0 / 100.0 / 2.0;
    for step in 0..16 {
        let at = (step as f64 * eighth * sr) as usize;
        // Hats on every eighth.
        let mut hp = 0.0f64;
        let mut last = 0.0f64;
        for k in 0..(0.03 * sr) as usize {
            let i = at + k;
            if i >= out.len() {
                break;
            }
            let x = f64::from(noise[i]);
            hp = 0.6 * (hp + x - last);
            last = x;
            out[i] += 0.15 * hp * (-(k as f64) / (0.008 * sr)).exp();
        }
        if step % 4 == 0 {
            let mut phase = 0.0f64;
            for k in 0..(0.35 * sr) as usize {
                let i = at + k;
                if i >= out.len() {
                    break;
                }
                let t = k as f64 / sr;
                phase += (45.0 + 75.0 * (-t / 0.04).exp()) / sr;
                out[i] += 0.6 * (TAU * phase).sin() * (-t / 0.12).exp();
            }
        }
        if step % 4 == 2 {
            for k in 0..(0.2 * sr) as usize {
                let i = at + k;
                if i >= out.len() {
                    break;
                }
                let t = k as f64 / sr;
                out[i] += 0.35 * f64::from(noise[(i * 7) % noise.len()]) * (-t / 0.06).exp()
                    + 0.25 * (TAU * 190.0 * t).sin() * (-t / 0.03).exp();
            }
        }
    }
    out.into_iter().map(|s| s as f32).collect()
}
