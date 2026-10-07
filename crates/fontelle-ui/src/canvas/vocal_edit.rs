//! Editing a vocal part in the roll, beyond notes and words: the lanes
//! under the notes, the handles on a note's vibrato and glide, the note
//! menu, and the phoneme row.
//!
//! Built for someone who lives in OpenUtau, Vocaloid or Synthesizer V:
//! one lane at a time with a tab each (greyed, with the reason, when the
//! voice ignores it — what is drawn is kept for a voice that does); the
//! roll's own tools do the drawing (draw is a pencil, Shift a straight
//! line, the eraser or the right button returns a stretch to neutral); a
//! selected note shows its vibrato as a wave with handles above it (delay,
//! fade in, fade out, depth, rate) and its glide's start ahead of its head;
//! everything else is one menu away. Pure geometry and editing here; the
//! window wires it (`WindowApp`).

use fontelle_model::{Arena, LaneCurve, LaneKey, Note, NoteVoice};
use fontelle_sing::{Capabilities, GlideShape, Lane, PhonemeEdit, Portamento, Vibrato};
use fontelle_types::{NoteId, Tick};

use super::menu::MenuEntry;
use super::piano_roll::{RollLayout, RollView, tick_to_x};
use crate::layout::Rect;

/// The tabs across the top of the lane.
pub const VOCAL_TAB_HEIGHT: f32 = 18.0;
/// The phoneme row under the notes.
pub const PHONEME_ROW_HEIGHT: f32 = 18.0;
/// A handle's size.
pub const HANDLE_SIZE: f32 = 8.0;

/// A lane a vocal part can draw.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VocalCurve {
    Standard(Lane),
    /// One the voice declared, by its id.
    Extra(String),
}

impl VocalCurve {
    /// Where the part keeps it, for the backend `backend`.
    pub fn key(&self, backend: &str) -> LaneKey {
        match self {
            Self::Standard(lane) => LaneKey::Standard(*lane),
            Self::Extra(id) => LaneKey::Extra {
                backend: backend.to_string(),
                id: id.clone(),
            },
        }
    }
}

/// The strips a vocal roll takes for itself.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VocalStrips {
    /// Under the grid: what each note is sung as.
    pub phonemes: Rect,
    /// Across the top of the lane: which lane it shows. Empty while the lane
    /// is hidden.
    pub tabs: Rect,
}

/// Takes the phoneme row off the bottom of the grid (and the keys beside
/// it) and the tab strip off the top of the lane.
pub fn carve_vocal_strips(layout: &mut RollLayout) -> VocalStrips {
    let row = PHONEME_ROW_HEIGHT.min(layout.grid.height.max(0.0));
    layout.grid.height -= row;
    layout.keys.height = (layout.keys.height - row).max(0.0);
    let phonemes = Rect::new(layout.grid.x, layout.grid.bottom(), layout.grid.width, row);
    let tabs = if layout.velocity.is_empty() || layout.velocity.height <= VOCAL_TAB_HEIGHT * 2.0 {
        Rect::ZERO
    } else {
        let tabs = Rect::new(
            layout.velocity.x,
            layout.velocity.y,
            layout.velocity.width,
            VOCAL_TAB_HEIGHT,
        );
        layout.velocity.y += VOCAL_TAB_HEIGHT;
        layout.velocity.height -= VOCAL_TAB_HEIGHT;
        layout.velocity_keys.y += VOCAL_TAB_HEIGHT;
        layout.velocity_keys.height -= VOCAL_TAB_HEIGHT;
        tabs
    };
    VocalStrips { phonemes, tabs }
}

/// One tab of the lane.
#[derive(Debug, Clone, PartialEq)]
pub struct VocalTab {
    /// `None` is the notes' own lane (velocity — consonant speed — and the
    /// other note properties).
    pub curve: Option<VocalCurve>,
    pub label: String,
    /// Whether the voice uses it.
    pub honoured: bool,
    /// Why it is greyed, for the tooltip.
    pub why: Option<String>,
    pub rect: Rect,
}

