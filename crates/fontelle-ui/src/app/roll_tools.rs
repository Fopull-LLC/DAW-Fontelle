//! The piano roll's tools, in the window: opening a dialog where it does not
//! cover the notes, pressing and dragging its rows, applying a tool as one
//! undo, hearing it, and remembering how each was set.
//!
//! `docs/roll-tools-plan.md`. The tools themselves — what they are, their
//! rows, their answers — are `canvas::tools`, and pure; this is the half that
//! needs a window and a document.

use super::*;
use crate::canvas::{
    RollEdit, ToolAction, ToolKind, ToolPress, ToolRow, tools_dialog_layout, tools_dialog_place,
    tools_dialog_press, tools_dialog_size,
};
use fontelle_model::{Note, ToolResult};
use fontelle_types::NoteId;

/// How far a drag across a value goes for one step.
const STEP_PIXELS: f32 = 14.0;

/// A press that travels less than this is a click.
const CLICK_SLOP: f32 = 3.0;

impl WindowApp {
    /// What the tools need to know about the song before a use: the roll's
    /// snap, where the open clip ends, the key, the tempo.
    pub(super) fn sync_tools(&mut self) {
        let unit = self.roll.meter().unit_at(0, self.roll.view.snap);
        self.tools.roll_grid = if unit > 0 {
            unit
        } else {
            fontelle_types::PPQN / 4
        };
        // A looped clip's notes are one period, played again: the period is
        // the end a last note reaches, or it would run on through every pass.
        let period = self
            .clips
            .iter()
            .find(|clip| clip.open && clip.kind == crate::document::ClipKind::Notes)
            .and_then(|clip| clip.loop_length);
        self.tools.clip_end = match (
            self.options.document.as_ref().and_then(|d| d.clip_length()),
            period,
        ) {
            (Some(length), Some(period)) => Some(length.min(period)),
            (length, _) => length,
        };
        // A drum kit has named keys, not a scale.
        self.tools.song_key = if self.key_map.is_named() {
            None
        } else {
            self.song_key.as_ref().and_then(|k| k.mask())
        };
        self.tools.tempo = self.tempo;
    }

    /// The notes a tool acts on: the selection, or with nothing selected
    /// every note the roll shows (plan §3.2) — and which it was.
    pub(super) fn tool_scope(&self) -> (Vec<(NoteId, Note)>, bool) {
        let Some(doc) = &self.options.document else {
            return (Vec::new(), false);
        };
        let notes = doc.notes();
        let chosen: Vec<(NoteId, Note)> = self
            .roll
            .selection()
            .iter()
            .filter_map(|id| notes.get(*id).map(|n| (*id, n.clone())))
            .collect();
        if !chosen.is_empty() {
            return (chosen, false);
        }
        let mut all: Vec<(NoteId, Note)> = notes.iter().map(|(id, n)| (id, n.clone())).collect();
        // An arena has no order to promise; time order is the one a whole
        // clip has.
        all.sort_by_key(|(_, n)| (n.start, n.key));
        (all, true)
    }

    /// *"Strum — 3 selected"*, *"Strum — whole clip (12)"*.
    fn tool_title(&self, kind: ToolKind) -> String {
        let (scope, whole) = self.tool_scope();
        if whole {
            format!("{} \u{2014} whole clip ({})", kind.title(), scope.len())
        } else {
            format!("{} \u{2014} {} selected", kind.title(), scope.len())
        }
    }

    /// The left and right of what a tool would change, on screen.
    fn tool_scope_span(&self) -> Option<(f32, f32)> {
        let (scope, _) = self.tool_scope();
        let grid = self.roll_layout.grid;
        let from = scope.iter().map(|(_, n)| n.start).min()?;
        let to = scope.iter().map(|(_, n)| n.start + n.length).max()?;
        Some((
            crate::canvas::tick_to_x(&self.roll.view, grid, from),
            crate::canvas::tick_to_x(&self.roll.view, grid, to),
        ))
    }

