//! Chords: what the piano roll's smart-chord strip offers, names and writes.
//!
//! Ty, 2026-10-18: chords that sound good on the note just placed, for
//! somebody who does not know the theory, and exactly the chord in their
//! head, fast, for somebody who does. One model serves both: a chord is a
//! root, a quality and perhaps a bass note; it has a name (`Am7`), a job in
//! the key (`vi7`), and keys to sound on. Suggestions come from the scale —
//! thirds stacked on its steps — when it has seven notes, and from the
//! chords lying wholly inside it when it has not, since stacking thirds
//! means nothing there. Pure functions, so every rule here is tested.
//!
//! **The note placed never moves.** Every voicing is built around it: the
//! strip is a way of harmonising the note somebody chose, not replacing it.

use crate::scale::{KeyScale, scale};

/// The chord qualities, simplest first within each size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Quality {
    Major,
    Minor,
    Diminished,
    Augmented,
    Sus2,
    Sus4,
    Power,
    Major6,
    Minor6,
    Dominant7,
    Major7,
    Minor7,
    HalfDiminished7,
    Diminished7,
    MinorMajor7,
    Augmented7,
    Sus4Dominant7,
    Add9,
    MinorAdd9,
    Dominant9,
    Major9,
    Minor9,
}

impl Quality {
    pub const ALL: [Quality; 22] = [
        Quality::Major,
        Quality::Minor,
        Quality::Diminished,
        Quality::Augmented,
        Quality::Sus2,
        Quality::Sus4,
        Quality::Power,
        Quality::Major6,
        Quality::Minor6,
        Quality::Dominant7,
        Quality::Major7,
        Quality::Minor7,
        Quality::HalfDiminished7,
        Quality::Diminished7,
        Quality::MinorMajor7,
        Quality::Augmented7,
        Quality::Sus4Dominant7,
        Quality::Add9,
        Quality::MinorAdd9,
        Quality::Dominant9,
        Quality::Major9,
        Quality::Minor9,
    ];

    /// Semitones above the root, in the order the tones are stacked; a ninth
    /// is 14, above the octave.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Quality::Major => &[0, 4, 7],
            Quality::Minor => &[0, 3, 7],
            Quality::Diminished => &[0, 3, 6],
            Quality::Augmented => &[0, 4, 8],
            Quality::Sus2 => &[0, 2, 7],
            Quality::Sus4 => &[0, 5, 7],
            Quality::Power => &[0, 7],
            Quality::Major6 => &[0, 4, 7, 9],
            Quality::Minor6 => &[0, 3, 7, 9],
            Quality::Dominant7 => &[0, 4, 7, 10],
            Quality::Major7 => &[0, 4, 7, 11],
            Quality::Minor7 => &[0, 3, 7, 10],
            Quality::HalfDiminished7 => &[0, 3, 6, 10],
            Quality::Diminished7 => &[0, 3, 6, 9],
            Quality::MinorMajor7 => &[0, 3, 7, 11],
            Quality::Augmented7 => &[0, 4, 8, 10],
            Quality::Sus4Dominant7 => &[0, 5, 7, 10],
            Quality::Add9 => &[0, 4, 7, 14],
            Quality::MinorAdd9 => &[0, 3, 7, 14],
            Quality::Dominant9 => &[0, 4, 7, 10, 14],
            Quality::Major9 => &[0, 4, 7, 11, 14],
            Quality::Minor9 => &[0, 3, 7, 10, 14],
        }
    }

    /// What follows the root in a chord's name.
    pub fn suffix(self) -> &'static str {
        match self {
            Quality::Major => "",
            Quality::Minor => "m",
            Quality::Diminished => "dim",
            Quality::Augmented => "aug",
            Quality::Sus2 => "sus2",
            Quality::Sus4 => "sus4",
            Quality::Power => "5",
            Quality::Major6 => "6",
            Quality::Minor6 => "m6",
            Quality::Dominant7 => "7",
            Quality::Major7 => "maj7",
            Quality::Minor7 => "m7",
            Quality::HalfDiminished7 => "m7b5",
            Quality::Diminished7 => "dim7",
            Quality::MinorMajor7 => "mMaj7",
            Quality::Augmented7 => "aug7",
            Quality::Sus4Dominant7 => "7sus4",
            Quality::Add9 => "add9",
            Quality::MinorAdd9 => "madd9",
            Quality::Dominant9 => "9",
            Quality::Major9 => "maj9",
            Quality::Minor9 => "m9",
        }
    }

    /// Whether its third is minor (or it is diminished): what writes its
    /// numeral in lower case.
    fn is_minor(self) -> bool {
        matches!(
            self,
            Quality::Minor
                | Quality::Diminished
                | Quality::Minor6
                | Quality::Minor7
                | Quality::HalfDiminished7
                | Quality::Diminished7
                | Quality::MinorMajor7
                | Quality::MinorAdd9
                | Quality::Minor9
        )
    }

    /// What follows a numeral: `vii°`, `V7`, `IVmaj7`, `viiø7`.
    fn roman_suffix(self) -> &'static str {
        match self {
            Quality::Major | Quality::Minor => "",
            Quality::Diminished => "°",
            Quality::Augmented => "+",
            Quality::Sus2 => "sus2",
            Quality::Sus4 => "sus4",
            Quality::Power => "5",
            Quality::Major6 | Quality::Minor6 => "6",
            Quality::Dominant7 | Quality::Minor7 => "7",
            Quality::Major7 | Quality::MinorMajor7 => "maj7",
            Quality::HalfDiminished7 => "ø7",
            Quality::Diminished7 => "°7",
            Quality::Augmented7 => "+7",
            Quality::Sus4Dominant7 => "7sus4",
            Quality::Add9 | Quality::MinorAdd9 => "add9",
            Quality::Dominant9 | Quality::Minor9 => "9",
            Quality::Major9 => "maj9",
        }
    }

    /// The quality whose stacked intervals these are.
    fn from_intervals(intervals: &[u8]) -> Option<Quality> {
        Quality::ALL
            .into_iter()
            .find(|q| q.intervals() == intervals)
    }
}

