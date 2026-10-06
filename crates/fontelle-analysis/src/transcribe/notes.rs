//! basic-pitch's post-processing, ported from `basic_pitch/note_creation.py`
//! (Apache-2.0, Spotify AB; licenses/basic-pitch): posteriorgrams in, note
//! events out. Pure, no model.

/// Keys the model reports: the piano's 88, A0 (MIDI 21) upward.
pub const N_KEYS: usize = 88;
/// Contour bins: three per semitone over the same 88 keys.
pub const N_CONTOUR_BINS: usize = 264;
/// The lowest key, as MIDI.
pub const MIDI_OFFSET: u8 = 21;

/// The model's three outputs for a whole piece of audio, frame-major
/// (`onset[frame * N_KEYS + key]`), at 22 050 / 256 ≈ 86 frames a second.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Posteriorgrams {
    pub frames: usize,
    pub onset: Vec<f32>,
    pub note: Vec<f32>,
    pub contour: Vec<f32>,
}

impl Posteriorgrams {
    /// Where frame `frame` sits in the audio, in seconds.
    ///
    /// Frame `f` is frame `f % 142` of window `f / 142` past the 15 frames
    /// cut from its start, and those 15 frames are exactly the half-overlap of
    /// silence the audio was preceded by, so the frame lands at
    /// `window * hop + (f % 142) * 256` samples. (basic-pitch's own
    /// `model_frames_to_time` corrects per 172 frames rather than per 142 and
    /// drifts late by about 0.9 ms a second; this does not.)
    pub fn frame_time(frame: usize) -> f64 {
        const KEPT: usize = 142;
        const WINDOW_HOP: usize = 43_844 - 30 * 256;
        let samples = (frame / KEPT) * WINDOW_HOP + (frame % KEPT) * 256;
        samples as f64 / 22_050.0
    }

    fn row(gram: &[f32], width: usize, frame: usize) -> &[f32] {
        &gram[frame * width..(frame + 1) * width]
    }
}

/// The thresholds of `model_output_to_notes`, basic-pitch's defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteParams {
    /// An onset peak at least this strong starts a note.
    pub onset_threshold: f32,
    /// A note stays on while its frame activation is at least this.
    pub frame_threshold: f32,
    /// Notes this many frames long or shorter are dropped.
    pub min_note_frames: usize,
    /// Onsets inferred from jumps in the note activation, beside the model's.
    pub infer_onsets: bool,
    /// The "melodia trick": notes grown from leftover energy with no onset.
    pub melodia_trick: bool,
}

impl Default for NoteParams {
    fn default() -> Self {
        // 127.7 ms at 86.13 frames a second is basic-pitch's 11 frames.
        Self {
            onset_threshold: 0.5,
            frame_threshold: 0.3,
            min_note_frames: 11,
            infer_onsets: true,
            melodia_trick: true,
        }
    }
}

/// One detected note.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteEvent {
    /// First frame, and one past the last.
    pub start_frame: usize,
    pub end_frame: usize,
    /// Seconds, from [`Posteriorgrams::frame_time`].
    pub start: f64,
    pub end: f64,
    pub midi: u8,
    /// Mean note activation over the note, 0..1.
    pub amplitude: f32,
    /// Per frame, the contour's peak relative to the key, in thirds of a
    /// semitone (basic-pitch's units). `None` when not estimated.
    pub bends: Option<Vec<i8>>,
}

/// Never further than this many frames below the threshold inside a note
/// (`ENERGY_TOLERANCE`).
const ENERGY_TOLERANCE: usize = 11;
/// The highest key index.
const MAX_KEY: usize = N_KEYS - 1;

