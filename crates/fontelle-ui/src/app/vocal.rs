//! The vocal roll's editing half of `WindowApp`: the lane tabs and strokes,
//! the vibrato and glide handles, the note menu, the phoneme row and its
//! field, the engine chip, the singer's terms, and the Voicebanks page.
//!
//! What any of it means is `canvas::vocal_edit`'s and
//! `canvas::voicebank_page`'s, which are pure and tested; this is the
//! plumbing between those, the host and the window.

use super::*;

use fontelle_model::{LaneCurve, LaneKey, NoteVoice};
use fontelle_types::{NoteId, PPQN, Tick};

use crate::canvas::{
    Composed, LaneStroke, NoteHandle, PageHit, PhonemeCell, PhonemeHit, PickerHit, PickerMove,
    PitchBase, PitchCommand, PitchStroke, Preedit, SingerPickerLayout, SingerRow, StrokeMode,
    TermsRow, TextEntry, VocalCurve, VocalNoteRow, VocalStrips, VocalTab, VoicebankPageLayout,
    VoicebankTab,
};
use crate::document::{CatalogueRow, SingerChoice, VoicebankRow};

/// A handle on a note's expression, held.
#[derive(Debug, Clone)]
pub(super) struct HandleDrag {
    ids: Vec<NoteId>,
    handle: NoteHandle,
    start: Vec<Option<NoteVoice>>,
    origin: (f32, f32),
    /// The first note, as drawn when it was taken hold of.
    rect: crate::layout::Rect,
}

/// The Voicebanks page, while open.
#[derive(Debug, Clone, Default)]
pub(super) struct PageState {
    pub tab: VoicebankTab,
    pub scroll: f32,
    pub installed: Vec<VoicebankRow>,
    pub catalogue: Vec<CatalogueRow>,
    pub folder: String,
    pub layout: Option<VoicebankPageLayout>,
}

/// The singer picker, while open.
#[derive(Debug, Clone, Default)]
pub(super) struct PickerState {
    /// The channel it chooses for.
    pub channel: usize,
    pub query: TextEntry,
    pub scroll: f32,
    /// The focused card, an index into the layout's cards.
    pub focus: Option<usize>,
    /// Which of the focused card's languages Tab has reached.
    pub language: usize,
    pub singers: Vec<SingerChoice>,
    pub problems: Vec<String>,
    pub current: Option<fontelle_model::VocalVoice>,
    /// Whose singers these are: `None` the Fontelle Singer's voicebanks,
    /// else the engine's name — each instrument's picker shows only its
    /// own; the others are a change of instrument.
    pub instrument: Option<String>,
    /// What that instrument is called.
    pub instrument_name: String,
    pub layout: Option<SingerPickerLayout>,
}

/// Everything the vocal roll's editing holds between frames.
#[derive(Debug, Default)]
pub(super) struct VocalEditing {
    pub strips: VocalStrips,
    pub tabs: Vec<VocalTab>,
    /// The lane the tabs chose; `None` is the notes' own.
    pub curve: Option<VocalCurve>,
    pub stroke: Option<(LaneKey, LaneStroke)>,
    pub handle: Option<HandleDrag>,
    pub cells: Vec<PhonemeCell>,
    /// The phoneme field: its note, what is typed.
    pub field: Option<(NoteId, TextEntry)>,
    pub suggestions: Vec<String>,
    /// A phoneme's start, held: its note and index.
    pub boundary: Option<(NoteId, usize)>,
    pub engine_chip: Option<crate::layout::Rect>,
    /// "Note ▾" at the end of the tabs: the note menu.
    pub note_button: Option<crate::layout::Rect>,
    pub page: Option<PageState>,
    /// The singer picker (a card grid), while open.
    pub picker: Option<PickerState>,
    /// Voicebank portraits, decoded once each.
    pub portraits: crate::portrait::Portraits,
    pub px_per_second: f32,
    /// The lane as drawn this frame: its scale and points.
    pub lane: Option<(crate::canvas::CurveScale, LaneCurve)>,
    /// The pitch pen: presses on the grid draw the pitch line, not notes.
    pub pitch_pen: bool,
    /// A pitch stroke in hand.
    pub pitch_stroke: Option<PitchStroke>,
    /// "Pitch pen" in the tab strip, beside "Note ▾".
    pub pitch_button: Option<crate::layout::Rect>,
    /// The drawn pitch over what the grid shows, this frame.
    pub drawn: Vec<Vec<(Tick, f32)>>,
    /// What an input method is composing into the lyric or phoneme field.
    pub preedit: Preedit,
    /// The lyric field as drawn: its text with the composition in it.
    pub lyric_shown: Option<Composed>,
    /// The composition's underline in the lyric field, from the text's left.
    pub lyric_underline: Option<(f32, f32)>,
    /// The phoneme field as drawn, its caret and selection, its underline.
    pub phoneme_shown: Option<Composed>,
    pub phoneme_marks: Option<crate::render::RenameMarks>,
    pub phoneme_underline: Option<(f32, f32)>,
    /// The phoneme field's note's head, for a note with nothing sung yet.
    pub phoneme_anchor: Option<f32>,
    /// A part just opened: frame its notes once the roll is laid out.
    pub frame_pending: bool,
    /// Where the input method was last told the field is.
    pub ime_area: Option<(f32, f32, f32, f32)>,
}

pub(super) const NOTE_BUTTON: &str = "Note \u{25be}";
pub(super) const PITCH_BUTTON: &str = crate::render::vocal::PITCH_BUTTON_TEXT;

/// A sample of a pitch command every this many seconds.
const PITCH_STEP_SECONDS: f64 = 0.005;
/// Smoothing reaches this far either side.
const SMOOTH_SECONDS: f64 = 0.04;

impl WindowApp {
    /// The px a second around the open clip.
    fn vocal_px_per_second(&self) -> f32 {
        let tps = self
            .vocal
            .as_ref()
            .map_or(1920.0, |v| v.ticks_per_second.max(1.0));
        let grid = self.roll_layout.grid;
        let px_per_tick = (crate::canvas::tick_to_x(&self.roll.view, grid, PPQN)
            - crate::canvas::tick_to_x(&self.roll.view, grid, 0))
            / PPQN as f32;
        (px_per_tick as f64 * tps) as f32
    }

