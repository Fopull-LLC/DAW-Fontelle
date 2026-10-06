//! The Analyze Musically window (`docs/analyze-musically-plan.md` §3.1–§3.6).
//!
//! Pure geometry and pure answers, like `canvas/tune.rs`: where everything
//! goes, what is under a point, what a press asks for and what every string
//! the window draws says. No `Scene`, no theme.
//!
//! The window is built from Flopsynth's parts. The **canopy is the
//! instrument**: one large screen inside it is the note lane — time across,
//! pitch up, the waveform (or the pitch picture) behind, the notes as blobs
//! on top — with the page tabs floating on the glass above it. The header
//! strip on the hull says the two confidences and the key; the consoles under
//! the canopy hold the note in hand and what to do with the notes.
//!
//! What it shows comes from the host as an [`AnalyzeView`] (plain data: the
//! UI never sees an analysis or a sample, INVARIANT 2); how it is looking at
//! it — the page, the zoom, the selection — is the window's own
//! [`AnalyzeState`].

use std::collections::BTreeSet;
use std::sync::Arc;

use fontelle_types::KeyScale;

use super::piano_roll::Modifiers;
use super::roll_scale::{RollScale, RowShade, row_shade};
use crate::layout::Rect;

// ------------------------------------------------------------- the view ---

/// Melody (one voice, pitch-tracked) or chords (every note heard).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalyzeMode {
    Melody,
    #[default]
    Chords,
}

impl AnalyzeMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Melody => "Melody",
            Self::Chords => "Chords",
        }
    }
}

/// How well notes could be pulled out at all (plan §2.6): the badge's word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzeClarity {
    Clear,
    Usable,
    Rough,
}

impl AnalyzeClarity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Usable => "Usable",
            Self::Rough => "Rough guess",
        }
    }
}

/// One note heard, in seconds from the start of the audio.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalyzedNote {
    pub start: f64,
    pub end: f64,
    /// The key it is nearest.
    pub midi: u8,
    /// Its centre, in cents from `midi`: what the "▲ 23ct" tag says.
    pub cents: f32,
    /// How loud, 0..1: the blob's height.
    pub amplitude: f32,
    /// How sure, 0..1: the blob's opacity.
    pub confidence: f32,
    /// One of several sounding at once (chords): it cannot be moved yet.
    pub poly: bool,
    /// The pitch through it: (seconds, cents from `midi`).
    pub curve: Vec<(f64, f32)>,
    /// What has been done to it, if anything (from the song's study).
    pub edit: Option<AnalyzeEdit>,
}

/// A note's pitch edit as the window sees it (`docs/analyze-musically-plan.md`
/// §2.4, §3.5): the host keeps it in the song as a `PitchEdit` over the
/// note's span of samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalyzeEdit {
    /// How far the note moves, in cents.
    pub shift_cents: f32,
    /// How much of its drift is taken out, 0..1 (F).
    pub flatten: f32,
    /// Its vibrato's depth as a factor of the sung one (V).
    pub vibrato: f32,
    /// How long the move takes to arrive and to leave (its ends dragged).
    pub glide_in_ms: f32,
    pub glide_out_ms: f32,
}

impl Default for AnalyzeEdit {
    fn default() -> Self {
        Self {
            shift_cents: 0.0,
            flatten: 0.0,
            vibrato: 1.0,
            glide_in_ms: fontelle_types::DEFAULT_GLIDE_MS,
            glide_out_ms: fontelle_types::DEFAULT_GLIDE_MS,
        }
    }
}

impl AnalyzeEdit {
    /// Whether it changes nothing that can be heard.
    pub fn is_identity(&self) -> bool {
        self.shift_cents.abs() < 0.05
            && self.flatten.abs() < 1e-3
            && (self.vibrato - 1.0).abs() < 1e-3
    }
}

/// One note's edit on its way to the host: the note's time and what it is
/// now (`None` takes the edit off: Del).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalyzeEditChange {
    pub start: f64,
    pub end: f64,
    pub edit: Option<AnalyzeEdit>,
}

/// One chord of the chord lane.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalyzedChord {
    pub start: f64,
    pub end: f64,
    pub label: String,
}

/// The key reading.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzeKey {
    pub key: KeyScale,
    /// 0..1.
    pub confidence: f32,
    /// The scale with the same notes on another tonic.
    pub relative: KeyScale,
    /// How sure the tonic is over the relative's, 0..1.
    pub tonic_confidence: f32,
    /// Other readings, best first.
    pub alternatives: Vec<KeyScale>,
}

/// The pitch picture behind the notes: `data[column * rows + row]`, row 0
/// lowest, `rows_per_semitone` rows a semitone from `lowest_midi` up.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalyzeImage {
    pub columns: usize,
    pub rows: usize,
    pub columns_per_second: f64,
    pub lowest_midi: f32,
    pub rows_per_semitone: f32,
    /// What most of the picture is: the level a cell has to rise over to be
    /// drawn at all ([`pitch_picture_floor`]). The model's contour is never
    /// zero, and drawn from zero the whole lane is a haze.
    pub floor: u8,
    pub data: Arc<[u8]>,
}

/// The pitch picture's noise floor: a little over the level most cells sit
/// at. basic-pitch's contour carries about a tenth of full scale in every
/// cell (73..89 of 255 on the test signals), and a note's ridge stands far
/// above it; the 60th percentile is still the floor in a dense chord (a
/// four-note chord and its partials light under a quarter of the rows), and
/// the margin takes the floor's own raggedness out.
///
/// Ty, on P1: *"right now it kind of just looks cloudy."*
pub fn pitch_picture_floor(data: &[u8]) -> u8 {
    if data.is_empty() {
        return 0;
    }
    let mut histogram = [0usize; 256];
    for v in data {
        histogram[usize::from(*v)] += 1;
    }
    let wanted = data.len() * 6 / 10;
    let mut seen = 0;
    let mut level = 0usize;
    for (value, count) in histogram.iter().enumerate() {
        seen += count;
        if seen > wanted {
            level = value;
            break;
        }
    }
    (level + 8).min(200) as u8
}

/// Everything the window shows, from the host.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalyzeView {
    /// The clip's name, for the title.
    pub name: String,
    /// Seconds of audio analysed.
    pub duration: f64,
    /// `Some(fraction)` while the analysis runs.
    pub analysing: Option<f32>,
    /// Why there is nothing to show, when there is not.
    pub error: Option<String>,
    /// What the analysis found it to be; `None` until it is done.
    pub detected: Option<AnalyzeMode>,
    /// The pitch-tracked notes (melody mode).
    pub melody: Vec<AnalyzedNote>,
    /// Every note heard (chords mode, and what arrives while analysing).
    pub notes: Vec<AnalyzedNote>,
    pub chords: Vec<AnalyzedChord>,
    pub key: Option<AnalyzeKey>,
    /// The badge: the word and the confidence.
    pub clarity: Option<(AnalyzeClarity, f32)>,
    /// Its one-line reason, for the tip.
    pub clarity_reason: String,
    /// How far the recording sits from A = 440, in cents.
    pub tuning_cents: Option<f32>,
    pub bpm: Option<f32>,
    /// The waveform: (min, max) per bucket, `peaks_per_second` a second.
    pub peaks: Arc<[(f32, f32)]>,
    pub peaks_per_second: f64,
    pub spectrogram: Option<AnalyzeImage>,
    /// The clip plays a render of the edits (Revert to original).
    pub rendered: bool,
    /// Edits made that the preview has not caught up with yet.
    pub preview_pending: bool,
}

// ------------------------------------------------------------ the state ---

/// The window's pages (plan §3.1). Only Notes does anything in P1; the others
/// are always there, so somebody looking for them finds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalyzePage {
    #[default]
    Notes,
    Clean,
    Slice,
    Record,
}

impl AnalyzePage {
    pub const ALL: [Self; 4] = [Self::Notes, Self::Clean, Self::Slice, Self::Record];

    /// The tab's caption, as Flopsynth's are written ("Synth", "Matrix"):
    /// capitals are for knobs' captions only (`flopsynth-next.md` §3.1).
    pub fn label(self) -> &'static str {
        match self {
            Self::Notes => "Notes",
            Self::Clean => "Clean",
            Self::Slice => "Slice",
            Self::Record => "Record",
        }
    }

    /// What the page will be, said on its card until it is.
    fn promise(self) -> &'static str {
        match self {
            Self::Notes => "",
            Self::Clean => "Noise capture and removal, trim, fades and gain",
            Self::Slice => {
                "Markers, slicing at transients or notes, and sending slices to a sampler"
            }
            Self::Record => "Recording takes straight into this window, and comping them",
        }
    }
}

/// How the window is looking at the analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzeState {
    pub page: AnalyzePage,
    /// The window's scale: one of Flopsynth's [`super::SCALES`].
    pub scale: f32,
    /// The pitch picture behind the notes rather than the waveform.
    pub spectrogram: bool,
    pub chords: bool,
    /// Out-of-scale rows dimmed (the ▾ menu's *Show scale on lane*).
    pub show_scale: bool,
    /// Melody or Chords chosen by hand; `None` follows the analysis.
    pub mode: Option<AnalyzeMode>,
    /// The Output card's *Keep slides and bends* (Ty, §6 answer 5: off).
    pub keep_bends: bool,
    /// Seconds at the lane's left edge, and pixels a second.
    pub start: f64,
    pub pixels_per_second: f32,
    /// The pitch at the grid's top edge (fractional MIDI), and pixels a row.
    pub top: f32,
    pub row_height: f32,
    /// What is under the pointer, for the popover and the lit chips.
    pub hover: Option<AnalyzeHit>,
    /// Move (drag notes to repitch them) or Select (drag to select).
    pub tool: AnalyzeTool,
    /// Where Space plays from, in seconds: a click on the ruler puts it.
    pub cursor: f64,
    /// Where the preview is, while it plays.
    pub playhead: Option<f64>,
    /// A stretch of the ruler dragged out: Space loops it.
    pub region: Option<(f64, f64)>,
    /// B: hearing the original rather than the edits.
    pub listen_original: bool,
    /// A note being dragged: its pitch, or one of its ends (the glide).
    drag: Option<NoteDrag>,
    /// A region being dragged out on the ruler, from here.
    ruler_from: Option<f64>,
    selection: BTreeSet<usize>,
    /// A marquee in progress: where it started, where it is, and what was
    /// selected before it (Shift adds to that).
    marquee: Option<Marquee>,
}