/// `get_infered_onsets`: the model's onsets, or where the note activation
/// jumps (the smaller of its rise over one and over two frames), scaled to the
/// onsets' peak, whichever is larger.
fn inferred_onsets(onset: &[f32], note: &[f32], frames: usize) -> Vec<f32> {
    const N_DIFF: usize = 2;
    let mut diff = vec![0.0f32; frames * N_KEYS];
    for t in N_DIFF..frames {
        for k in 0..N_KEYS {
            let now = note[t * N_KEYS + k];
            let rise = (1..=N_DIFF)
                .map(|n| now - note[(t - n) * N_KEYS + k])
                .fold(f32::INFINITY, f32::min);
            diff[t * N_KEYS + k] = rise.max(0.0);
        }
    }
    let onset_max = onset.iter().copied().fold(0.0f32, f32::max);
    let diff_max = diff.iter().copied().fold(0.0f32, f32::max);
    // Python divides by a zero maximum and gets NaN everywhere, which then
    // finds no onsets at all; no jumps simply add nothing here.
    let scale = if diff_max > 0.0 {
        onset_max / diff_max
    } else {
        0.0
    };
    onset
        .iter()
        .zip(&diff)
        .map(|(&o, &d)| o.max(d * scale))
        .collect()
}

/// The mean of a key's activation over `[start, end)`.
fn mean_activation(note: &[f32], key: usize, start: usize, end: usize) -> f32 {
    let sum: f64 = (start..end)
        .map(|t| f64::from(note[t * N_KEYS + key]))
        .sum();
    (sum / (end - start) as f64) as f32
}

/// Notes from posteriorgrams: `output_to_notes_polyphonic` and
/// `get_pitch_bends`, decision for decision. Sorted by start then key.
pub fn notes_from_posteriorgrams(post: &Posteriorgrams, params: &NoteParams) -> Vec<NoteEvent> {
    let n = post.frames;
    if n == 0 {
        return Vec::new();
    }
    let frames = &post.note[..n * N_KEYS];
    let onsets = if params.infer_onsets {
        inferred_onsets(&post.onset[..n * N_KEYS], frames, n)
    } else {
        post.onset[..n * N_KEYS].to_vec()
    };

    // Onset peaks over time, per key (`argrelmax` along time: strictly above
    // both neighbours, never the first or last frame), at or over threshold.
    let mut peaks: Vec<(usize, usize)> = Vec::new();
    for t in 1..n.saturating_sub(1) {
        for k in 0..N_KEYS {
            let v = onsets[t * N_KEYS + k];
            if v > onsets[(t - 1) * N_KEYS + k]
                && v > onsets[(t + 1) * N_KEYS + k]
                && v >= params.onset_threshold
            {
                peaks.push((t, k));
            }
        }
    }

    let mut remaining = frames.to_vec();
    let clear = |remaining: &mut [f32], t: usize, k: usize| {
        remaining[t * N_KEYS + k] = 0.0;
        if k < MAX_KEY {
            remaining[t * N_KEYS + k + 1] = 0.0;
        }
        if k > 0 {
            remaining[t * N_KEYS + k - 1] = 0.0;
        }
    };
    let mut events: Vec<(usize, usize, usize, f32)> = Vec::new();

    // Backwards in time, so a later note's onset never cuts an earlier note.
    for &(start, k) in peaks.iter().rev() {
        if start >= n - 1 {
            continue;
        }
        let mut i = start + 1;
        let mut below = 0;
        while i < n - 1 && below < ENERGY_TOLERANCE {
            if remaining[i * N_KEYS + k] < params.frame_threshold {
                below += 1;
            } else {
                below = 0;
            }
            i += 1;
        }
        i -= below;
        if i - start <= params.min_note_frames {
            continue;
        }
        for t in start..i {
            clear(&mut remaining, t, k);
        }
        events.push((start, i, k, mean_activation(frames, k, start, i)));
    }

    if params.melodia_trick {
        // Python takes the global maximum of what is left, again and again.
        // What is left only ever drops to zero, so that is the same as
        // walking every cell above threshold from the largest down (ties in
        // flat order, as `argmax` breaks them) and skipping the cleared ones.
        let mut order: Vec<usize> = (0..n * N_KEYS)
            .filter(|&c| remaining[c] > params.frame_threshold)
            .collect();
        order.sort_by(|&a, &b| remaining[b].total_cmp(&remaining[a]).then(a.cmp(&b)));
        for cell in order {
            if remaining[cell] <= params.frame_threshold {
                continue;
            }
            let (mid, k) = (cell / N_KEYS, cell % N_KEYS);
            remaining[cell] = 0.0;

            let mut i = mid + 1;
            let mut below = 0;
            while i < n - 1 && below < ENERGY_TOLERANCE {
                if remaining[i * N_KEYS + k] < params.frame_threshold {
                    below += 1;
                } else {
                    below = 0;
                }
                clear(&mut remaining, i, k);
                i += 1;
            }
            let end = i - 1 - below;

            let mut i = mid as isize - 1;
            let mut below = 0;
            while i > 0 && below < ENERGY_TOLERANCE {
                let t = i as usize;
                if remaining[t * N_KEYS + k] < params.frame_threshold {
                    below += 1;
                } else {
                    below = 0;
                }
                clear(&mut remaining, t, k);
                i -= 1;
            }
            let start = (i + 1 + below as isize) as usize;

            if end <= start || end - start <= params.min_note_frames {
                continue;
            }
            events.push((start, end, k, mean_activation(frames, k, start, end)));
        }
    }

    let mut notes: Vec<NoteEvent> = events
        .into_iter()
        .map(|(start, end, k, amplitude)| NoteEvent {
            start_frame: start,
            end_frame: end,
            start: Posteriorgrams::frame_time(start),
            end: Posteriorgrams::frame_time(end),
            midi: MIDI_OFFSET + k as u8,
            amplitude,
            bends: Some(pitch_bends(post, start, end, k)),
        })
        .collect();
    notes.sort_by(|a, b| {
        a.start_frame
            .cmp(&b.start_frame)
            .then(a.midi.cmp(&b.midi))
            .then(a.end_frame.cmp(&b.end_frame))
    });
    notes
}