    /// Lays out the vocal roll's editing for this frame and shapes what it
    /// draws: tabs, phoneme cells, the lane's scale, the engine chip.
    pub(super) fn shape_vocal_edit(&mut self, font: &crate::theme::FontTokens) {
        use crate::render::vocal::{
            LYRIC_STYLE, PHONEME_STYLE, edit_strings, engine_chip_rect, engine_chip_text,
        };
        let Some(view) = self.vocal.clone() else {
            self.vocal_edit.tabs.clear();
            self.vocal_edit.cells.clear();
            self.vocal_edit.engine_chip = None;
            self.vocal_edit.note_button = None;
            self.vocal_edit.lane = None;
            return;
        };
        self.vocal_edit.px_per_second = self.vocal_px_per_second();
        // Tab labels first, so the tabs can be measured.
        let mut labels_to_shape: Vec<String> =
            vec![NOTE_BUTTON.to_string(), PITCH_BUTTON.to_string()];
        labels_to_shape.push("notes".to_string());
        for lane in fontelle_sing::Lane::ALL {
            labels_to_shape.push(fontelle_model::lane_spec(lane).short.to_string());
        }
        labels_to_shape.extend(view.capabilities.extra_lanes.iter().map(|l| l.name.clone()));
        for text in &labels_to_shape {
            self.labels
                .ensure_styled(text, font, PHONEME_STYLE, &mut self.text);
        }
        fn width_in(labels: &crate::text::Labels, s: &str) -> f32 {
            labels
                .get_styled(s, crate::render::vocal::PHONEME_STYLE)
                .map_or(s.len() as f32 * 6.0, |t| t.width)
        }
        let width = |s: &str| width_in(&self.labels, s);
        let strip = self.vocal_edit.strips.tabs;
        self.vocal_edit.tabs = crate::canvas::vocal_tabs(strip, &view.capabilities, &width);
        self.vocal_edit.note_button = (!strip.is_empty()).then(|| {
            let w = width(NOTE_BUTTON) + 14.0;
            crate::layout::Rect::new(
                strip.right() - w - 4.0,
                strip.y + 1.0,
                w,
                strip.height - 2.0,
            )
        });
        self.vocal_edit.pitch_button = self.vocal_edit.note_button.map(|note| {
            let w = width(PITCH_BUTTON) + 14.0;
            crate::layout::Rect::new(note.x - w - 4.0, note.y, w, note.height)
        });
        if let Some(doc) = &self.options.document {
            self.vocal_edit.cells = crate::canvas::phoneme_cells(
                self.vocal_edit.strips.phonemes,
                &self.roll.view,
                self.roll_layout.grid,
                doc.notes(),
                &view.timed,
                view.ticks_per_second,
            );
        }
        // The lane: its scale, and the stroke in hand or the part's curve.
        self.vocal_edit.lane = self.vocal_edit.curve.as_ref().map(|curve| {
            let scale = crate::canvas::curve_scale(curve, &view.capabilities);
            let key = curve.key(&view.backend);
            let points = match &self.vocal_edit.stroke {
                Some((k, stroke)) if *k == key => stroke.curve().clone(),
                _ => view.part.curve(&key).cloned().unwrap_or_default(),
            };
            (scale, points)
        });
        if let Some((_, typed)) = &self.vocal_edit.field {
            self.vocal_edit.suggestions =
                crate::canvas::phoneme_suggestions(&view.capabilities.phonemes, typed.text(), 6);
        }
        // The drawn pitch over what the grid shows: the stroke in hand, or
        // the part's bend.
        {
            let base = self.vocal_pitch_base(&view);
            let key = LaneKey::Standard(fontelle_sing::Lane::PitchBend);
            let bend = match &self.vocal_edit.pitch_stroke {
                Some(stroke) => stroke.curve().clone(),
                None => view.part.curve(&key).cloned().unwrap_or_default(),
            };
            let ticks = crate::canvas::visible_ticks(&self.roll.view, self.roll_layout.grid);
            self.vocal_edit.drawn =
                crate::canvas::drawn_pitch(&base, &bend, ticks.start, ticks.end, self.vocal_step());
        }
        // The phoneme field as drawn, with any composition, and its marks.
        self.vocal_edit.phoneme_shown = None;
        self.vocal_edit.phoneme_marks = None;
        self.vocal_edit.phoneme_underline = None;
        self.vocal_edit.phoneme_anchor = self.vocal_edit.field.as_ref().and_then(|(id, _)| {
            let doc = self.options.document.as_ref()?;
            let note = doc.notes().get(*id)?;
            Some(crate::canvas::tick_to_x(
                &self.roll.view,
                self.roll_layout.grid,
                note.start,
            ))
        });
        if let Some((_, entry)) = &self.vocal_edit.field {
            let shown = crate::canvas::compose(entry, &self.vocal_edit.preedit);
            let sized = crate::theme::FontTokens {
                family: font.family.clone(),
                size: PHONEME_STYLE.size(),
                line_height: 1.0,
                display: None,
            };
            let text = shown.text.clone();
            let mut width_to = |at: usize| {
                if at == 0 {
                    0.0
                } else {
                    self.text
                        .layout_weighted(&text[..at], &sized, None, PHONEME_STYLE.weight())
                        .width
                }
            };
            self.vocal_edit.phoneme_marks = Some(crate::render::RenameMarks {
                caret_x: width_to(shown.caret),
                selection: shown.selection.map(|(a, b)| (width_to(a), width_to(b))),
                caret_on: self.caret_on,
            });
            self.vocal_edit.phoneme_underline =
                shown.underline.map(|(a, b)| (width_to(a), width_to(b)));
            self.labels
                .ensure_styled(&shown.text, font, PHONEME_STYLE, &mut self.text);
            self.vocal_edit.phoneme_shown = Some(shown);
        }
        // The engine chip, left of the singer chip.
        self.vocal_edit.engine_chip = match (&view.editor, self.vocal_caption) {
            (Some(engine), Some(caption)) => {
                // At the singer chip's size: the way into the engine's own
                // window is the engine instrument's headline.
                let text = engine_chip_text(engine, view.editor_open);
                self.labels
                    .ensure_styled(&text, font, LYRIC_STYLE, &mut self.text);
                let w = self
                    .labels
                    .get_styled(&text, LYRIC_STYLE)
                    .map_or(100.0, |t| t.width);
                Some(engine_chip_rect(caption, w))
            }
            _ => None,
        };
        let rv = self.roll_vocal(&view);
        for text in edit_strings(&rv) {
            self.labels
                .ensure_styled(&text, font, PHONEME_STYLE, &mut self.text);
        }
    }

    /// Lays out and shapes the Voicebanks page and the singer picker, and
    /// decodes the portraits they (and the roll's corner) show — every
    /// frame, whatever is in the roll: both open from anywhere.
    pub(super) fn shape_vocal_overlays(&mut self, font: &crate::theme::FontTokens) {
        self.lay_out_voicebank_page();
        self.lay_out_singer_picker();
        let mut styled: Vec<(String, crate::text::TextStyle)> = Vec::new();
        let mut faces: Vec<String> = Vec::new();
        if let Some(page) = &self.vocal_edit.page
            && let Some(layout) = &page.layout
        {
            let p = crate::render::vocal::VoicebankPage {
                layout,
                tab: page.tab,
                installed: &page.installed,
                catalogue: &page.catalogue,
                folder: &page.folder,
            };
            styled.extend(crate::render::vocal::page_strings(&p));
            if page.tab == VoicebankTab::Installed {
                faces.extend(page.installed.iter().filter_map(|r| r.portrait.clone()));
            }
        }
        if let Some(picker) = &self.vocal_edit.picker
            && let Some(layout) = &picker.layout
        {
            let view = picker_view(picker, layout);
            styled.extend(crate::render::vocal::picker_strings(&view));
            faces.extend(
                layout
                    .cards
                    .iter()
                    .filter_map(|c| picker.singers.get(c.singer))
                    .filter_map(|s| s.image.clone()),
            );
        }
        for (text, style) in styled {
            self.labels
                .ensure_styled(&text, font, style, &mut self.text);
        }
        if let Some(face) = self.vocal.as_ref().and_then(|v| v.portrait.clone()) {
            faces.push(face);
        }
        for face in faces {
            self.vocal_edit.portraits.get(&face);
        }
    }