#[derive(Debug, Clone, PartialEq)]
struct Marquee {
    from: (f32, f32),
    to: (f32, f32),
    kept: BTreeSet<usize>,
}

impl Default for AnalyzeState {
    fn default() -> Self {
        Self {
            page: AnalyzePage::Notes,
            scale: 1.0,
            // Ty: *"the pitch picture is more useful to look at than the
            // waveform so lets make that the default"*.
            spectrogram: true,
            chords: true,
            show_scale: true,
            mode: None,
            keep_bends: false,
            start: 0.0,
            pixels_per_second: 80.0,
            top: 84.0,
            row_height: 14.0,
            hover: None,
            tool: AnalyzeTool::Move,
            cursor: 0.0,
            playhead: None,
            region: None,
            listen_original: false,
            drag: None,
            ruler_from: None,
            selection: BTreeSet::new(),
            marquee: None,
        }
    }
}

/// The lane's tools (plan §3.5): Move is what opens — Ty, on P1: *"i dont
/// have the ability to drag notes around in here"*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalyzeTool {
    Select,
    #[default]
    Move,
}

impl AnalyzeTool {
    pub const ALL: [Self; 2] = [Self::Select, Self::Move];

    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Move => "Move",
        }
    }
}

/// Which part of a note a press took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzeNotePart {
    /// The body: up and down is its pitch.
    Body,
    /// Its left or right end: how long the move takes to arrive or leave.
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq)]
struct NoteDrag {
    index: usize,
    part: AnalyzeNotePart,
    from: (f32, f32),
    /// Each dragged note's edit when the drag began.
    began: Vec<(usize, AnalyzeEdit)>,
    moved: bool,
}

/// What [`AnalyzeState::edit`] does to the selected notes (plan §3.5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyzeEditOp {
    /// ↑ ↓ (a semitone), with Shift an octave, with Alt ten cents.
    Nudge(f32),
    /// Q: to the nearest note, in the scale when it is shown; half way the
    /// first press, the whole way the next.
    Snap,
    /// F: the drift taken out, 70 %, or put back.
    Flatten,
    /// V: the vibrato as sung, half, none, round again.
    Vibrato,
    /// Del: as recorded.
    Reset,
}

/// What a chord note says when somebody tries to move it (P6 moves them).
pub const CHORD_CANT_MOVE: &str =
    "Chord notes can't be moved yet \u{2014} copy them to a piano roll";
/// What an edit key says with nothing selected.
pub const SELECT_FIRST: &str = "Select a note first (click it, or Ctrl+A for all)";

/// And the most, before a row is too thin to read.
const MIN_ROW_HEIGHT: f32 = 5.0;
const MAX_ROW_HEIGHT: f32 = 28.0;
/// Room above and below the notes when fitting, in semitones.
const FIT_MARGIN: f32 = 2.5;
/// The fewest rows a fit shows: two octaves, so a short phrase is not drawn
/// as three fat bars.
const FIT_ROWS: f32 = 24.0;
/// Zoom limits, pixels a second.
const MIN_PPS: f32 = 2.0;
const MAX_PPS: f32 = 2000.0;
const LOWEST: f32 = 12.0;
const HIGHEST: f32 = 120.0;

impl AnalyzeState {
    /// The state for another clip: the page, the window's scale and the
    /// switches kept, the zoom, the mode and the selection started over.
    pub fn for_another_clip(&self) -> Self {
        Self {
            page: self.page,
            scale: self.scale,
            spectrogram: self.spectrogram,
            chords: self.chords,
            show_scale: self.show_scale,
            keep_bends: self.keep_bends,
            tool: self.tool,
            ..Self::default()
        }
    }

    /// Melody or chords: the hand's choice, else the analysis's, else chords
    /// (what arrives while analysing).
    pub fn effective_mode(&self, view: &AnalyzeView) -> AnalyzeMode {
        self.mode.or(view.detected).unwrap_or(AnalyzeMode::Chords)
    }

