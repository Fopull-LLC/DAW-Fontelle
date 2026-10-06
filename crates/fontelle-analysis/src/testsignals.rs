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