/// The tabs: the notes' lane, the six standard lanes, then the voice's own.
pub fn vocal_tabs(
    strip: Rect,
    caps: &Capabilities,
    width_of: &dyn Fn(&str) -> f32,
) -> Vec<VocalTab> {
    let mut items: Vec<(Option<VocalCurve>, String, bool, Option<String>)> =
        vec![(None, "notes".to_string(), true, None)];
    for lane in Lane::ALL {
        let spec = fontelle_model::lane_spec(lane);
        let honoured = caps.lanes.contains(&lane);
        let why = (!honoured).then(|| {
            format!(
                "This voice ignores {} \u{2014} what you draw is kept for a voice that uses it",
                spec.name.to_lowercase()
            )
        });
        items.push((
            Some(VocalCurve::Standard(lane)),
            spec.short.to_string(),
            honoured,
            why,
        ));
    }
    for extra in &caps.extra_lanes {
        items.push((
            Some(VocalCurve::Extra(extra.id.clone())),
            extra.name.clone(),
            true,
            None,
        ));
    }
    let mut x = strip.x + 2.0;
    items
        .into_iter()
        .map(|(curve, label, honoured, why)| {
            let w = width_of(&label) + 14.0;
            let rect = Rect::new(x, strip.y + 1.0, w, (strip.height - 2.0).max(0.0));
            x += w + 2.0;
            VocalTab {
                curve,
                label,
                honoured,
                why,
                rect,
            }
        })
        .collect()
}

/// Which tab is at `(x, y)`.
pub fn vocal_tab_at(tabs: &[VocalTab], x: f32, y: f32) -> Option<usize> {
    tabs.iter().position(|t| t.rect.contains(x, y))
}

/// A lane's range, neutral and unit, for drawing and reading it.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveScale {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub neutral: f32,
    pub unit: String,
}

pub fn curve_scale(curve: &VocalCurve, caps: &Capabilities) -> CurveScale {
    match curve {
        VocalCurve::Standard(lane) => {
            let spec = fontelle_model::lane_spec(*lane);
            CurveScale {
                name: spec.name.to_string(),
                min: spec.min,
                max: spec.max,
                neutral: lane.neutral(),
                unit: spec.unit.to_string(),
            }
        }
        VocalCurve::Extra(id) => match caps.extra_lanes.iter().find(|l| &l.id == id) {
            Some(l) => CurveScale {
                name: l.name.clone(),
                min: l.min,
                max: l.max.max(l.min + f32::EPSILON),
                neutral: l.default,
                unit: l.unit.clone(),
            },
            None => CurveScale {
                name: id.clone(),
                min: 0.0,
                max: 1.0,
                neutral: 0.0,
                unit: String::new(),
            },
        },
    }
}

/// Where `value` sits in `plot`: the top of the range at the top.
pub fn curve_value_y(scale: &CurveScale, plot: Rect, value: f32) -> f32 {
    let t = ((value - scale.min) / (scale.max - scale.min)).clamp(0.0, 1.0);
    plot.bottom() - t * plot.height
}

/// The value at height `y` of `plot`, within the range.
pub fn curve_y_value(scale: &CurveScale, plot: Rect, y: f32) -> f32 {
    if plot.height <= 0.0 {
        return scale.neutral;
    }
    let t = ((plot.bottom() - y) / plot.height).clamp(0.0, 1.0);
    scale.min + t * (scale.max - scale.min)
}

/// How a stroke across a lane draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeMode {
    /// Follows the hand.
    Pencil,
    /// One straight line from where it began to where the hand is.
    Line,
    /// Returns what it crosses to neutral.
    Erase,
}

/// One stroke across a lane, from press to release.
#[derive(Debug, Clone)]
pub struct LaneStroke {
    mode: StrokeMode,
    base: LaneCurve,
    curve: LaneCurve,
    anchor: (Tick, f32),
    last: (Tick, f32),
    neutral: f32,
    step: Tick,
}

fn segment(curve: &mut LaneCurve, a: (Tick, f32), b: (Tick, f32), step: Tick, neutral: f32) {
    let (from, to) = if a.0 <= b.0 { (a, b) } else { (b, a) };
    let run = (to.0 - from.0).max(1) as f32;
    curve.write(
        from.0,
        to.0,
        step,
        |t| from.1 + (to.1 - from.1) * ((t - from.0) as f32 / run),
        neutral,
    );
}

