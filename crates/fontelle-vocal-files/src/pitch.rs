//! Per-note pitch curves (UTAU's and OpenUtau's) as the pitch-bend lane.
//!
//! UTAU's Mode2 and OpenUtau draw a note's pitch as points around it — in
//! milliseconds from the note's start, tenths of a semitone from its key —
//! and a note's points reach back over the note before it: the curve sung
//! at any moment is the **latest-starting** note whose points cover it. The
//! part here is notes as steps plus a pitch-bend lane added on top
//! (`fontelle_sing::Lane::PitchBend`), so the lane is that curve less the
//! step of the note sounding under it. Such a note's own glide is then
//! none: the drawn curve is the glide.

use fontelle_model::{LaneCurve, Note, NoteVoice, TempoMap};
use fontelle_sing::{GlideShape, Portamento};
use fontelle_types::Tick;

/// How a stretch between two points is drawn (OpenUtau's names).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Ease in and out.
    Io,
    Linear,
    /// Slow then arriving.
    In,
    /// Fast then settling.
    Out,
}

impl Shape {
    pub fn from_ustx(s: &str) -> Shape {
        match s {
            "l" => Shape::Linear,
            "i" => Shape::In,
            "o" => Shape::Out,
            _ => Shape::Io,
        }
    }

    pub fn ustx(self) -> &'static str {
        match self {
            Shape::Io => "io",
            Shape::Linear => "l",
            Shape::In => "i",
            Shape::Out => "o",
        }
    }

    /// UTAU's PBM letters: empty is the S curve, `s` straight, `r` and `j`
    /// the two halves.
    pub fn from_pbm(s: &str) -> Shape {
        match s.trim() {
            "s" => Shape::Linear,
            "r" => Shape::Out,
            "j" => Shape::In,
            _ => Shape::Io,
        }
    }

    pub fn glide(self) -> GlideShape {
        match self {
            Shape::Io => GlideShape::S,
            Shape::Linear => GlideShape::Linear,
            Shape::In => GlideShape::In,
            Shape::Out => GlideShape::Out,
        }
    }

    pub fn of_glide(glide: GlideShape) -> Shape {
        match glide {
            GlideShape::S => Shape::Io,
            GlideShape::Linear => Shape::Linear,
            GlideShape::In => Shape::In,
            GlideShape::Out => Shape::Out,
        }
    }

    /// 0..1 along, eased.
    fn ease(self, u: f64) -> f64 {
        let u = u.clamp(0.0, 1.0);
        match self {
            Shape::Io => (1.0 - (std::f64::consts::PI * u).cos()) / 2.0,
            Shape::Linear => u,
            Shape::In => 1.0 - (std::f64::consts::FRAC_PI_2 * u).cos(),
            Shape::Out => (std::f64::consts::FRAC_PI_2 * u).sin(),
        }
    }
}

/// One point of a note's curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Milliseconds from the note's start.
    pub ms: f64,
    /// Cents from the note's key.
    pub cents: f64,
    /// How the stretch to the next point is drawn.
    pub shape: Shape,
}

/// A note's drawn curve, by index into the part's notes.
#[derive(Debug, Clone)]
pub struct NoteCurve {
    pub note: usize,
    pub points: Vec<Point>,
}

/// The lane's resolution: ten ticks (about 5 ms at 120 bpm).
const STEP: Tick = 10;