    /// Opens `kind`'s dialog: where it was last dragged to, or beside the
    /// notes it acts on rather than over them.
    pub(crate) fn open_tool_dialog(&mut self, kind: ToolKind) {
        self.silence_tool_hearing();
        self.sync_tools();
        self.place_tool_dialog(kind, None);
    }

    /// Lays `kind`'s dialog out at `at`, or where it belongs if `None`.
    fn place_tool_dialog(&mut self, kind: ToolKind, at: Option<(f32, f32)>) {
        let rows = self.tools.rows(kind);
        let size = tools_dialog_size(&rows, &self.options.theme.metrics);
        let frame = self.roll_layout.frame;
        let chosen = self.tools_at.map(|(dx, dy)| (frame.x + dx, frame.y + dy));
        let at = at.unwrap_or_else(|| {
            tools_dialog_place(
                size,
                self.roll_layout.grid,
                self.tool_scope_span(),
                chosen,
                frame,
            )
        });
        self.tools_panel = Some(tools_dialog_layout(
            kind,
            &rows,
            self.tool_title(kind),
            at,
            frame,
            &self.options.theme.metrics,
        ));
        self.shape_labels();
        self.tree.invalidate(PANEL);
    }

    /// The open dialog laid out again where it is — a mode that shows other
    /// rows, a scope that changed.
    fn relayout_tool_dialog(&mut self) {
        if let Some(panel) = &self.tools_panel {
            let (kind, at) = (panel.kind, (panel.frame.x, panel.frame.y));
            self.place_tool_dialog(kind, Some(at));
        }
    }

    /// The open dialog told the document moved: an undo, a collaborator, a
    /// note drawn — what it acts on may not be what its title says.
    pub(super) fn refresh_tool_dialog(&mut self) {
        let Some(panel) = &self.tools_panel else {
            return;
        };
        if self.tool_title(panel.kind) != panel.title_text {
            self.relayout_tool_dialog();
        }
    }

    /// Shuts the open dialog, remembering how its tool was set.
    pub(super) fn close_tool_dialog(&mut self) -> bool {
        if self.tools_panel.take().is_none() {
            return false;
        }
        self.silence_tool_hearing();
        self.hover_tool = None;
        self.save_tool_settings();
        self.shape_labels();
        self.tree.invalidate(PANEL);
        true
    }

    /// Hands the tools' settings to the host to keep.
    fn save_tool_settings(&mut self) {
        let json = self.tools.to_json();
        if let Some(doc) = &mut self.options.document {
            doc.set_roll_tool_settings(json);
        }
    }

    /// A press while a tool's dialog is open.
    ///
    /// **The dialog stays up** unless the press missed it: set something,
    /// apply, look, apply again. A press that missed closes it, the way
    /// clicking away from anything does.
    pub(crate) fn press_tools_panel(&mut self, x: f32, y: f32) {
        let Some(panel) = &self.tools_panel else {
            return;
        };
        let (kind, frame) = (panel.kind, panel.frame);
        let Some(press) = tools_dialog_press(panel, &self.tools, x, y) else {
            if !frame.contains(x, y) {
                self.close_tool_dialog();
            }
            return;
        };
        self.sync_tools();
        match press {
            ToolPress::Title => {
                self.drag = Drag::ToolTitle {
                    dx: (x - frame.x).round() as i32,
                    dy: (y - frame.y).round() as i32,
                };
            }
            ToolPress::Step(row, delta) => self.step_tool_row(row, delta),
            ToolPress::Cell(row, cell) => {
                self.tools.toggle_cell(row, cell);
                self.tool_changed();
            }
            ToolPress::Row(ToolRow::Apply) => self.run_tool(ToolAction::Apply(kind)),
            ToolPress::Row(ToolRow::Hear) => self.hear_tool(),
            ToolPress::Row(ToolRow::Reset) => {
                self.tools.reset(kind);
                self.tool_changed();
            }
            // A value: a click steps it when the button comes up, a drag
            // scrubs it — which it is is not known until then.
            ToolPress::Row(row) => {
                self.drag = Drag::ToolValue {
                    row,
                    from: x.round() as i32,
                    applied: 0,
                    moved: false,
                };
            }
        }
        self.tree.invalidate(PANEL);
    }

