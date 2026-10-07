//! Pitch drawn on the grid of a vocal roll, and where the roll looks.
//!
//! OpenUtau, Synthesizer V and Vocaloid all let the pitch line be drawn
//! straight over the notes, and that is what a vocal producer reaches for
//! first: the pen follows the hand, Shift draws a straight line, the right
//! button (or the eraser) puts a stretch back on the notes. What is stored
//! is still the pitch-bend lane (cents, in ticks from the clip's start), so
//! a stroke is written as **where the hand is minus the pitch the notes,
//! glides and vibrato make** there — the drawn line is then exactly what
//! sounds, and moving a note later carries the drawn shape with it, which
//! is how all three behave.
//!
//! Smooth and Snap to scale are commands over a range (the selected notes,
//! else the time selection, else the whole part), each one undo step.
//! Snapping moves the drawn line's **held** stretches onto the nearest
//! scale degree and bends the moves between them to meet — the scoop into
//! a note and its vibrato are kept, only where it settles changes.
//!
//! Also here, because they are the same kind of arithmetic: framing a part
//! when it opens, following the lyric field as Tab walks it off screen,
//! and where the note menu opens from the keyboard. Pure; the window wires
//! it (`app/vocal.rs`).

use fontelle_model::{Arena, LaneCurve, Note};
use fontelle_types::{NoteId, Tick};

use super::piano_roll::{RollView, key_row, tick_to_x};
use super::vocal_edit::StrokeMode;
use crate::layout::Rect;

/// A command over the drawn pitch, on a range (see [`pitch_edit_range`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PitchCommand {
    /// Take the corners off ([`smooth_bend`]).
    Smooth,
    /// Held stretches onto the scale ([`snap_bend_to_scale`]).
    Snap,
    /// Back to the pitch the notes make.
    Reset,
}

impl PitchCommand {
    /// The note menu's row, and the undo entry's name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Smooth => "Smooth pitch",
            Self::Snap => "Snap pitch to scale",
            Self::Reset => "Reset pitch to the notes",
        }
    }
}

/// The pitch-bend lane's reach, in cents (`fontelle_model::lane_spec`).
const BEND_LIMIT: f32 = 1200.0;
/// Under this many cents a bend is nothing drawn.
const DRAWN_CENTS: f32 = 1.0;

/// The height of `pitch` (a MIDI key, cents as the fraction): a key's own
/// pitch is the middle of its row, as the notes' pitch line is drawn.
pub fn pitch_to_y(view: &RollView, grid: Rect, pitch: f32) -> f32 {
    grid.y
        + (f32::from(view.top_key) - pitch + view.key_offset) * view.key_height
        + view.key_height / 2.0
}

/// The pitch at height `y` — [`pitch_to_y`] backwards.
pub fn y_to_pitch(view: &RollView, grid: Rect, y: f32) -> f32 {
    if view.key_height <= 0.0 {
        return f32::from(view.top_key);
    }
    f32::from(view.top_key) + view.key_offset + 0.5 - (y - grid.y) / view.key_height
}

/// The pitch the notes make, the line a drawn pitch is measured from: the
/// computed line per phrase (`VocalView::note_line`), or the notes
/// themselves before anything has been sung. Between lines (a rest) the
/// nearer end holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PitchBase {
    lines: Vec<Vec<(Tick, f32)>>,
}

