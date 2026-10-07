//! Where each phoneme sits in time, and the frames the models see.
//!
//! A note's onset consonants (its `lead`) are sung before the note starts,
//! ending on it; its vowel starts on the note and lasts until the next
//! note's onset (or the note's end, before a rest); consonants after the
//! vowel (a coda) close the span. Consonant lengths come from the bank's
//! duration model when it has one, else a fixed length, and are divided by
//! the note's velocity. Phonemes the writer placed by hand stay where they
//! were put.

use fontelle_sing::PhraseNote;

use crate::phonemize::NotePhonemes;

/// A consonant's length when the bank has no duration model.
pub const DEFAULT_CONSONANT: f64 = 0.06;
/// The shortest a phoneme is made.
pub const MIN_PHONEME: f64 = 0.01;
/// The longest the onset before a phrase's first note may be.
pub const MAX_FIRST_LEAD: f64 = 0.5;
/// Silence frames before and after a phrase, as every bank is trained.
pub const PAD_FRAMES: usize = 8;

/// A phoneme placed in phrase time.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub symbol: String,
    /// The note it belongs to.
    pub note: usize,
    pub start: f64,
    pub end: f64,
}

/// A stretch of the model's input: a placed phoneme, or silence.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub symbol: String,
    pub start: f64,
    pub end: f64,
    /// Index into the placed phonemes; `None` for silence.
    pub placed: Option<usize>,
}

/// Every phoneme of `per_note` in order (each note's lead, then its body),
/// as `(note, symbol)`.
pub fn flatten(per_note: &[NotePhonemes]) -> Vec<(usize, &str)> {
    per_note
        .iter()
        .enumerate()
        .flat_map(|(i, n)| {
            n.lead
                .iter()
                .chain(&n.body)
                .map(move |p| (i, p.symbol.as_str()))
        })
        .collect()
}

/// A note that sings, and the notes after it that only hold its vowel.
#[derive(Debug, Clone, Copy)]
struct Run {
    note: usize,
    start: f64,
    end: f64,
}

/// Each note's end, cut at the next note's start where they overlap.
fn note_end(notes: &[PhraseNote], i: usize) -> f64 {
    let end = notes[i].end();
    match notes.get(i + 1) {
        Some(next) if next.start < end => next.start.max(notes[i].start),
        _ => end,
    }
}

fn runs(notes: &[PhraseNote], per_note: &[NotePhonemes]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut last_note: Option<usize> = None;
    for (i, note) in notes.iter().enumerate() {
        let sings = per_note.get(i).is_some_and(|p| !p.is_empty());
        if sings {
            runs.push(Run {
                note: i,
                start: note.start,
                end: note_end(notes, i),
            });
            last_note = Some(i);
        } else if let (Some(run), Some(last)) = (runs.last_mut(), last_note)
            && last + 1 == i
            && note.start - note_end(notes, last) <= crate::lyrics::TOUCH
        {
            // A held vowel carries on over this note.
            run.end = note_end(notes, i);
            last_note = Some(i);
        }
    }
    runs
}

/// The index of each note's first phoneme among the flattened ones.
fn first_indices(per_note: &[NotePhonemes]) -> Vec<usize> {
    let mut at = 0;
    per_note
        .iter()
        .map(|n| {
            let first = at;
            at += n.lead.len() + n.body.len();
            first
        })
        .collect()
}

