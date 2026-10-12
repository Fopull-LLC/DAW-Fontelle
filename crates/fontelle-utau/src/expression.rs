//! What a singer does to the written pitch without being asked.
//!
//! The notes, their glides and any vibrato the writer set make the line
//! [`note_pitch`] draws. A singer does more: comes into a phrase from a
//! little below, overshoots a leap and settles, holds a long note steady
//! before a vibrato grows on it, lets the pitch fall a little as a phrase
//! ends, and never holds it perfectly still. UTAU's "Mode2" default curves
//! and OpenUtau's default vibrato are the same idea; the shapes here are
//! our own, checked against renders of Kasane Teto's English bank.
//!
//! Ty: "i still think it could be more natural sounding."
//!
//! Everything is added in semitones on top of the written line and scaled
//! by `amount`, so 0 is the written pitch exactly. A vibrato or glide the
//! writer set is kept as written: no second vibrato on it, no overshoot
//! added to it.

use fontelle_sing::{CURVE_STEP, Curve, PhraseNote, note_pitch};

use crate::phonemize::GAP;

/// The scoop into a phrase: this far below, rising over `SCOOP_SECS`.
const SCOOP: f64 = 0.35;
const SCOOP_SECS: f64 = 0.10;
/// The overshoot after a leap: a share of the interval, at most
/// `OVERSHOOT_MAX`, peaking `OVERSHOOT_SECS` after the glide.
const OVERSHOOT_SHARE: f64 = 0.10;
const OVERSHOOT_MAX: f64 = 0.4;
const OVERSHOOT_SECS: f64 = 0.04;
/// The small dip the other way before a leap.
const PREPARE_SHARE: f64 = 0.03;
const PREPARE_MAX: f64 = 0.12;
const PREPARE_SECS: f64 = 0.03;
/// The default glide, as `fontelle_sing` draws it.
const GLIDE_LEN: f64 = 0.08;
const GLIDE_POSITION: f64 = 0.3;
/// The vibrato on notes at least this long.
const VIBRATO_MIN_NOTE: f64 = 0.5;
const VIBRATO_RATE: f64 = 5.6;
/// Peak deviation, semitones: from `VIBRATO_DEPTH` on the shortest notes
/// to that plus `VIBRATO_GROW` on notes 2 s long.
const VIBRATO_DEPTH: f64 = 0.18;
const VIBRATO_GROW: f64 = 0.12;
const VIBRATO_FADE_IN: f64 = 0.3;
/// The fall at the end of a run before a rest.
const FALL: f64 = 0.3;
const FALL_SECS: f64 = 0.12;
/// Drift: slow, small, never still.
const DRIFT: [(f64, f64); 3] = [(0.02, 0.8), (0.015, 2.1), (0.01, 3.7)];

/// The line sung for `notes` from `start` to `end`, MIDI every
/// [`CURVE_STEP`]: [`note_pitch`] with `amount` (0..1) of the singer's
/// expression on it.
pub fn pitch_line(notes: &[PhraseNote], start: f64, end: f64, amount: f64) -> Curve {
    let mut line = note_pitch(notes, start, end);
    if amount <= 0.0 || notes.is_empty() {
        return line;
    }
    let seed = notes.iter().fold(0x9e37_79b9_7f4a_7c15u64, |h, n| {
        (h ^ u64::from(n.pitch.to_bits())).wrapping_mul(0x1000_0000_01b3)
    });
    let phases: Vec<f64> = (0..DRIFT.len())
        .map(|k| ((seed >> (k * 16)) & 0xffff) as f64 / 65536.0 * std::f64::consts::TAU)
        .collect();
    for (j, v) in line.values.iter_mut().enumerate() {
        let t = start + j as f64 * CURVE_STEP;
        let mut off = 0.0;
        for i in 0..notes.len() {
            off += note_offset(notes, i, t);
        }
        for ((depth, rate), phase) in DRIFT.iter().zip(&phases) {
            off += depth * (std::f64::consts::TAU * rate * t + phase).sin();
        }
        *v += (amount * off) as f32;
    }
    line
}

/// Note `i`'s part of the expression at `t`, semitones.
fn note_offset(notes: &[PhraseNote], i: usize, t: f64) -> f64 {
    let n = &notes[i];
    // Far from this note, nothing.
    if t < n.start - 0.2 || t > n.end() + 0.25 {
        return 0.0;
    }
    let run_start = i == 0 || n.start - notes[i - 1].end() > GAP;
    let run_end = i + 1 == notes.len() || notes[i + 1].start - n.end() > GAP;
    let mut off = 0.0;
    // Into a phrase from a little below.
    let prev_end = if i > 0 {
        notes[i - 1].end()
    } else {
        f64::NEG_INFINITY
    };
    let next_start = notes.get(i + 1).map_or(f64::INFINITY, |m| m.start);
    if run_start && t >= (n.start - SCOOP_SECS).max(prev_end) && t < n.start + SCOOP_SECS {
        let x = ((t - n.start) / SCOOP_SECS).max(0.0);
        off -= SCOOP * (1.0 - x) * (1.0 - x);
    }
    // A leap: a small dip the other way, then past the note and back.
    if !run_start && n.portamento.is_none() {
        let d = f64::from(n.pitch - notes[i - 1].pitch);
        if d.abs() >= 0.5 {
            let sign = d.signum();
            let begin = n.start - GLIDE_LEN * (1.0 - GLIDE_POSITION);
            let arrive = begin + GLIDE_LEN;
            if t > arrive && t < n.end() {
                let u = (t - arrive) / OVERSHOOT_SECS;
                off += sign * (OVERSHOOT_SHARE * d.abs()).min(OVERSHOOT_MAX) * u * (1.0 - u).exp();
            }
            if t < begin && t > notes[i - 1].start {
                let v = (begin - t) / PREPARE_SECS;
                off -= sign * (PREPARE_SHARE * d.abs()).min(PREPARE_MAX) * v * (1.0 - v).exp();
            }
        }
    }
    // A vibrato that grows late in a long note.
    if n.vibrato.is_none() && n.duration >= VIBRATO_MIN_NOTE && t < n.end() {
        let from = n.start + (0.3 * n.duration).max(0.25);
        if t > from {
            let grow = ((n.duration - VIBRATO_MIN_NOTE) / 1.5).clamp(0.0, 1.0);
            let depth = VIBRATO_DEPTH + VIBRATO_GROW * grow;
            let x = ((t - from) / VIBRATO_FADE_IN).min(1.0);
            let mut env = x * x * (3.0 - 2.0 * x);
            // Into the next note it dies away; before a rest it eases off.
            let left = n.end() - t;
            if run_end {
                if left < 0.15 {
                    env *= 0.5 + 0.5 * left / 0.15;
                }
            } else if left < 0.1 {
                env *= left / 0.1;
            }
            // A slightly wandering rate, as a voice has.
            let s = t - from;
            let phase = std::f64::consts::TAU
                * (VIBRATO_RATE * s
                    + 0.03 * VIBRATO_RATE / 0.7 * (1.0 - (0.7 * s).cos()) / std::f64::consts::TAU);
            off += depth * env * phase.sin();
        }
    }
    // The end of a run falls a little, on into the release.
    if run_end && t < next_start {
        let x = (t - (n.end() - FALL_SECS)) / FALL_SECS;
        if x > 0.0 {
            let x = x.min(1.4);
            off -= FALL * x * x;
        }
    }
    off
}