impl LaneStroke {
    /// A stroke over `base` (the lane as it was) starting at `at` — `(tick
    /// in the clip, value)` — sampled every `step` ticks.
    pub fn begin(
        base: LaneCurve,
        mode: StrokeMode,
        at: (Tick, f32),
        neutral: f32,
        step: Tick,
    ) -> Self {
        let mut stroke = Self {
            mode,
            curve: base.clone(),
            base,
            anchor: at,
            last: at,
            neutral,
            step: step.max(1),
        };
        match mode {
            StrokeMode::Pencil => segment(&mut stroke.curve, at, at, stroke.step, neutral),
            StrokeMode::Erase => stroke.curve.erase(at.0, at.0, neutral),
            StrokeMode::Line => {}
        }
        stroke
    }

    pub fn drag(&mut self, at: (Tick, f32)) {
        match self.mode {
            StrokeMode::Pencil => segment(&mut self.curve, self.last, at, self.step, self.neutral),
            StrokeMode::Line => {
                self.curve = self.base.clone();
                segment(&mut self.curve, self.anchor, at, self.step, self.neutral);
            }
            StrokeMode::Erase => {
                let (a, b) = (self.last.0.min(at.0), self.last.0.max(at.0));
                self.curve.erase(a, b, self.neutral);
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

// ------------------------------------------------------------- handles

/// A handle on a note's expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteHandle {
    Depth,
    Rate,
    Delay,
    FadeIn,
    FadeOut,
    /// Where the glide in begins.
    Glide,
}

impl NoteHandle {
    /// What a drag of it does, for the status line.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Depth => "Vibrato depth \u{2014} drag up for more",
            Self::Rate => "Vibrato rate \u{2014} drag right for faster",
            Self::Delay => "Vibrato delay \u{2014} where in the note it starts",
            Self::FadeIn => "Vibrato fade in",
            Self::FadeOut => "Vibrato fade out",
            Self::Glide => "Glide in \u{2014} drag left for longer; Shift moves where it sits",
        }
    }
}

fn handle_at(x: f32, y: f32) -> Rect {
    Rect::new(
        x - HANDLE_SIZE / 2.0,
        y - HANDLE_SIZE / 2.0,
        HANDLE_SIZE,
        HANDLE_SIZE,
    )
}

/// What the glide in is when the note does not say: the backend's usual
/// (`fontelle_sing`'s default).
pub const DEFAULT_GLIDE: Portamento = Portamento {
    length: 0.08,
    position: 0.3,
    shape: GlideShape::S,
};

fn vibrato_span(note: Rect, v: &Vibrato) -> (f32, f32) {
    let x0 = note.x + note.width * v.delay.clamp(0.0, 0.95);
    (x0, note.right())
}

/// The handles a note shows: its vibrato's (when it has one) on a rail just
/// above it, and its glide's start ahead of its head (when the voice
/// glides). `px_per_second` places the glide, whose length is in seconds.
pub fn note_handles(
    note: Rect,
    voice: Option<&NoteVoice>,
    px_per_second: f32,
    glides: bool,
) -> Vec<(NoteHandle, Rect)> {
    let mut out = Vec::new();
    if let Some(v) = voice.and_then(|v| v.vibrato) {
        let (x0, x1) = vibrato_span(note, &v);
        let span = x1 - x0;
        let rail = note.y - HANDLE_SIZE * 0.75;
        let depth_px = v.depth / 100.0 * note.height;
        out.push((NoteHandle::Delay, handle_at(x0, rail)));
        out.push((NoteHandle::FadeIn, handle_at(x0 + span * v.fade_in, rail)));
        out.push((NoteHandle::FadeOut, handle_at(x1 - span * v.fade_out, rail)));
        out.push((
            NoteHandle::Depth,
            handle_at((x0 + x1) / 2.0, rail - depth_px - 4.0),
        ));
        out.push((NoteHandle::Rate, handle_at(x1, rail - depth_px - 4.0)));
    }
    if glides {
        let p = voice.and_then(|v| v.portamento).unwrap_or(DEFAULT_GLIDE);
        let ahead = p.length * px_per_second * (1.0 - p.position.clamp(0.0, 1.0));
        out.push((
            NoteHandle::Glide,
            handle_at(note.x - ahead, note.y + note.height / 2.0),
        ));
    }
    out
}

/// `start` with `handle` dragged by `(dx, dy)` pixels on a note drawn at
/// `note`.
pub fn drag_handle(
    handle: NoteHandle,
    start: &NoteVoice,
    dx: f32,
    dy: f32,
    note: Rect,
    px_per_second: f32,
) -> NoteVoice {
    let mut out = start.clone();
    if handle == NoteHandle::Glide {
        let p = start.portamento.unwrap_or(DEFAULT_GLIDE);
        let share = (1.0 - p.position.clamp(0.0, 1.0)).max(0.1);
        let length = p.length - dx / (px_per_second.max(1.0) * share);
        out.portamento = Some(Portamento {
            length: length.clamp(0.0, 1.0),
            ..p
        });
        return out;
    }
    let Some(v) = start.vibrato else {
        return out;
    };
    let (x0, x1) = vibrato_span(note, &v);
    let span = (x1 - x0).max(1.0);
    let mut n = v;
    match handle {
        NoteHandle::Depth => {
            n.depth = (v.depth - dy / note.height.max(1.0) * 100.0).clamp(0.0, 300.0)
        }
        NoteHandle::Rate => n.rate = (v.rate + dx / 12.0).clamp(1.0, 15.0),
        NoteHandle::Delay => n.delay = (v.delay + dx / note.width.max(1.0)).clamp(0.0, 0.95),
        NoteHandle::FadeIn => {
            n.fade_in = (v.fade_in + dx / span).clamp(0.0, (1.0 - v.fade_out).max(0.0))
        }
        NoteHandle::FadeOut => {
            n.fade_out = (v.fade_out - dx / span).clamp(0.0, (1.0 - v.fade_in).max(0.0))
        }
        NoteHandle::Glide => {}
    }
    out.vibrato = Some(n);
    out
}

/// Moves where a glide sits against its note (Shift on the glide handle):
/// right is later, onto the note.
pub fn drag_glide_position(start: &NoteVoice, dx: f32, px_per_second: f32) -> NoteVoice {
    let mut out = start.clone();
    let p = start.portamento.unwrap_or(DEFAULT_GLIDE);
    let len_px = (p.length * px_per_second).max(1.0);
    out.portamento = Some(Portamento {
        position: (p.position + dx / len_px).clamp(0.0, 1.0),
        ..p
    });
    out
}

/// A note's vibrato as a wave across it: `(x, y)` every two pixels, swinging
/// a row per hundred cents, faded as it is sung.
pub fn vibrato_wave(note: Rect, v: &Vibrato, px_per_second: f32) -> Vec<(f32, f32)> {
    let (x0, x1) = vibrato_span(note, v);
    let centre = note.y + note.height / 2.0;
    let span = (x1 - x0).max(1.0);
    let mut out = Vec::new();
    let mut x = x0;
    while x <= x1 {
        let along = (x - x0) / span;
        let fade_in = if v.fade_in > 0.0 {
            (along / v.fade_in).min(1.0)
        } else {
            1.0
        };
        let fade_out = if v.fade_out > 0.0 {
            ((1.0 - along) / v.fade_out).min(1.0)
        } else {
            1.0
        };
        let seconds = (x - x0) / px_per_second.max(1.0);
        let swing = (seconds * v.rate * std::f32::consts::TAU).sin();
        out.push((
            x,
            centre - v.depth / 100.0 * note.height * fade_in.min(fade_out) * swing,
        ));
        x += 2.0;
    }
    out
}

// ------------------------------------------------------------ note menu

/// What a row of the note menu does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VocalNoteRow {
    Info,
    /// Add (`true`) or take off a vibrato.
    Vibrato(bool),
    /// The glide's shape; `None` is the voice's own glide.
    Glide(Option<GlideShape>),
    /// Type the phonemes.
    EditPhonemes,
    /// Back to the voice's own reading.
    ResetPhonemes,
    /// A per-note option the voice declared.
    NoteOption {
        id: String,
        value: String,
    },
}