/// The pitch-bend lane `curves` make over `notes` (sorted, one voice), as
/// `(tick, cents)` samples; also each curved note is given a glide of none.
/// `extra` is added on top (UTAU's Mode1 list, OpenUtau's PITD), `None`
/// where there is nothing.
pub fn bend_lane(
    notes: &mut [Note],
    curves: &[NoteCurve],
    tempo: &TempoMap,
    extra: &dyn Fn(Tick) -> f64,
    extra_span: Option<(Tick, Tick)>,
) -> Option<LaneCurve> {
    let at_ms = |tick: Tick, ms: f64| -> Tick {
        let seconds = tempo.tick_to_seconds(tick) + ms / 1000.0;
        tick_of_seconds(tempo, seconds)
    };
    // Each curve's span in ticks, and its points in ticks.
    struct Placed {
        note: usize,
        from: Tick,
        to: Tick,
        points: Vec<(Tick, f64, Shape)>,
    }
    let mut placed: Vec<Placed> = Vec::new();
    for curve in curves {
        let Some(note) = notes.get(curve.note) else {
            continue;
        };
        if curve.points.is_empty() {
            continue;
        }
        let points: Vec<(Tick, f64, Shape)> = curve
            .points
            .iter()
            .map(|p| (at_ms(note.start, p.ms), p.cents, p.shape))
            .collect();
        let from = points[0].0.min(note.start);
        let to = note.start + note.length;
        placed.push(Placed {
            note: curve.note,
            from,
            to,
            points,
        });
    }
    if placed.is_empty() && extra_span.is_none() {
        return None;
    }
    let sounding = |tick: Tick| -> Option<&Note> {
        notes
            .iter()
            .rev()
            .find(|n| n.start <= tick && tick < n.start + n.length)
    };
    let mut from = placed.iter().map(|p| p.from).min().unwrap_or(Tick::MAX);
    let mut to = placed.iter().map(|p| p.to).max().unwrap_or(Tick::MIN);
    if let Some((a, b)) = extra_span {
        // One sample past the last, so the lane comes back to the notes.
        from = from.min(a);
        to = to.max(b + STEP);
    }
    // On a grid, and either side of every note's start: the notes under
    // the lane step there, so the lane does too.
    let mut ticks: Vec<Tick> = (0..)
        .map(|i| from + i * STEP)
        .take_while(|t| *t <= to)
        .collect();
    for n in notes.iter() {
        if n.start > from && n.start <= to {
            ticks.push(n.start - 1);
            ticks.push(n.start);
        }
    }
    ticks.sort_unstable();
    ticks.dedup();
    let mut samples: Vec<(Tick, f32)> = Vec::new();
    for t in ticks {
        // The latest-starting note whose curve covers this tick.
        let curve = placed
            .iter()
            .filter(|p| p.from <= t && t < p.to)
            .max_by_key(|p| notes[p.note].start);
        let mut value = 0.0f64;
        if let Some(c) = curve {
            let key = f64::from(notes[c.note].key) * 100.0;
            let base = sounding(t).map_or(key, |n| f64::from(n.key) * 100.0);
            value = key + curve_at(&c.points, t) - base;
        }
        value += extra(t);
        samples.push((t, value as f32));
    }
    for c in &placed {
        let note = &mut notes[c.note];
        let mut voice = note.voice.take().map(|v| *v).unwrap_or_default();
        voice.portamento = Some(Portamento {
            length: 0.0,
            position: 0.0,
            shape: GlideShape::Linear,
        });
        note.voice = Some(Box::new(voice));
    }
    let curve = LaneCurve::from_samples(samples, 0.0);
    (!curve.is_empty()).then_some(curve)
}

fn curve_at(points: &[(Tick, f64, Shape)], t: Tick) -> f64 {
    let first = points[0];
    if t <= first.0 {
        return first.1;
    }
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t < b.0 {
            if b.0 <= a.0 {
                return b.1;
            }
            let u = (t - a.0) as f64 / (b.0 - a.0) as f64;
            return a.1 + (b.1 - a.1) * a.2.ease(u);
        }
    }
    points[points.len() - 1].1
}

/// The tick at `seconds` on `tempo`, unrounded through the sample grid.
pub fn tick_of_seconds(tempo: &TempoMap, seconds: f64) -> Tick {
    let rate = tempo.sample_rate_hz();
    tempo.sample_to_tick((seconds * rate).round() as i64)
}

/// A glide as two points round the note's start: OpenUtau's form of one.
pub fn glide_points(glide: &Portamento) -> [Point; 2] {
    let ms = f64::from(glide.length) * 1000.0;
    let start = -ms * (1.0 - f64::from(glide.position));
    let shape = Shape::of_glide(glide.shape);
    [
        Point {
            ms: start,
            cents: 0.0,
            shape,
        },
        Point {
            ms: start + ms,
            cents: 0.0,
            shape,
        },
    ]
}

/// The glide two flat points snapped to the note before describe, when
/// that is all they say.
pub fn glide_of(points: &[Point]) -> Option<Portamento> {
    let [a, b] = points else {
        return None;
    };
    if a.cents.abs() > 1e-6 || b.cents.abs() > 1e-6 || b.ms <= a.ms {
        return None;
    }
    let length = (b.ms - a.ms) / 1000.0;
    Some(Portamento {
        length: length as f32,
        // Ends on the note's start at 0, starts on it at 1.
        position: (b.ms / (b.ms - a.ms)).clamp(0.0, 1.0) as f32,
        shape: a.shape.glide(),
    })
}

/// Sets `note`'s voice through `edit`, leaving none when nothing is in it.
pub fn edit_voice(note: &mut Note, edit: impl FnOnce(&mut NoteVoice)) {
    let mut voice = note.voice.take().map(|v| *v).unwrap_or_default();
    edit(&mut voice);
    note.voice = (!voice.is_empty()).then(|| Box::new(voice));
}