/// Chord names spell with sharps, as Analyze Musically's chord lane does.
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// A chord: a root pitch class (0 = C), a quality, and a bass note when it
/// is a slash chord (`C/E`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Chord {
    pub root: u8,
    pub quality: Quality,
    /// Written only when there is one, so a chord saved before slash
    /// chords existed reads, and writes, the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bass: Option<u8>,
}

impl Chord {
    pub fn new(root: u8, quality: Quality) -> Self {
        Self {
            root: root % 12,
            quality,
            bass: None,
        }
    }

    /// `"Am"`, `"F#m7b5"`, `"C/E"`.
    pub fn label(&self) -> String {
        let mut name = format!(
            "{}{}",
            NAMES[usize::from(self.root % 12)],
            self.quality.suffix()
        );
        if let Some(bass) = self.bass {
            name.push('/');
            name.push_str(NAMES[usize::from(bass % 12)]);
        }
        name
    }

    /// Its pitch classes, in the order the tones are stacked (the bass of a
    /// slash chord, when it is not a chord tone, first).
    pub fn pitch_classes(&self) -> Vec<u8> {
        let mut pcs: Vec<u8> = self
            .quality
            .intervals()
            .iter()
            .map(|i| (self.root + i) % 12)
            .collect();
        if let Some(bass) = self.bass.map(|b| b % 12)
            && !pcs.contains(&bass)
        {
            pcs.insert(0, bass);
        }
        pcs
    }

    /// Its pitch classes, one bit each, bit 0 = C.
    pub fn mask(&self) -> u16 {
        self.pitch_classes()
            .iter()
            .fold(0, |mask, pc| mask | 1 << pc)
    }
}

// ------------------------------------------------------------------ keys

/// The key's scale as steps above its root, when it has seven of them —
/// the scales thirds can be stacked on.
fn heptatonic(key: &KeyScale) -> Option<&'static [u8]> {
    scale(&key.scale)
        .map(|s| s.steps)
        .filter(|steps| steps.len() == 7)
}