    /// The notes the lane draws.
    pub fn notes<'a>(&self, view: &'a AnalyzeView) -> &'a [AnalyzedNote] {
        match self.effective_mode(view) {
            AnalyzeMode::Melody if view.detected.is_some() => &view.melody,
            _ => &view.notes,
        }
    }

    /// The selected notes' indices into [`notes`](Self::notes), in order.
    pub fn selected(&self) -> Vec<usize> {
        self.selection.iter().copied().collect()
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selection.contains(&index)
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    pub fn select_all(&mut self, view: &AnalyzeView) {
        self.selection = (0..self.notes(view).len()).collect();
    }

    /// A click on note `index`: it alone, or with Shift, in or out.
    pub fn click_note(&mut self, index: usize, shift: bool) {
        if shift {
            if !self.selection.remove(&index) {
                self.selection.insert(index);
            }
        } else {
            self.selection = std::iter::once(index).collect();
        }
    }

    /// Starts a marquee at `at`; without Shift the selection goes.
    pub fn begin_marquee(&mut self, at: (f32, f32), shift: bool) {
        if !shift {
            self.selection.clear();
        }
        self.marquee = Some(Marquee {
            from: at,
            to: at,
            kept: self.selection.clone(),
        });
    }

    /// The marquee dragged to `to`: what it touches, with what was kept.
    pub fn drag_marquee(&mut self, layout: &AnalyzeLayout, view: &AnalyzeView, to: (f32, f32)) {
        let Some(mut marquee) = self.marquee.take() else {
            return;
        };
        marquee.to = to;
        let area = marquee_rect(marquee.from, marquee.to);
        let mut selection = marquee.kept.clone();
        for index in 0..self.notes(view).len() {
            if let Some(blob) = layout.blob(view, self, index)
                && area.intersects(&blob)
            {
                selection.insert(index);
            }
        }
        self.selection = selection;
        self.marquee = Some(marquee);
    }

    pub fn end_marquee(&mut self) {
        self.marquee = None;
    }

    /// The marquee's rectangle, while there is one.
    pub fn marquee(&self) -> Option<Rect> {
        self.marquee.as_ref().map(|m| marquee_rect(m.from, m.to))
    }

    pub fn dragging_marquee(&self) -> bool {
        self.marquee.is_some()
    }

    // --------------------------------------------------- the transport ---

    /// What Space plays: from the cursor to the end — or, with a region,
    /// round the region (from the cursor when it is inside it).
    pub fn space_range(&self, view: &AnalyzeView) -> (f64, Option<f64>, bool) {
        match self.region {
            Some((a, b)) if b > a => {
                let from = if self.cursor >= a && self.cursor < b {
                    self.cursor
                } else {
                    a
                };
                (from, Some(b), true)
            }
            _ => (self.cursor.clamp(0.0, view.duration.max(0.0)), None, false),
        }
    }

    /// What Enter plays: the selected notes, first start to last end, once;
    /// with none selected, the region once.
    pub fn selection_range(&self, view: &AnalyzeView) -> Option<(f64, f64)> {
        let notes = self.notes(view);
        let picked: Vec<&AnalyzedNote> = self
            .selection
            .iter()
            .filter_map(|i| notes.get(*i))
            .collect();
        if picked.is_empty() {
            return self.region.filter(|(a, b)| b > a);
        }
        let start = picked.iter().map(|n| n.start).fold(f64::MAX, f64::min);
        let end = picked.iter().map(|n| n.end).fold(f64::MIN, f64::max);
        Some((start, end))
    }

    /// The playhead kept on screen while it plays: past the right edge (or
    /// before the left), the lane pages on so it sits a tenth in.
    pub fn follow(&mut self, lane: &AnalyzeLane, view: &AnalyzeView) {
        let Some(at) = self.playhead else {
            return;
        };
        let grid = lane.grid;
        if grid.is_empty() {
            return;
        }
        let x = lane.x_of(self, at);
        if x > grid.right() - 2.0 || x < grid.x {
            self.start = at - f64::from(grid.width * 0.1 / self.pixels_per_second.max(MIN_PPS));
            self.clamp(view, grid);
        }
    }

    /// A press on the ruler: the cursor there, and a region begun.
    pub fn press_ruler(&mut self, t: f64) {
        self.cursor = t.max(0.0);
        self.ruler_from = Some(self.cursor);
    }

    /// The ruler dragged: the region from where it was pressed (none until
    /// it is wider than a sliver).
    pub fn drag_ruler(&mut self, t: f64) -> bool {
        let Some(from) = self.ruler_from else {
            return false;
        };
        let (a, b) = (from.min(t.max(0.0)), from.max(t.max(0.0)));
        self.region = (b - a > 0.02).then_some((a, b));
        true
    }

    pub fn end_ruler(&mut self) {
        self.ruler_from = None;
    }

    // ------------------------------------------------------ the edits ---

    /// The selected notes' indices, refused if any is a chord note.
    fn editable(&self, view: &AnalyzeView) -> Result<Vec<usize>, String> {
        let notes = self.notes(view);
        let picked: Vec<usize> = self
            .selection
            .iter()
            .copied()
            .filter(|i| *i < notes.len())
            .collect();
        if picked.is_empty() {
            return Err(SELECT_FIRST.to_string());
        }
        if picked.iter().any(|i| notes[*i].poly) {
            return Err(CHORD_CANT_MOVE.to_string());
        }
        Ok(picked)
    }

    /// A key's edit of the selected notes (plan §3.5), as changes for the
    /// host; `Err` says why nothing changed.
    pub fn edit(
        &self,
        view: &AnalyzeView,
        op: AnalyzeEditOp,
    ) -> Result<Vec<AnalyzeEditChange>, String> {
        let notes = self.notes(view);
        let scale = self
            .show_scale
            .then(|| view.key.as_ref().and_then(|k| k.key.mask()))
            .flatten();
        let picked = self.editable(view)?;
        Ok(picked
            .into_iter()
            .map(|i| {
                let note = &notes[i];
                let was = note.edit.unwrap_or_default();
                let edit = match op {
                    AnalyzeEditOp::Nudge(cents) => Some(AnalyzeEdit {
                        shift_cents: was.shift_cents + cents,
                        ..was
                    }),
                    AnalyzeEditOp::Snap => {
                        let sung = f32::from(note.midi) * 100.0 + note.cents;
                        let target = nearest_note(sung + was.shift_cents, scale);
                        let full = target - sung;
                        let shift = if (was.shift_cents - full).abs() < 0.5
                            || (was.shift_cents - full * 0.5).abs() < 0.5
                        {
                            full
                        } else {
                            full * 0.5
                        };
                        Some(AnalyzeEdit {
                            shift_cents: shift,
                            ..was
                        })
                    }
                    AnalyzeEditOp::Flatten => Some(AnalyzeEdit {
                        flatten: if was.flatten > 0.0 { 0.0 } else { 0.7 },
                        ..was
                    }),
                    AnalyzeEditOp::Vibrato => Some(AnalyzeEdit {
                        vibrato: if was.vibrato > 0.75 {
                            0.5
                        } else if was.vibrato > 0.25 {
                            0.0
                        } else {
                            1.0
                        },
                        ..was
                    }),
                    AnalyzeEditOp::Reset => None,
                };
                AnalyzeEditChange {
                    start: note.start,
                    end: note.end,
                    edit: edit.filter(|e| !e.is_identity()),
                }
            })
            .collect())
    }

    /// A press on note `index` in the Move tool: it is selected (the
    /// selection kept when it is already in it) and a drag of `part`
    /// begins.
    pub fn begin_note_drag(
        &mut self,
        view: &AnalyzeView,
        index: usize,
        part: AnalyzeNotePart,
        at: (f32, f32),
    ) {
        if !self.selection.contains(&index) {
            self.selection = std::iter::once(index).collect();
        }
        let notes = self.notes(view);
        let indices: Vec<usize> = match part {
            AnalyzeNotePart::Body => self.selection.iter().copied().collect(),
            _ => vec![index],
        };
        let began = indices
            .into_iter()
            .filter(|i| notes.get(*i).is_some_and(|n| !n.poly))
            .map(|i| (i, notes[i].edit.unwrap_or_default()))
            .collect();
        self.drag = Some(NoteDrag {
            index,
            part,
            from: at,
            began,
            moved: false,
        });
    }

    pub fn dragging_note(&self) -> bool {
        self.drag.is_some()
    }

    /// The note a drag holds.
    pub fn dragged_note(&self) -> Option<usize> {
        self.drag.as_ref().map(|d| d.index)
    }

    /// The note drag carried to `to`: the dragged notes' edits now, for the
    /// host (`None` until it has moved). Up and down snaps the dragged
    /// note's pitch to semitones (`free`, Alt: to the cent) and moves the
    /// rest of the selection as far; an end sets that glide.
    pub fn drag_note(
        &mut self,
        lane: &AnalyzeLane,
        view: &AnalyzeView,
        to: (f32, f32),
        free: bool,
    ) -> Option<Result<Vec<AnalyzeEditChange>, String>> {
        let notes = self.notes(view).to_vec();
        let row = self.row_height;
        let t_at = lane.t_of(self, to.0);
        let drag = self.drag.as_mut()?;
        if !drag.moved && (to.0 - drag.from.0).abs() < 3.0 && (to.1 - drag.from.1).abs() < 3.0 {
            return None;
        }
        drag.moved = true;
        let held = notes.get(drag.index)?;
        if held.poly {
            return Some(Err(CHORD_CANT_MOVE.to_string()));
        }
        let changes = match drag.part {
            AnalyzeNotePart::Body => {
                let raw = (drag.from.1 - to.1) / row.max(MIN_ROW_HEIGHT) * 100.0;
                let base = drag
                    .began
                    .iter()
                    .find(|(i, _)| *i == drag.index)
                    .map_or(0.0, |(_, e)| e.shift_cents);
                let sung = f32::from(held.midi) * 100.0 + held.cents;
                let landed = sung + base + raw;
                let landed = if free {
                    landed
                } else {
                    (landed / 100.0).round() * 100.0
                };
                let delta = landed - sung - base;
                drag.began
                    .iter()
                    .map(|(i, was)| AnalyzeEditChange {
                        start: notes[*i].start,
                        end: notes[*i].end,
                        edit: Some(AnalyzeEdit {
                            shift_cents: was.shift_cents + delta,
                            ..*was
                        }),
                    })
                    .collect()
            }
            part => {
                let (_, was) = drag.began.first().copied()?;
                let length = ((held.end - held.start) * 1000.0) as f32;
                let mut edit = was;
                match part {
                    AnalyzeNotePart::Start => {
                        edit.glide_in_ms = (((t_at - held.start) * 1000.0) as f32)
                            .clamp(0.0, (length - edit.glide_out_ms).max(0.0));
                    }
                    _ => {
                        edit.glide_out_ms = (((held.end - t_at) * 1000.0) as f32)
                            .clamp(0.0, (length - edit.glide_in_ms).max(0.0));
                    }
                }
                vec![AnalyzeEditChange {
                    start: held.start,
                    end: held.end,
                    edit: Some(edit),
                }]
            }
        };
        Some(Ok(changes))
    }

    /// The drag let go: whether it moved anything (a press that did not is
    /// a click).
    pub fn end_note_drag(&mut self) -> bool {
        self.drag.take().is_some_and(|d| d.moved)
    }

    /// Every note on screen: the whole length across, the notes' range (two
    /// octaves at least) up the side. With no notes yet, around middle C.
    pub fn fit(&mut self, view: &AnalyzeView, layout: &AnalyzeLayout) {
        let grid = layout.lane.grid;
        if grid.is_empty() {
            return;
        }
        let seconds = view.duration.max(1.0);
        self.start = 0.0;
        self.pixels_per_second = (grid.width / seconds as f32).clamp(MIN_PPS, MAX_PPS);
        let notes = self.notes(view);
        let (lo, hi) = if notes.is_empty() {
            (54.0, 78.0)
        } else {
            notes.iter().fold((f32::MAX, f32::MIN), |(lo, hi), n| {
                let m = f32::from(n.midi) + n.cents / 100.0;
                (lo.min(m), hi.max(m))
            })
        };
        let mut lo = lo - FIT_MARGIN;
        let mut hi = hi + FIT_MARGIN;
        if hi - lo < FIT_ROWS {
            let middle = (lo + hi) / 2.0;
            lo = middle - FIT_ROWS / 2.0;
            hi = middle + FIT_ROWS / 2.0;
        }
        self.row_height = (grid.height / (hi - lo)).clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT);
        // Centred, whatever the clamp left over.
        let rows = grid.height / self.row_height;
        self.top = ((lo + hi) / 2.0 + rows / 2.0).clamp(LOWEST + rows, HIGHEST);
    }

    /// The wheel over the lane, as the roll has it: up and down the keys;
    /// Shift (or a sideways wheel) along time; Ctrl zooms time and Ctrl+Shift
    /// or Alt zooms pitch, about the pointer.
    #[allow(clippy::too_many_arguments)]
    pub fn wheel(
        &mut self,
        lane: &AnalyzeLane,
        view: &AnalyzeView,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        modifiers: Modifiers,
    ) {
        let grid = lane.grid;
        if grid.is_empty() {
            return;
        }
        let factor = 1.15_f32.powf(dy);
        if modifiers.ctrl && modifiers.shift || modifiers.alt {
            let at = lane.midi_of(self, y);
            self.row_height = (self.row_height * factor).clamp(MIN_ROW_HEIGHT, MAX_ROW_HEIGHT);
            // The pitch under the pointer stays under it.
            self.top = at + (y - grid.y) / self.row_height;
        } else if modifiers.ctrl {
            let at = lane.t_of(self, x);
            self.pixels_per_second = (self.pixels_per_second * factor).clamp(MIN_PPS, MAX_PPS);
            self.start = at - f64::from((x - grid.x) / self.pixels_per_second);
        } else if modifiers.shift || dx != 0.0 {
            let by = if dx != 0.0 { dx } else { dy };
            let px = super::wheel_travel(grid.width, by);
            self.start -= f64::from(px / self.pixels_per_second);
        } else {
            let px = super::wheel_travel(grid.height, dy);
            self.top += px / self.row_height;
        }
        self.clamp(view, grid);
    }

    /// Keeps the view on the audio and the keyboard.
    fn clamp(&mut self, view: &AnalyzeView, grid: Rect) {
        let visible = f64::from(grid.width / self.pixels_per_second.max(MIN_PPS));
        let last = (view.duration - visible * 0.5).max(0.0);
        self.start = self.start.clamp(0.0, last);
        let rows = grid.height / self.row_height.max(MIN_ROW_HEIGHT);
        self.top = self.top.clamp(LOWEST + rows.min(HIGHEST - LOWEST), HIGHEST);
    }
}

/// The nearest key to `cents` (MIDI cents), in cents — among the scale's
/// pitch classes when `scale` (a twelve-bit mask) is given.
pub fn nearest_note(cents: f32, scale: Option<u16>) -> f32 {
    let key = (cents / 100.0).round() as i32;
    let allowed = |k: i32| scale.is_none_or(|m| m & (1 << k.rem_euclid(12)) != 0);
    (0..=6)
        .flat_map(|d| [key - d, key + d])
        .filter(|k| allowed(*k))
        .min_by(|a, b| {
            ((*a * 100) as f32 - cents)
                .abs()
                .total_cmp(&((*b * 100) as f32 - cents).abs())
        })
        .map_or(key as f32 * 100.0, |k| k as f32 * 100.0)
}

fn marquee_rect(a: (f32, f32), b: (f32, f32)) -> Rect {
    Rect::new(
        a.0.min(b.0),
        a.1.min(b.1),
        (a.0 - b.0).abs(),
        (a.1 - b.1).abs(),
    )
}

// ------------------------------------------------------------- the lane ---