    /// The vocal layer as the roll draws it.
    pub(super) fn roll_vocal<'a>(
        &'a self,
        view: &'a crate::document::VocalView,
    ) -> crate::render::vocal::RollVocal<'a> {
        roll_vocal_of(
            &self.vocal_edit,
            view,
            self.lyric.as_ref(),
            self.lyric_marks,
            self.vocal_caption,
        )
    }

    // ----------------------------------------------------------- presses

    /// A press on the vocal roll's own parts. Whether it was theirs.
    pub(super) fn press_vocal_edit(&mut self, button: MouseButton, x: f32, y: f32) -> bool {
        let Some(view) = self.vocal.clone() else {
            return false;
        };
        if button == MouseButton::Left
            && self
                .vocal_edit
                .engine_chip
                .is_some_and(|c| c.contains(x, y))
        {
            self.toggle_singer_editor();
            return true;
        }
        if self.vocal_edit.field.is_some() {
            self.commit_phoneme_field();
        }
        if button == MouseButton::Left
            && self
                .vocal_edit
                .note_button
                .is_some_and(|b| b.contains(x, y))
        {
            self.open_vocal_note_menu(x, y);
            return true;
        }
        if button == MouseButton::Left
            && self
                .vocal_edit
                .pitch_button
                .is_some_and(|b| b.contains(x, y))
        {
            self.toggle_pitch_pen();
            return true;
        }
        if self.vocal_edit.strips.tabs.contains(x, y) {
            if let Some(i) = crate::canvas::vocal_tab_at(&self.vocal_edit.tabs, x, y) {
                let tab = self.vocal_edit.tabs[i].clone();
                self.vocal_edit.curve = tab.curve.clone();
                self.status = match (&tab.why, &tab.curve) {
                    (Some(why), _) => why.clone(),
                    (None, None) => "The notes' own lane: velocity is consonant speed".to_string(),
                    (None, Some(_)) => format!(
                        "Drawing {} \u{2014} Shift for a straight line, right button or the eraser to clear",
                        tab.label
                    ),
                };
                self.tree.invalidate(PANEL);
            }
            return true;
        }
        // A stroke on the chosen lane.
        if let Some(curve) = self.vocal_edit.curve.clone()
            && self.roll_layout.velocity.contains(x, y)
        {
            let scale = crate::canvas::curve_scale(&curve, &view.capabilities);
            let key = curve.key(&view.backend);
            let base = view.part.curve(&key).cloned().unwrap_or_default();
            let mode =
                if button == MouseButton::Right || self.roll.tool == crate::canvas::Tool::Delete {
                    StrokeMode::Erase
                } else if self.modifiers.shift_key() {
                    StrokeMode::Line
                } else {
                    StrokeMode::Pencil
                };
            let at = self.vocal_lane_point(&scale, x, y);
            let stroke = LaneStroke::begin(base, mode, at, scale.neutral, self.vocal_step());
            if let Some(doc) = &mut self.options.document {
                doc.end_vocal_gesture();
                doc.set_vocal_lane(key.clone(), Some(stroke.curve().clone()));
            }
            self.vocal_edit.stroke = Some((key, stroke));
            self.drag = Drag::Vocal;
            self.tree.invalidate(PANEL);
            return true;
        }
        if self.vocal_edit.strips.phonemes.contains(x, y) {
            self.press_phoneme_row(button, x, y, &view);
            return true;
        }
        // A handle on a selected note.
        if button == MouseButton::Left
            && let Some(drag) = self.vocal_handle_at(x, y, &view)
        {
            self.status = drag.handle.hint().to_string();
            self.vocal_edit.handle = Some(drag);
            if let Some(doc) = &mut self.options.document {
                doc.end_vocal_gesture();
            }
            self.drag = Drag::Vocal;
            return true;
        }
        // The pitch pen on the grid: the pitch line, drawn over the notes.
        if self.vocal_edit.pitch_pen
            && matches!(button, MouseButton::Left | MouseButton::Right)
            && self.roll_layout.grid.contains(x, y)
        {
            let mode =
                if button == MouseButton::Right || self.roll.tool == crate::canvas::Tool::Delete {
                    StrokeMode::Erase
                } else if self.modifiers.shift_key() {
                    StrokeMode::Line
                } else {
                    StrokeMode::Pencil
                };
            let key = LaneKey::Standard(fontelle_sing::Lane::PitchBend);
            let bend = view.part.curve(&key).cloned().unwrap_or_default();
            let at = self.vocal_pitch_point(x, y);
            let stroke = PitchStroke::begin(
                self.vocal_pitch_base(&view),
                bend,
                mode,
                at,
                self.vocal_step(),
            );
            if let Some(doc) = &mut self.options.document {
                doc.end_vocal_gesture();
                doc.set_vocal_lane(key, Some(stroke.curve().clone()));
            }
            self.status = match mode {
                StrokeMode::Erase => "Back to the notes where you drag",
                StrokeMode::Line => "A straight pitch line \u{2014} let go where it ends",
                StrokeMode::Pencil => {
                    "Drawing the pitch \u{2014} Shift for a straight line, right button back to the notes"
                }
            }
            .to_string();
            if !view
                .capabilities
                .lanes
                .contains(&fontelle_sing::Lane::PitchBend)
            {
                self.status = "This voice ignores pitch bend \u{2014} what you draw is kept for a voice that uses it".to_string();
            }
            self.vocal_edit.pitch_stroke = Some(stroke);
            self.drag = Drag::Vocal;
            self.tree.invalidate(PANEL);
            return true;
        }
        false
    }

    /// `(tick in the clip, pitch)` under the pointer on the grid.
    fn vocal_pitch_point(&self, x: f32, y: f32) -> (Tick, f32) {
        let grid = self.roll_layout.grid;
        let x = x.clamp(grid.x, grid.right());
        let y = y.clamp(grid.y, grid.bottom());
        (
            crate::canvas::x_to_tick(&self.roll.view, grid, x),
            crate::canvas::y_to_pitch(&self.roll.view, grid, y),
        )
    }

    /// The pitch the notes make: the computed line once something has been
    /// sung, the notes themselves before.
    pub(super) fn vocal_pitch_base(&self, view: &crate::document::VocalView) -> PitchBase {
        if !view.note_line.is_empty() {
            return PitchBase::from_lines(&view.note_line);
        }
        self.options
            .document
            .as_ref()
            .map(|doc| PitchBase::from_notes(doc.notes()))
            .unwrap_or_default()
    }

    /// The pitch pen on or off (Shift+P, or its chip).
    pub(super) fn toggle_pitch_pen(&mut self) {
        if self.vocal.is_none() {
            return;
        }
        self.vocal_edit.pitch_pen = !self.vocal_edit.pitch_pen;
        self.status = if self.vocal_edit.pitch_pen {
            "Pitch pen \u{2014} draw the pitch over the notes; Shift a straight line; right button back to the notes; a tool key leaves it"
        } else {
            "Pitch pen off \u{2014} the grid draws notes again"
        }
        .to_string();
        self.tree.invalidate(PANEL);
    }

    /// Smooth, snap or reset the drawn pitch over the selected notes, else
    /// the time selection, else the whole part. One undo step.
    pub(super) fn pitch_command(&mut self, command: PitchCommand) {
        let Some(view) = self.vocal.clone() else {
            return;
        };
        let Some(doc) = &self.options.document else {
            return;
        };
        let time = self
            .loop_range
            .map(|(a, b)| (doc.clip_tick_of_song_tick(a), doc.clip_tick_of_song_tick(b)));
        let selection = self.roll.selection().to_vec();
        let Some((from, to)) = crate::canvas::pitch_edit_range(doc.notes(), &selection, time)
        else {
            self.status = "No notes to work on".to_string();
            return;
        };
        let key = LaneKey::Standard(fontelle_sing::Lane::PitchBend);
        let bend = view.part.curve(&key).cloned().unwrap_or_default();
        let tps = view.ticks_per_second.max(1.0);
        let step = ((tps * PITCH_STEP_SECONDS).round() as Tick).max(1);
        let base = self.vocal_pitch_base(&view);
        let scale = self.roll.scale;
        let new = match command {
            PitchCommand::Smooth => crate::canvas::smooth_bend(
                &bend,
                from,
                to,
                step,
                ((tps * SMOOTH_SECONDS).round() as Tick).max(step),
            ),
            PitchCommand::Snap => crate::canvas::snap_bend_to_scale(
                &base,
                &bend,
                from,
                to,
                step,
                scale.map_or(0, |s| s.mask),
                tps,
            ),
            PitchCommand::Reset => {
                let mut b = bend.clone();
                b.erase(from, to, 0.0);
                b
            }
        };
        let what = if !selection.is_empty() {
            "the selected notes"
        } else if time.is_some() {
            "the time selection"
        } else {
            "the whole part"
        };
        if new == bend {
            self.status = match command {
                PitchCommand::Snap if bend.is_empty() => {
                    "Nothing drawn to snap \u{2014} draw the pitch first (Shift+P)".to_string()
                }
                _ => format!("{} changed nothing on {what}", command.label()),
            };
            return;
        }
        let Some(doc) = &mut self.options.document else {
            return;
        };
        doc.end_vocal_gesture();
        doc.set_vocal_lane(key, Some(new));
        doc.end_vocal_gesture();
        self.status = match (command, scale) {
            (PitchCommand::Snap, None) => format!(
                "Snapped {what} to semitones \u{2014} the song has no key; set one to snap to its scale. Ctrl+Z takes it back"
            ),
            _ => format!(
                "{} on {what} \u{2014} Ctrl+Z takes it back",
                command.label()
            ),
        };
        self.refresh_vocal();
        self.tree.invalidate(PANEL);
    }

    /// The note menu from the keyboard, at the first selected note.
    pub(super) fn note_menu_from_key(&mut self) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let first = self
            .roll
            .selection()
            .iter()
            .filter_map(|id| doc.notes().get(*id))
            .min_by_key(|n| n.start)
            .cloned();
        match first {
            Some(note) => {
                let (x, y) =
                    crate::canvas::note_menu_anchor(&self.roll.view, self.roll_layout.grid, &note);
                self.open_vocal_note_menu(x, y);
            }
            None => {
                self.status = "Select a note first \u{2014} the note menu is about it".to_string()
            }
        }
    }

    /// Brings the lyric field's note into view when Tab took it off screen.
    pub(super) fn follow_lyric_note(&mut self, id: NoteId, field_width: f32) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let Some(note) = doc.notes().get(id) else {
            return;
        };
        if let Some(view) =
            crate::canvas::follow_note(&self.roll.view, self.roll_layout.grid, note, field_width)
        {
            self.roll.view = view;
            self.tree.invalidate(PANEL);
        }
    }

    /// Tells the input method where the field being typed into is, so its
    /// candidate window opens beside the word rather than in a corner.
    pub(super) fn place_ime_cursor(&mut self) {
        let grid = self.roll_layout.grid;
        let area = if let Some(editor) = &self.lyric {
            let doc = self.options.document.as_ref();
            doc.and_then(|d| d.notes().get(editor.ids()[0])).map(|n| {
                let row = crate::canvas::key_row(&self.roll.view, grid, n.key);
                let x = crate::canvas::tick_to_x(&self.roll.view, grid, n.start).max(grid.x);
                (x, row.y - 3.0, 120.0, row.height.max(16.0) + 6.0)
            })
        } else if self.vocal_edit.field.is_some() {
            let row = self.vocal_edit.strips.phonemes;
            let x = self.vocal_edit.phoneme_anchor.unwrap_or(row.x).max(row.x);
            Some((x, row.y - 4.0, 100.0, row.height + 6.0))
        } else {
            None
        };
        if area == self.vocal_edit.ime_area {
            return;
        }
        self.vocal_edit.ime_area = area;
        if let (Some((x, y, w, h)), Some(live)) = (area, &self.live) {
            live.window.set_ime_cursor_area(
                winit::dpi::LogicalPosition::new(f64::from(x), f64::from(y)),
                winit::dpi::LogicalSize::new(f64::from(w), f64::from(h)),
            );
        }
    }

    /// Frames the open part's notes, once, after it opened.
    pub(super) fn frame_vocal_if_pending(&mut self) {
        if !self.vocal_edit.frame_pending || self.vocal.is_none() {
            return;
        }
        let Some(doc) = &self.options.document else {
            return;
        };
        if self.roll_layout.grid.is_empty() {
            return;
        }
        self.vocal_edit.frame_pending = false;
        self.roll.view =
            crate::canvas::frame_notes(&self.roll.view, self.roll_layout.grid, doc.notes());
        self.tree.invalidate(PANEL);
    }

    fn vocal_step(&self) -> Tick {
        // A point every two pixels or so.
        let grid = self.roll_layout.grid;
        let a = crate::canvas::x_to_tick(&self.roll.view, grid, grid.x);
        let b = crate::canvas::x_to_tick(&self.roll.view, grid, grid.x + 2.0);
        (b - a).max(1)
    }

    fn vocal_lane_point(&self, scale: &crate::canvas::CurveScale, x: f32, y: f32) -> (Tick, f32) {
        let grid = self.roll_layout.grid;
        let x = x.clamp(grid.x, grid.right());
        (
            crate::canvas::x_to_tick(&self.roll.view, grid, x),
            crate::canvas::curve_y_value(scale, self.roll_layout.velocity, y),
        )
    }

    fn note_rect(&self, note: &fontelle_model::Note) -> crate::layout::Rect {
        let grid = self.roll_layout.grid;
        let row = crate::canvas::key_row(&self.roll.view, grid, note.key);
        let x0 = crate::canvas::tick_to_x(&self.roll.view, grid, note.start);
        let x1 = crate::canvas::tick_to_x(&self.roll.view, grid, note.start + note.length);
        crate::layout::Rect::new(x0, row.y, x1 - x0, row.height)
    }

    fn vocal_handle_at(
        &self,
        x: f32,
        y: f32,
        view: &crate::document::VocalView,
    ) -> Option<HandleDrag> {
        let doc = self.options.document.as_ref()?;
        let notes = doc.notes();
        let selection = self.roll.selection().to_vec();
        for id in &selection {
            let note = notes.get(*id)?;
            let rect = self.note_rect(note);
            for (handle, r) in crate::canvas::note_handles(
                rect,
                note.voice.as_deref(),
                self.vocal_edit.px_per_second,
                view.capabilities.portamento,
            ) {
                if r.contains(x, y) {
                    // Every selected note with the same thing moves together.
                    let ids: Vec<NoteId> = selection
                        .iter()
                        .copied()
                        .filter(|n| notes.get(*n).is_some())
                        .filter(|n| {
                            handle == NoteHandle::Glide
                                || notes[*n]
                                    .voice
                                    .as_ref()
                                    .is_some_and(|v| v.vibrato.is_some())
                        })
                        .collect();
                    let start = ids
                        .iter()
                        .map(|n| notes[*n].voice.as_deref().cloned())
                        .collect();
                    return Some(HandleDrag {
                        ids,
                        handle,
                        start,
                        origin: (x, y),
                        rect,
                    });
                }
            }
        }
        None
    }

    fn press_phoneme_row(
        &mut self,
        button: MouseButton,
        x: f32,
        y: f32,
        view: &crate::document::VocalView,
    ) {
        if button != MouseButton::Left {
            return;
        }
        let doubled =
            self.double_click
                .press(x, y, self.input_clock.stamp(std::time::Instant::now()));
        let timing = view.capabilities.phoneme_timing;
        match crate::canvas::phoneme_hit(&self.vocal_edit.cells, x, y, timing) {
            Some(PhonemeHit::Boundary { note, index }) if !doubled => {
                self.vocal_edit.boundary = Some((note, index));
                if let Some(doc) = &mut self.options.document {
                    doc.end_vocal_gesture();
                }
                self.drag = Drag::Vocal;
                self.status = "Retiming a phoneme \u{2014} drag its start".to_string();
            }
            Some(PhonemeHit::Cell { note, .. } | PhonemeHit::Boundary { note, .. }) if doubled => {
                self.open_phoneme_field(note);
            }
            Some(_) => {
                self.status = if timing {
                    "Double-click to type a note's phonemes; drag a phoneme's start to retime it"
                } else {
                    "Double-click to type a note's phonemes (this voice times them itself)"
                }
                .to_string();
            }
            None => {
                // An empty stretch: a note without sung phonemes yet.
                if doubled
                    && let Some(doc) = &self.options.document
                    && let crate::canvas::RollHit::Note(id, _) = crate::canvas::hit_test(
                        &self.roll.view,
                        self.roll_layout.grid,
                        doc.notes(),
                        x,
                        self.roll_layout.grid.y + 1.0,
                    )
                {
                    self.open_phoneme_field(id);
                }
            }
        }
    }

    /// Opens the phoneme field on `note`, with what it sings now.
    pub(super) fn open_phoneme_field(&mut self, note: NoteId) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let Some(n) = doc.notes().get(note) else {
            return;
        };
        let text = match n.voice.as_ref().and_then(|v| v.phonemes.as_ref()) {
            Some(edits) => edits
                .iter()
                .map(|e| e.symbol.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            None => self
                .vocal
                .as_ref()
                .and_then(|v| v.timed.iter().find(|(id, _)| *id == note))
                .map(|(_, p)| {
                    p.iter()
                        .map(|(s, _)| s.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default(),
        };
        let mut entry = TextEntry::new(text);
        entry.select_all();
        self.vocal_edit.field = Some((note, entry));
        self.roll.select(vec![note]);
        self.status = "Type the phonemes, a space between each \u{2014} Tab takes the first suggestion, Enter keeps, Esc leaves, empty is the voice's own reading".to_string();
        self.set_ime(true);
        self.tree.invalidate(PANEL);
    }

    fn commit_phoneme_field(&mut self) {
        let Some((note, entry)) = self.vocal_edit.field.take() else {
            return;
        };
        self.vocal_edit.preedit.clear();
        self.set_ime(false);
        let edits = crate::canvas::parse_phonemes(entry.text());
        self.set_note_voice_with(&[note], "Set phonemes", |v| v.phonemes = edits.clone());
        self.tree.invalidate(PANEL);
    }

    /// Changes each of `ids`' voice with `change`, one undo step.
    fn set_note_voice_with(
        &mut self,
        ids: &[NoteId],
        label: &str,
        change: impl Fn(&mut NoteVoice),
    ) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let voices: Vec<Option<NoteVoice>> = ids
            .iter()
            .map(|id| {
                let mut v = doc
                    .notes()
                    .get(*id)
                    .and_then(|n| n.voice.as_deref().cloned())
                    .unwrap_or_default();
                change(&mut v);
                (!v.is_empty()).then_some(v)
            })
            .collect();
        doc.end_vocal_gesture();
        if let Err(why) = doc.set_note_voices(ids, voices, label) {
            self.status = why;
        }
        doc.end_vocal_gesture();
        self.refresh_vocal();
    }

    /// A key while the phoneme field is open. Whether it was taken.
    pub(super) fn phoneme_field_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        // Esc closes the Voicebanks page, which otherwise lets keys through.
        if self.vocal_edit.page.is_some()
            && event.state == winit::event::ElementState::Pressed
            && event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape)
            && self.menu.is_none()
        {
            self.vocal_edit.page = None;
            self.tree.invalidate_rect(self.layout.window);
            return true;
        }
        if self.vocal_edit.field.is_none() || event.state != winit::event::ElementState::Pressed {
            return self.vocal_edit.field.is_some();
        }
        // While an input method composes, its keys are its own: an Enter
        // that picks a kanji must not also keep the field.
        if self.vocal_edit.preedit.is_active() {
            return true;
        }
        use winit::keyboard::{Key, NamedKey};
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => self.commit_phoneme_field(),
            Key::Named(NamedKey::Escape) => {
                self.vocal_edit.field = None;
                self.vocal_edit.preedit.clear();
                self.set_ime(false);
                self.status = "Phonemes left as they were".to_string();
            }
            Key::Named(NamedKey::Tab) => {
                if let (Some((_, entry)), Some(first)) = (
                    &mut self.vocal_edit.field,
                    self.vocal_edit.suggestions.first(),
                ) {
                    let text = entry.text().to_string();
                    let keep = text.rsplit_once(char::is_whitespace).map_or("", |(a, _)| a);
                    let next = if keep.is_empty() {
                        format!("{first} ")
                    } else {
                        format!("{keep} {first} ")
                    };
                    entry.set(next);
                    entry.end(false);
                }
            }
            key => {
                let ctrl = self.modifiers.control_key();
                let shift = self.modifiers.shift_key();
                if let Some((_, entry)) = &mut self.vocal_edit.field {
                    crate::canvas::text_key(entry, key, ctrl, shift, &mut self.clipboard_text);
                }
            }
        }
        self.tree.invalidate(PANEL);
        true
    }

    /// A finished input-method composition into the phoneme field.
    pub(super) fn phoneme_field_commit_text(&mut self, text: &str) -> bool {
        match &mut self.vocal_edit.field {
            Some((_, entry)) => {
                self.vocal_edit.preedit.clear();
                let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                entry.insert(&clean);
                true
            }
            None => false,
        }
    }

    // -------------------------------------------------------------- drags

    pub(super) fn drag_vocal_edit(&mut self) {
        let (x, y) = self.cursor;
        if let Some(mut stroke) = self.vocal_edit.pitch_stroke.take() {
            stroke.drag(self.vocal_pitch_point(x, y));
            if let Some(doc) = &mut self.options.document {
                doc.set_vocal_lane(
                    LaneKey::Standard(fontelle_sing::Lane::PitchBend),
                    Some(stroke.curve().clone()),
                );
            }
            self.vocal_edit.pitch_stroke = Some(stroke);
            self.tree.invalidate(PANEL);
            return;
        }
        if let Some((key, mut stroke)) = self.vocal_edit.stroke.take() {
            if let Some((scale, _)) = &self.vocal_edit.lane {
                let at = self.vocal_lane_point(scale, x, y);
                stroke.drag(at);
                if let Some(doc) = &mut self.options.document {
                    doc.set_vocal_lane(key.clone(), Some(stroke.curve().clone()));
                }
            }
            self.vocal_edit.stroke = Some((key, stroke));
            self.tree.invalidate(PANEL);
            return;
        }
        if let Some(drag) = self.vocal_edit.handle.clone() {
            let (dx, dy) = (x - drag.origin.0, y - drag.origin.1);
            let pps = self.vocal_edit.px_per_second;
            let shift = self.modifiers.shift_key();
            let voices: Vec<Option<NoteVoice>> = drag
                .start
                .iter()
                .map(|start| {
                    let start = start.clone().unwrap_or_default();
                    let v = if drag.handle == NoteHandle::Glide && shift {
                        crate::canvas::drag_glide_position(&start, dx, pps)
                    } else {
                        crate::canvas::drag_handle(drag.handle, &start, dx, dy, drag.rect, pps)
                    };
                    (!v.is_empty()).then_some(v)
                })
                .collect();
            if let Some(doc) = &mut self.options.document {
                let label = drag
                    .handle
                    .hint()
                    .split(" \u{2014}")
                    .next()
                    .unwrap_or("Expression");
                let _ = doc.set_note_voices(&drag.ids, voices, label);
            }
            self.refresh_vocal();
            return;
        }
        if let Some((note, index)) = self.vocal_edit.boundary {
            let Some(doc) = &self.options.document else {
                return;
            };
            let Some(n) = doc.notes().get(note) else {
                return;
            };
            let tps = self.vocal.as_ref().map_or(1920.0, |v| v.ticks_per_second);
            let tick = crate::canvas::x_to_tick(&self.roll.view, self.roll_layout.grid, x);
            let offset = (tick - n.start) as f64 / tps.max(1.0);
            let sung = self
                .vocal
                .as_ref()
                .and_then(|v| v.timed.iter().find(|(id, _)| *id == note))
                .map(|(_, p)| p.clone())
                .unwrap_or_default();
            let current = n.voice.as_ref().and_then(|v| v.phonemes.clone());
            let edits = crate::canvas::retime_phoneme(&sung, current.as_ref(), index, offset);
            let mut v = n.voice.as_deref().cloned().unwrap_or_default();
            v.phonemes = Some(edits);
            if let Some(doc) = &mut self.options.document {
                let _ = doc.set_note_voices(&[note], vec![Some(v)], "Retime a phoneme");
            }
            self.refresh_vocal();
        }
    }

    pub(super) fn release_vocal_edit(&mut self) {
        self.vocal_edit.stroke = None;
        self.vocal_edit.pitch_stroke = None;
        self.vocal_edit.handle = None;
        self.vocal_edit.boundary = None;
        if let Some(doc) = &mut self.options.document {
            doc.end_vocal_gesture();
        }
        self.refresh_vocal();
        self.tree.invalidate(PANEL);
    }

    // ---------------------------------------------------------- note menu

    fn open_vocal_note_menu(&mut self, x: f32, y: f32) {
        if self.roll.selection().is_empty() {
            self.status = "Select the notes first".to_string();
            return;
        }
        let bounds = self.layout.window;
        self.open_menu(MenuTarget::VocalNote, x, y, bounds);
    }

    pub(super) fn vocal_note_rows(&self) -> Vec<(crate::canvas::MenuEntry, VocalNoteRow)> {
        let (Some(view), Some(doc)) = (&self.vocal, &self.options.document) else {
            return Vec::new();
        };
        let first = self
            .roll
            .selection()
            .first()
            .and_then(|id| doc.notes().get(*id))
            .and_then(|n| n.voice.as_deref());
        crate::canvas::vocal_note_menu_rows(first, &view.capabilities, &view.backend)
    }

    pub(super) fn choose_vocal_note_row(&mut self, index: usize) {
        let Some((_, row)) = self.vocal_note_rows().into_iter().nth(index) else {
            return;
        };
        let ids = self.roll.selection().to_vec();
        if let VocalNoteRow::Pitch(command) = row {
            self.pitch_command(command);
            return;
        }
        if row == VocalNoteRow::EditPhonemes {
            if let Some(first) = ids.first() {
                self.open_phoneme_field(*first);
            }
            return;
        }
        let backend = self
            .vocal
            .as_ref()
            .map(|v| v.backend.clone())
            .unwrap_or_default();
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let voices: Vec<Option<NoteVoice>> = ids
            .iter()
            .map(|id| {
                let now = doc
                    .notes()
                    .get(*id)
                    .and_then(|n| n.voice.as_deref().cloned());
                crate::canvas::apply_note_row(&row, now.as_ref(), &backend).unwrap_or(now)
            })
            .collect();
        doc.end_vocal_gesture();
        match doc.set_note_voices(&ids, voices, "Change how notes are sung") {
            Ok(()) => self.status = "Done \u{2014} Ctrl+Z takes it back".to_string(),
            Err(why) => self.status = why,
        }
        doc.end_vocal_gesture();
        self.refresh_vocal();
    }

    // ---------------------------------------------------- the engine window

    pub(super) fn toggle_singer_editor(&mut self) {
        let open = self.vocal.as_ref().is_some_and(|v| v.editor_open);
        let Some(doc) = &mut self.options.document else {
            return;
        };
        if open {
            doc.close_singer_editor();
            self.status = "Closed the singer's window".to_string();
        } else {
            self.status = doc.open_singer_editor().unwrap_or_else(|e| e);
        }
        self.refresh_vocal();
    }

    // ------------------------------------------------------------ terms

    pub(super) fn singer_terms_rows(
        &self,
        singer: usize,
    ) -> Vec<(crate::canvas::MenuEntry, TermsRow)> {
        let Some(doc) = &self.options.document else {
            return Vec::new();
        };
        let singers = doc.singers();
        let Some(s) = singers.get(singer) else {
            return Vec::new();
        };
        crate::canvas::terms_menu_rows_with(
            &s.name,
            s.terms.as_deref().unwrap_or(""),
            s.terms_summary.as_deref(),
            s.terms_foreign,
        )
    }

    pub(super) fn choose_singer_terms(
        &mut self,
        channel: usize,
        singer: usize,
        language: &str,
        index: usize,
    ) {
        let Some((_, row)) = self.singer_terms_rows(singer).into_iter().nth(index) else {
            return;
        };
        if row == TermsRow::Translate {
            if let Some(doc) = &mut self.options.document
                && let Some(terms) = doc.singers().get(singer).and_then(|s| s.terms.clone())
            {
                doc.translate_terms(&terms);
                self.status = "Opened the terms in Google Translate, in your browser".to_string();
            }
            return;
        }
        if row != TermsRow::Agree {
            if row == TermsRow::Cancel {
                self.status =
                    "Nothing changed \u{2014} that voice sings once its terms are agreed to"
                        .to_string();
            }
            return;
        }
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let singers = doc.singers();
        let Some(s) = singers.get(singer) else {
            return;
        };
        doc.agree_singer_terms(&s.engine, &s.id);
        doc.set_singer(
            channel,
            Some(fontelle_model::VocalVoice::new(
                s.engine.clone(),
                s.id.clone(),
                language,
            )),
        );
        self.status = format!(
            "{} sings this track, in {}",
            s.name,
            crate::canvas::language_name(language)
        );
        self.refresh_studio();
        self.tree.invalidate(PANEL);
    }

    pub(super) fn bank_terms_rows(&self, id: &str) -> Vec<(crate::canvas::MenuEntry, TermsRow)> {
        let Some(page) = &self.vocal_edit.page else {
            return Vec::new();
        };
        if let Some(r) = page.installed.iter().find(|r| r.id == id) {
            return crate::canvas::terms_menu_rows_with(
                &r.name,
                r.terms.as_deref().unwrap_or(""),
                r.terms_summary.as_deref(),
                r.terms_foreign,
            );
        }
        if let Some(r) = page.catalogue.iter().find(|r| r.id == id) {
            let text = format!(
                "{}\n\nCommercial use: {} \u{b7} Credit required: {} \u{b7} AI training: {}\n\nThe author's full terms: {}",
                r.terms_summary, r.commercial, r.credit, r.ai_training, r.terms_url
            );
            let mut rows = crate::canvas::terms_menu_rows(&r.name, &text);
            rows.retain(|(_, row)| *row != TermsRow::Agree);
            return rows;
        }
        Vec::new()
    }

    pub(super) fn choose_bank_terms(&mut self, id: &str, index: usize) {
        let Some((_, row)) = self.bank_terms_rows(id).into_iter().nth(index) else {
            return;
        };
        if row == TermsRow::Translate {
            let terms = self
                .vocal_edit
                .page
                .as_ref()
                .and_then(|p| p.installed.iter().find(|r| r.id == id))
                .and_then(|r| r.terms.clone());
            if let (Some(terms), Some(doc)) = (terms, &mut self.options.document) {
                doc.translate_terms(&terms);
                self.status = "Opened the terms in Google Translate, in your browser".to_string();
            }
            return;
        }
        if row == TermsRow::Agree
            && let Some(doc) = &mut self.options.document
        {
            doc.agree_voicebank_terms(id);
            self.status = "Terms agreed".to_string();
            self.refresh_voicebank_page();
        }
    }

    // ------------------------------------------------------------- the page

    pub(super) fn open_voicebank_page(&mut self, tab: VoicebankTab) {
        self.vocal_edit.page = Some(PageState {
            tab,
            ..PageState::default()
        });
        self.refresh_voicebank_page();
    }

    pub(super) fn refresh_voicebank_page(&mut self) {
        // An install or uninstall may have put another picture at a path.
        self.vocal_edit.portraits.clear();
        let Some(doc) = &self.options.document else {
            return;
        };
        let installed = doc.voicebanks();
        let catalogue = doc.catalogue();
        let folder = doc.voicebanks_folder();
        if let Some(page) = &mut self.vocal_edit.page {
            page.installed = installed;
            page.catalogue = catalogue;
            page.folder = folder;
        }
        self.lay_out_voicebank_page();
        self.tree.invalidate_rect(self.layout.window);
    }

    fn lay_out_voicebank_page(&mut self) {
        let window = self.layout.window;
        if let Some(page) = &mut self.vocal_edit.page {
            let count = match page.tab {
                VoicebankTab::Installed => page.installed.len(),
                VoicebankTab::Find => page.catalogue.len(),
            };
            page.layout = Some(crate::canvas::voicebank_page_layout(
                window,
                page.tab,
                count,
                page.scroll,
            ));
        }
    }

    pub(super) fn press_voicebank_page(&mut self, x: f32, y: f32) {
        let Some(page) = &self.vocal_edit.page else {
            return;
        };
        let Some(layout) = &page.layout else {
            return;
        };
        let tab = page.tab;
        let hit = crate::canvas::voicebank_page_hit(layout, x, y);
        match hit {
            Some(PageHit::Outside | PageHit::Close) => {
                self.vocal_edit.page = None;
                self.tree.invalidate_rect(self.layout.window);
            }
            Some(PageHit::Tab(t)) => {
                if let Some(p) = &mut self.vocal_edit.page {
                    p.tab = t;
                    p.scroll = 0.0;
                }
                self.refresh_voicebank_page();
            }
            Some(PageHit::InstallFromFile) => self.choose_and_install_voicebank(),
            Some(PageHit::OpenFolder) => {
                if let Some(doc) = &mut self.options.document {
                    doc.reveal_voicebanks_dir();
                }
            }
            Some(PageHit::Reveal(i)) => {
                if let (Some(r), Some(doc)) = (page.installed.get(i), &mut self.options.document) {
                    doc.reveal_voicebank(&r.id);
                }
            }
            Some(PageHit::Sample(i)) => {
                let Some(id) = page
                    .installed
                    .get(i)
                    .filter(|r| r.sample.is_some())
                    .map(|r| r.id.clone())
                else {
                    return;
                };
                self.preview_voicebank_sample(&id);
            }
            Some(PageHit::Terms(i)) => {
                let id = match tab {
                    VoicebankTab::Installed => page.installed.get(i).map(|r| r.id.clone()),
                    VoicebankTab::Find => page.catalogue.get(i).map(|r| r.id.clone()),
                };
                if let Some(id) = id {
                    let bounds = self.layout.window;
                    self.open_menu(MenuTarget::BankTerms(id), x, y, bounds);
                }
            }
            Some(PageHit::Action(i)) => {
                let Some(doc) = &mut self.options.document else {
                    return;
                };
                let said = match tab {
                    VoicebankTab::Installed => match page.installed.get(i) {
                        Some(r) => doc.uninstall_voicebank(&r.id),
                        None => return,
                    },
                    VoicebankTab::Find => match page.catalogue.get(i) {
                        Some(r) if !r.waiting => doc.catalogue_get(&r.id),
                        _ => return,
                    },
                };
                self.status = said.unwrap_or_else(|e| e);
                self.refresh_voicebank_page();
            }
            _ => {}
        }
    }

    pub(super) fn choose_and_install_voicebank(&mut self) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        match doc.choose_and_install_voicebank() {
            Ok(Some(said)) => self.status = said,
            Ok(None) => {}
            Err(why) => self.status = why,
        }
        self.refresh_voicebank_page();
        self.refresh_vocal();
    }

    /// Dropped files that are voicebanks are installed; the rest go on to
    /// wherever they were dropped.
    pub(super) fn install_dropped_voicebanks(
        &mut self,
        paths: Vec<std::path::PathBuf>,
    ) -> Vec<std::path::PathBuf> {
        let (banks, rest): (Vec<_>, Vec<_>) =
            paths.into_iter().partition(|p| is_voicebank_package(p));
        for path in banks {
            let Some(doc) = &mut self.options.document else {
                break;
            };
            self.status = doc.install_voicebank(&path).unwrap_or_else(|e| e);
        }
        if self.vocal_edit.page.is_some() {
            self.refresh_voicebank_page();
        }
        rest
    }
}