/// Places the phonemes. `lengths` (seconds, one per [`flatten`]ed phoneme)
/// are the duration model's; without them consonants take
/// [`DEFAULT_CONSONANT`].
pub fn place(
    notes: &[PhraseNote],
    per_note: &[NotePhonemes],
    lengths: Option<&[f64]>,
    is_vowel: &dyn Fn(&str) -> bool,
) -> Vec<Placed> {
    let runs = runs(notes, per_note);
    let firsts = first_indices(per_note);
    let given = |index: usize| -> Option<f64> {
        lengths
            .and_then(|l| l.get(index).copied())
            .filter(|l| l.is_finite() && *l > 0.0)
    };
    // Onsets, from each note's start backwards.
    let mut lead_starts = Vec::with_capacity(runs.len());
    let mut lead_lengths: Vec<Vec<f64>> = Vec::with_capacity(runs.len());
    for (r, run) in runs.iter().enumerate() {
        let np = &per_note[run.note];
        let velocity = f64::from(notes[run.note].velocity).clamp(0.25, 4.0);
        let mut lens: Vec<f64> = (0..np.lead.len())
            .map(|k| given(firsts[run.note] + k).unwrap_or(DEFAULT_CONSONANT) / velocity)
            .collect();
        let total: f64 = lens.iter().sum();
        let room = if r == 0 {
            MAX_FIRST_LEAD
        } else {
            let prev = runs[r - 1];
            (run.start - prev.end).max(0.0) + 0.5 * (prev.end - prev.start).max(0.0)
        };
        if total > room && total > 0.0 {
            let k = room / total;
            for l in &mut lens {
                *l *= k;
            }
        }
        lead_starts.push(run.start - lens.iter().sum::<f64>());
        lead_lengths.push(lens);
    }
    let mut placed = Vec::new();
    let mut anchors = Vec::new();
    let mut bounds = Vec::new();
    for (r, run) in runs.iter().enumerate() {
        let np = &per_note[run.note];
        let mut t = lead_starts[r];
        for (k, ph) in np.lead.iter().enumerate() {
            let len = lead_lengths[r][k];
            placed.push(Placed {
                symbol: ph.symbol.clone(),
                note: run.note,
                start: t,
                end: t + len,
            });
            anchors.push(ph.offset);
            bounds.push(f64::INFINITY);
            t += len;
        }
        // The body, to the next onset or the run's end.
        let body_end = match lead_starts.get(r + 1) {
            Some(&next) if next < run.end => next,
            _ => run.end,
        }
        .max(run.start + MIN_PHONEME);
        let span = body_end - run.start;
        let base = firsts[run.note] + np.lead.len();
        let vowels: Vec<bool> = np.body.iter().map(|p| is_vowel(&p.symbol)).collect();
        let mut lens: Vec<f64> = (0..np.body.len())
            .map(|k| given(base + k).unwrap_or(if vowels[k] { 1.0 } else { DEFAULT_CONSONANT }))
            .collect();
        if vowels.iter().any(|v| *v) {
            let consonants: f64 = (0..lens.len())
                .filter(|&k| !vowels[k])
                .map(|k| lens[k])
                .sum();
            let k = if consonants > 0.5 * span {
                0.5 * span / consonants
            } else {
                1.0
            };
            let weight: f64 = (0..lens.len())
                .filter(|&j| vowels[j])
                .map(|j| lens[j])
                .sum();
            let rest = span - consonants * k;
            for j in 0..lens.len() {
                lens[j] = if vowels[j] {
                    rest * lens[j] / weight
                } else {
                    lens[j] * k
                };
            }
        } else {
            let total: f64 = lens.iter().sum();
            for l in &mut lens {
                *l *= span / total;
            }
        }
        let mut t = run.start;
        for (k, ph) in np.body.iter().enumerate() {
            placed.push(Placed {
                symbol: ph.symbol.clone(),
                note: run.note,
                start: t,
                end: t + lens[k],
            });
            anchors.push(ph.offset);
            bounds.push(run.end);
            t += lens[k];
        }
    }
    // Hand-placed phonemes, then everything back in order around them.
    let n = placed.len();
    for i in 0..n {
        if let Some(offset) = anchors[i].filter(|o| o.is_finite()) {
            placed[i].start = notes[placed[i].note].start + offset;
        }
    }
    for i in (0..n.saturating_sub(1)).rev() {
        if anchors[i].is_none() && placed[i].start > placed[i + 1].start - MIN_PHONEME {
            placed[i].start = placed[i + 1].start - MIN_PHONEME;
        }
    }
    for i in 1..n {
        if placed[i].start < placed[i - 1].start + MIN_PHONEME {
            placed[i].start = placed[i - 1].start + MIN_PHONEME;
        }
    }
    for i in 0..n {
        let next = placed.get(i + 1).map_or(f64::INFINITY, |p| p.start);
        let end = next.min(bounds[i]);
        let end = if end.is_finite() { end } else { placed[i].end };
        placed[i].end = end.max(placed[i].start + MIN_PHONEME);
    }
    placed
}