/// The note lane's parts, inside the canopy's screen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalyzeLane {
    /// The whole screen the lane is drawn on.
    pub screen: Rect,
    /// The chord names, along the top (empty when hidden).
    pub chords: Rect,
    /// The mini keyboard down the left.
    pub keys: Rect,
    /// Time across, pitch up.
    pub grid: Rect,
    /// Seconds, along the bottom.
    pub ruler: Rect,
}

impl AnalyzeLane {
    pub fn x_of(&self, state: &AnalyzeState, seconds: f64) -> f32 {
        self.grid.x + ((seconds - state.start) as f32) * state.pixels_per_second
    }

    pub fn t_of(&self, state: &AnalyzeState, x: f32) -> f64 {
        state.start + f64::from((x - self.grid.x) / state.pixels_per_second.max(MIN_PPS))
    }

    /// The y of pitch `midi`'s centre (fractional: 71.23 is 23 cents above
    /// B4's).
    pub fn y_of(&self, state: &AnalyzeState, midi: f32) -> f32 {
        self.grid.y + (state.top - midi) * state.row_height
    }

    pub fn midi_of(&self, state: &AnalyzeState, y: f32) -> f32 {
        state.top - (y - self.grid.y) / state.row_height.max(MIN_ROW_HEIGHT)
    }

    pub fn row_height(&self, state: &AnalyzeState) -> f32 {
        state.row_height
    }

    /// The keys with a row on screen, lowest first.
    pub fn visible_keys(&self, state: &AnalyzeState) -> std::ops::RangeInclusive<u8> {
        let low = self
            .midi_of(state, self.grid.bottom())
            .floor()
            .clamp(0.0, 127.0) as u8;
        let high = self.midi_of(state, self.grid.y).ceil().clamp(0.0, 127.0) as u8;
        low..=high
    }

    /// Row `key`'s band.
    pub fn row(&self, state: &AnalyzeState, key: u8) -> Rect {
        let y = self.y_of(state, f32::from(key) + 0.5);
        Rect::new(self.grid.x, y, self.grid.width, state.row_height)
    }
}

// ----------------------------------------------------------- the layout ---

/// The window's design size and its smallest, at scale 1 (plan §3.1:
/// Flopsynth's).
pub const ANALYZE_DESIGN: (u32, u32) = (1180, 740);
pub const ANALYZE_DESIGN_MINIMUM: (u32, u32) = (900, 600);

/// Design heights and widths, at scale 1.
const HEADER_H: f32 = 34.0;
const GAP: f32 = 8.0;
const TAB_H: f32 = 24.0;
const TAB_W: f32 = 78.0;
const TOGGLE_H: f32 = 22.0;
const CANOPY_INSET: f32 = 14.0;
const KEYS_W: f32 = 44.0;
const CHORDS_H: f32 = 22.0;
const RULER_H: f32 = 18.0;
const CARDS_H: f32 = 100.0;
const NOTE_CARD_W: f32 = 300.0;
const JOB_H: f32 = 22.0;
const CHIP_PAD: f32 = 10.0;
const ICON_W: f32 = 24.0;
const LAMP_W: f32 = 20.0;
const BUTTON_H: f32 = 26.0;

/// The two consoles under the canopy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzeCard {
    Note,
    Output,
}

impl AnalyzeCard {
    pub fn label(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Output => "Output",
        }
    }
}

/// A console: its frame and its nameplate.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AnalyzeCardLayout {
    pub frame: Rect,
    pub header: Rect,
    pub body: Rect,
}

/// Where everything ended up.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalyzeLayout {
    pub body: Rect,
    pub scale: f32,
    /// The strip across the top, on the hull.
    pub header: Rect,
    pub badge: Rect,
    pub scale_name: Rect,
    pub scale_menu: Rect,
    pub scale_copy: Rect,
    pub tuning: Rect,
    /// Empty without a tempo.
    pub bpm: Rect,
    pub mode_melody: Rect,
    pub mode_chords: Rect,
    /// The bridge's window, the tabs floating on it and the lane inside.
    pub canopy: Rect,
    pub tabs: Vec<(AnalyzePage, Rect)>,
    pub wave_toggle: Rect,
    pub chords_toggle: Rect,
    pub scale_toggle: Rect,
    pub window_scale: Rect,
    /// The tools and the transport, on the glass between the tabs and the
    /// switches.
    pub tools: Vec<(AnalyzeTool, Rect)>,
    pub play: Rect,
    pub listen: Rect,
    pub readout: Rect,
    pub lane: AnalyzeLane,
    /// The card a page that is not built yet shows, instead of the lane.
    pub later: Rect,
    pub note_card: AnalyzeCardLayout,
    pub output_card: AnalyzeCardLayout,
    pub copy_notes: Rect,
    pub keep_bends: Rect,
    pub make_clip: Rect,
    pub copy_scale: Rect,
    /// Render to clip, its ▾ (a new clip below), and Revert when the clip
    /// plays a render.
    pub render: Rect,
    pub render_menu: Rect,
    pub revert: Rect,
    /// The job strip, only while analysing.
    pub job: Rect,
    /// The popover under the scale chip, while it is hovered.
    pub popover: Rect,
}

/// What a string is drawn as — the bridge's three styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AnalyzeText {
    /// Tabs and card names.
    Heading,
    /// Chips, buttons and read-outs.
    Value,
    /// Captions: the key names, the ruler, the tags.
    Caption,
}

fn sc(value: f32, scale: f32) -> f32 {
    (value * scale).round()
}

/// The window, laid out in `body`. `measure` is a string's width in design
/// pixels as a value (the window's shaper; the tests' estimate).
pub fn analyze_layout(
    body: Rect,
    view: &AnalyzeView,
    state: &AnalyzeState,
    measure: &dyn Fn(&str) -> f32,
) -> AnalyzeLayout {
    let s = if state.scale.is_finite() && state.scale > 0.0 {
        state.scale
    } else {
        1.0
    };
    let mut l = AnalyzeLayout {
        body,
        scale: s,
        ..Default::default()
    };
    if body.is_empty() {
        return l;
    }
    let gap = sc(GAP, s);

    // The header strip: the badge, the key and its two icons, the tuning and
    // the tempo from the left; Melody | Chords at the right end.
    l.header = Rect::new(body.x, body.y, body.width, sc(HEADER_H, s));
    let chip_h = sc(HEADER_H - 8.0, s);
    let chip_y = l.header.y + (l.header.height - chip_h) / 2.0;
    let width_of = |text: &str| (measure(text) * s).ceil() + sc(CHIP_PAD, s) * 2.0;
    let mut x = l.header.x;
    let place = |w: f32, x: &mut f32, after: f32| {
        let r = Rect::new(*x, chip_y, w, chip_h);
        *x += w + after;
        r
    };
    l.badge = place(sc(LAMP_W, s) + width_of(&badge_text(view)), &mut x, gap);
    l.scale_name = place(width_of(&scale_chip_text(view)), &mut x, 0.0);
    l.scale_menu = place(sc(ICON_W, s), &mut x, 0.0);
    l.scale_copy = place(sc(ICON_W, s), &mut x, gap);
    l.tuning = place(width_of(&tuning_text(view)), &mut x, gap);
    if let Some(bpm) = bpm_text(view) {
        l.bpm = place(width_of(&bpm), &mut x, gap);
    }
    let mode_w = width_of("Melody").max(width_of("Chords"));
    l.mode_chords = Rect::new(l.header.right() - mode_w, chip_y, mode_w, chip_h);
    l.mode_melody = Rect::new(l.mode_chords.x - mode_w, chip_y, mode_w, chip_h);

    // Bottom up: the consoles, the job strip over them while analysing, and
    // the canopy takes what is left.
    let cards_h = sc(CARDS_H, s);
    let cards_y = body.bottom() - cards_h;
    let job_h = if view.analysing.is_some() {
        sc(JOB_H, s)
    } else {
        0.0
    };
    if job_h > 0.0 {
        l.job = Rect::new(body.x, cards_y - gap - job_h, body.width, job_h);
    }
    let canopy_bottom = if job_h > 0.0 { l.job.y } else { cards_y } - gap;
    let canopy_top = l.header.bottom() + gap;
    l.canopy = Rect::new(
        body.x,
        canopy_top,
        body.width,
        (canopy_bottom - canopy_top).max(0.0),
    );

    // The tabs on the glass, and the lane's switches at the other end.
    let inset = sc(CANOPY_INSET, s);
    let tab_y = l.canopy.y + sc(10.0, s);
    let tab_h = sc(TAB_H, s);
    let tab_w = sc(TAB_W, s);
    let mut tx = l.canopy.x + inset + sc(18.0, s);
    for page in AnalyzePage::ALL {
        l.tabs.push((page, Rect::new(tx, tab_y, tab_w, tab_h)));
        tx += tab_w + sc(4.0, s);
    }
    let toggle_h = sc(TOGGLE_H, s);
    let toggle_y = tab_y + (tab_h - toggle_h) / 2.0;
    let mut rx = l.canopy.right() - inset - sc(18.0, s);
    let toggle = |text: &str, rx: &mut f32| {
        let w = width_of(text);
        *rx -= w;
        let r = Rect::new(*rx, toggle_y, w, toggle_h);
        *rx -= sc(4.0, s);
        r
    };
    l.window_scale = toggle(&window_scale_label(s), &mut rx);
    rx -= sc(8.0, s);
    l.scale_toggle = toggle(SCALE_TOGGLE, &mut rx);
    l.chords_toggle = toggle(CHORDS_TOGGLE, &mut rx);
    l.wave_toggle = toggle(wave_toggle_text(state), &mut rx);

    // The tools and the transport, after the tabs.
    let mut gx = tx + sc(12.0, s);
    for tool in AnalyzeTool::ALL {
        let w = width_of(tool.label());
        l.tools.push((tool, Rect::new(gx, toggle_y, w, toggle_h)));
        gx += w + sc(4.0, s);
    }
    gx += sc(8.0, s);
    l.play = Rect::new(gx, toggle_y, sc(30.0, s), toggle_h);
    gx += l.play.width + sc(4.0, s);
    let listen_w = width_of(LISTEN_ORIGINAL);
    l.listen = Rect::new(gx, toggle_y, listen_w, toggle_h);
    gx += listen_w + sc(8.0, s);
    let readout_w = (measure(&readout_text(view, state)) * s).ceil() + sc(8.0, s);
    let room = rx - sc(8.0, s) - gx;
    if room >= readout_w {
        l.readout = Rect::new(gx, toggle_y, readout_w, toggle_h);
    }

    let screen = Rect::new(
        l.canopy.x + inset,
        tab_y + tab_h + sc(8.0, s),
        l.canopy.width - inset * 2.0,
        (l.canopy.bottom() - inset - (tab_y + tab_h + sc(8.0, s))).max(0.0),
    );
    if state.page == AnalyzePage::Notes {
        let chords_h = if state.chords { sc(CHORDS_H, s) } else { 0.0 };
        let keys_w = sc(KEYS_W, s);
        let ruler_h = sc(RULER_H, s);
        let pad = sc(4.0, s);
        let inner = screen.inset(pad);
        let grid = Rect::new(
            inner.x + keys_w,
            inner.y + chords_h,
            (inner.width - keys_w).max(0.0),
            (inner.height - chords_h - ruler_h).max(0.0),
        );
        l.lane = AnalyzeLane {
            screen,
            chords: if chords_h > 0.0 {
                Rect::new(grid.x, inner.y, grid.width, chords_h)
            } else {
                Rect::ZERO
            },
            keys: Rect::new(inner.x, grid.y, keys_w, grid.height),
            grid,
            ruler: Rect::new(grid.x, grid.bottom(), grid.width, ruler_h),
        };
    } else {
        l.lane = AnalyzeLane {
            screen,
            ..Default::default()
        };
        let w = (screen.width * 0.6).min(sc(560.0, s));
        let h = sc(120.0, s).min(screen.height);
        l.later = Rect::new(
            screen.x + (screen.width - w) / 2.0,
            screen.y + (screen.height - h) / 2.0,
            w,
            h,
        );
    }

    // The consoles.
    let header_h = sc(super::CARD_HEADER + 4.0, s);
    let card = |frame: Rect| AnalyzeCardLayout {
        frame,
        header: Rect::new(frame.x, frame.y, frame.width, header_h),
        body: Rect::new(
            frame.x + sc(10.0, s),
            frame.y + header_h + sc(4.0, s),
            (frame.width - sc(20.0, s)).max(0.0),
            (frame.height - header_h - sc(10.0, s)).max(0.0),
        ),
    };
    let note_w = sc(NOTE_CARD_W, s).min(body.width * 0.4);
    l.note_card = card(Rect::new(body.x, cards_y, note_w, cards_h));
    l.output_card = card(Rect::new(
        body.x + note_w + gap,
        cards_y,
        (body.width - note_w - gap).max(0.0),
        cards_h,
    ));
    let ob = l.output_card.body;
    let button_h = sc(BUTTON_H, s);
    let row1 = ob.y + sc(2.0, s);
    let row2 = row1 + button_h + sc(8.0, s);
    let mut bx = ob.x;
    let button = |text: &str, y: f32, bx: &mut f32| {
        let w = width_of(text) + sc(8.0, s);
        let r = Rect::new(*bx, y, w, button_h);
        *bx += w + gap;
        r
    };
    l.copy_notes = button(COPY_NOTES, row1, &mut bx);
    l.make_clip = button(MAKE_CLIP, row1, &mut bx);
    l.copy_scale = button(COPY_SCALE, row1, &mut bx);
    // Render at the right end, its ▾ joined to it; Revert under it.
    let render_w = width_of(RENDER_TO_CLIP) + sc(8.0, s);
    let menu_w = sc(ICON_W, s);
    l.render_menu = Rect::new(ob.right() - menu_w, row1, menu_w, button_h);
    l.render = Rect::new(l.render_menu.x - render_w, row1, render_w, button_h);
    if view.rendered {
        let w = width_of(REVERT) + sc(8.0, s);
        l.revert = Rect::new(ob.right() - w, row2, w, button_h);
    }
    let switch_w = sc(40.0, s) + width_of(KEEP_BENDS);
    l.keep_bends = Rect::new(ob.x, row2, switch_w, button_h);

    // The popover, under the scale chip while it is hovered.
    if matches!(
        state.hover,
        Some(AnalyzeHit::ScaleName | AnalyzeHit::ScaleCopy)
    ) && view.key.is_some()
    {
        let w = sc(300.0, s);
        let h = sc(164.0, s);
        let x = l.scale_name.x.min(body.right() - w);
        l.popover = Rect::new(x, l.header.bottom() + sc(4.0, s), w, h);
    }
    l
}