impl WindowApp {
    // ------------------------------------------------ the singer panel

    /// A press on the singer panel's stage (its buttons, a language chip,
    /// the terms, the portrait). Whether it landed on one — a press on a
    /// control is the knob grid's.
    pub(super) fn press_singer_panel(&mut self, x: f32, y: f32) -> bool {
        use crate::canvas::{SingerPanelButton as B, SingerPanelHit as H};
        let Some(hit) = crate::canvas::singer_panel_hit(&self.singer_layout, x, y) else {
            return false;
        };
        let Some(view) = self.singer_panel.clone() else {
            return false;
        };
        let Some(channel) = self.options.document.as_ref().map(|d| d.selected_channel()) else {
            return false;
        };
        match hit {
            H::Button(B::ChangeSinger) | H::Portrait => {
                // The picker is the studio's: it opens over the main window,
                // which is brought forward to show it.
                self.open_singer_picker(channel);
                if let Some(live) = &self.live {
                    live.window.focus_window();
                }
                self.status = "Choose a singer in the studio window".to_string();
                self.tree.invalidate_rect(self.layout.window);
            }
            H::Button(B::Sample) => {
                if let Some(bank) = &view.bank {
                    self.preview_voicebank_sample(bank);
                }
            }
            H::Button(B::OpenEditor) => self.toggle_singer_editor(),
            H::Button(B::Voicebanks) => {
                self.open_voicebank_page(VoicebankTab::Installed);
                if let Some(live) = &self.live {
                    live.window.focus_window();
                }
            }
            H::Button(B::FindSingers) => {
                self.open_voicebank_page(VoicebankTab::Find);
                if let Some(live) = &self.live {
                    live.window.focus_window();
                }
            }
            H::Language(n) => {
                let Some(singer) = &view.singer else {
                    return true;
                };
                let Some(tag) = singer.languages.get(n) else {
                    return true;
                };
                if let Some(doc) = &mut self.options.document {
                    doc.set_singer(
                        channel,
                        Some(fontelle_model::VocalVoice::new(
                            singer.engine.clone(),
                            singer.id.clone(),
                            tag.clone(),
                        )),
                    );
                }
                self.status = format!(
                    "{} sings this track in {}",
                    singer.name,
                    crate::canvas::language_name(tag)
                );
                self.refresh_studio();
            }
            H::Terms => {
                let Some(singer) = &view.singer else {
                    return true;
                };
                let index = self.options.document.as_ref().and_then(|doc| {
                    doc.singers()
                        .iter()
                        .position(|s| s.engine == singer.engine && s.id == singer.id)
                });
                if let Some(index) = index {
                    let bounds = self
                        .editors
                        .iter()
                        .find(|e| e.kind == EditorKind::Instrument)
                        .map_or(self.singer_layout.body, |e| e.panel.body);
                    let target = MenuTarget::SingerTerms {
                        channel,
                        singer: index,
                        language: view.language.clone(),
                    };
                    self.open_menu(target, x, y, bounds);
                }
            }
        }
        self.redraw_editor(EditorKind::Instrument);
        true
    }