impl PitchBase {
    pub fn from_lines(lines: &[Vec<(Tick, f32)>]) -> Self {
        let mut lines: Vec<Vec<(Tick, f32)>> = lines
            .iter()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let mut l = l.clone();
                l.retain(|(_, k)| k.is_finite());
                l.sort_by_key(|(t, _)| *t);
                l
            })
            .filter(|l| !l.is_empty())
            .collect();
        lines.sort_by_key(|l| l[0].0);
        Self { lines }
    }

    /// Each note held flat across itself.
    pub fn from_notes(notes: &Arena<NoteId, Note>) -> Self {
        let lines: Vec<Vec<(Tick, f32)>> = notes
            .iter()
            .map(|(_, n)| {
                vec![
                    (n.start, f32::from(n.key)),
                    (n.start + n.length.max(1), f32::from(n.key)),
                ]
            })
            .collect();
        Self::from_lines(&lines)
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The pitch at `tick`; middle C with nothing to go on.
    pub fn at(&self, tick: Tick) -> f32 {
        let mut nearest: Option<(Tick, f32)> = None;
        for line in &self.lines {
            let (first, last) = (line[0], line[line.len() - 1]);
            if tick >= first.0 && tick <= last.0 {
                let i = line.partition_point(|(t, _)| *t <= tick);
                if i == 0 {
                    return first.1;
                }
                if i >= line.len() {
                    return last.1;
                }
                let (t0, v0) = line[i - 1];
                let (t1, v1) = line[i];
                if t1 == t0 {
                    return v1;
                }
                return v0 + (v1 - v0) * ((tick - t0) as f32 / (t1 - t0) as f32);
            }
            for (t, v) in [first, last] {
                let d = (t - tick).abs();
                if nearest.is_none_or(|(best, _)| d < best) {
                    nearest = Some((d, v));
                }
            }
        }
        nearest.map_or(60.0, |(_, v)| v)
    }
}

fn bend_for(base: &PitchBase, tick: Tick, pitch: f32) -> f32 {
    ((pitch - base.at(tick)) * 100.0).clamp(-BEND_LIMIT, BEND_LIMIT)
}

/// One stroke of the pitch pen, press to release: the bend lane as it
/// would be if the hand let go now.
#[derive(Debug, Clone)]
pub struct PitchStroke {
    mode: StrokeMode,
    base: PitchBase,
    before: LaneCurve,
    curve: LaneCurve,
    anchor: (Tick, f32),
    last: (Tick, f32),
    step: Tick,
}

impl PitchStroke {
    /// A stroke over the bend lane `bend` against `base`, starting at
    /// `(tick in the clip, pitch)`, a point every `step` ticks.
    /// [`StrokeMode::Erase`] puts what it crosses back on the notes.
    pub fn begin(
        base: PitchBase,
        bend: LaneCurve,
        mode: StrokeMode,
        at: (Tick, f32),
        step: Tick,
    ) -> Self {
        let mut stroke = Self {
            mode,
            base,
            curve: bend.clone(),
            before: bend,
            anchor: at,
            last: at,
            step: step.max(1),
        };
        match mode {
            StrokeMode::Pencil => stroke.segment(at, at),
            StrokeMode::Erase => stroke.curve.erase(at.0, at.0, 0.0),
            StrokeMode::Line => {}
        }
        stroke
    }

    fn segment(&mut self, a: (Tick, f32), b: (Tick, f32)) {
        let (from, to) = if a.0 <= b.0 { (a, b) } else { (b, a) };
        let run = (to.0 - from.0).max(1) as f32;
        let base = &self.base;
        self.curve.write(
            from.0,
            to.0,
            self.step,
            |t| {
                let pitch = from.1 + (to.1 - from.1) * ((t - from.0) as f32 / run);
                bend_for(base, t, pitch)
            },
            0.0,
        );
    }

    pub fn drag(&mut self, at: (Tick, f32)) {
        match self.mode {
            StrokeMode::Pencil => self.segment(self.last, at),
            StrokeMode::Line => {
                self.curve = self.before.clone();
                self.segment(self.anchor, at);
            }
            StrokeMode::Erase => {
                let (a, b) = (self.last.0.min(at.0), self.last.0.max(at.0));
                self.curve.erase(a, b, 0.0);
            }
        }
        self.last = at;
    }

    pub fn curve(&self) -> &LaneCurve {
        &self.curve
    }

    pub fn mode(&self) -> StrokeMode {
        self.mode
    }
}