impl AnalyzeLayout {
    /// Every control and region with its name, for the tests' sweep and
    /// the tips: `header.*`, `tab.*`, `toggle.*`, `lane.*`, `card.*`.
    pub fn named(&self) -> Vec<(String, Rect)> {
        let mut out = vec![
            ("header.badge".to_string(), self.badge),
            ("header.scale".to_string(), self.scale_name),
            ("header.scale-menu".to_string(), self.scale_menu),
            ("header.scale-copy".to_string(), self.scale_copy),
            ("header.tuning".to_string(), self.tuning),
            ("header.melody".to_string(), self.mode_melody),
            ("header.chords".to_string(), self.mode_chords),
        ];
        if !self.bpm.is_empty() {
            out.push(("header.bpm".to_string(), self.bpm));
        }
        for (page, rect) in &self.tabs {
            out.push((format!("tab.{}", page.label().to_lowercase()), *rect));
        }
        out.extend([
            ("toggle.wave".to_string(), self.wave_toggle),
            ("toggle.chords".to_string(), self.chords_toggle),
            ("toggle.scale".to_string(), self.scale_toggle),
            ("toggle.window-scale".to_string(), self.window_scale),
            ("transport.play".to_string(), self.play),
            ("transport.listen".to_string(), self.listen),
            ("canopy".to_string(), self.canopy),
        ]);
        for (tool, rect) in &self.tools {
            out.push((format!("tool.{}", tool.label().to_lowercase()), *rect));
        }
        // Left out when a small window has no room for it.
        if !self.readout.is_empty() {
            out.push(("transport.readout".to_string(), self.readout));
        }
        if self.later.is_empty() {
            out.extend([
                ("lane.keys".to_string(), self.lane.keys),
                ("lane.grid".to_string(), self.lane.grid),
                ("lane.ruler".to_string(), self.lane.ruler),
            ]);
            if !self.lane.chords.is_empty() {
                out.push(("lane.chords".to_string(), self.lane.chords));
            }
        } else {
            out.push(("later".to_string(), self.later));
        }
        out.extend([
            ("card.note".to_string(), self.note_card.frame),
            ("card.output".to_string(), self.output_card.frame),
            ("card.output.copy-notes".to_string(), self.copy_notes),
            ("card.output.make-clip".to_string(), self.make_clip),
            ("card.output.copy-scale".to_string(), self.copy_scale),
            ("card.output.keep-bends".to_string(), self.keep_bends),
            ("card.output.render".to_string(), self.render),
            ("card.output.render-menu".to_string(), self.render_menu),
        ]);
        if !self.revert.is_empty() {
            out.push(("card.output.revert".to_string(), self.revert));
        }
        if !self.job.is_empty() {
            out.push(("job".to_string(), self.job));
        }
        out
    }

    /// Note `index`'s blob: across its time, centred on its true pitch, as
    /// tall as it is loud (a row at most). `None` when it is off screen.
    pub fn blob(&self, view: &AnalyzeView, state: &AnalyzeState, index: usize) -> Option<Rect> {
        let note = state.notes(view).get(index)?;
        let lane = &self.lane;
        if lane.grid.is_empty() {
            return None;
        }
        let x0 = lane.x_of(state, note.start);
        let x1 = lane.x_of(state, note.end).max(x0 + 2.0);
        let centre = lane.y_of(state, analyze_pitch(note));
        let h = state.row_height * (0.45 + 0.55 * note.amplitude.clamp(0.0, 1.0)).min(1.0);
        let rect = Rect::new(x0, centre - h / 2.0, x1 - x0, h);
        rect.intersects(&lane.grid).then_some(rect)
    }
}

// --------------------------------------------------------------- hits ---

/// Where a note is heard now, in MIDI (fractional): its sung centre and
/// its edit's move.
pub fn analyze_pitch(note: &AnalyzedNote) -> f32 {
    f32::from(note.midi) + (note.cents + note.edit.map_or(0.0, |e| e.shift_cents)) / 100.0
}

/// What is under a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyzeHit {
    Badge,
    ScaleName,
    ScaleMenu,
    ScaleCopy,
    Tuning,
    Bpm,
    Mode(AnalyzeMode),
    Tab(AnalyzePage),
    WaveToggle,
    ChordsToggle,
    ScaleToggle,
    WindowScale,
    /// A row of the mini keyboard.
    Key(u8),
    Chord(usize),
    Note(usize),
    /// Bare lane, at this time and pitch.
    Lane,
    Ruler,
    CopyNotes,
    MakeClip,
    CopyScaleButton,
    KeepBends,
    Job,
    Later,
    Tool(AnalyzeTool),
    Play,
    Listen,
    Readout,
    /// An end of a note that can be moved: dragging it sets that glide.
    NoteEnd(usize, AnalyzeNotePart),
    Render,
    RenderMenu,
    Revert,
}

