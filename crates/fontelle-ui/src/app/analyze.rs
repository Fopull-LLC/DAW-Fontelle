//! The Analyze Musically window's half of `WindowApp`
//! (`docs/analyze-musically-plan.md` §3): opening it, keeping its view in
//! step with the host's job, and what its presses, keys and wheel do.
//!
//! Everything it decides is `canvas::analyze`'s, which is pure and tested;
//! this is the plumbing between that, the host and the other windows.

use super::*;

use crate::canvas::{
    AnalyzeAction, AnalyzeEditChange, AnalyzeEditOp, AnalyzeHit, AnalyzeScaleRow, AnalyzeTool,
};

impl WindowApp {
    /// **Analyze Musically**, on one audio clip: the host starts (or finds)
    /// the analysis, and the window opens on it.
    pub(super) fn analyze_musically(&mut self, clip: fontelle_types::ClipId) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        match doc.analyze_musically(clip) {
            Ok(said) => {
                self.status = said;
                // A new clip is a new look at it: the page, the window's
                // scale and the switches stay, the rest starts over.
                self.analyze_state = self.analyze_state.for_another_clip();
                self.analyze_fitted = false;
                self.analyze_revision = u64::MAX;
                self.refresh_analyze();
                self.open_editor(EditorKind::Analyze);
                self.refresh_editors();
            }
            Err(said) => self.status = said,
        }
        self.tree.invalidate(TRANSPORT);
    }

    /// The host's view again, if it has changed since the copy the window
    /// holds.
    pub(super) fn refresh_analyze(&mut self) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let revision = doc.analysis_revision();
        if revision == self.analyze_revision {
            return;
        }
        self.analyze_revision = revision;
        let named = self.analyze.as_ref().map(|v| v.name.clone());
        self.analyze = doc.analyze_view();
        if self.analyze.as_ref().map(|v| v.name.clone()) != named {
            self.refresh_editors();
        } else {
            self.relayout_editors();
        }
        self.redraw_editor(EditorKind::Analyze);
    }

    /// How the analysis is getting on, once a frame: the view follows it
    /// while it runs, and its end is said once.
    pub(super) fn poll_analysis(&mut self) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        match doc.poll_analysis() {
            crate::document::JobPoll::Idle => {}
            crate::document::JobPoll::Running(_) => self.refresh_analyze(),
            crate::document::JobPoll::Finished(result) => {
                self.refresh_analyze();
                if let Err(said) = result {
                    self.status = said.clone();
                    self.show_toast(said, false);
                    self.tree.invalidate(TRANSPORT);
                }
            }
        }
    }

    /// The window closed: the host stops the job and forgets the view.
    pub(super) fn closed_analyze(&mut self) {
        if let Some(doc) = &mut self.options.document {
            doc.close_analysis();
        }
        if let Some(doc) = &mut self.options.document {
            doc.analysis_stop();
        }
        self.analyze_state.playhead = None;
        self.analyze = None;
        self.analyze_revision = u64::MAX;
        self.analyze_tip = None;
        self.end_analyze_audition();
    }

    pub(super) fn relayout_analyze(&mut self, body: crate::layout::Rect) {
        let Some(view) = self.analyze.clone() else {
            self.analyze_layout = crate::canvas::AnalyzeLayout {
                body,
                ..Default::default()
            };
            return;
        };
        // Measured by the shaper, so the chips fit what is drawn.
        let font = self.options.theme.font.clone();
        let mut layout = crate::render::lay_out_analyze(
            &mut self.labels,
            &mut self.text,
            &font,
            body,
            &view,
            &self.analyze_state,
        );
        // Fitted to the notes until the analysis is done, then left where the
        // person puts it.
        if !self.analyze_fitted {
            self.analyze_state.fit(&view, &layout);
            self.analyze_fitted = view.detected.is_some();
            layout = crate::render::lay_out_analyze(
                &mut self.labels,
                &mut self.text,
                &font,
                body,
                &view,
                &self.analyze_state,
            );
        }
        self.analyze_layout = layout;
    }

    /// Every string the window draws, shaped in the style it is drawn in,
    /// and the tip.
    pub(super) fn shape_analyze_labels(&mut self) {
        let Some(view) = self.analyze.clone() else {
            return;
        };
        let font = self.options.theme.font.clone();
        crate::render::shape_analyze(
            &mut self.labels,
            &mut self.text,
            &font,
            &view,
            &self.analyze_state,
            &self.analyze_layout,
        );
        if let Some(tip) = self.analyze_tip.clone() {
            self.labels.ensure(&tip, &font, &mut self.text);
        }
    }

    /// The tip to draw now: `None` until the pointer has rested.
    pub(super) fn due_analyze_tip(&self) -> Option<&str> {
        if self.menu.is_some() || self.analyze_state.dragging_marquee() {
            return None;
        }
        let tip = self.analyze_tip.as_deref()?;
        (self.analyze_tip_since.elapsed() >= crate::tooltip::TOOLTIP_DELAY).then_some(tip)
    }

    /// One frame of its own when the tip falls due, as Flopsynth's.
    pub(super) fn tick_analyze_tip(&mut self) {
        if self.due_analyze_tip().is_some() && !self.analyze_tip_drawn {
            self.analyze_tip_drawn = true;
            self.redraw_editor(EditorKind::Analyze);
        } else if self.analyze_tip.is_none() && self.analyze_tip_drawn {
            self.analyze_tip_drawn = false;
            self.redraw_editor(EditorKind::Analyze);
        }
    }

    pub(super) fn hover_analyze(&mut self, x: f32, y: f32) {
        let Some(view) = self.analyze.clone() else {
            return;
        };
        if self.analyze_state.dragging_marquee() {
            self.analyze_state
                .drag_marquee(&self.analyze_layout, &view, (x, y));
        }
        if self.analyze_state.dragging_note() {
            let free = self.modifiers.alt_key();
            let lane = self.analyze_layout.lane.clone();
            match self.analyze_state.drag_note(&lane, &view, (x, y), free) {
                Some(Ok(changes)) => self.send_analysis_edits(&changes, true),
                Some(Err(said)) => self.analyze_says(said),
                None => {}
            }
        }
        let t = self.analyze_layout.lane.t_of(&self.analyze_state, x);
        if self.analyze_state.drag_ruler(t) {
            self.redraw_editor(EditorKind::Analyze);
        }
        let hit =
            crate::canvas::analyze_hit(&self.analyze_layout, &view, &self.analyze_state, x, y);
        let popover = |hit: Option<AnalyzeHit>| {
            matches!(hit, Some(AnalyzeHit::ScaleName | AnalyzeHit::ScaleCopy))
        };
        let relayout = popover(hit) != popover(self.analyze_state.hover);
        self.analyze_state.hover = hit;
        if relayout {
            self.relayout_editors();
        }
        let tip = match hit {
            _ if self.analyze_state.dragging_marquee() => None,
            Some(hit) => crate::canvas::analyze_tip(&hit, &view, &self.analyze_state),
            None => None,
        };
        if tip != self.analyze_tip {
            self.analyze_tip = tip;
            self.analyze_tip_since = std::time::Instant::now();
            self.analyze_tip_drawn = false;
        }
    }

    pub(super) fn analyze_pointer(&self) -> Pointer {
        match self.analyze_state.hover {
            None | Some(AnalyzeHit::Tuning | AnalyzeHit::Bpm | AnalyzeHit::Job) => Pointer::Default,
            Some(AnalyzeHit::Badge | AnalyzeHit::Ruler | AnalyzeHit::Later) => Pointer::Default,
            Some(AnalyzeHit::Lane) => Pointer::Default,
            Some(AnalyzeHit::NoteEnd(..)) => Pointer::ResizeX,
            Some(AnalyzeHit::Readout) => Pointer::Default,
            Some(_) => Pointer::Hand,
        }
    }

    fn analyze_bounds(&self) -> crate::layout::Rect {
        self.editors
            .iter()
            .find(|e| e.kind == EditorKind::Analyze)
            .map(|e| e.panel.frame)
            .unwrap_or(self.layout.window)
    }

    pub(super) fn press_analyze(&mut self, button: MouseButton, x: f32, y: f32) {
        let Some(view) = self.analyze.clone() else {
            return;
        };
        if button == MouseButton::Right {
            // The key's menu from a right-click on it, as well as from ▾.
            if matches!(
                crate::canvas::analyze_hit(&self.analyze_layout, &view, &self.analyze_state, x, y),
                Some(AnalyzeHit::ScaleName | AnalyzeHit::ScaleMenu | AnalyzeHit::ScaleCopy)
            ) {
                self.open_analyze_scale_menu();
            }
            return;
        }
        let modifiers = crate::canvas::Modifiers {
            ctrl: self.modifiers.control_key(),
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
        };
        let Some(action) = crate::canvas::analyze_press(
            &self.analyze_layout,
            &view,
            &mut self.analyze_state,
            x,
            y,
            modifiers,
        ) else {
            return;
        };
        match action {
            AnalyzeAction::CopyScale => self.copy_analysis_scale(),
            AnalyzeAction::ScaleMenu => self.open_analyze_scale_menu(),
            AnalyzeAction::Mode(mode) => {
                self.analyze_state.mode = Some(mode);
                self.analyze_state.clear_selection();
                self.analyze_fitted = false;
                self.relayout_editors();
            }
            AnalyzeAction::Page(page) => {
                self.analyze_state.page = page;
                self.relayout_editors();
            }
            AnalyzeAction::ToggleSpectrogram => self.toggle_analyze_spectrogram(),
            AnalyzeAction::ToggleChords => self.toggle_analyze_chords(),
            AnalyzeAction::ToggleScaleRows => {
                self.analyze_state.show_scale = !self.analyze_state.show_scale;
            }
            AnalyzeAction::WindowScale => {
                let chip = self.analyze_layout.window_scale;
                let bounds = self.analyze_bounds();
                self.open_menu(
                    MenuTarget::AnalyzeWindowScale,
                    chip.x,
                    chip.bottom(),
                    bounds,
                );
            }
            AnalyzeAction::Audition(key) => self.analyze_audition(key),
            AnalyzeAction::CopyNotes => self.copy_analysis_notes(),
            AnalyzeAction::MakeNoteClip => self.make_analysis_clip(),
            AnalyzeAction::ToggleKeepBends => {
                self.analyze_state.keep_bends = !self.analyze_state.keep_bends;
            }
            AnalyzeAction::Selected | AnalyzeAction::Marquee => {}
            // Ty: *"when i preview a note its not playing that section
            // repitched to the new note, its just playing like a synth
            // wave"* — a note clicked plays its own span of the audio, as
            // edited.
            AnalyzeAction::PlayNote(index) => self.play_analysis_note(index),
            AnalyzeAction::Tool(tool) => self.analyze_state.tool = tool,
            AnalyzeAction::PlayStop => self.analyze_play_stop(),
            AnalyzeAction::ToggleOriginal => self.toggle_analyze_original(),
            AnalyzeAction::Seek(t) => {
                // The cursor is there; playing, it carries on from there.
                if self.analyze_state.playhead.is_some() {
                    let (_, to, looped) = self.analyze_state.space_range(&view);
                    if let Some(doc) = &mut self.options.document {
                        doc.analysis_play(t, to, looped);
                    }
                }
            }
            AnalyzeAction::Render => self.render_analysis(false),
            AnalyzeAction::RenderMenu => {
                let chip = self.analyze_layout.render_menu;
                let bounds = self.analyze_bounds();
                self.open_menu(MenuTarget::AnalyzeRender, chip.x, chip.bottom(), bounds);
            }
            AnalyzeAction::Revert => {
                let said = match &mut self.options.document {
                    Some(doc) => doc.revert_analysis(),
                    None => return,
                };
                self.after_render(said);
            }
        }
        self.redraw_editor(EditorKind::Analyze);
    }

    fn analyze_audition(&mut self, key: u8) {
        self.end_analyze_audition();
        if let Some(doc) = &mut self.options.document {
            doc.audition_on(key, 100, 0);
            self.analyze_held = Some(key);
        }
    }

    fn end_analyze_audition(&mut self) {
        if let Some(key) = self.analyze_held.take()
            && let Some(doc) = &mut self.options.document
        {
            doc.audition_off(key);
        }
    }

    /// The button let go: the marquee ends and a sounding key stops; a
    /// note drag is one undo, and the note is heard where it went.
    pub(super) fn release_analyze(&mut self) {
        self.analyze_state.end_marquee();
        self.analyze_state.end_ruler();
        self.end_analyze_audition();
        let dragged = self.analyze_state.dragged_note();
        if self.analyze_state.end_note_drag() {
            if let Some(doc) = &mut self.options.document {
                doc.end_gesture();
            }
            if let Some(index) = dragged {
                self.play_analysis_note(index);
            }
            self.refresh_studio();
            self.refresh_title();
        }
    }

    /// Note `index`'s own span of the audio, as edited, through the preview.
    fn play_analysis_note(&mut self, index: usize) {
        let Some(view) = &self.analyze else {
            return;
        };
        let Some(note) = self.analyze_state.notes(view).get(index) else {
            return;
        };
        let (start, end) = (note.start, note.end);
        if let Some(doc) = &mut self.options.document {
            doc.analysis_play(start, Some(end), false);
        }
        self.analyze_state.playhead = Some(start);
    }

    /// Space: play from the cursor (round the region when one is set), or
    /// stop.
    fn analyze_play_stop(&mut self) {
        let Some(view) = &self.analyze else {
            return;
        };
        let (from, to, looped) = self.analyze_state.space_range(view);
        let Some(doc) = &mut self.options.document else {
            return;
        };
        if doc.analysis_playhead().is_some() {
            doc.analysis_stop();
            self.analyze_state.playhead = None;
        } else {
            doc.analysis_play(from, to, looped);
            self.analyze_state.playhead = Some(from);
        }
        self.redraw_editor(EditorKind::Analyze);
    }

    /// Enter: the selected notes (or the region), once.
    fn analyze_play_selection(&mut self) {
        let Some(view) = &self.analyze else {
            return;
        };
        let Some((from, to)) = self.analyze_state.selection_range(view) else {
            self.analyze_says(
                "Select notes (or drag out a region on the ruler) to play them".to_string(),
            );
            return;
        };
        if let Some(doc) = &mut self.options.document {
            doc.analysis_play(from, Some(to), false);
        }
        self.analyze_state.playhead = Some(from);
        self.redraw_editor(EditorKind::Analyze);
    }

    /// B.
    fn toggle_analyze_original(&mut self) {
        self.analyze_state.listen_original = !self.analyze_state.listen_original;
        let original = self.analyze_state.listen_original;
        if let Some(doc) = &mut self.options.document {
            doc.analysis_set_original(original);
        }
        self.status = if original {
            "A/B: the original".to_string()
        } else {
            "A/B: your edits".to_string()
        };
        self.tree.invalidate(TRANSPORT);
        self.redraw_editor(EditorKind::Analyze);
    }

    /// The preview's playhead, once a frame: the lane follows it.
    pub(super) fn tick_analyze_playhead(&mut self) {
        if self.analyze.is_none() {
            return;
        }
        let Some(doc) = &self.options.document else {
            return;
        };
        let now = doc.analysis_playhead();
        if now == self.analyze_state.playhead {
            return;
        }
        self.analyze_state.playhead = now;
        if let Some(view) = &self.analyze {
            let lane = self.analyze_layout.lane.clone();
            self.analyze_state.follow(&lane, view);
        }
        self.redraw_editor(EditorKind::Analyze);
    }

    /// Edits to the host; what it refuses is said.
    fn send_analysis_edits(&mut self, changes: &[AnalyzeEditChange], merge: bool) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        if let Err(said) = doc.set_analysis_edits(changes, merge) {
            self.analyze_says(said);
            return;
        }
        if !merge {
            doc.end_gesture();
        }
        self.refresh_analyze();
        self.redraw_editor(EditorKind::Analyze);
    }

    /// A key's edit of the selected notes.
    fn analyze_edit(&mut self, op: AnalyzeEditOp) {
        let Some(view) = &self.analyze else {
            return;
        };
        match self.analyze_state.edit(view, op) {
            Ok(changes) => {
                self.send_analysis_edits(&changes, false);
                self.refresh_title();
            }
            Err(said) => self.analyze_says(said),
        }
    }

    fn analyze_says(&mut self, said: String) {
        if self.status != said {
            self.status = said.clone();
            self.show_toast(said, false);
            self.tree.invalidate(TRANSPORT);
        }
    }

    /// Render to clip (Ctrl+Enter), or as a new clip below.
    pub(super) fn render_analysis(&mut self, below: bool) {
        let said = match &mut self.options.document {
            Some(doc) => doc.render_analysis(below),
            None => return,
        };
        self.after_render(said);
    }

    fn after_render(&mut self, said: Result<String, String>) {
        let ok = said.is_ok();
        let said = said.unwrap_or_else(|e| e);
        self.status = said.clone();
        self.show_toast(said, ok);
        self.analyze_revision = u64::MAX;
        self.refresh_analyze();
        self.refresh_studio();
        self.refresh_title();
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(TRANSPORT);
        self.relayout_editors();
        self.redraw_editor(EditorKind::Analyze);
    }

    pub(super) fn analyze_render_menu(&self) -> Vec<crate::canvas::MenuEntry> {
        vec![
            crate::canvas::MenuEntry::new("Render to clip (replaces its audio, original kept)"),
            crate::canvas::MenuEntry::new("Render as a new clip below"),
        ]
    }

    pub(super) fn wheel_analyze(&mut self, x: f32, y: f32, dx: f32, dy: f32) {
        let Some(view) = self.analyze.clone() else {
            return;
        };
        let lane = self.analyze_layout.lane.clone();
        if !lane.screen.contains(x, y) {
            return;
        }
        let modifiers = crate::canvas::Modifiers {
            ctrl: self.modifiers.control_key(),
            shift: self.modifiers.shift_key(),
            alt: self.modifiers.alt_key(),
        };
        self.analyze_state
            .wheel(&lane, &view, x, y, dx, dy, modifiers);
        self.analyze_fitted = true;
    }

    fn toggle_analyze_spectrogram(&mut self) {
        self.analyze_state.spectrogram = !self.analyze_state.spectrogram;
        self.relayout_editors();
    }

    fn toggle_analyze_chords(&mut self) {
        self.analyze_state.chords = !self.analyze_state.chords;
        self.relayout_editors();
    }

    /// The window's own keys (plan §3.5), from the keymap's Editor context.
    /// Escape lets go of a selection before it closes the window.
    pub(super) fn analyze_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use crate::canvas::Action;
        use winit::keyboard::{Key, NamedKey};
        if event.logical_key == Key::Named(NamedKey::Escape)
            && !self.analyze_state.selected().is_empty()
        {
            self.analyze_state.clear_selection();
            return true;
        }
        // The arrows, the fixed family the keymap leaves alone: the selected
        // notes a semitone, with Shift an octave, with Alt ten cents.
        if let Key::Named(arrow @ (NamedKey::ArrowUp | NamedKey::ArrowDown)) = &event.logical_key {
            let step = if self.modifiers.shift_key() {
                1200.0
            } else if self.modifiers.alt_key() {
                10.0
            } else {
                100.0
            };
            let sign = if *arrow == NamedKey::ArrowUp {
                1.0
            } else {
                -1.0
            };
            self.analyze_edit(AnalyzeEditOp::Nudge(sign * step));
            return true;
        }
        let Some(action) = self.action_of(event, crate::canvas::Context::Editor) else {
            return false;
        };
        match action {
            Action::AnalyzeCopyNotes => self.copy_analysis_notes(),
            Action::AnalyzeCopyScale => self.copy_analysis_scale(),
            Action::AnalyzeSelectAll => {
                if let Some(view) = &self.analyze {
                    self.analyze_state.select_all(view);
                }
            }
            Action::AnalyzeSpectrogram => self.toggle_analyze_spectrogram(),
            Action::AnalyzeChordLane => self.toggle_analyze_chords(),
            // In this window the transport's key is this window's player
            // (plan §3.5): Space plays the audio here, from the cursor.
            Action::Play => self.analyze_play_stop(),
            Action::AnalyzePlaySelection => self.analyze_play_selection(),
            Action::AnalyzeAb => self.toggle_analyze_original(),
            Action::AnalyzeRender => self.render_analysis(false),
            Action::AnalyzeSnap => self.analyze_edit(AnalyzeEditOp::Snap),
            Action::AnalyzeFlatten => self.analyze_edit(AnalyzeEditOp::Flatten),
            Action::AnalyzeVibrato => self.analyze_edit(AnalyzeEditOp::Vibrato),
            // Delete in an editor window is "take away the selected thing":
            // here, the selected notes' edits.
            Action::RemoveBand => self.analyze_edit(AnalyzeEditOp::Reset),
            Action::AnalyzeSelectTool => self.analyze_state.tool = AnalyzeTool::Select,
            Action::AnalyzeMoveTool => self.analyze_state.tool = AnalyzeTool::Move,
            _ => return false,
        }
        true
    }

    /// The key, as the roll's chooser names it, to the clipboard Tune and the
    /// roll paste from — and the desktop's (`copy_scale_text`).
    fn copy_analysis_scale(&mut self) {
        let Some(key) = self.analyze.as_ref().and_then(|v| v.key.clone()) else {
            self.status = "No key has been heard yet".to_string();
            self.tree.invalidate(TRANSPORT);
            return;
        };
        let text = fontelle_types::scale_text(&key.key);
        self.copy_scale_text(&text);
        self.show_toast(
            format!("Scale copied: {text} \u{2014} paste it into Tune or the piano roll's scale"),
            false,
        );
    }

    /// The selected notes (all, with none) to the window's clipboard of
    /// notes, for any piano roll.
    fn copy_analysis_notes(&mut self) {
        let Some(view) = &self.analyze else {
            return;
        };
        let mode = self.analyze_state.effective_mode(view);
        let selection = self.analyze_state.selected();
        let keep = self.analyze_state.keep_bends;
        let Some((notes, origin)) = self
            .options
            .document
            .as_ref()
            .and_then(|doc| doc.analysis_notes(mode, &selection, keep))
            .filter(|(notes, _)| !notes.is_empty())
        else {
            self.status = "There are no notes to copy yet".to_string();
            self.tree.invalidate(TRANSPORT);
            return;
        };
        let count = notes.len();
        self.roll.clipboard_mut().put(notes, Some(origin));
        let said = format!(
            "{count} note(s) copied \u{2014} Ctrl+V in a piano roll pastes them at the playhead, \
             Ctrl+Shift+V where they were heard"
        );
        self.status = said.clone();
        self.show_toast(said, false);
        self.tree.invalidate(TRANSPORT);
    }

    fn make_analysis_clip(&mut self) {
        let Some(view) = &self.analyze else {
            return;
        };
        let mode = self.analyze_state.effective_mode(view);
        let selection = self.analyze_state.selected();
        let keep = self.analyze_state.keep_bends;
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let said = match doc.make_analysis_clip(mode, &selection, keep) {
            Ok(said) => {
                doc.end_gesture();
                said
            }
            Err(said) => said,
        };
        self.status = said.clone();
        self.show_toast(said, true);
        self.refresh_studio();
        self.refresh_title();
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(TRANSPORT);
    }

    fn open_analyze_scale_menu(&mut self) {
        let chip = self.analyze_layout.scale_menu;
        let bounds = self.analyze_bounds();
        self.open_menu(MenuTarget::AnalyzeScale, chip.x, chip.bottom(), bounds);
    }

    /// The ▾ menu's rows, in the order [`crate::canvas::analyze_scale_menu`]
    /// lists them.
    pub(super) fn analyze_scale_menu(
        &self,
    ) -> (Vec<crate::canvas::MenuEntry>, Vec<AnalyzeScaleRow>) {
        match &self.analyze {
            Some(view) => crate::canvas::analyze_scale_menu(view, &self.analyze_state),
            None => (Vec::new(), Vec::new()),
        }
    }

    pub(super) fn choose_analyze_scale(&mut self, index: usize) {
        let (_, rows) = self.analyze_scale_menu();
        let Some(row) = rows.get(index).cloned() else {
            return;
        };
        match row {
            AnalyzeScaleRow::Copy(key) => {
                let text = fontelle_types::scale_text(&key);
                self.copy_scale_text(&text);
            }
            AnalyzeScaleRow::ShowOnLane => {
                self.analyze_state.show_scale = !self.analyze_state.show_scale;
            }
            AnalyzeScaleRow::SetSongKey => {
                let Some(key) = self.analyze.as_ref().and_then(|v| v.key.clone()) else {
                    return;
                };
                if let Some(doc) = &mut self.options.document {
                    doc.set_song_key(Some(key.key.clone()), Vec::new(), Vec::new());
                    doc.end_gesture();
                }
                let said = format!("Song key: {}", fontelle_types::scale_text(&key.key));
                self.status = said.clone();
                self.show_toast(said, true);
                self.refresh_studio();
                self.refresh_title();
                self.tree.invalidate(PANEL);
                self.tree.invalidate(TRANSPORT);
            }
            AnalyzeScaleRow::CopyNotesInScale => {
                if let Some(key) = self.analyze.as_ref().and_then(|v| v.key.clone()) {
                    let text = fontelle_types::scale_notes(&key.key).join(" ");
                    self.copy_scale_text(&text);
                }
            }
        }
        self.redraw_editor(EditorKind::Analyze);
    }

    pub(super) fn analyze_window_scale_menu(&self) -> Vec<crate::canvas::MenuEntry> {
        let current = self.analyze_state.scale;
        let mut entries = vec![crate::canvas::MenuEntry::disabled("Window scale")];
        for scale in crate::canvas::SCALES {
            let label = crate::canvas::window_scale_label(scale);
            entries.push(if (scale - current).abs() < 0.001 {
                crate::canvas::MenuEntry::disabled(label)
            } else {
                crate::canvas::MenuEntry::new(label)
            });
        }
        entries
    }

    /// The window at another of Flopsynth's scales: it resizes to it.
    pub(super) fn choose_analyze_window_scale(&mut self, index: usize) {
        let Some(scale) = index
            .checked_sub(1)
            .and_then(|which| crate::canvas::SCALES.get(which).copied())
        else {
            return;
        };
        self.analyze_state.scale = scale;
        self.analyze_fitted = false;
        if let Some(editor) = self.editors.iter().find(|e| e.kind == EditorKind::Analyze) {
            let (w, h) = crate::layout::analyze_window_size(scale);
            let (min_w, min_h) = crate::layout::analyze_minimum_size(scale);
            editor
                .window
                .set_min_inner_size(Some(winit::dpi::LogicalSize::new(min_w, min_h)));
            let _ = editor
                .window
                .request_inner_size(winit::dpi::LogicalSize::new(w, h));
        }
        self.relayout_editors();
        self.redraw_editor(EditorKind::Analyze);
    }
}