/// The chord the scale builds on its `degree`th step: `tones` thirds
/// stacked (3 a triad, 4 a seventh, 5 a ninth). `None` when the stack is
/// no quality this knows.
fn stacked(key: &KeyScale, steps: &[u8], degree: usize, tones: usize) -> Option<Chord> {
    let root = steps[degree];
    let intervals: Vec<u8> = (0..tones)
        .map(|k| {
            let step = steps[(degree + 2 * k) % 7];
            let above = (step + 12 - root) % 12;
            // The ninth sits above the octave.
            if k == 4 { above + 12 } else { above }
        })
        .collect();
    let quality = Quality::from_intervals(&intervals)?;
    Some(Chord::new(key.root + root, quality))
}

/// The numeral for `chord` in `key`: its step of the scale, in upper case
/// for a major chord and lower for a minor one, with an accidental when its
/// root is outside the scale (`bVII`). `None` in a scale without seven
/// notes, which has no steps to count by thirds.
pub fn roman(key: &KeyScale, chord: &Chord) -> Option<String> {
    const NUMERALS: [&str; 7] = ["I", "II", "III", "IV", "V", "VI", "VII"];
    let steps = heptatonic(key)?;
    let above = (chord.root + 12 - key.root % 12) % 12;
    let (accidental, degree) = if let Some(d) = steps.iter().position(|s| *s == above) {
        ("", d)
    } else if let Some(d) = steps.iter().position(|s| *s == (above + 1) % 12) {
        ("b", d)
    } else {
        ("#", steps.iter().position(|s| *s == (above + 11) % 12)?)
    };
    let numeral = if chord.quality.is_minor() {
        NUMERALS[degree].to_lowercase()
    } else {
        NUMERALS[degree].to_string()
    };
    Some(format!(
        "{accidental}{numeral}{}",
        chord.quality.roman_suffix()
    ))
}

// ----------------------------------------------------------- suggestions

/// Which chords a note is offered: those it is the root of, or every one
/// it belongs to — how a melody is harmonised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Suggest {
    #[default]
    Rooted,
    Containing,
}

/// How far the strip's chords reach beyond a triad — set once, it changes
/// every chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Extension {
    #[default]
    Triad,
    Seventh,
    Ninth,
    Sus,
    Add9,
}

impl Extension {
    pub const ALL: [Extension; 5] = [
        Extension::Triad,
        Extension::Seventh,
        Extension::Ninth,
        Extension::Sus,
        Extension::Add9,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Extension::Triad => "Triad",
            Extension::Seventh => "7",
            Extension::Ninth => "9",
            Extension::Sus => "Sus",
            Extension::Add9 => "Add9",
        }
    }
}

/// One chip on the strip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub chord: Chord,
    /// Its job in the key, when the key has steps to count.
    pub roman: Option<String>,
    /// Some of its notes are outside the key: offered only with the scale
    /// lock off, and marked.
    pub borrowed: bool,
}

/// How many chips a strip offers: enough to choose from, few enough to read.
pub const MOST_SUGGESTIONS: usize = 10;

/// The chord a seven-note scale builds on `degree`, reaching as far as
/// `extension` asks — or the triad, where the scale cannot reach that far
/// in a quality this knows.
fn diatonic(key: &KeyScale, steps: &[u8], degree: usize, extension: Extension) -> Option<Chord> {
    let triad = stacked(key, steps, degree, 3)?;
    let mask = key.mask().unwrap_or(0xfff);
    let inside = |c: &Chord| c.mask() & !mask == 0;
    let reached = match extension {
        Extension::Triad => None,
        Extension::Seventh => stacked(key, steps, degree, 4),
        Extension::Ninth => {
            stacked(key, steps, degree, 5).or_else(|| stacked(key, steps, degree, 4))
        }
        Extension::Sus => [Quality::Sus4, Quality::Sus2]
            .into_iter()
            .map(|q| Chord::new(triad.root, q))
            .find(inside),
        Extension::Add9 => match triad.quality {
            Quality::Major => Some(Chord::new(triad.root, Quality::Add9)),
            Quality::Minor => Some(Chord::new(triad.root, Quality::MinorAdd9)),
            _ => None,
        }
        .filter(inside),
    };
    Some(reached.unwrap_or(triad))
}