/// Where a bend is drawn, as the pitch it makes: `(tick, pitch)` every
/// `step` from `from` to `to`, one run per stretch with a bend in it.
pub fn drawn_pitch(
    base: &PitchBase,
    bend: &LaneCurve,
    from: Tick,
    to: Tick,
    step: Tick,
) -> Vec<Vec<(Tick, f32)>> {
    let mut out = Vec::new();
    if bend.is_empty() {
        return out;
    }
    let step = step.max(1);
    let mut run: Vec<(Tick, f32)> = Vec::new();
    let mut t = from;
    loop {
        let cents = bend.at(t, 0.0);
        if cents.abs() > DRAWN_CENTS / 2.0 {
            run.push((t, base.at(t) + cents / 100.0));
        } else if !run.is_empty() {
            out.push(std::mem::take(&mut run));
        }
        if t >= to {
            break;
        }
        t = (t + step).min(to);
    }
    if !run.is_empty() {
        out.push(run);
    }
    out
}

/// The bend over `from..=to` with its corners taken off: each point the
/// weighted average (a triangle `radius` ticks either way) of the lane
/// around it. Outside the range nothing changes; applied again it smooths
/// further.
pub fn smooth_bend(bend: &LaneCurve, from: Tick, to: Tick, step: Tick, radius: Tick) -> LaneCurve {
    let mut out = bend.clone();
    if bend.is_empty() || to < from {
        return out;
    }
    let step = step.max(1);
    let radius = radius.max(step);
    let span = (radius + step) as f32;
    let average = |t: Tick| {
        let (mut sum, mut weight) = (0.0f32, 0.0f32);
        let mut k = -radius;
        while k <= radius {
            let w = 1.0 - k.abs() as f32 / span;
            sum += bend.at(t + k, 0.0) * w;
            weight += w;
            k += step;
        }
        sum / weight.max(f32::EPSILON)
    };
    out.write(from, to, step, average, 0.0);
    out
}

/// The scale degree nearest `pitch`: `mask` is the twelve pitch classes in
/// the scale, bit 0 for C (`RollScale::mask`). No scale is every semitone.
pub fn nearest_in_scale(pitch: f32, mask: u16) -> f32 {
    let mask = mask & 0x0fff;
    if mask == 0 || mask == 0x0fff {
        return pitch.round();
    }
    let centre = pitch.round() as i32;
    let mut best: Option<i32> = None;
    for k in centre - 12..=centre + 12 {
        if mask & (1 << k.rem_euclid(12)) == 0 {
            continue;
        }
        let closer = best.is_none_or(|b| (k as f32 - pitch).abs() < (b as f32 - pitch).abs());
        if closer {
            best = Some(k);
        }
    }
    best.map_or(pitch.round(), |k| k as f32)
}

/// How far either side the line is averaged to tell held from moving: a
/// vibrato cycle or so, so a vibrato reads as held.
const HELD_WINDOW_SECONDS: f64 = 0.1;
/// Slower than this (semitones a second, averaged) is held.
const HELD_SLOPE: f32 = 4.0;
/// A held stretch shorter than this is part of a move.
const HELD_MIN_SECONDS: f64 = 0.05;