/// The placed phonemes with silence between and around them: `head` and
/// `tail` seconds of `SP` at the ends, `SP` in every gap.
pub fn segments(placed: &[Placed], head: f64, tail: f64) -> Vec<Segment> {
    let Some(first) = placed.first() else {
        return vec![Segment {
            symbol: "SP".into(),
            start: -head,
            end: tail,
            placed: None,
        }];
    };
    let silence = |start: f64, end: f64| Segment {
        symbol: "SP".into(),
        start,
        end,
        placed: None,
    };
    let mut out = vec![silence(first.start - head, first.start)];
    let mut at = first.start;
    for (i, p) in placed.iter().enumerate() {
        if p.start > at + 1e-9 {
            out.push(silence(at, p.start));
        }
        out.push(Segment {
            symbol: p.symbol.clone(),
            start: p.start,
            end: p.end,
            placed: Some(i),
        });
        at = p.end;
    }
    out.push(silence(at, at + tail));
    out
}

/// Each segment's length in frames, rounded at the boundaries so that
/// nothing drifts, and at least one frame each.
pub fn frames_of(segments: &[Segment], frame: f64) -> Vec<i64> {
    let Some(first) = segments.first() else {
        return Vec::new();
    };
    let ends: Vec<f64> = segments.iter().map(|s| s.end).collect();
    frames_between(first.start, &ends, frame)
}

/// Frames between `start` and each boundary in turn, rounded at the
/// boundaries, at least one each.
fn frames_between(start: f64, boundaries: &[f64], frame: f64) -> Vec<i64> {
    let mut out = Vec::with_capacity(boundaries.len());
    let mut prev = 0i64;
    for &b in boundaries {
        let at = (((b - start) / frame).round() as i64).max(prev + 1);
        out.push(at - prev);
        prev = at;
    }
    out
}

/// The duration model's input: the phrase as words in DiffSinger's sense
/// (a syllable from its vowel to the next one, with a rest's silence and
/// the onset after it as a word of their own).
#[derive(Debug, Clone, PartialEq)]
pub struct DurInput {
    pub symbols: Vec<String>,
    /// Phonemes in each word.
    pub word_div: Vec<i64>,
    /// Frames each word lasts.
    pub word_dur: Vec<i64>,
    /// The pitch of the note each phoneme is sung on.
    pub ph_midi: Vec<i64>,
    /// For each symbol, its index among the [`flatten`]ed phonemes; `None`
    /// for the silences added around them.
    pub index: Vec<Option<usize>>,
}

pub fn dur_input(notes: &[PhraseNote], per_note: &[NotePhonemes], frame: f64) -> DurInput {
    let runs = runs(notes, per_note);
    let firsts = first_indices(per_note);
    let mut d = DurInput {
        symbols: Vec::new(),
        word_div: Vec::new(),
        word_dur: Vec::new(),
        ph_midi: Vec::new(),
        index: Vec::new(),
    };
    let Some(first) = runs.first() else {
        return d;
    };
    let midi = |note: usize| notes[note].pitch.round() as i64;
    let mut boundaries = Vec::new();
    let mut in_word = 0i64;
    let close = |d: &mut DurInput, in_word: &mut i64, at: f64, boundaries: &mut Vec<f64>| {
        if *in_word > 0 {
            d.word_div.push(*in_word);
            boundaries.push(at);
            *in_word = 0;
        }
    };
    let push_lead = |d: &mut DurInput, in_word: &mut i64, note: usize| {
        let np = &per_note[note];
        for (k, ph) in np.lead.iter().enumerate() {
            d.symbols.push(ph.symbol.clone());
            d.ph_midi.push(midi(note));
            d.index.push(Some(firsts[note] + k));
            *in_word += 1;
        }
    };
    let push_silence = |d: &mut DurInput, in_word: &mut i64, note: usize| {
        d.symbols.push("SP".into());
        d.ph_midi.push(midi(note));
        d.index.push(None);
        *in_word += 1;
    };
    let begin = first.start - MAX_FIRST_LEAD;
    push_silence(&mut d, &mut in_word, first.note);
    push_lead(&mut d, &mut in_word, first.note);
    close(&mut d, &mut in_word, first.start, &mut boundaries);
    for (r, run) in runs.iter().enumerate() {
        let np = &per_note[run.note];
        let base = firsts[run.note] + np.lead.len();
        for (k, ph) in np.body.iter().enumerate() {
            d.symbols.push(ph.symbol.clone());
            d.ph_midi.push(midi(run.note));
            d.index.push(Some(base + k));
            in_word += 1;
        }
        match runs.get(r + 1) {
            Some(next) if next.start - run.end <= crate::lyrics::TOUCH => {
                push_lead(&mut d, &mut in_word, next.note);
                close(&mut d, &mut in_word, next.start, &mut boundaries);
            }
            Some(next) => {
                close(&mut d, &mut in_word, run.end, &mut boundaries);
                push_silence(&mut d, &mut in_word, next.note);
                push_lead(&mut d, &mut in_word, next.note);
                close(&mut d, &mut in_word, next.start, &mut boundaries);
            }
            None => close(&mut d, &mut in_word, run.end, &mut boundaries),
        }
    }
    d.word_dur = frames_between(begin, &boundaries, frame);
    d
}