fn shape_name(shape: GlideShape) -> &'static str {
    match shape {
        GlideShape::S => "S curve",
        GlideShape::Linear => "straight",
        GlideShape::Out => "fast, then settling",
        GlideShape::In => "slow, then arriving",
    }
}

/// The note menu for a selection whose first note sings as `voice`.
pub fn vocal_note_menu_rows(
    voice: Option<&NoteVoice>,
    caps: &Capabilities,
    backend: &str,
) -> Vec<(MenuEntry, VocalNoteRow)> {
    let enabled = |e: MenuEntry, on: bool| {
        if on {
            e
        } else {
            MenuEntry::disabled(format!("{} (this voice ignores it)", e.label))
        }
    };
    let mut rows = vec![(MenuEntry::disabled("Note"), VocalNoteRow::Info)];
    let has_vibrato = voice.is_some_and(|v| v.vibrato.is_some());
    rows.push((
        enabled(
            MenuEntry::new(if has_vibrato {
                "Remove vibrato"
            } else {
                "Add vibrato"
            }),
            caps.vibrato,
        ),
        VocalNoteRow::Vibrato(!has_vibrato),
    ));
    let current = voice.and_then(|v| v.portamento).map(|p| p.shape);
    for shape in [
        GlideShape::S,
        GlideShape::Linear,
        GlideShape::Out,
        GlideShape::In,
    ] {
        let mark = if current == Some(shape) {
            "\u{2713} "
        } else {
            ""
        };
        rows.push((
            enabled(
                MenuEntry::new(format!("{mark}Glide in: {}", shape_name(shape))),
                caps.portamento,
            ),
            VocalNoteRow::Glide(Some(shape)),
        ));
    }
    rows.push((
        MenuEntry::new(if current.is_none() {
            "\u{2713} Glide in: the voice's own"
        } else {
            "Glide in: the voice's own"
        }),
        VocalNoteRow::Glide(None),
    ));
    rows.push((
        MenuEntry::new("Edit phonemes\u{2026}").after_rule(),
        VocalNoteRow::EditPhonemes,
    ));
    if voice.is_some_and(|v| v.phonemes.is_some()) {
        rows.push((
            MenuEntry::new("Phonemes: the voice's own reading"),
            VocalNoteRow::ResetPhonemes,
        ));
    }
    let chosen = voice.map(|v| v.options_for(backend)).unwrap_or_default();
    for option in &caps.note_options {
        rows.push((
            MenuEntry::disabled(option.name.clone()).after_rule(),
            VocalNoteRow::Info,
        ));
        for (i, (value, label)) in option.choices.iter().enumerate() {
            let on = chosen.get(&option.id).map_or(i == 0, |v| v == value);
            rows.push((
                MenuEntry::new(format!("{}{label}", if on { "\u{2713} " } else { "" })),
                VocalNoteRow::NoteOption {
                    id: option.id.clone(),
                    value: value.clone(),
                },
            ));
        }
    }
    rows
}