/// The qualities offered on a note with no scale to go by, the extension's
/// first and the triads after.
fn common(extension: Extension) -> Vec<Quality> {
    let mut first: Vec<Quality> = match extension {
        Extension::Triad => vec![],
        Extension::Seventh => vec![
            Quality::Dominant7,
            Quality::Major7,
            Quality::Minor7,
            Quality::HalfDiminished7,
            Quality::Diminished7,
        ],
        Extension::Ninth => vec![Quality::Dominant9, Quality::Major9, Quality::Minor9],
        Extension::Sus => vec![Quality::Sus4, Quality::Sus2, Quality::Sus4Dominant7],
        Extension::Add9 => vec![Quality::Add9, Quality::MinorAdd9],
    };
    for q in [
        Quality::Major,
        Quality::Minor,
        Quality::Dominant7,
        Quality::Major7,
        Quality::Minor7,
        Quality::Diminished,
        Quality::Sus4,
        Quality::Sus2,
    ] {
        if !first.contains(&q) {
            first.push(q);
        }
    }
    first
}

/// The chips for a note on pitch class `note`: in `key` when there is one,
/// rooted on the note or containing it, reaching as far as `extension`,
/// simplest first. With the scale `locked`, nothing from outside the key;
/// unlocked, borrowed chords follow, marked.
pub fn suggestions(
    key: Option<&KeyScale>,
    note: u8,
    suggest: Suggest,
    extension: Extension,
    locked: bool,
) -> Vec<Suggestion> {
    let note = note % 12;
    let mask = key.and_then(KeyScale::mask);
    let out_of_key = |c: &Chord| mask.is_some_and(|m| c.mask() & !m != 0);
    let numeral = |c: &Chord| key.and_then(|k| roman(k, c));
    let mut chords: Vec<Chord> = Vec::new();
    let push = |c: Chord, chords: &mut Vec<Chord>| {
        if !chords.contains(&c) {
            chords.push(c);
        }
    };

    match (key, key.and_then(heptatonic)) {
        // A seven-note scale: thirds stacked on its steps.
        (Some(key), Some(steps)) => {
            let degree_of = |pc: u8| steps.iter().position(|s| (key.root + s) % 12 == pc);
            match suggest {
                Suggest::Rooted => {
                    if let Some(d) = degree_of(note) {
                        let shapes = [
                            extension,
                            Extension::Triad,
                            Extension::Seventh,
                            Extension::Sus,
                            Extension::Add9,
                            Extension::Ninth,
                        ];
                        for shape in shapes {
                            if let Some(c) = diatonic(key, steps, d, shape) {
                                push(c, &mut chords);
                            }
                        }
                        // The other suspension too, when it is in the key.
                        for q in [Quality::Sus4, Quality::Sus2] {
                            let c = Chord::new(note, q);
                            if !out_of_key(&c) {
                                push(c, &mut chords);
                            }
                        }
                    }
                }
                Suggest::Containing => {
                    // Every step's chord the note is in, by where the note
                    // sits in it: the root first, then the third, the fifth.
                    let mut found: Vec<(usize, Chord)> = (0..7)
                        .filter_map(|d| diatonic(key, steps, d, extension))
                        .filter_map(|c| {
                            c.pitch_classes()
                                .iter()
                                .position(|pc| *pc == note)
                                .map(|at| (at, c))
                        })
                        .collect();
                    found.sort_by_key(|(at, _)| *at);
                    for (_, c) in found {
                        push(c, &mut chords);
                    }
                }
            }
        }
        // Any other scale, or none: the common chords, kept to the scale
        // when there is one.
        _ => {
            let qualities = common(extension);
            match suggest {
                Suggest::Rooted => {
                    for q in &qualities {
                        let c = Chord::new(note, *q);
                        if !out_of_key(&c) {
                            push(c, &mut chords);
                        }
                    }
                }
                Suggest::Containing => {
                    let mut found: Vec<(usize, usize, Chord)> = Vec::new();
                    for (rank, q) in qualities.iter().enumerate() {
                        for root in 0..12 {
                            let c = Chord::new(root, *q);
                            if out_of_key(&c) {
                                continue;
                            }
                            if let Some(at) = c.pitch_classes().iter().position(|pc| *pc == note) {
                                found.push((at, rank, c));
                            }
                        }
                    }
                    found.sort_by_key(|(at, rank, _)| (*rank, *at));
                    for (_, _, c) in found {
                        push(c, &mut chords);
                    }
                }
            }
        }
    }

    // Unlocked, or a note outside the key: the common chords from outside,
    // marked as borrowed.
    let note_outside = mask.is_some_and(|m| m & (1 << note) == 0);
    if key.is_some() && (!locked || note_outside) {
        let borrowed = [Quality::Major, Quality::Minor, Quality::Dominant7];
        for q in borrowed {
            match suggest {
                Suggest::Rooted => push(Chord::new(note, q), &mut chords),
                Suggest::Containing => {
                    for root in 0..12 {
                        let c = Chord::new(root, q);
                        if c.pitch_classes().contains(&note) && out_of_key(&c) {
                            push(c, &mut chords);
                        }
                    }
                }
            }
        }
    }

    chords
        .into_iter()
        .filter(|c| !locked || note_outside || !out_of_key(c))
        .take(MOST_SUGGESTIONS)
        .map(|chord| Suggestion {
            roman: numeral(&chord),
            borrowed: out_of_key(&chord),
            chord,
        })
        .collect()
}