    /// One step of `row`, and everything that follows from it.
    fn step_tool_row(&mut self, row: ToolRow, delta: i32) {
        self.tools.nudge(row, delta);
        self.tool_changed();
    }

    /// A setting changed: the rows (a mode shows others), the labels, and
    /// the sound if it was being heard.
    fn tool_changed(&mut self) {
        self.relayout_tool_dialog();
        // Heard again as it now is: the point of hearing it is hearing what
        // the press changed.
        if self.tool_hearing.is_some() {
            self.hear_tool();
        }
    }

    /// The pointer moved while a press on the dialog is held.
    pub(super) fn drag_tool(&mut self) {
        let (x, y) = self.cursor;
        match self.drag {
            Drag::ToolTitle { dx, dy } => {
                let Some(panel) = &self.tools_panel else {
                    return;
                };
                let kind = panel.kind;
                let frame = self.roll_layout.frame;
                let size = (panel.frame.width, panel.frame.height);
                let at = tools_dialog_place(
                    size,
                    frame,
                    None,
                    Some((x - dx as f32, y - dy as f32)),
                    frame,
                );
                self.tools_at = Some((at.0 - frame.x, at.1 - frame.y));
                self.place_tool_dialog(kind, Some(at));
            }
            Drag::ToolValue {
                row,
                from,
                applied,
                moved,
            } => {
                let travelled = x - from as f32;
                let steps = (travelled / STEP_PIXELS) as i32;
                if steps != applied {
                    self.tools.nudge(row, steps - applied);
                    self.tool_changed();
                }
                self.drag = Drag::ToolValue {
                    row,
                    from,
                    applied: steps,
                    moved: moved || travelled.abs() >= CLICK_SLOP,
                };
            }
            _ => {}
        }
    }

    /// The button came up on a press that began on the dialog. Whether it
    /// was one.
    pub(super) fn release_tool_drag(&mut self) -> bool {
        match self.drag {
            Drag::ToolTitle { .. } => {
                self.drag = Drag::None;
                true
            }
            Drag::ToolValue { row, moved, .. } => {
                self.drag = Drag::None;
                if !moved {
                    // A click: on, or back with Ctrl — the settings tab's
                    // pair of gestures.
                    let delta = if self.modifiers.control_key() { -1 } else { 1 };
                    self.step_tool_row(row, delta);
                }
                self.tree.invalidate(PANEL);
                true
            }
            _ => false,
        }
    }