/// The voice a row makes of `voice`: `Some(new)` (where `None` is no voice
/// at all) when the row changes it; `None` when it is not that kind of row.
pub fn apply_note_row(
    row: &VocalNoteRow,
    voice: Option<&NoteVoice>,
    backend: &str,
) -> Option<Option<NoteVoice>> {
    let mut v = voice.cloned().unwrap_or_default();
    match row {
        VocalNoteRow::Info | VocalNoteRow::EditPhonemes => return None,
        VocalNoteRow::Vibrato(on) => v.vibrato = on.then(Vibrato::default),
        VocalNoteRow::Glide(shape) => {
            v.portamento = shape.map(|shape| Portamento {
                shape,
                ..v.portamento.unwrap_or(DEFAULT_GLIDE)
            })
        }
        VocalNoteRow::ResetPhonemes => v.phonemes = None,
        VocalNoteRow::NoteOption { id, value } => {
            v.options
                .entry(backend.to_string())
                .or_default()
                .insert(id.clone(), value.clone());
        }
    }
    Some((!v.is_empty()).then_some(v))
}

// ---------------------------------------------------------- phoneme row

/// One sung phoneme in the phoneme row.
#[derive(Debug, Clone, PartialEq)]
pub struct PhonemeCell {
    pub note: NoteId,
    pub index: usize,
    pub symbol: String,
    /// From where it starts to where the next starts (or the note ends).
    pub rect: Rect,
}