/// The bend over `from..=to` with the drawn line's held stretches moved
/// onto the nearest degree of `mask`, and the moves between them bent to
/// meet. Only where a bend is drawn changes: a note written off the scale
/// on purpose, with nothing drawn over it, stays where it was written.
pub fn snap_bend_to_scale(
    base: &PitchBase,
    bend: &LaneCurve,
    from: Tick,
    to: Tick,
    step: Tick,
    mask: u16,
    ticks_per_second: f64,
) -> LaneCurve {
    let mut out = bend.clone();
    if bend.is_empty() || to <= from {
        return out;
    }
    let step = step.max(1);
    let tps = ticks_per_second.max(1.0);
    let mut ticks = Vec::new();
    let mut t = from;
    loop {
        ticks.push(t);
        if t >= to {
            break;
        }
        t = (t + step).min(to);
    }
    let n = ticks.len();
    let total: Vec<f32> = ticks
        .iter()
        .map(|t| base.at(*t) + bend.at(*t, 0.0) / 100.0)
        .collect();
    let drawn: Vec<bool> = ticks
        .iter()
        .map(|t| bend.at(*t, 0.0).abs() > DRAWN_CENTS)
        .collect();
    // The line averaged over about a vibrato cycle, and how fast that moves.
    let half = ((HELD_WINDOW_SECONDS * tps) / step as f64).round().max(1.0) as usize;
    let mut prefix = vec![0.0f64; n + 1];
    for i in 0..n {
        prefix[i + 1] = prefix[i] + f64::from(total[i]);
    }
    let centre: Vec<f32> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half + 1).min(n));
            ((prefix[b] - prefix[a]) / (b - a) as f64) as f32
        })
        .collect();
    let held: Vec<bool> = (0..n)
        .map(|i| {
            let (a, b) = (i.saturating_sub(1), (i + 1).min(n - 1));
            if a == b {
                return true;
            }
            let dt = (ticks[b] - ticks[a]).max(1) as f64 / tps;
            ((centre[b] - centre[a]).abs() as f64 / dt) < f64::from(HELD_SLOPE)
        })
        .collect();
    // Held runs, long enough to be a pitch someone settles on.
    let min_len = ((HELD_MIN_SECONDS * tps) / step as f64).ceil() as usize;
    let mut runs: Vec<(usize, usize, f32)> = Vec::new();
    let mut i = 0;
    while i < n {
        if !held[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && held[i] {
            i += 1;
        }
        if i - start < min_len.max(1) {
            continue;
        }
        let mut sung: Vec<f32> = (start..i).filter(|j| drawn[*j]).map(|j| total[j]).collect();
        let offset = if sung.is_empty() {
            0.0
        } else {
            sung.sort_by(|a, b| a.total_cmp(b));
            let median = sung[sung.len() / 2];
            nearest_in_scale(median, mask) - median
        };
        runs.push((start, i, offset));
    }
    if runs.is_empty() {
        return out;
    }
    // Each sample's move: its run's, or between two runs' along the way.
    let mut offsets = vec![0.0f32; n];
    for (k, &(a, b, offset)) in runs.iter().enumerate() {
        offsets[a..b].iter_mut().for_each(|o| *o = offset);
        let (gap_from, from_offset) = if k == 0 {
            (0, offset)
        } else {
            (runs[k - 1].1, runs[k - 1].2)
        };
        let gap = a - gap_from;
        for (i, o) in offsets[gap_from..a].iter_mut().enumerate() {
            let x = if k == 0 {
                1.0
            } else {
                (i + 1) as f32 / (gap + 1) as f32
            };
            *o = from_offset + (offset - from_offset) * x;
        }
    }
    let (_, last_end, last_offset) = runs[runs.len() - 1];
    offsets[last_end..]
        .iter_mut()
        .for_each(|o| *o = last_offset);
    let offset_at = |t: Tick| {
        let i = (((t - from) / step) as usize).min(n - 1);
        let j = (i + 1).min(n - 1);
        if i == j || ticks[j] == ticks[i] {
            return offsets[i];
        }
        let x = (t - ticks[i]) as f32 / (ticks[j] - ticks[i]) as f32;
        offsets[i] + (offsets[j] - offsets[i]) * x.clamp(0.0, 1.0)
    };
    out.write(
        from,
        to,
        step,
        |t| {
            let cents = bend.at(t, 0.0);
            if cents.abs() > DRAWN_CENTS {
                (cents + offset_at(t) * 100.0).clamp(-BEND_LIMIT, BEND_LIMIT)
            } else {
                cents
            }
        },
        0.0,
    );
    out
}