/// The pitch model's notes: rests before, between and after the phrase's
/// notes, in frames that add up to the phrase's.
#[derive(Debug, Clone, PartialEq)]
pub struct PitchNotes {
    pub midi: Vec<f32>,
    pub rest: Vec<bool>,
    pub dur: Vec<i64>,
}

/// `voiced[i]`: note `i` sings a vowel (its own or one held from before).
pub fn pitch_notes(
    notes: &[PhraseNote],
    voiced: &[bool],
    start: f64,
    frame: f64,
    total_frames: i64,
) -> PitchNotes {
    let mut midi = Vec::new();
    let mut rest = Vec::new();
    let mut boundaries = Vec::new();
    let half = frame / 2.0;
    let mut at = start;
    for (i, note) in notes.iter().enumerate() {
        if note.start - at > half {
            midi.push(note.pitch);
            rest.push(true);
            boundaries.push(note.start);
        }
        let end = note_end(notes, i);
        midi.push(note.pitch);
        rest.push(!voiced.get(i).copied().unwrap_or(true));
        boundaries.push(end);
        at = end;
    }
    let end = start + total_frames as f64 * frame;
    if end - at > half || boundaries.is_empty() {
        midi.push(notes.last().map_or(60.0, |n| n.pitch));
        rest.push(true);
        boundaries.push(end);
    }
    let mut dur = frames_between(start, &boundaries, frame);
    // The last note takes up whatever rounding left over.
    let sum: i64 = dur.iter().sum();
    if let Some(last) = dur.last_mut() {
        *last = (*last + total_frames - sum).max(1);
    }
    fill_rests(&mut midi, &rest);
    PitchNotes { midi, rest, dur }
}

/// A rest's pitch is its neighbours': the note before for its first half,
/// the note after for the second; at the ends, the nearest note's.
fn fill_rests(midi: &mut [f32], rest: &[bool]) {
    if rest.iter().all(|r| *r) {
        midi.fill(60.0);
        return;
    }
    let n = rest.len();
    let mut i = 0;
    while i < n {
        if !rest[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && rest[j] {
            j += 1;
        }
        if i == 0 {
            let v = midi[j];
            midi[..j].fill(v);
        } else if j == n {
            let v = midi[i - 1];
            midi[i..].fill(v);
        } else {
            let mid = (i + j).div_ceil(2);
            let (before, after) = (midi[i - 1], midi[j]);
            midi[i..mid].fill(before);
            midi[mid..j].fill(after);
        }
        i = j;
    }
}

/// A word-mode linguistic encoder's division of the segments (`word_div`,
/// `word_dur`): each word begins at a sung vowel, the silence and onset
/// before the first vowel being a word of their own, the end the last.
pub fn word_division(
    segments: &[Segment],
    frames: &[i64],
    is_vowel: &dyn Fn(&str) -> bool,
) -> (Vec<i64>, Vec<i64>) {
    let n = segments.len();
    let mut starts: Vec<usize> = (0..n)
        .filter(|&i| segments[i].placed.is_some() && is_vowel(&segments[i].symbol))
        .collect();
    if starts.is_empty() {
        starts.push(n.saturating_sub(2));
    }
    let mut bounds = vec![0];
    bounds.extend(starts.iter().copied().filter(|&s| s > 0));
    bounds.push(n);
    bounds.dedup();
    let div: Vec<i64> = bounds.windows(2).map(|w| (w[1] - w[0]) as i64).collect();
    let dur = bounds
        .windows(2)
        .map(|w| frames[w[0]..w[1]].iter().sum())
        .collect();
    (div, dur)
}