    // ------------------------------------------------------- the picker

    /// Opens the singer picker for `channel`: a grid of cards, the search
    /// focused.
    pub(super) fn open_singer_picker(&mut self, channel: usize) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let current = doc.vocal_voice(channel);
        let singers = doc.singers();
        let problems = doc.singer_problems();
        // The track's own instrument's singers only.
        let instrument = doc.vocal_instrument(channel).flatten();
        let instrument_name = match &instrument {
            None => crate::document::FONTELLE_SINGER.to_string(),
            Some(engine) => doc
                .singer_instruments()
                .into_iter()
                .find(|i| i.engine.as_deref() == Some(engine.as_str()))
                .map_or_else(|| engine.clone(), |i| i.name),
        };
        // The card singing now is focused, so Enter keeps it.
        self.vocal_edit.picker = Some(PickerState {
            channel,
            singers,
            problems,
            current,
            instrument,
            instrument_name,
            ..PickerState::default()
        });
        self.lay_out_singer_picker();
        if let Some(picker) = &mut self.vocal_edit.picker
            && let Some(layout) = &picker.layout
        {
            picker.focus = layout.cards.iter().position(|c| {
                picker.singers.get(c.singer).is_some_and(|s| {
                    picker
                        .current
                        .as_ref()
                        .is_some_and(|v| v.engine == s.engine && v.singer == s.id)
                })
            });
        }
        self.tree.invalidate_rect(self.layout.window);
    }

    pub(super) fn close_singer_picker(&mut self) {
        self.vocal_edit.picker = None;
        self.tree.invalidate_rect(self.layout.window);
    }

    fn lay_out_singer_picker(&mut self) {
        let window = self.layout.window;
        if let Some(p) = &mut self.vocal_edit.picker {
            let shown = crate::canvas::picker_filter_for(
                &p.singers,
                p.query.text(),
                p.instrument.as_deref(),
            );
            // Engines that did not load are the Fontelle Singer picker's
            // to say: an engine's own picker is that engine, loaded.
            let problems = if p.instrument.is_none() {
                p.problems.len()
            } else {
                0
            };
            let layout = crate::canvas::singer_picker_layout_for(
                window,
                &p.singers,
                &shown,
                p.scroll,
                problems,
                p.instrument.is_none(),
            );
            p.scroll = p
                .scroll
                .clamp(0.0, (layout.content_height - layout.list.height).max(0.0));
            if p.focus.is_some_and(|f| f >= layout.cards.len()) {
                p.focus = None;
            }
            p.layout = Some(layout);
        }
    }

    /// Chooses `singer` in `language` (or the language the track has when
    /// the singer sings it, else its first) for the picker's channel.
    fn pick_singer(&mut self, singer: usize, language: Option<String>) {
        let Some(picker) = self.vocal_edit.picker.take() else {
            return;
        };
        let Some(s) = picker.singers.get(singer) else {
            return;
        };
        let language = language
            .or_else(|| {
                picker
                    .current
                    .as_ref()
                    .map(|v| v.language.clone())
                    .filter(|l| s.languages.contains(l))
            })
            .or_else(|| s.languages.first().cloned())
            .unwrap_or_default();
        self.tree.invalidate_rect(self.layout.window);
        // By the singer's place in the host's list, as the menu's rows are.
        let index = self
            .options
            .document
            .as_ref()
            .and_then(|doc| {
                doc.singers()
                    .iter()
                    .position(|x| x.engine == s.engine && x.id == s.id)
            })
            .unwrap_or(singer);
        self.choose_singer_row(
            picker.channel,
            Some(SingerRow::Choose {
                singer: index,
                language,
            }),
        );
    }

    pub(super) fn press_singer_picker(&mut self, x: f32, y: f32) {
        let Some(layout) = self
            .vocal_edit
            .picker
            .as_ref()
            .and_then(|p| p.layout.clone())
        else {
            return;
        };
        let channel = self.vocal_edit.picker.as_ref().map_or(0, |p| p.channel);
        match crate::canvas::singer_picker_hit(&layout, x, y) {
            Some(PickerHit::Outside | PickerHit::Close) => self.close_singer_picker(),
            Some(PickerHit::Card(singer)) => self.pick_singer(singer, None),
            Some(PickerHit::Language(singer, language)) => self.pick_singer(singer, Some(language)),
            Some(PickerHit::Nobody) => {
                let picker = self.vocal_edit.picker.clone();
                self.close_singer_picker();
                match picker.and_then(|p| p.instrument.map(|e| (e, p.current))) {
                    // An engine's track with nobody chosen is still the
                    // engine's.
                    Some((engine, current)) => {
                        let language = current.map_or_else(|| "en".to_string(), |v| v.language);
                        if let Some(doc) = &mut self.options.document {
                            doc.set_singer(
                                channel,
                                Some(fontelle_model::VocalVoice::new(engine, "", language)),
                            );
                        }
                        self.status =
                            "Nobody sings this track \u{2014} its notes and words are kept"
                                .to_string();
                        self.refresh_studio();
                        self.tree.invalidate(PANEL);
                    }
                    None => self.choose_singer_row(channel, Some(SingerRow::Nobody)),
                }
            }
            Some(PickerHit::ChangeInstrument) => {
                self.close_singer_picker();
                let (x, y) = self.cursor;
                let bounds = self.layout.window;
                self.open_menu(MenuTarget::ChangeInstrument(channel), x, y, bounds);
            }
            Some(PickerHit::Install) => {
                self.close_singer_picker();
                self.choose_and_install_voicebank();
            }
            Some(PickerHit::Manage) => {
                self.close_singer_picker();
                self.open_voicebank_page(VoicebankTab::Installed);
            }
            Some(PickerHit::Search | PickerHit::Page) | None => {}
        }
    }

    pub(super) fn scroll_singer_picker(&mut self, dy: f32) {
        if let Some(p) = &mut self.vocal_edit.picker {
            p.scroll = (p.scroll - dy * 40.0).max(0.0);
        }
        self.lay_out_singer_picker();
        self.tree.invalidate_rect(self.layout.window);
    }

    /// A key while the picker is open: it takes them all. Arrows move
    /// between cards, Tab through the focused card's languages, Enter
    /// chooses, Esc closes; anything else is the search.
    pub(super) fn singer_picker_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.vocal_edit.picker.is_none() || self.menu.is_some() {
            return false;
        }
        if event.state != winit::event::ElementState::Pressed {
            return true;
        }
        use winit::keyboard::{Key, NamedKey};
        let step = match &event.logical_key {
            Key::Named(NamedKey::ArrowLeft) => Some(PickerMove::Left),
            Key::Named(NamedKey::ArrowRight) => Some(PickerMove::Right),
            Key::Named(NamedKey::ArrowUp) => Some(PickerMove::Up),
            Key::Named(NamedKey::ArrowDown) => Some(PickerMove::Down),
            Key::Named(NamedKey::PageUp) => Some(PickerMove::Home),
            Key::Named(NamedKey::PageDown) => Some(PickerMove::End),
            _ => None,
        };
        if let Some(step) = step {
            if let Some(p) = &mut self.vocal_edit.picker
                && let Some(layout) = &p.layout
            {
                p.focus = crate::canvas::picker_move(layout, p.focus, step);
                p.language = 0;
                // Keep the focused card in sight.
                if let Some(card) = p.focus.and_then(|f| layout.cards.get(f)) {
                    if card.frame.y < layout.list.y {
                        p.scroll -= layout.list.y - card.frame.y + 4.0;
                    } else if card.frame.bottom() > layout.list.bottom() {
                        p.scroll += card.frame.bottom() - layout.list.bottom() + 4.0;
                    }
                }
            }
            self.lay_out_singer_picker();
            self.tree.invalidate_rect(self.layout.window);
            return true;
        }
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => self.close_singer_picker(),
            Key::Named(NamedKey::Tab) => {
                if let Some(p) = &mut self.vocal_edit.picker
                    && let Some(layout) = &p.layout
                    && let Some(card) = p.focus.and_then(|f| layout.cards.get(f))
                {
                    let n = card.languages.len().max(1);
                    p.language = (p.language + 1) % n;
                }
                self.tree.invalidate_rect(self.layout.window);
            }
            Key::Named(NamedKey::Enter) => {
                let chosen = self.vocal_edit.picker.as_ref().and_then(|p| {
                    let layout = p.layout.as_ref()?;
                    let card = layout.cards.get(p.focus.unwrap_or(0))?;
                    let language = (p.focus.is_some() && p.language > 0)
                        .then(|| card.languages.get(p.language).map(|l| l.0.clone()))
                        .flatten();
                    Some((card.singer, language))
                });
                if let Some((singer, language)) = chosen {
                    self.pick_singer(singer, language);
                }
            }
            _ => {
                let (ctrl, shift) = (self.modifiers.control_key(), self.modifiers.shift_key());
                let mut clipboard = std::mem::take(&mut self.clipboard_text);
                if let Some(p) = &mut self.vocal_edit.picker {
                    let before = p.query.text().to_string();
                    let _ = crate::canvas::text_key(
                        &mut p.query,
                        &event.logical_key,
                        ctrl,
                        shift,
                        &mut clipboard,
                    );
                    if p.query.text() != before {
                        p.scroll = 0.0;
                        p.focus = None;
                    }
                }
                self.clipboard_text = clipboard;
                self.lay_out_singer_picker();
                if let Some(p) = &mut self.vocal_edit.picker
                    && p.focus.is_none()
                    && p.layout.as_ref().is_some_and(|l| !l.cards.is_empty())
                    && !p.query.text().is_empty()
                {
                    p.focus = Some(0);
                }
                self.tree.invalidate_rect(self.layout.window);
            }
        }
        true
    }
}