// --------------------------------------------------------------- voicing

/// How a chord's tones are spread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Voicing {
    /// Stacked as tightly as they go.
    #[default]
    Close,
    /// The second tone up an octave.
    Open,
    /// The second from the top down an octave — the jazz pianist's.
    Drop2,
    /// The bass down an octave under the rest.
    Spread,
}

impl Voicing {
    pub const ALL: [Voicing; 4] = [
        Voicing::Close,
        Voicing::Open,
        Voicing::Drop2,
        Voicing::Spread,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Voicing::Close => "Close",
            Voicing::Open => "Open",
            Voicing::Drop2 => "Drop 2",
            Voicing::Spread => "Spread",
        }
    }
}

/// Which tone is at the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Inversion {
    #[default]
    Root,
    First,
    Second,
    Third,
    /// Whichever moves least from the chord before: voice leading done for
    /// you.
    Nearest,
}

impl Inversion {
    pub const ALL: [Inversion; 5] = [
        Inversion::Root,
        Inversion::First,
        Inversion::Second,
        Inversion::Third,
        Inversion::Nearest,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Inversion::Root => "Root",
            Inversion::First => "1st",
            Inversion::Second => "2nd",
            Inversion::Third => "3rd",
            Inversion::Nearest => "Nearest",
        }
    }
}

/// The chord's tones stacked from `bass` (an index into its tones),
/// ascending, as semitones above the lowest.
fn close_stack(chord: &Chord, bass: usize) -> Vec<i32> {
    let mut pcs = chord.pitch_classes();
    // A slash chord's bass is the bass whatever the inversion.
    let bass = if chord.bass.is_some() {
        0
    } else {
        bass % pcs.len().max(1)
    };
    pcs.rotate_left(bass);
    let mut stack: Vec<i32> = Vec::with_capacity(pcs.len());
    for pc in pcs {
        let pc = i32::from(pc);
        let next = match stack.last() {
            None => pc,
            Some(&below) => {
                let mut k = below - below.rem_euclid(12) + pc;
                while k <= below {
                    k += 12;
                }
                k
            }
        };
        stack.push(next);
    }
    stack
}

fn spread(mut stack: Vec<i32>, voicing: Voicing) -> Vec<i32> {
    match voicing {
        Voicing::Close => {}
        Voicing::Open if stack.len() >= 3 => stack[1] += 12,
        Voicing::Drop2 if stack.len() >= 3 => {
            let at = stack.len() - 2;
            stack[at] -= 12;
        }
        Voicing::Spread if stack.len() >= 2 => stack[0] -= 12,
        _ => {}
    }
    stack.sort_unstable();
    stack
}