/// What a pitch command works on: the selected notes' span, else the time
/// selection (in clip ticks), else every note's.
pub fn pitch_edit_range(
    notes: &Arena<NoteId, Note>,
    selection: &[NoteId],
    time: Option<(Tick, Tick)>,
) -> Option<(Tick, Tick)> {
    let span = |ids: &mut dyn Iterator<Item = &Note>| {
        ids.fold(None, |acc: Option<(Tick, Tick)>, n| {
            let (a, b) = (n.start, n.start + n.length);
            Some(acc.map_or((a, b), |(x, y)| (x.min(a), y.max(b))))
        })
    };
    if let Some(range) = span(&mut selection.iter().filter_map(|id| notes.get(*id))) {
        return Some(range);
    }
    if let Some((a, b)) = time
        && b > a
    {
        return Some((a, b));
    }
    span(&mut notes.iter().map(|(_, n)| n))
}

/// The top key that puts `pitch` in the middle of `grid`, kept on the
/// keyboard.
fn top_for_middle(view: &RollView, grid: Rect, pitch: f32) -> u8 {
    let rows = grid.height / view.key_height.max(1.0);
    let top = (pitch - 0.5 + rows / 2.0).round();
    top.clamp(rows.floor().min(127.0), 127.0) as u8
}

/// The view that frames `notes` when their part opens: the first note a
/// little in from the left, the keys they use around the middle (or the
/// highest near the top, when they span more than the grid shows). The
/// zoom is the writer's; with no notes nothing moves.
pub fn frame_notes(view: &RollView, grid: Rect, notes: &Arena<NoteId, Note>) -> RollView {
    let mut out = *view;
    let Some(first) = notes.iter().map(|(_, n)| n.start).min() else {
        return out;
    };
    let lo = notes.iter().map(|(_, n)| n.key).min().unwrap_or(60);
    let hi = notes.iter().map(|(_, n)| n.key).max().unwrap_or(60);
    if view.pixels_per_tick > 0.0 {
        let margin = (grid.width * 0.08 / view.pixels_per_tick) as Tick;
        out.scroll_tick = (first - margin).max(0);
    }
    let rows = grid.height / view.key_height.max(1.0);
    out.key_offset = 0.0;
    out.top_key = if f32::from(hi - lo) + 3.0 > rows {
        (u16::from(hi) + 1).min(127) as u8
    } else {
        top_for_middle(view, grid, (f32::from(lo) + f32::from(hi)) / 2.0)
    };
    out
}

/// Room kept between a followed field and the grid's edge.
const FOLLOW_MARGIN: f32 = 16.0;

/// The view that brings the lyric field on `note` (`field_width` wide at
/// least) into sight — a quarter in from the left, leaving room for the
/// words to come — or `None` when it already is.
pub fn follow_note(view: &RollView, grid: Rect, note: &Note, field_width: f32) -> Option<RollView> {
    let mut out = *view;
    let x0 = tick_to_x(view, grid, note.start);
    let x1 = tick_to_x(view, grid, note.start + note.length);
    let width = (x1 - x0)
        .max(field_width)
        .min(grid.width - 2.0 * FOLLOW_MARGIN);
    if (x0 < grid.x + FOLLOW_MARGIN || x0 + width > grid.right() - FOLLOW_MARGIN)
        && view.pixels_per_tick > 0.0
    {
        let room = grid.width * 0.25;
        let lead = if width > grid.width - room - FOLLOW_MARGIN {
            FOLLOW_MARGIN
        } else {
            room
        };
        out.scroll_tick = (note.start - (lead / view.pixels_per_tick) as Tick).max(0);
    }
    let row = key_row(view, grid, note.key);
    if row.y < grid.y || row.bottom() > grid.bottom() {
        out.top_key = top_for_middle(view, grid, f32::from(note.key));
        out.key_offset = 0.0;
    }
    (out != *view).then_some(out)
}

/// Where the note menu opens from the keyboard: under the note's head,
/// kept on the grid.
pub fn note_menu_anchor(view: &RollView, grid: Rect, note: &Note) -> (f32, f32) {
    let x = tick_to_x(view, grid, note.start);
    let y = key_row(view, grid, note.key).bottom();
    (
        x.clamp(grid.x, (grid.right() - 1.0).max(grid.x)),
        y.clamp(grid.y, (grid.bottom() - 1.0).max(grid.y)),
    )
}