/// Whether a dropped path looks like a voicebank package: an archive a bank
/// ships as, or a folder with a bank's own files in it (or one level down).
pub(super) fn is_voicebank_package(path: &std::path::Path) -> bool {
    if path.is_dir() {
        let marks = [
            "oto.ini",
            "dsconfig.yaml",
            "character.txt",
            "character.yaml",
            "vocoder.yaml",
        ];
        let has = |dir: &std::path::Path| marks.iter().any(|m| dir.join(m).is_file());
        return has(path)
            || std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| e.path().is_dir() && has(&e.path()));
    }
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "zip" | "oudep" | "7z" | "uar"
        )
    })
}

/// The vocal layer as the roll draws it, from the window's parts (so the
/// draw can borrow them beside the renderer).
pub(super) fn roll_vocal_of<'a>(
    e: &'a VocalEditing,
    view: &'a crate::document::VocalView,
    lyric: Option<&'a crate::canvas::LyricEditor>,
    marks: Option<crate::render::RenameMarks>,
    caption: Option<crate::layout::Rect>,
) -> crate::render::vocal::RollVocal<'a> {
    crate::render::vocal::RollVocal {
        view,
        editing: lyric.map(|l| {
            (
                l.ids(),
                e.lyric_shown
                    .as_ref()
                    .map_or(l.entry().text(), |c| c.text.as_str()),
            )
        }),
        lyric_underline: e.lyric_underline,
        marks,
        caption,
        strips: e.strips,
        tabs: &e.tabs,
        curve: e.curve.as_ref(),
        lane: e.lane.as_ref().map(|(s, c)| (s.clone(), c)),
        cells: &e.cells,
        phoneme_field: e.field.as_ref().map(|(n, t)| {
            (
                *n,
                e.phoneme_shown
                    .as_ref()
                    .map_or(t.text(), |c| c.text.as_str()),
                e.suggestions.as_slice(),
            )
        }),
        phoneme_marks: e.phoneme_marks,
        phoneme_underline: e.phoneme_underline,
        pitch_pen: e.pitch_pen,
        pitch_button: e.pitch_button,
        phoneme_anchor: e.phoneme_anchor,
        drawn: &e.drawn,
        px_per_second: e.px_per_second,
        engine_chip: e.engine_chip,
        note_button: e.note_button,
        portrait: view
            .portrait
            .as_deref()
            .and_then(|path| e.portraits.peek(path)),
    }
}