    /// Enter applies the open dialog; Escape shuts it. Whether the key was
    /// the dialog's.
    pub(super) fn tool_key(&mut self, key: &winit::keyboard::Key) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let Some(kind) = self.tools_panel.as_ref().map(|p| p.kind) else {
            return false;
        };
        match key {
            Key::Named(NamedKey::Enter) => {
                self.sync_tools();
                self.run_tool(ToolAction::Apply(kind));
                true
            }
            Key::Named(NamedKey::Escape) => self.close_tool_dialog(),
            _ => false,
        }
    }

    /// Carries out a tool, a menu row or a key.
    pub(super) fn run_tool(&mut self, action: ToolAction) {
        match action {
            // Not edits: reading a file is the window's job.
            ToolAction::ImportMidi | ToolAction::ImportScore => {
                self.open_import_browser(action);
                return;
            }
            ToolAction::ExportMidi => {
                self.export_midi();
                return;
            }
            ToolAction::Hear => {
                self.hear_tool();
                return;
            }
            ToolAction::Reset => {
                if let Some(kind) = self.tools_panel.as_ref().map(|p| p.kind) {
                    self.tools.reset(kind);
                    self.tool_changed();
                }
                return;
            }
            _ => {}
        }
        self.silence_tool_hearing();
        self.sync_tools();
        if self.roll_showing().is_none() {
            self.status = "Open a clip in the piano roll to use its tools".to_string();
            return;
        }
        let (scope, whole) = self.tool_scope();
        if scope.is_empty() {
            self.status = "There are no notes here for the tools to work on".to_string();
            return;
        }
        let result = match action {
            ToolAction::Select(how) => {
                let chosen = self.roll.selection().to_vec();
                // Choosing *among* everything, whatever is chosen now.
                let mut all: Vec<(NoteId, Note)> = self
                    .options
                    .document
                    .as_ref()
                    .map(|d| d.notes().iter().map(|(id, n)| (id, n.clone())).collect())
                    .unwrap_or_default();
                all.sort_by_key(|(_, n)| (n.start, n.key));
                let picked = self.tools.select(how, &all, &chosen);
                self.status = format!("Selected {} note(s)", picked.len());
                self.roll.select(picked);
                self.relayout_tool_dialog();
                self.tree.invalidate(PANEL);
                return;
            }
            ToolAction::Legato => {
                let ids: Vec<NoteId> = scope.iter().map(|(id, _)| *id).collect();
                let edits = match &self.options.document {
                    Some(doc) => {
                        crate::canvas::legato_edits_to(&ids, doc.notes(), self.tools.clip_end)
                    }
                    None => Vec::new(),
                };
                if !edits.is_empty() {
                    self.apply_roll_edits(edits);
                    if let Some(doc) = &mut self.options.document {
                        doc.end_gesture();
                    }
                }
                self.status = format!("Joined up {} note(s)", scope.len());
                return;
            }
            ToolAction::Apply(kind) => {
                self.last_tool = Some(kind);
                self.tools.result(kind, &scope)
            }
            // The singer renders its phrases by its own road, which does not
            // know about mute yet: a hollow note that still sang would be
            // worse than the refusal.
            ToolAction::ToggleMute
                if self.selected_kind == Some(fontelle_types::InstrumentKind::Vocal) =>
            {
                self.status = "Muting sung notes is not built yet \u{2014} delete the note instead"
                    .to_string();
                return;
            }
            other => match self.tools.instant(other, &scope) {
                Some(result) => result,
                None => return,
            },
        };
        let label = match action {
            ToolAction::Apply(kind) => kind.title().to_string(),
            other => crate::canvas::ToolMenuItem::Run(other).label(),
        };
        if result.is_empty() {
            self.status = format!(
                "Nothing to change \u{2014} {} note(s) are already that way",
                scope.len()
            );
            return;
        }
        self.apply_tool_result(label, &scope, whole, result);
        if let ToolAction::Apply(kind) = action {
            self.tools.applied(kind);
            self.save_tool_settings();
        }
        self.status = format!("{} {} note(s)", action.done(), scope.len());
        self.relayout_tool_dialog();
    }

    /// Writes a tool's answer as one undo, and leaves the result chosen if
    /// a choice was what it acted on — so tools chain: arpeggiate, then
    /// strum what it made.
    fn apply_tool_result(
        &mut self,
        label: String,
        scope: &[(NoteId, Note)],
        whole: bool,
        result: ToolResult,
    ) {
        let removed = result.remove.clone();
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let made = doc.edit(RollEdit::Apply { label, result });
        doc.end_gesture();
        if !whole {
            let mut chosen: Vec<NoteId> = scope
                .iter()
                .map(|(id, _)| *id)
                .filter(|id| !removed.contains(id))
                .collect();
            chosen.extend(made);
            self.roll.select(chosen);
        }
        self.tree.invalidate(PANEL);
        self.refresh_title();
    }

    /// Ctrl+Q: Quantize at its dialog's settings, on the selection or the
    /// whole clip.
    pub(super) fn quantize_now(&mut self) {
        self.run_tool(ToolAction::Apply(ToolKind::Quantize));
    }

    /// Ctrl+Shift+T: the last tool applied, again, at its settings, on what
    /// is chosen now.
    pub(super) fn repeat_tool(&mut self) {
        match self.last_tool {
            Some(kind) => self.run_tool(ToolAction::Apply(kind)),
            None => self.status = "No tool has been used yet".to_string(),
        }
    }

    /// The open dialog's answer, and the notes it is about: what the roll
    /// ghosts and *Hear it* plays.
    pub(super) fn tool_preview(&self) -> Option<(Vec<(NoteId, Note)>, ToolResult)> {
        let panel = self.tools_panel.as_ref()?;
        let (scope, _) = self.tool_scope();
        let result = self.tools.result(panel.kind, &scope);
        Some((scope, result))
    }

    /// Which row of the open dialog `(x, y)` is over.
    pub(super) fn tool_row_at(&self, x: f32, y: f32) -> Option<ToolRow> {
        crate::canvas::tools_dialog_hit(self.tools_panel.as_ref()?, x, y)
    }

    /// Sounds and lets go of whatever the tool being heard has due now.
    pub(super) fn step_tool_hearing(&mut self) {
        // However the dialog went, what it was playing goes with it.
        if self.tools_panel.is_none() {
            self.silence_tool_hearing();
            return;
        }
        let Some(hearing) = &mut self.tool_hearing else {
            return;
        };
        let now = hearing.began.elapsed().as_secs_f64();
        let mut ons = Vec::new();
        let mut offs = Vec::new();
        for (i, &(from, to, key, velocity, pan)) in hearing.notes.iter().enumerate() {
            let due = now >= from && now < to;
            if due && !hearing.on[i] {
                hearing.on[i] = true;
                ons.push((key, velocity, pan));
            } else if !due && hearing.on[i] {
                hearing.on[i] = false;
                offs.push(key);
            }
        }
        let over = hearing.notes.iter().all(|n| now >= n.1);
        if let Some(doc) = &mut self.options.document {
            // Offs first: a note struck again on its own key must not be
            // let go of by the one before it.
            for key in offs {
                doc.audition_off(key);
            }
            for (key, velocity, pan) in ons {
                doc.audition_on(key, velocity, pan);
            }
        }
        if over {
            self.tool_hearing = None;
        }
    }

    /// Stops the tool being heard, letting go of every note it has on.
    pub(super) fn silence_tool_hearing(&mut self) {
        let Some(hearing) = self.tool_hearing.take() else {
            return;
        };
        if let Some(doc) = &mut self.options.document {
            for (i, (_, _, key, _, _)) in hearing.notes.iter().enumerate() {
                if hearing.on[i] {
                    doc.audition_off(*key);
                }
            }
        }
    }

    /// Plays the open dialog's answer once — *Hear it* — from its first
    /// note, at the song's tempo (Ty, 2026-10-20: *"make arpeggiator let
    /// you preview changes"*).
    pub(super) fn hear_tool(&mut self) {
        self.silence_tool_hearing();
        self.silence_chord();
        self.end_preview();
        self.sync_tools();
        let Some((scope, result)) = self.tool_preview() else {
            return;
        };
        let notes = result.after(&scope);
        let Some(first) = notes.iter().map(|n| n.start).min() else {
            self.status = "There are no notes here for the tools to work on".to_string();
            return;
        };
        let seconds_per_tick = 60.0 / self.tempo.max(1.0) / fontelle_types::PPQN as f64;
        let notes: Vec<(f64, f64, u8, u8, i8)> = notes
            .iter()
            .map(|n| {
                let from = (n.start - first) as f64 * seconds_per_tick;
                // A hair short of the next step, so a legato run is heard as
                // notes and not one smear.
                let to = from + (n.length as f64 * seconds_per_tick - 0.005).max(0.02);
                (from, to, n.key, n.velocity, n.pan)
            })
            .collect();
        let on = vec![false; notes.len()];
        self.tool_hearing = Some(ToolHearing {
            notes,
            began: std::time::Instant::now(),
            on,
        });
        self.step_tool_hearing();
    }
}