/// `get_pitch_bends`: per frame, the strongest contour bin within ±25 bins
/// of the key (weighted by a Gaussian of 5 bins' deviation), relative to
/// the key, in thirds of a semitone.
fn pitch_bends(post: &Posteriorgrams, start: usize, end: usize, key: usize) -> Vec<i8> {
    const TOL: usize = 25;
    let centre = 3 * key;
    let lo = centre.saturating_sub(TOL);
    let hi = (centre + TOL + 1).min(N_CONTOUR_BINS);
    (start..end)
        .map(|t| {
            let row = Posteriorgrams::row(&post.contour, N_CONTOUR_BINS, t);
            let mut best = lo;
            let mut best_v = f64::NEG_INFINITY;
            for (bin, &v) in row.iter().enumerate().take(hi).skip(lo) {
                let d = bin as f64 - centre as f64;
                let w = (-0.5 * (d / 5.0).powi(2)).exp();
                let weighted = f64::from(v) * w;
                if weighted > best_v {
                    best_v = weighted;
                    best = bin;
                }
            }
            (best as isize - centre as isize) as i8
        })
        .collect()
}

/// A split is only a split if the model's own onset there is weaker than
/// this; a note struck again scores 0.85 and up, vibrato 0.5 to 0.7.
const RESTRIKE_ONSET: f32 = 0.75;
/// A note this much quieter (in mean activation) than one a partial below it
/// is that note's partial.
const GHOST_RATIO: f32 = 0.55;
/// Intervals of the 2nd to 6th partials, in semitones.
const PARTIALS: [u8; 5] = [12, 19, 24, 28, 31];
/// How much of a ghost the louder note must cover.
const GHOST_COVER: f32 = 0.8;
/// Above the frame threshold by less than this, a note is only faint.
const FAINT_MARGIN: f32 = 0.1;

/// What basic-pitch's own decoding leaves for a musician to clean up, done
/// here. Three passes, all reading the posteriorgrams the notes came from:
///
/// 1. **A note split where nothing was struck again** is joined: two notes
///    of one key that touch, where the model's onset at the join is weak
///    (under 0.75) and the note never let go. Vibrato makes the inferred
///    onsets fire mid-note; a held chord tone splits where the melody above
///    it starts.
/// 2. **A ghost partial** is dropped: a note an octave, a twelfth, two
///    octaves, or the 5th or 6th partial above a louder note (by 0.55 in
///    activation) that covers 80 % of it. The model hears a strong partial
///    as a quiet note. (A real, much quieter octave doubling goes too.)
/// 3. **A faint sliver** is dropped: barely over the frame threshold,
///    touching a stronger, longer note within two semitones. It is that
///    note's scoop or release, not a note.
///
/// Sorted by start then key.
pub fn tidy_notes(notes: Vec<NoteEvent>, post: &Posteriorgrams) -> Vec<NoteEvent> {
    tidy_notes_with(notes, post, &NoteParams::default())
}