pub fn analyze_hit(
    layout: &AnalyzeLayout,
    view: &AnalyzeView,
    state: &AnalyzeState,
    x: f32,
    y: f32,
) -> Option<AnalyzeHit> {
    let l = layout;
    let fixed = [
        (l.badge, AnalyzeHit::Badge),
        (l.scale_name, AnalyzeHit::ScaleName),
        (l.scale_menu, AnalyzeHit::ScaleMenu),
        (l.scale_copy, AnalyzeHit::ScaleCopy),
        (l.tuning, AnalyzeHit::Tuning),
        (l.bpm, AnalyzeHit::Bpm),
        (l.mode_melody, AnalyzeHit::Mode(AnalyzeMode::Melody)),
        (l.mode_chords, AnalyzeHit::Mode(AnalyzeMode::Chords)),
        (l.wave_toggle, AnalyzeHit::WaveToggle),
        (l.chords_toggle, AnalyzeHit::ChordsToggle),
        (l.scale_toggle, AnalyzeHit::ScaleToggle),
        (l.window_scale, AnalyzeHit::WindowScale),
        (l.copy_notes, AnalyzeHit::CopyNotes),
        (l.make_clip, AnalyzeHit::MakeClip),
        (l.copy_scale, AnalyzeHit::CopyScaleButton),
        (l.keep_bends, AnalyzeHit::KeepBends),
        (l.job, AnalyzeHit::Job),
        (l.later, AnalyzeHit::Later),
        (l.play, AnalyzeHit::Play),
        (l.listen, AnalyzeHit::Listen),
        (l.readout, AnalyzeHit::Readout),
        (l.render, AnalyzeHit::Render),
        (l.render_menu, AnalyzeHit::RenderMenu),
        (l.revert, AnalyzeHit::Revert),
    ];
    for (rect, hit) in fixed {
        if !rect.is_empty() && rect.contains(x, y) {
            return Some(hit);
        }
    }
    for (page, rect) in &l.tabs {
        if rect.contains(x, y) {
            return Some(AnalyzeHit::Tab(*page));
        }
    }
    for (tool, rect) in &l.tools {
        if rect.contains(x, y) {
            return Some(AnalyzeHit::Tool(*tool));
        }
    }
    let lane = &l.lane;
    if lane.grid.is_empty() {
        return None;
    }
    if lane.keys.contains(x, y) {
        let key = lane.midi_of(state, y).round().clamp(0.0, 127.0) as u8;
        return Some(AnalyzeHit::Key(key));
    }
    if lane.chords.contains(x, y) {
        let t = lane.t_of(state, x);
        return view
            .chords
            .iter()
            .position(|c| t >= c.start && t < c.end && !c.label.is_empty())
            .map(AnalyzeHit::Chord)
            .or(Some(AnalyzeHit::Lane));
    }
    if lane.ruler.contains(x, y) {
        return Some(AnalyzeHit::Ruler);
    }
    if lane.grid.contains(x, y) {
        // Last drawn is on top.
        for index in (0..state.notes(view).len()).rev() {
            if let Some(blob) = l.blob(view, state, index)
                && blob.inset(-1.0).contains(x, y)
            {
                // A moved note's ends, in the Move tool, are its glides.
                let note = &state.notes(view)[index];
                let edge = (5.0f32).min(blob.width / 4.0);
                if state.tool == AnalyzeTool::Move && !note.poly && note.edit.is_some() {
                    if x < blob.x + edge {
                        return Some(AnalyzeHit::NoteEnd(index, AnalyzeNotePart::Start));
                    }
                    if x > blob.right() - edge {
                        return Some(AnalyzeHit::NoteEnd(index, AnalyzeNotePart::End));
                    }
                }
                return Some(AnalyzeHit::Note(index));
            }
        }
        return Some(AnalyzeHit::Lane);
    }
    None
}

/// What a press asks the window to do. Selection and the marquee are the
/// state's own and are done here; everything that reaches the host or
/// another window comes back as an action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyzeAction {
    /// Copy the key as words, to the clipboard Tune and the roll paste from.
    CopyScale,
    /// Open the scale's ▾ menu.
    ScaleMenu,
    Mode(AnalyzeMode),
    Page(AnalyzePage),
    ToggleSpectrogram,
    ToggleChords,
    ToggleScaleRows,
    /// Open the window-size menu.
    WindowScale,
    /// Hear this key.
    Audition(u8),
    CopyNotes,
    MakeNoteClip,
    ToggleKeepBends,
    /// The selection changed.
    Selected,
    /// A marquee started; drag it with [`AnalyzeState::drag_marquee`].
    Marquee,
    Tool(AnalyzeTool),
    /// Play or stop, from the cursor (Space).
    PlayStop,
    /// A/B (B).
    ToggleOriginal,
    /// The cursor put here by a click on the ruler (a drag makes a region).
    Seek(f64),
    /// Hear note `index`'s own span of the audio, as edited; and in the
    /// Move tool, a drag of it has begun.
    PlayNote(usize),
    Render,
    RenderMenu,
    Revert,
}

pub fn analyze_press(
    layout: &AnalyzeLayout,
    view: &AnalyzeView,
    state: &mut AnalyzeState,
    x: f32,
    y: f32,
    modifiers: Modifiers,
) -> Option<AnalyzeAction> {
    let hit = analyze_hit(layout, view, state, x, y)?;
    Some(match hit {
        AnalyzeHit::ScaleName | AnalyzeHit::ScaleCopy | AnalyzeHit::CopyScaleButton => {
            AnalyzeAction::CopyScale
        }
        AnalyzeHit::ScaleMenu => AnalyzeAction::ScaleMenu,
        AnalyzeHit::Mode(mode) => AnalyzeAction::Mode(mode),
        AnalyzeHit::Tab(page) => AnalyzeAction::Page(page),
        AnalyzeHit::WaveToggle => AnalyzeAction::ToggleSpectrogram,
        AnalyzeHit::ChordsToggle => AnalyzeAction::ToggleChords,
        AnalyzeHit::ScaleToggle => AnalyzeAction::ToggleScaleRows,
        AnalyzeHit::WindowScale => AnalyzeAction::WindowScale,
        AnalyzeHit::Key(key) => AnalyzeAction::Audition(key),
        AnalyzeHit::CopyNotes => AnalyzeAction::CopyNotes,
        AnalyzeHit::MakeClip => AnalyzeAction::MakeNoteClip,
        AnalyzeHit::KeepBends => AnalyzeAction::ToggleKeepBends,
        AnalyzeHit::Note(index) => {
            if state.tool == AnalyzeTool::Move && !modifiers.shift {
                state.begin_note_drag(view, index, AnalyzeNotePart::Body, (x, y));
            } else {
                state.click_note(index, modifiers.shift);
            }
            AnalyzeAction::PlayNote(index)
        }
        AnalyzeHit::NoteEnd(index, part) => {
            state.begin_note_drag(view, index, part, (x, y));
            AnalyzeAction::Selected
        }
        AnalyzeHit::Tool(tool) => AnalyzeAction::Tool(tool),
        AnalyzeHit::Play => AnalyzeAction::PlayStop,
        AnalyzeHit::Listen => AnalyzeAction::ToggleOriginal,
        AnalyzeHit::Render => AnalyzeAction::Render,
        AnalyzeHit::RenderMenu => AnalyzeAction::RenderMenu,
        AnalyzeHit::Revert => AnalyzeAction::Revert,
        AnalyzeHit::Ruler => {
            let t = layout
                .lane
                .t_of(state, x)
                .clamp(0.0, view.duration.max(0.0));
            state.press_ruler(t);
            AnalyzeAction::Seek(t)
        }
        AnalyzeHit::Chord(index) => {
            // A chord's notes: everything sounding inside it.
            let chord = view.chords.get(index)?;
            let inside: Vec<usize> = state
                .notes(view)
                .iter()
                .enumerate()
                .filter(|(_, n)| n.start < chord.end && n.end > chord.start)
                .map(|(i, _)| i)
                .collect();
            if !modifiers.shift {
                state.clear_selection();
            }
            for i in inside {
                state.selection.insert(i);
            }
            AnalyzeAction::Selected
        }
        AnalyzeHit::Lane => {
            state.begin_marquee((x, y), modifiers.shift);
            AnalyzeAction::Marquee
        }
        AnalyzeHit::Badge
        | AnalyzeHit::Tuning
        | AnalyzeHit::Bpm
        | AnalyzeHit::Readout
        | AnalyzeHit::Job
        | AnalyzeHit::Later => return None,
    })
}

// ------------------------------------------------------------- strings ---

/// The captions of the switches on the glass.
pub const CHORDS_TOGGLE: &str = "Chords";
pub const SCALE_TOGGLE: &str = "Scale";
pub const WAVE_TOGGLE: &str = "Waveform";
pub const PITCH_TOGGLE: &str = "Pitch picture";
/// The Output card's buttons and switch.
pub const COPY_NOTES: &str = "Copy notes";
pub const MAKE_CLIP: &str = "Notes under the audio";
pub const COPY_SCALE: &str = "Copy scale";
pub const KEEP_BENDS: &str = "Keep slides and bends";
pub const RENDER_TO_CLIP: &str = "Render to clip";
pub const REVERT: &str = "Revert to original";
/// The A/B switch on the glass.
pub const LISTEN_ORIGINAL: &str = "A/B";
/// The page cards' line.
pub const LATER: &str = "coming in a later update";

/// The transport's read-out: where it is (the playhead while it plays,
/// else the cursor) of how long.
pub fn readout_text(view: &AnalyzeView, state: &AnalyzeState) -> String {
    format!(
        "{} / {}",
        time_text(state.playhead.unwrap_or(state.cursor), 1),
        time_text(view.duration, 1)
    )
}

/// A note's edit in words, for the note card: "moved +30 ct · drift
/// flattened 70 % · vibrato 50 %".
pub fn edit_text(edit: &AnalyzeEdit) -> String {
    let mut parts = vec![format!("moved {:+} ct", edit.shift_cents.round() as i32)];
    if edit.flatten > 0.0 {
        parts.push(format!("drift flattened {}", percent(edit.flatten)));
    }
    if (edit.vibrato - 1.0).abs() > 1e-3 {
        parts.push(format!("vibrato {}", percent(edit.vibrato)));
    }
    parts.join(" \u{b7} ")
}

/// The keys that edit a note, said on the note card under a sung one.
pub const EDIT_HINT: &str = "Drag or \u{2191}\u{2193} to move \u{b7} Q snap \u{b7} F flatten \u{b7} V vibrato \u{b7} Del reset";

/// What the window-size chip says: "100 %" — the words Flopsynth's chooser
/// uses (`render::scale_label`).
pub fn window_scale_label(scale: f32) -> String {
    format!("{} %", (scale * 100.0).round() as i32)
}

