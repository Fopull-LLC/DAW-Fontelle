//! Chord names over time (plan §2.6): `Am  F  C  G` for the chord lane.
//!
//! Each step's pitch-class weight (how long each note sounds inside it,
//! times its weight) is compared with every chord template at every root by
//! cosine similarity, and the lowest sounding note's pitch class lends its
//! chord a bonus: `A C E` over an F in the bass is an Fmaj7, over an A an
//! Am. Equal neighbours merge into one span.

use crate::transcribe::NoteEvent;

/// A note as chord reading weighs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimedPitch {
    pub start: f64,
    pub end: f64,
    pub midi: u8,
    /// Amplitude times confidence, 0..1.
    pub weight: f32,
}

impl From<&NoteEvent> for TimedPitch {
    fn from(n: &NoteEvent) -> Self {
        Self {
            start: n.start,
            end: n.end,
            midi: n.midi,
            weight: n.amplitude,
        }
    }
}

/// The chord qualities read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Quality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Sus2,
    Sus4,
    Dominant7,
    Major7,
    Minor7,
    Power,
}

/// A chord: a root pitch class (0 = C) and a quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Chord {
    pub root: u8,
    pub quality: Quality,
}

impl Chord {
    /// `"Am"`, `"F#7"`, `"Csus4"`, `"G5"`.
    pub fn label(&self) -> String {
        let root = NAMES[usize::from(self.root % 12)];
        let suffix = match self.quality {
            Quality::Major => "",
            Quality::Minor => "m",
            Quality::Diminished => "dim",
            Quality::Augmented => "aug",
            Quality::Sus2 => "sus2",
            Quality::Sus4 => "sus4",
            Quality::Dominant7 => "7",
            Quality::Major7 => "maj7",
            Quality::Minor7 => "m7",
            Quality::Power => "5",
        };
        format!("{root}{suffix}")
    }

    /// Its pitch classes, one bit each, bit 0 = C.
    pub fn mask(&self) -> u16 {
        intervals(self.quality)
            .iter()
            .fold(0, |m, i| m | 1 << ((self.root + i) % 12))
    }
}

/// One stretch of the chord lane.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChordSpan {
    pub start: f64,
    pub end: f64,
    /// `None` where nothing (or too little) sounds.
    pub chord: Option<Chord>,
    /// How well the chord's template matches what sounds, 0..1.
    pub confidence: f32,
}

/// Chord names use sharps, as the chord lane prints them.
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

const QUALITIES: [Quality; 10] = [
    Quality::Major,
    Quality::Minor,
    Quality::Diminished,
    Quality::Augmented,
    Quality::Sus2,
    Quality::Sus4,
    Quality::Dominant7,
    Quality::Major7,
    Quality::Minor7,
    Quality::Power,
];

fn intervals(quality: Quality) -> &'static [u8] {
    match quality {
        Quality::Major => &[0, 4, 7],
        Quality::Minor => &[0, 3, 7],
        Quality::Diminished => &[0, 3, 6],
        Quality::Augmented => &[0, 4, 8],
        Quality::Sus2 => &[0, 2, 7],
        Quality::Sus4 => &[0, 5, 7],
        Quality::Dominant7 => &[0, 4, 7, 10],
        Quality::Major7 => &[0, 4, 7, 11],
        Quality::Minor7 => &[0, 3, 7, 10],
        Quality::Power => &[0, 7],
    }
}

/// What a chord rooted on the bass note gains.
const BASS_BONUS: f32 = 0.1;
/// Under this much sounding weight per second of step, a step has no chord.
const MIN_WEIGHT_PER_SECOND: f64 = 0.1;

/// Chords over `[0, length)` in steps of `step` seconds (a beat, or 0.5 s
/// with no tempo), equal neighbours merged.
pub fn detect_chords(notes: &[TimedPitch], length: f64, step: f64) -> Vec<ChordSpan> {
    let step = step.max(1e-3);
    let steps = (length / step).ceil().max(0.0) as usize;
    let mut spans: Vec<ChordSpan> = Vec::new();
    for k in 0..steps {
        let (a, b) = (k as f64 * step, ((k + 1) as f64 * step).min(length));
        let (chord, confidence) = read_step(notes, a, b);
        match spans.last_mut() {
            Some(last) if last.chord == chord => {
                last.end = b;
                // A merged span is as sure as its average step.
                let n = ((last.end - last.start) / step).round().max(1.0) as f32;
                last.confidence += (confidence - last.confidence) / n;
            }
            _ => spans.push(ChordSpan {
                start: a,
                end: b,
                chord,
                confidence,
            }),
        }
    }
    spans
}