/// [`tidy_notes`] for notes decoded with other thresholds.
pub fn tidy_notes_with(
    mut notes: Vec<NoteEvent>,
    post: &Posteriorgrams,
    params: &NoteParams,
) -> Vec<NoteEvent> {
    let rebuild = |start: usize, end: usize, midi: u8| {
        let key = usize::from(midi - MIDI_OFFSET);
        NoteEvent {
            start_frame: start,
            end_frame: end,
            start: Posteriorgrams::frame_time(start),
            end: Posteriorgrams::frame_time(end),
            midi,
            amplitude: mean_activation(&post.note, key, start, end),
            bends: Some(pitch_bends(post, start, end, key)),
        }
    };

    // 1. Joins.
    notes.sort_by(|a, b| a.midi.cmp(&b.midi).then(a.start_frame.cmp(&b.start_frame)));
    let mut joined: Vec<NoteEvent> = Vec::with_capacity(notes.len());
    for note in notes {
        if let Some(prev) = joined.last_mut()
            && prev.midi == note.midi
            && note.start_frame <= prev.end_frame + 1
            && note.start_frame >= prev.start_frame
        {
            let key = usize::from(note.midi - MIDI_OFFSET);
            let at = note.start_frame;
            let near = at.saturating_sub(2)..(at + 3).min(post.frames);
            let onset = near
                .map(|t| post.onset[t * N_KEYS + key])
                .fold(0.0f32, f32::max);
            let held = at > 0 && post.note[(at - 1) * N_KEYS + key] >= params.frame_threshold;
            if onset < RESTRIKE_ONSET && held {
                *prev = rebuild(
                    prev.start_frame,
                    prev.end_frame.max(note.end_frame),
                    note.midi,
                );
                continue;
            }
        }
        joined.push(note);
    }

    // 2. Ghost partials.
    let covered = |ghost: &NoteEvent, by: &NoteEvent| {
        let overlap = ghost
            .end_frame
            .min(by.end_frame)
            .saturating_sub(ghost.start_frame.max(by.start_frame));
        overlap as f32 >= GHOST_COVER * (ghost.end_frame - ghost.start_frame) as f32
    };
    let ghosts: Vec<bool> = joined
        .iter()
        .map(|g| {
            joined.iter().any(|h| {
                g.midi > h.midi
                    && PARTIALS.contains(&(g.midi - h.midi))
                    && g.amplitude < GHOST_RATIO * h.amplitude
                    && covered(g, h)
            })
        })
        .collect();
    let kept: Vec<NoteEvent> = joined
        .into_iter()
        .zip(ghosts)
        .filter(|(_, g)| !g)
        .map(|(n, _)| n)
        .collect();

    // 3. Faint slivers.
    let faint = |n: &NoteEvent| n.amplitude < params.frame_threshold + FAINT_MARGIN;
    let touches = |a: &NoteEvent, b: &NoteEvent| {
        a.start_frame <= b.end_frame + 1 && b.start_frame <= a.end_frame + 1
    };
    let slivers: Vec<bool> = kept
        .iter()
        .map(|s| {
            faint(s)
                && kept.iter().any(|n| {
                    !std::ptr::eq(n, s)
                        && n.midi.abs_diff(s.midi) <= 2
                        && n.amplitude > s.amplitude
                        && n.end_frame - n.start_frame > s.end_frame - s.start_frame
                        && touches(s, n)
                })
        })
        .collect();
    let mut out: Vec<NoteEvent> = kept
        .into_iter()
        .zip(slivers)
        .filter(|(_, s)| !s)
        .map(|(n, _)| n)
        .collect();
    out.sort_by(|a, b| {
        a.start_frame
            .cmp(&b.start_frame)
            .then(a.midi.cmp(&b.midi))
            .then(a.end_frame.cmp(&b.end_frame))
    });
    out
}