fn wave_toggle_text(state: &AnalyzeState) -> &'static str {
    if state.spectrogram {
        PITCH_TOGGLE
    } else {
        WAVE_TOGGLE
    }
}

fn percent(value: f32) -> String {
    format!("{} %", (value.clamp(0.0, 1.0) * 100.0).round() as i32)
}

/// The badge: "Clear 91 %", or what it is doing before there is one.
pub fn badge_text(view: &AnalyzeView) -> String {
    match view.clarity {
        Some((clarity, confidence)) => format!("{} {}", clarity.label(), percent(confidence)),
        None if view.error.is_some() => "Not analysed".to_string(),
        None => "Listening\u{2026}".to_string(),
    }
}

/// The key chip: "A minor 82 %" (the roll's words for it, so what is
/// copied is what is read).
pub fn scale_chip_text(view: &AnalyzeView) -> String {
    match &view.key {
        Some(key) => format!(
            "{} {}",
            fontelle_types::scale_text(&key.key),
            percent(key.confidence)
        ),
        None if view.analysing.is_some() => "Key: listening\u{2026}".to_string(),
        None => "No key heard".to_string(),
    }
}

pub fn tuning_text(view: &AnalyzeView) -> String {
    match view.tuning_cents {
        Some(cents) => format!("Tuning {:+} ct", cents.round() as i32),
        None => "Tuning \u{2014}".to_string(),
    }
}

pub fn bpm_text(view: &AnalyzeView) -> Option<String> {
    view.bpm.map(|bpm| format!("{} BPM", bpm.round() as i32))
}

/// "▲ 23ct" over a note more than 15 cents sharp, "▼ 18ct" flat.
pub fn analyze_cents_tag(note: &AnalyzedNote) -> Option<String> {
    let cents = note.cents.round() as i32;
    if cents.abs() <= 15 {
        return None;
    }
    let arrow = if cents > 0 { '\u{25b2}' } else { '\u{25bc}' };
    Some(format!("{arrow} {}ct", cents.abs()))
}

/// A key as the roll names it: "C4" for MIDI 60.
pub fn key_name(key: u8) -> String {
    format!(
        "{}{}",
        fontelle_types::PITCH_NAMES[usize::from(key % 12)],
        i32::from(key) / 12 - 1
    )
}

/// Seconds as the ruler and the note card say them: "0:04.21".
pub fn time_text(seconds: f64, decimals: usize) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor();
    let rest = seconds - minutes * 60.0;
    let width = if decimals == 0 { 2 } else { 3 + decimals };
    format!("{}:{rest:0width$.decimals$}", minutes as i64)
}

/// The scale's notes and their degrees against the major scale:
/// A minor is A B C D E F G and 1 2 ♭3 4 5 ♭6 ♭7.
pub fn scale_degrees(key: &KeyScale) -> (Vec<String>, Vec<String>) {
    const DEGREES: [&str; 12] = [
        "1",
        "\u{266d}2",
        "2",
        "\u{266d}3",
        "3",
        "4",
        "\u{266d}5",
        "5",
        "\u{266d}6",
        "6",
        "\u{266d}7",
        "7",
    ];
    let Some(scale) = fontelle_types::scale(&key.scale) else {
        return (Vec::new(), Vec::new());
    };
    scale
        .steps
        .iter()
        .map(|step| {
            (
                fontelle_types::PITCH_NAMES[usize::from((key.root % 12 + step) % 12)].to_string(),
                DEGREES[usize::from(step % 12)].to_string(),
            )
        })
        .unzip()
}

/// The popover's lines: the key, the other readings and what a click does.
/// The notes and their degrees are drawn under their keys (from
/// [`scale_degrees`]).
pub fn popover_lines(view: &AnalyzeView) -> Vec<String> {
    let Some(key) = &view.key else {
        return Vec::new();
    };
    let mut out = vec![fontelle_types::scale_text(&key.key)];
    let mut others = vec![format!(
        "or {} (relative)",
        fontelle_types::scale_text(&key.relative)
    )];
    others.extend(
        key.alternatives
            .iter()
            .filter(|alt| **alt != key.relative && **alt != key.key)
            .take(2)
            .map(fontelle_types::scale_text),
    );
    out.push(others.join("  \u{b7}  "));
    out.push("Click to copy \u{2014} paste into Tune or the piano roll".to_string());
    out
}

/// The note the note card describes: the one under the pointer, else the
/// one selected when it is one.
pub fn note_card_focus(view: &AnalyzeView, state: &AnalyzeState) -> Option<usize> {
    let hovered = match state.hover {
        Some(AnalyzeHit::Note(i)) => Some(i),
        _ => None,
    };
    let selected = state.selected();
    hovered
        .or_else(|| (selected.len() == 1).then(|| selected[0]))
        .filter(|i| *i < state.notes(view).len())
}

/// The note card's lines for what is selected (or under the pointer).
pub fn note_card_lines(view: &AnalyzeView, state: &AnalyzeState) -> Vec<String> {
    let notes = state.notes(view);
    let selected = state.selected();
    if let Some(note) = note_card_focus(view, state).and_then(|i| notes.get(i)) {
        // Where it is heard now: a moved note says where it went.
        let now = analyze_pitch(note) * 100.0;
        let key = (now / 100.0).round().clamp(0.0, 127.0);
        let mut out = vec![
            format!(
                "{}  {:+} ct",
                key_name(key as u8),
                (now - key * 100.0).round() as i32
            ),
            format!(
                "{} \u{2013} {}",
                time_text(note.start, 2),
                time_text(note.end, 2)
            ),
            format!("confidence {}", percent(note.confidence)),
        ];
        if note.poly {
            out.push("Chord notes can't be moved yet \u{2014} copy them".to_string());
        } else if let Some(edit) = &note.edit {
            out.push(format!(
                "{}, sung {} {:+} ct",
                edit_text(edit),
                key_name(note.midi),
                note.cents.round() as i32
            ));
        } else {
            out.push(EDIT_HINT.to_string());
        }
        return out;
    }
    if notes.is_empty() {
        return vec![if view.analysing.is_some() {
            "Notes appear here as they're found".to_string()
        } else if state.effective_mode(view) == AnalyzeMode::Melody && view.detected.is_some() {
            "No single voice to follow \u{2014} try Chords".to_string()
        } else {
            "No notes heard".to_string()
        }];
    }
    vec![
        match selected.len() {
            0 => format!("{} notes \u{2014} click one to see it", notes.len()),
            n => format!("{n} of {} notes selected", notes.len()),
        },
        "Drag on the lane to select; Ctrl+A for all".to_string(),
    ]
}

/// What the job strip says.
pub fn job_text(view: &AnalyzeView) -> String {
    format!(
        "Analysing\u{2026} {}  (notes appear as they're found)",
        percent(view.analysing.unwrap_or(0.0))
    )
}

/// What a page that is not built yet says.
pub fn later_lines(page: AnalyzePage) -> [String; 2] {
    [
        format!("{} \u{2014} {LATER}", page.label()),
        page.promise().to_string(),
    ]
}

/// The ruler's step, in seconds, for labels about 70 px apart.
pub fn ruler_step(pixels_per_second: f32) -> f64 {
    const STEPS: [f64; 11] = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0];
    STEPS
        .into_iter()
        .find(|step| *step as f32 * pixels_per_second >= 70.0)
        .unwrap_or(300.0)
}

/// The ruler's labels on screen: (x, text).
pub fn ruler_labels(
    layout: &AnalyzeLayout,
    view: &AnalyzeView,
    state: &AnalyzeState,
) -> Vec<(f32, String)> {
    let lane = &layout.lane;
    if lane.ruler.is_empty() {
        return Vec::new();
    }
    let step = ruler_step(state.pixels_per_second);
    let decimals = if step < 1.0 { 1 } else { 0 };
    let first = (state.start / step).ceil() as i64;
    let last_t = lane
        .t_of(state, lane.grid.right())
        .min(view.duration.max(0.0));
    let mut out = Vec::new();
    let mut i = first;
    loop {
        let t = i as f64 * step;
        if t > last_t + 1e-9 {
            break;
        }
        out.push((lane.x_of(state, t), time_text(t, decimals)));
        i += 1;
        if out.len() > 200 {
            break;
        }
    }
    out
}