/// The picker as drawn, from its state and layout.
fn picker_view<'a>(
    p: &'a PickerState,
    layout: &'a SingerPickerLayout,
) -> crate::render::vocal::SingerPickerView<'a> {
    crate::render::vocal::SingerPickerView {
        layout,
        singers: &p.singers,
        query: p.query.text(),
        focus: p.focus,
        language: p.language,
        current: p.current.as_ref(),
        problems: if p.instrument.is_none() {
            &p.problems
        } else {
            &[]
        },
        instrument: &p.instrument_name,
        built_in: p.instrument.is_none(),
    }
}

/// The page and the picker as the window draws them over everything.
pub(super) fn vocal_overlay_of(e: &VocalEditing) -> Option<crate::render::vocal::VocalOverlay<'_>> {
    let page = e.page.as_ref().and_then(|p| {
        p.layout
            .as_ref()
            .map(|layout| crate::render::vocal::VoicebankPage {
                layout,
                tab: p.tab,
                installed: &p.installed,
                catalogue: &p.catalogue,
                folder: &p.folder,
            })
    });
    let picker = e
        .picker
        .as_ref()
        .and_then(|p| p.layout.as_ref().map(|layout| picker_view(p, layout)));
    (page.is_some() || picker.is_some()).then_some(crate::render::vocal::VocalOverlay {
        page,
        picker,
        portraits: &e.portraits,
    })
}