/// The stack moved so the tone sharing `anchor`'s pitch class lands on
/// `anchor` — the note placed stays where it was put. A chord without that
/// tone sits with its lowest note just under the anchor.
fn anchored(stack: &[i32], anchor: u8) -> Vec<u8> {
    let anchor = i32::from(anchor.min(127));
    let shift = match stack
        .iter()
        .find(|k| k.rem_euclid(12) == anchor.rem_euclid(12))
    {
        Some(&tone) => anchor - tone,
        None => {
            let lowest = stack.first().copied().unwrap_or(0);
            let mut shift = anchor - lowest;
            shift -= (shift + lowest - anchor).rem_euclid(12);
            shift
        }
    };
    let mut keys: Vec<u8> = stack
        .iter()
        .map(|k| k + shift)
        .filter(|k| (0..=127).contains(k))
        .map(|k| k as u8)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// How far a voicing's notes move from `previous`'s: each note to the
/// nearest of the other, both ways.
fn movement(keys: &[u8], previous: &[u8]) -> u32 {
    let near = |k: u8, others: &[u8]| {
        others
            .iter()
            .map(|o| u32::from(k.abs_diff(*o)))
            .min()
            .unwrap_or(0)
    };
    keys.iter().map(|k| near(*k, previous)).sum::<u32>()
        + previous.iter().map(|p| near(*p, keys)).sum::<u32>()
}

/// The keys `chord` sounds on, voiced and inverted, around `anchor` — the
/// note placed, which stays exactly where it is.
pub fn voice(
    chord: &Chord,
    anchor: u8,
    voicing: Voicing,
    inversion: Inversion,
    previous: Option<&[u8]>,
) -> Vec<u8> {
    let tones = chord.pitch_classes().len();
    let at = |bass: usize| anchored(&spread(close_stack(chord, bass), voicing), anchor);
    match inversion {
        Inversion::Root => at(0),
        Inversion::First => at(1),
        Inversion::Second => at(2),
        Inversion::Third => at(3),
        Inversion::Nearest => match previous.filter(|p| !p.is_empty()) {
            None => at(0),
            Some(previous) => (0..tones)
                .map(at)
                .min_by_key(|keys| movement(keys, previous))
                .unwrap_or_else(|| at(0)),
        },
    }
}

// --------------------------------------------------------------- reading

/// Names a quality may be written with, after the root.
const SPELLINGS: &[(&str, Quality)] = &[
    ("", Quality::Major),
    ("maj", Quality::Major),
    ("M", Quality::Major),
    ("m", Quality::Minor),
    ("min", Quality::Minor),
    ("-", Quality::Minor),
    ("dim", Quality::Diminished),
    ("°", Quality::Diminished),
    ("o", Quality::Diminished),
    ("aug", Quality::Augmented),
    ("+", Quality::Augmented),
    ("sus2", Quality::Sus2),
    ("sus4", Quality::Sus4),
    ("sus", Quality::Sus4),
    ("5", Quality::Power),
    ("6", Quality::Major6),
    ("m6", Quality::Minor6),
    ("min6", Quality::Minor6),
    ("7", Quality::Dominant7),
    ("dom7", Quality::Dominant7),
    ("maj7", Quality::Major7),
    ("M7", Quality::Major7),
    ("ma7", Quality::Major7),
    ("Δ", Quality::Major7),
    ("Δ7", Quality::Major7),
    ("m7", Quality::Minor7),
    ("min7", Quality::Minor7),
    ("-7", Quality::Minor7),
    ("m7b5", Quality::HalfDiminished7),
    ("min7b5", Quality::HalfDiminished7),
    ("ø", Quality::HalfDiminished7),
    ("ø7", Quality::HalfDiminished7),
    ("dim7", Quality::Diminished7),
    ("°7", Quality::Diminished7),
    ("o7", Quality::Diminished7),
    ("mMaj7", Quality::MinorMajor7),
    ("mmaj7", Quality::MinorMajor7),
    ("mM7", Quality::MinorMajor7),
    ("m(maj7)", Quality::MinorMajor7),
    ("aug7", Quality::Augmented7),
    ("+7", Quality::Augmented7),
    ("7#5", Quality::Augmented7),
    ("7sus4", Quality::Sus4Dominant7),
    ("7sus", Quality::Sus4Dominant7),
    ("add9", Quality::Add9),
    ("add2", Quality::Add9),
    ("madd9", Quality::MinorAdd9),
    ("9", Quality::Dominant9),
    ("maj9", Quality::Major9),
    ("M9", Quality::Major9),
    ("m9", Quality::Minor9),
    ("min9", Quality::Minor9),
];

/// A letter and its accidentals, at the start of `text`: the pitch class
/// and what is left.
fn read_note(text: &str) -> Option<(u8, &str)> {
    let mut chars = text.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let base: i32 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut rest = chars.as_str();
    let mut shift = 0;
    loop {
        if let Some(r) = rest.strip_prefix(['#', '♯']) {
            shift += 1;
            rest = r;
        } else if let Some(r) = rest.strip_prefix(['b', '♭']) {
            // `Bb` is B flat; a `b` that starts `b5` belongs to the suffix.
            if r.starts_with(|c: char| c.is_ascii_digit()) {
                break;
            }
            shift -= 1;
            rest = r;
        } else {
            break;
        }
    }
    Some(((base + shift).rem_euclid(12) as u8, rest))
}

fn read_quality(suffix: &str) -> Option<Quality> {
    SPELLINGS
        .iter()
        .find(|(s, _)| *s == suffix)
        .or_else(|| {
            let lower = suffix.to_lowercase();
            SPELLINGS
                .iter()
                .find(|(s, _)| *s != "M" && *s != "M7" && *s != "M9" && s.to_lowercase() == lower)
        })
        .map(|(_, q)| *q)
}

/// A numeral in `key` — `ii7`, `V`, `bVII`, `viiø7`.
fn read_roman(text: &str, key: &KeyScale) -> Option<Chord> {
    let steps = heptatonic(key)?;
    let (shift, rest) = if let Some(r) = text.strip_prefix(['b', '♭']) {
        (-1, r)
    } else if let Some(r) = text.strip_prefix(['#', '♯']) {
        (1, r)
    } else {
        (0, text)
    };
    const NUMERALS: [&str; 7] = ["VII", "VI", "IV", "V", "III", "II", "I"];
    const DEGREES: [usize; 7] = [6, 5, 3, 4, 2, 1, 0];
    let (degree, upper, suffix) = NUMERALS.iter().zip(DEGREES).find_map(|(n, d)| {
        if let Some(s) = rest.strip_prefix(n) {
            Some((d, true, s))
        } else {
            rest.strip_prefix(n.to_lowercase().as_str())
                .map(|s| (d, false, s))
        }
    })?;
    let quality = Quality::ALL
        .into_iter()
        .find(|q| q.is_minor() != upper && q.roman_suffix() == suffix)
        .or(match (upper, suffix) {
            (false, "o") => Some(Quality::Diminished),
            (false, "o7") => Some(Quality::Diminished7),
            (false, "ø") => Some(Quality::HalfDiminished7),
            _ => None,
        })?;
    let root = (i32::from(key.root) + i32::from(steps[degree]) + shift).rem_euclid(12) as u8;
    Some(Chord::new(root, quality))
}

/// A chord as somebody types it: a name (`Cmaj9/E`, `F#m7b5`, `Bbsus4`),
/// or in a key a numeral (`ii7`, `bVII`). `None` for anything else.
pub fn parse_chord(text: &str, key: Option<&KeyScale>) -> Option<Chord> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(chord) = key.and_then(|k| read_roman(text, k)) {
        return Some(chord);
    }
    let (body, bass) = match text.rsplit_once('/') {
        Some((body, bass)) => {
            let (pc, rest) = read_note(bass)?;
            if !rest.is_empty() {
                return None;
            }
            (body, Some(pc))
        }
        None => (text, None),
    };
    let (root, suffix) = read_note(body)?;
    let quality = read_quality(suffix)?;
    Some(Chord {
        bass,
        ..Chord::new(root, quality)
    })
}