/// Every string the window draws, with the style it is drawn in: the window
/// shapes from this list and the renderer draws from the same functions, so
/// a caption cannot be drawn before it has been shaped.
pub fn analyze_strings(
    view: &AnalyzeView,
    state: &AnalyzeState,
    layout: &AnalyzeLayout,
) -> Vec<(String, AnalyzeText)> {
    use AnalyzeText::{Caption, Heading, Value};
    let mut out: Vec<(String, AnalyzeText)> = vec![
        (badge_text(view), Value),
        (scale_chip_text(view), Value),
        (tuning_text(view), Value),
        ("Melody".to_string(), Value),
        ("Chords".to_string(), Value),
        (CHORDS_TOGGLE.to_string(), Value),
        (SCALE_TOGGLE.to_string(), Value),
        (wave_toggle_text(state).to_string(), Value),
        (window_scale_label(state.scale), Value),
        (COPY_NOTES.to_string(), Value),
        (MAKE_CLIP.to_string(), Value),
        (COPY_SCALE.to_string(), Value),
        (RENDER_TO_CLIP.to_string(), Value),
        (REVERT.to_string(), Value),
        (LISTEN_ORIGINAL.to_string(), Value),
        (readout_text(view, state), Value),
        (AnalyzeTool::Select.label().to_string(), Value),
        (AnalyzeTool::Move.label().to_string(), Value),
        (KEEP_BENDS.to_string(), Caption),
        (AnalyzeCard::Note.label().to_string(), Heading),
        (AnalyzeCard::Output.label().to_string(), Heading),
        (time_text(view.duration, 1), Caption),
    ];
    out.extend(bpm_text(view).map(|t| (t, Value)));
    for page in AnalyzePage::ALL {
        out.push((page.label().to_string(), Heading));
    }
    // The note card's first line is its heading: the note itself.
    let focused = note_card_focus(view, state).is_some();
    for (i, line) in note_card_lines(view, state).into_iter().enumerate() {
        out.push((line, if i == 0 && focused { Heading } else { Value }));
    }
    if view.analysing.is_some() {
        out.push((job_text(view), Value));
    }
    if let Some(error) = &view.error {
        out.push((error.clone(), Value));
    }
    if state.page != AnalyzePage::Notes {
        for line in later_lines(state.page) {
            out.push((line, Value));
        }
    }
    if !layout.popover.is_empty() {
        out.extend(popover_lines(view).into_iter().map(|l| (l, Value)));
        if let Some(key) = &view.key {
            let (names, degrees) = scale_degrees(&key.key);
            out.extend(names.into_iter().map(|n| (n, Value)));
            out.extend(degrees.into_iter().map(|d| (d, Caption)));
        }
    }
    let lane = &layout.lane;
    if !lane.grid.is_empty() {
        for key in lane.visible_keys(state) {
            if key % 12 == 0 {
                out.push((key_name(key), Caption));
            }
        }
        out.extend(
            ruler_labels(layout, view, state)
                .into_iter()
                .map(|(_, t)| (t, Caption)),
        );
        if state.chords {
            out.extend(view.chords.iter().map(|c| (c.label.clone(), Value)));
        }
        let notes = state.notes(view);
        for (index, note) in notes.iter().enumerate() {
            if layout.blob(view, state, index).is_some()
                && let Some(tag) = analyze_cents_tag(note)
            {
                out.push((tag, Caption));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

// ---------------------------------------------------------------- tips ---

/// What the pointer is over, in a sentence: every hit has one.
pub fn analyze_tip(hit: &AnalyzeHit, view: &AnalyzeView, state: &AnalyzeState) -> Option<String> {
    Some(match hit {
        AnalyzeHit::Badge => {
            let reason = if view.clarity_reason.is_empty() {
                "Still listening".to_string()
            } else {
                view.clarity_reason.clone()
            };
            format!("How clearly notes could be heard: {reason}")
        }
        AnalyzeHit::ScaleName | AnalyzeHit::ScaleCopy | AnalyzeHit::CopyScaleButton => {
            match &view.key {
                Some(key) => format!(
                    "Copy \u{201c}{}\u{201d} \u{2014} paste it into Tune or the piano roll's scale (Ctrl+Shift+C)",
                    fontelle_types::scale_text(&key.key)
                ),
                None => "The key appears when the analysis has heard enough".to_string(),
            }
        }
        AnalyzeHit::ScaleMenu => {
            "Other readings, show the scale on the lane, set it as the song's key".to_string()
        }
        AnalyzeHit::Tuning => match view.tuning_cents {
            Some(cents) => format!(
                "The recording sits {} cents {} of A = 440 Hz",
                cents.abs().round() as i32,
                if cents >= 0.0 { "sharp" } else { "flat" }
            ),
            None => "How far the recording sits from A = 440 Hz".to_string(),
        },
        AnalyzeHit::Bpm => "The tempo heard in the audio".to_string(),
        AnalyzeHit::Mode(mode) => {
            let detected = view
                .detected
                .map(|d| format!(" (heard as {})", d.label()))
                .unwrap_or_default();
            match mode {
                AnalyzeMode::Melody => {
                    format!("Melody: one voice, pitch-tracked{detected}")
                }
                AnalyzeMode::Chords => format!("Chords: every note heard{detected}"),
            }
        }
        AnalyzeHit::Tab(AnalyzePage::Notes) => {
            "Notes: what was heard, to copy into a piano roll".to_string()
        }
        AnalyzeHit::Tab(page) => format!(
            "{}: {} \u{2014} {LATER}",
            page.label(),
            page.promise().to_lowercase()
        ),
        AnalyzeHit::WaveToggle => {
            "The waveform or the pitch picture behind the notes (Tab)".to_string()
        }
        AnalyzeHit::ChordsToggle => "Show or hide the chord lane (C)".to_string(),
        AnalyzeHit::ScaleToggle => "Dim the rows outside the scale".to_string(),
        AnalyzeHit::WindowScale => "The window's size".to_string(),
        AnalyzeHit::Key(key) => format!(
            "{} \u{2014} click for a reference tone (the selected instrument)",
            key_name(*key)
        ),
        AnalyzeHit::Tool(AnalyzeTool::Move) => {
            "Move: drag a note up or down to repitch it (Alt: by the cent), its ends for the glide (M)"
                .to_string()
        }
        AnalyzeHit::Tool(AnalyzeTool::Select) => {
            "Select: drag to select notes without moving them (S)".to_string()
        }
        AnalyzeHit::Play => match state.playhead {
            Some(_) => "Stop (Space)".to_string(),
            None => "Play from the cursor (Space) \u{b7} Enter plays the selection".to_string(),
        },
        AnalyzeHit::Listen => {
            if state.listen_original {
                "Hearing the original \u{2014} click for your edits (B)".to_string()
            } else {
                "Hearing your edits \u{2014} click for the original (B)".to_string()
            }
        }
        AnalyzeHit::Readout => "Where it plays from, of how long".to_string(),
        AnalyzeHit::NoteEnd(_, AnalyzeNotePart::Start) => {
            "Drag: how long the move takes to arrive".to_string()
        }
        AnalyzeHit::NoteEnd(_, _) => "Drag: how long the move takes to leave".to_string(),
        AnalyzeHit::Render => {
            "Render the edits into the clip (Ctrl+Enter) \u{2014} the original is kept".to_string()
        }
        AnalyzeHit::RenderMenu => "Render as a new clip below".to_string(),
        AnalyzeHit::Revert => "Play the original audio again (your edits are kept)".to_string(),
        AnalyzeHit::Chord(index) => {
            let chord = view.chords.get(*index)?;
            format!(
                "{}, {} \u{2013} {} \u{2014} click to select its notes",
                chord.label,
                time_text(chord.start, 1),
                time_text(chord.end, 1)
            )
        }
        AnalyzeHit::Note(index) => {
            let note = state.notes(view).get(*index)?;
            let mut tip = format!(
                "{} {:+} ct \u{b7} {} \u{2013} {} \u{b7} confidence {}",
                key_name(note.midi),
                note.cents.round() as i32,
                time_text(note.start, 2),
                time_text(note.end, 2),
                percent(note.confidence)
            );
            if note.poly {
                tip.push_str(
                    " \u{2014} Chord notes can't be moved yet. Copy them to a piano roll.",
                );
            } else if state.tool == AnalyzeTool::Move {
                tip.push_str(" \u{2014} click to hear it, drag to move it");
            }
            tip
        }
        AnalyzeHit::Lane => {
            "Drag to select notes \u{b7} wheel scrolls, Ctrl+wheel zooms, Shift+wheel moves in time"
                .to_string()
        }
        AnalyzeHit::Ruler => {
            "Click to put the cursor here; drag out a region to loop".to_string()
        }
        AnalyzeHit::CopyNotes => {
            "Copy the selected notes (all, with none selected) for any piano roll (Ctrl+C)"
                .to_string()
        }
        AnalyzeHit::MakeClip => {
            "A note clip on a new row under this audio, playing these notes (one undo)".to_string()
        }
        AnalyzeHit::KeepBends => {
            "Off: clean semitones. On: the slides and bends come too".to_string()
        }
        AnalyzeHit::Job => "Analysing \u{2014} notes appear as they're found".to_string(),
        AnalyzeHit::Later => format!("This page is {LATER}"),
    })
}

/// A row's shade on the lane: the roll's own rule, with the found key as
/// the scale while *Show scale on lane* is on.
pub fn analyze_row_shade(view: &AnalyzeView, state: &AnalyzeState, key: u8) -> RowShade {
    let scale = state
        .show_scale
        .then(|| view.key.as_ref().and_then(|k| RollScale::of(&k.key)))
        .flatten();
    row_shade(key, true, super::KeyStyle::Piano, scale)
}

// ------------------------------------------------------- the ▾ menu ---

/// The rows of the scale's ▾ menu (plan §3.4).
#[derive(Debug, Clone, PartialEq)]
pub enum AnalyzeScaleRow {
    /// Copy this reading (the key itself first, then the others).
    Copy(KeyScale),
    ShowOnLane,
    SetSongKey,
    CopyNotesInScale,
}

pub fn analyze_scale_menu(
    view: &AnalyzeView,
    state: &AnalyzeState,
) -> (Vec<super::MenuEntry>, Vec<AnalyzeScaleRow>) {
    let mut entries = Vec::new();
    let mut rows = Vec::new();
    let Some(key) = &view.key else {
        return (entries, rows);
    };
    entries.push(super::MenuEntry::new(format!(
        "Copy {}",
        fontelle_types::scale_text(&key.key)
    )));
    rows.push(AnalyzeScaleRow::Copy(key.key.clone()));
    let mut others = vec![key.relative.clone()];
    others.extend(key.alternatives.iter().cloned());
    let mut seen = vec![key.key.clone()];
    for other in others {
        if seen.contains(&other) {
            continue;
        }
        seen.push(other.clone());
        let relative = if other == key.relative {
            " (relative)"
        } else {
            ""
        };
        entries.push(super::MenuEntry::new(format!(
            "Copy {}{relative}",
            fontelle_types::scale_text(&other)
        )));
        rows.push(AnalyzeScaleRow::Copy(other));
    }
    entries.push(
        super::MenuEntry::new(format!(
            "{} Show scale on lane",
            if state.show_scale { "\u{2713}" } else { "  " }
        ))
        .after_rule(),
    );
    rows.push(AnalyzeScaleRow::ShowOnLane);
    entries.push(super::MenuEntry::new(format!(
        "Set as song key ({})",
        fontelle_types::scale_text(&key.key)
    )));
    rows.push(AnalyzeScaleRow::SetSongKey);
    entries.push(super::MenuEntry::new(format!(
        "Copy notes in scale: {}",
        fontelle_types::scale_notes(&key.key).join(" ")
    )));
    rows.push(AnalyzeScaleRow::CopyNotesInScale);
    (entries, rows)
}