/// Where each sung phoneme sits: at its time when the singer said, else
/// shared out over its note.
pub fn phoneme_cells(
    row: Rect,
    view: &RollView,
    grid: Rect,
    notes: &Arena<NoteId, Note>,
    timed: &[(NoteId, Vec<(String, Option<f64>)>)],
    ticks_per_second: f64,
) -> Vec<PhonemeCell> {
    let mut out = Vec::new();
    for (id, phonemes) in timed {
        let Some(note) = notes.get(*id) else {
            continue;
        };
        if phonemes.is_empty() {
            continue;
        }
        let n = phonemes.len() as f64;
        let starts: Vec<f64> = phonemes
            .iter()
            .enumerate()
            .map(|(i, (_, offset))| match offset {
                Some(o) => note.start as f64 + o * ticks_per_second,
                None => note.start as f64 + note.length as f64 * i as f64 / n,
            })
            .collect();
        let end = (note.start + note.length) as f64;
        for (i, (symbol, _)) in phonemes.iter().enumerate() {
            let x0 = tick_to_x(view, grid, starts[i].round() as Tick);
            let next = starts.get(i + 1).copied().unwrap_or(end).max(starts[i]);
            let x1 = tick_to_x(view, grid, next.round() as Tick);
            out.push(PhonemeCell {
                note: *id,
                index: i,
                symbol: symbol.clone(),
                rect: Rect::new(x0, row.y, (x1 - x0).max(1.0), row.height),
            });
        }
    }
    out
}

/// What a press in the phoneme row lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhonemeHit {
    Cell {
        note: NoteId,
        index: usize,
    },
    /// A phoneme's start, to retime it (only where the voice honours
    /// timing).
    Boundary {
        note: NoteId,
        index: usize,
    },
}

pub fn phoneme_hit(cells: &[PhonemeCell], x: f32, y: f32, timing: bool) -> Option<PhonemeHit> {
    let row = cells.first()?.rect;
    if y < row.y || y >= row.bottom() {
        return None;
    }
    if timing && let Some(c) = cells.iter().find(|c| (x - c.rect.x).abs() <= 3.0) {
        return Some(PhonemeHit::Boundary {
            note: c.note,
            index: c.index,
        });
    }
    cells
        .iter()
        .find(|c| x >= c.rect.x && x < c.rect.right())
        .map(|c| PhonemeHit::Cell {
            note: c.note,
            index: c.index,
        })
}

/// Typed phonemes (`k a`, or `[k a]`); `None` for nothing typed — the
/// voice's own reading.
pub fn parse_phonemes(text: &str) -> Option<Vec<PhonemeEdit>> {
    let inner = text.trim().trim_start_matches('[').trim_end_matches(']');
    let edits: Vec<PhonemeEdit> = inner
        .split_whitespace()
        .map(|s| PhonemeEdit {
            symbol: s.to_string(),
            offset: None,
        })
        .collect();
    (!edits.is_empty()).then_some(edits)
}

/// The voice's phonemes that start with what is being typed (the last
/// word), shortest first.
pub fn phoneme_suggestions(inventory: &[String], typed: &str, limit: usize) -> Vec<String> {
    let last = typed.rsplit(char::is_whitespace).next().unwrap_or("");
    if last.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = inventory
        .iter()
        .filter(|p| p.starts_with(last))
        .cloned()
        .collect();
    out.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
    out.truncate(limit);
    out
}

/// The note's phonemes with phoneme `index` starting at `offset` (seconds
/// from the note's start), kept between its neighbours. Starts from the
/// writer's own edit when there is one, else from what was sung.
pub fn retime_phoneme(
    sung: &[(String, Option<f64>)],
    current: Option<&Vec<PhonemeEdit>>,
    index: usize,
    offset: f64,
) -> Vec<PhonemeEdit> {
    let mut edits: Vec<PhonemeEdit> = match current {
        Some(c) => c.clone(),
        None => sung
            .iter()
            .map(|(s, o)| PhonemeEdit {
                symbol: s.clone(),
                offset: *o,
            })
            .collect(),
    };
    let Some(_) = edits.get(index) else {
        return edits;
    };
    let lo = (0..index)
        .rev()
        .find_map(|i| edits[i].offset)
        .map_or(f64::NEG_INFINITY, |o| o + 0.005);
    let hi = edits[index + 1..]
        .iter()
        .find_map(|e| e.offset)
        .map_or(f64::INFINITY, |o| o - 0.005);
    edits[index].offset = Some(offset.clamp(lo.min(hi), hi.max(lo)));
    edits
}