fn read_step(notes: &[TimedPitch], a: f64, b: f64) -> (Option<Chord>, f32) {
    let mut chroma = [0.0f32; 12];
    let mut bass: Option<(u8, f64)> = None;
    let mut total = 0.0f64;
    for n in notes {
        let overlap = n.end.min(b) - n.start.max(a);
        if overlap <= 0.0 {
            continue;
        }
        let w = overlap * f64::from(n.weight.max(0.0));
        chroma[usize::from(n.midi % 12)] += w as f32;
        total += w;
        // The bass: the lowest note sounding through at least a third of it.
        if overlap >= (b - a) / 3.0 && bass.is_none_or(|(m, _)| n.midi < m) {
            bass = Some((n.midi, overlap));
        }
    }
    if total < MIN_WEIGHT_PER_SECOND * (b - a) {
        return (None, 0.0);
    }
    let norm = chroma.iter().map(|c| c * c).sum::<f32>().sqrt();
    let mut best: Option<(Chord, f32, f32)> = None;
    for root in 0..12u8 {
        for quality in QUALITIES {
            let tones = intervals(quality);
            let dot: f32 = tones
                .iter()
                .map(|i| chroma[usize::from((root + i) % 12)])
                .sum();
            let cosine = dot / (norm * (tones.len() as f32).sqrt());
            let bonus = if bass.is_some_and(|(m, _)| m % 12 == root) {
                BASS_BONUS
            } else {
                0.0
            };
            let score = cosine + bonus;
            if best.is_none_or(|(_, s, _)| score > s) {
                best = Some((Chord { root, quality }, score, cosine));
            }
        }
    }
    match best {
        Some((chord, _, cosine)) => (Some(chord), cosine.clamp(0.0, 1.0)),
        None => (None, 0.0),
    }
}

/// The share of a bar's weight a pitch class needs to count as sounded.
const MELODY_CLASS_SHARE: f32 = 0.1;
/// The share of a bar's weight a chord's tones must carry together for a
/// single line to imply it.
const MELODY_CHORD_SHARE: f32 = 0.6;

/// Chords implied by **one voice** (Melody mode's chord lane), a span of
/// `bar` seconds at a time — a bar of the song's tempo, or two seconds
/// without one — equal neighbours merged.
///
/// A sung line holds one note at a time, so [`detect_chords`]' half-second
/// steps read each note as a chord of its own: a held E is "E5", a step
/// from D to E "Dsus2". Over a bar, a line *implies* a chord only when it
/// spells one: at least three pitch classes sounded, every tone of the
/// chord among them, and the chord's tones most of what was sung. No power
/// chords (two notes are an interval, not a chord). A bar that spells none
/// says nothing — better a blank than a wrong name.
pub fn melody_chords(notes: &[TimedPitch], length: f64, bar: f64) -> Vec<ChordSpan> {
    let bar = bar.max(0.25);
    let steps = (length / bar).ceil().max(0.0) as usize;
    let mut spans: Vec<ChordSpan> = Vec::new();
    for k in 0..steps {
        let (a, b) = (k as f64 * bar, ((k + 1) as f64 * bar).min(length));
        let (chord, confidence) = read_line(notes, a, b);
        match spans.last_mut() {
            Some(last) if last.chord == chord => {
                last.end = b;
                last.confidence = last.confidence.min(confidence);
            }
            _ => spans.push(ChordSpan {
                start: a,
                end: b,
                chord,
                confidence,
            }),
        }
    }
    spans
}

fn read_line(notes: &[TimedPitch], a: f64, b: f64) -> (Option<Chord>, f32) {
    let mut chroma = [0.0f32; 12];
    for n in notes {
        let overlap = n.end.min(b) - n.start.max(a);
        if overlap > 0.0 {
            chroma[usize::from(n.midi % 12)] += (overlap * f64::from(n.weight.max(0.0))) as f32;
        }
    }
    let total: f32 = chroma.iter().sum();
    if f64::from(total) < MIN_WEIGHT_PER_SECOND * (b - a) {
        return (None, 0.0);
    }
    let sounded = |class: usize| chroma[class] >= total * MELODY_CLASS_SHARE;
    if (0..12).filter(|c| sounded(*c)).count() < 3 {
        return (None, 0.0);
    }
    let mut best: Option<(Chord, f32)> = None;
    for root in 0..12u8 {
        for quality in QUALITIES {
            if quality == Quality::Power {
                continue;
            }
            let tones = intervals(quality);
            let classes = tones.iter().map(|i| usize::from((root + i) % 12));
            if !classes.clone().all(sounded) {
                continue;
            }
            let share = classes.map(|c| chroma[c]).sum::<f32>() / total;
            if share < MELODY_CHORD_SHARE {
                continue;
            }
            // A seventh only when its fourth tone is there: the triad wins a
            // tie, being the simpler reading of the same notes.
            if best.is_none_or(|(_, s)| share > s + 1e-4) {
                best = Some((Chord { root, quality }, share));
            }
        }
    }
    match best {
        Some((chord, share)) => (Some(chord), share.clamp(0.0, 1.0)),
        None => (None, 0.0),
    }
}
