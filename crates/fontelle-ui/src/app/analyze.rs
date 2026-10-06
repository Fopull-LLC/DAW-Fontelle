//! The Analyze Musically window's half of `WindowApp`
//! (`docs/analyze-musically-plan.md` §3): opening it, keeping its view in
//! step with the host's job, and what its presses, keys and wheel do.
//!
//! Everything it decides is `canvas::analyze`'s, which is pure and tested;
//! this is the plumbing between that, the host and the other windows.

use super::*;

use crate::canvas::{
    AnalyzeAction, AnalyzeControl, AnalyzeEditChange, AnalyzeEditOp, AnalyzeHit, AnalyzeKnob,
    AnalyzeKnobChange, AnalyzeLaneChange, AnalyzeLaneEnd, AnalyzePage, AnalyzeScaleRow,
    AnalyzeTakeOp, AnalyzeTool, AnalyzeTyping, AnalyzeTypingTarget,
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
        // A render off the window's thread: its progress on the job strip,
        // and once, where it went.
        match doc.poll_analysis_render() {
            crate::document::JobPoll::Idle => {}
            crate::document::JobPoll::Running(_) => self.refresh_analyze(),
            crate::document::JobPoll::Finished(result) => {
                self.after_render(result);
                return;
            }
        }
        let Some(doc) = &mut self.options.document else {
            return;
        };
        if self.analyze.is_some() && self.analyze_state.page == AnalyzePage::Record {
            let meter = doc.analysis_meter();
            if meter != self.analyze_state.meter {
                self.analyze_state.meter = meter;
                self.redraw_editor(EditorKind::Analyze);
            }
        }
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
            // Alt frees a note's pitch from the semitones; on a flatten
            // handle Shift (or Alt) is the fine drag, as on every knob.
            let free = self.modifiers.alt_key()
                || (self.analyze_state.dragging_flatten() && self.modifiers.shift_key());
            let lane = self.analyze_layout.lane.clone();
            match self.analyze_state.drag_note(&lane, &view, (x, y), free) {
                Some(Ok(changes)) => self.send_analysis_edits(&changes, true),
                Some(Err(said)) => self.analyze_says(said),
                None => {}
            }
        }
        if self.analyze_state.dragging_knob().is_some() {
            let precision = crate::canvas::Precision::from_modifiers(
                self.modifiers.shift_key(),
                self.modifiers.control_key(),
            );
            if let Some((knob, value)) = self.analyze_state.drag_knob(y, precision) {
                self.set_analyze_knob(knob, value, true);
            }
        }
        if self.analyze_state.dragging_lane() {
            let layout = self.analyze_layout.clone();
            if let Some(change) = self.analyze_state.drag_lane(&layout, &view, x) {
                self.send_lane_change(change);
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
            // The handle's own read-out says it while it is in hand.
            _ if self.analyze_state.dragging_flatten() => None,
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
        // Up and down, wherever the pointer has wandered while it is held.
        if self.analyze_state.dragging_flatten() {
            return Pointer::ResizeY;
        }
        match self.analyze_state.hover {
            Some(AnalyzeHit::FlattenHandle(_)) => Pointer::ResizeY,
            None | Some(AnalyzeHit::Tuning | AnalyzeHit::Bpm | AnalyzeHit::Job) => Pointer::Default,
            Some(AnalyzeHit::Badge | AnalyzeHit::Ruler | AnalyzeHit::Later) => Pointer::Default,
            Some(AnalyzeHit::Lane) => Pointer::Default,
            Some(AnalyzeHit::NoteEnd(..)) => Pointer::ResizeX,
            Some(AnalyzeHit::TrimEnd(_) | AnalyzeHit::FadeEnd(_) | AnalyzeHit::Marker(_)) => {
                Pointer::ResizeX
            }
            Some(AnalyzeHit::Takes | AnalyzeHit::Keyboard) => Pointer::Default,
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
            match crate::canvas::analyze_hit(&self.analyze_layout, &view, &self.analyze_state, x, y)
            {
                // The key's menu from a right-click on it, as well as from ▾.
                Some(AnalyzeHit::ScaleName | AnalyzeHit::ScaleMenu | AnalyzeHit::ScaleCopy) => {
                    self.open_analyze_scale_menu();
                }
                // A knob's: back to its default, or type a value.
                Some(AnalyzeHit::Control(AnalyzeControl::Knob(knob))) => {
                    let bounds = self.analyze_bounds();
                    self.open_menu(MenuTarget::AnalyzeKnob(knob), x, y, bounds);
                }
                _ => {}
            }
            return;
        }
        // A press anywhere but on what is being typed into ends the typing,
        // keeping what was typed.
        if self.analyze_state.typing.is_some() {
            self.commit_analyze_typing();
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
                let tools = AnalyzeTool::of(page);
                if !tools.contains(&self.analyze_state.tool) {
                    self.analyze_state.tool = match page {
                        AnalyzePage::Notes => AnalyzeTool::Move,
                        _ => AnalyzeTool::Select,
                    };
                }
                self.refresh_slice_keys();
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
            // The flatten handle: double-click is none again, one undo.
            AnalyzeAction::Selected if self.analyze_state.dragging_flatten() => {
                let held = self.analyze_state.flatten_readout().map(|(i, _)| i);
                if self.double_click.press(x, y, std::time::Instant::now())
                    && let Some(index) = held
                {
                    self.analyze_state.end_note_drag();
                    let changes = self.analyze_state.reset_flatten(&view, index);
                    if !changes.is_empty() {
                        self.send_analysis_edits(&changes, false);
                        self.refresh_title();
                    }
                }
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
            AnalyzeAction::Studies => {
                let chip = self.analyze_layout.studies;
                let bounds = self.analyze_bounds();
                self.open_menu(MenuTarget::AnalyzeStudies, chip.x, chip.bottom(), bounds);
            }
            AnalyzeAction::Control(control) => self.analyze_control(control),
            AnalyzeAction::KnobGrab(knob) => {
                // A double-click types a value (Flopsynth's knobs).
                if self.double_click.press(x, y, std::time::Instant::now()) {
                    self.analyze_state.end_knob_drag();
                    self.begin_analyze_typing(AnalyzeTypingTarget::Knob(knob));
                }
            }
            AnalyzeAction::KnobReset(knob) => {
                self.set_analyze_knob(knob, knob.default_value(), false);
            }
            AnalyzeAction::AddMarker(t) => {
                let (markers, id) = crate::canvas::with_marker_at(&view, t);
                self.send_markers(markers, false);
                self.analyze_state.grab_marker(id);
            }
            AnalyzeAction::LaneGrab => {}
            AnalyzeAction::TakeLoad(id) => {
                if self.double_click.press(x, y, std::time::Instant::now()) {
                    self.begin_analyze_typing(AnalyzeTypingTarget::TakeName(id));
                } else {
                    self.analyze_take(AnalyzeTakeOp::Load(id));
                }
            }
            AnalyzeAction::TakeStar(id) => self.analyze_take(AnalyzeTakeOp::Star(id)),
            AnalyzeAction::TakeDiscard(id) => self.analyze_take(AnalyzeTakeOp::Discard(id)),
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
        // A knob let go is one undo.
        if self.analyze_state.end_knob_drag().is_some()
            && let Some(doc) = &mut self.options.document
        {
            doc.end_gesture();
            self.refresh_title();
        }
        match self.analyze_state.end_lane_drag() {
            // The Noise tool captures as it lets go.
            AnalyzeLaneEnd::Span(a, b)
                if self.analyze_state.page == AnalyzePage::Clean
                    && self.analyze_state.tool == AnalyzeTool::Noise =>
            {
                self.capture_analyze_noise(a, b);
            }
            AnalyzeLaneEnd::Changed => {
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
                self.refresh_slice_keys();
                self.refresh_title();
            }
            _ => {}
        }
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

    /// Home: the listen stopped, the cursor at the start, the lane there.
    fn analyze_to_start(&mut self) {
        if let Some(doc) = &mut self.options.document {
            doc.analysis_stop();
        }
        self.analyze_state.playhead = None;
        self.analyze_state.cursor = 0.0;
        self.analyze_state.start = 0.0;
        self.redraw_editor(EditorKind::Analyze);
    }

    /// R in this window: its own record arm, where it records; never the
    /// song's.
    fn analyze_record_key(&mut self) {
        let Some(view) = self.analyze.clone() else {
            return;
        };
        let Some(record) = &view.record else {
            self.analyze_says(
                "Recording is for a mixer insert or a microphone \u{2014} add Analyze Musically \
                 to a mixer track, or Record \u{25b8} Record into Analyze Musically\u{2026}"
                    .to_string(),
            );
            return;
        };
        let armed = record.armed;
        if self.analyze_state.page != AnalyzePage::Record {
            self.analyze_state.page = AnalyzePage::Record;
            self.analyze_state.tool = AnalyzeTool::Select;
        }
        if let Some(Err(said)) = self.analyze_record(crate::canvas::AnalyzeRecordOp::Arm(!armed)) {
            self.analyze_says(said);
        }
        self.relayout_editors();
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
    ///
    /// A listen holds the loop awake at the frame rate on its own account
    /// (`AnalyzeState::wants_frames`). It used to ride on the song's:
    /// nothing else asked for frames while only the listen played, so its
    /// playhead crept at the engine poll's ten a second, and — with the
    /// graph asleep under a stopped song as well — not at all (Ty: *"the
    /// playhead inside analyze musically only moves when the arrangement
    /// playhead is moving"*).
    pub(super) fn tick_analyze_playhead(&mut self) {
        let wants = self.analyze.is_some() && self.analyze_state.wants_frames();
        if wants != self.analyze_animating {
            if wants {
                self.tree.redraw_mut().begin_animating();
            } else {
                self.tree.redraw_mut().end_animating();
            }
            self.analyze_animating = wants;
        }
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
        // A value or a name being typed has the keyboard.
        if self.analyze_typing_key(event) {
            return true;
        }
        if event.logical_key == Key::Named(NamedKey::Escape)
            && !self.analyze_state.selected().is_empty()
        {
            self.analyze_state.clear_selection();
            return true;
        }
        if event.logical_key == Key::Named(NamedKey::Escape) && self.analyze_state.span.is_some() {
            self.analyze_state.span = None;
            self.redraw_editor(EditorKind::Analyze);
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
            // In this window the transport's keys are this window's own
            // (plan §3.5; Ty: *"pressing space to pause and play inside of
            // the analyze musically window should not play and pause the
            // arrangement"*): Space plays the audio here, from the cursor;
            // Home stops it and goes back to the start; R arms this
            // window's recording. None of them ever reaches the song.
            action if crate::canvas::analyze_transport_key(action).is_some() => {
                match crate::canvas::analyze_transport_key(action) {
                    Some(crate::canvas::AnalyzeTransportKey::PlayStop) => self.analyze_play_stop(),
                    Some(crate::canvas::AnalyzeTransportKey::ToStart) => self.analyze_to_start(),
                    Some(crate::canvas::AnalyzeTransportKey::Record) => self.analyze_record_key(),
                    None => {}
                }
            }
            Action::AnalyzePlaySelection => self.analyze_play_selection(),
            Action::AnalyzeAb => self.toggle_analyze_original(),
            Action::AnalyzeRender => self.render_analysis(false),
            Action::AnalyzeSnap => self.analyze_edit(AnalyzeEditOp::Snap),
            Action::AnalyzeFlatten => self.analyze_edit(AnalyzeEditOp::Flatten),
            Action::AnalyzeVibrato => self.analyze_edit(AnalyzeEditOp::Vibrato),
            // Delete in an editor window is "take away the selected thing":
            // the marker in hand on the Slice page, else the selected notes'
            // edits.
            Action::RemoveBand if self.analyze_state.page == AnalyzePage::Slice => {
                let Some(view) = self.analyze.clone() else {
                    return true;
                };
                match self.analyze_state.without_selected_marker(&view) {
                    Some(markers) => self.send_markers(markers, false),
                    None => self.analyze_says("Click a marker first, then Del".to_string()),
                }
            }
            Action::RemoveBand => self.analyze_edit(AnalyzeEditOp::Reset),
            Action::AnalyzeZoomSelection | Action::AnalyzeZoomAll => {
                let all = action == Action::AnalyzeZoomAll;
                let Some(view) = self.analyze.clone() else {
                    return true;
                };
                let layout = self.analyze_layout.clone();
                if crate::canvas::zoom_to(&mut self.analyze_state, &view, &layout, all) {
                    self.analyze_fitted = true;
                    self.redraw_editor(EditorKind::Analyze);
                } else {
                    self.analyze_says(
                        "Select notes, or drag out a stretch, then Z (Shift+Z shows it all)"
                            .to_string(),
                    );
                }
            }
            Action::AnalyzeNoiseTool => {
                self.analyze_state.page = AnalyzePage::Clean;
                self.analyze_state.tool = AnalyzeTool::Noise;
                self.relayout_editors();
            }
            Action::AnalyzeMarkerTool => {
                self.analyze_state.page = AnalyzePage::Slice;
                self.analyze_state.tool = AnalyzeTool::Marker;
                self.refresh_slice_keys();
                self.relayout_editors();
            }
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

// ------------------------------------------- the pages, through the host ---

impl WindowApp {
    /// A knob set to `value`: the notes, the clean, the slice preview or the
    /// recorder, as the knob is about.
    fn set_analyze_knob(&mut self, knob: AnalyzeKnob, value: f32, merge: bool) {
        let Some(view) = self.analyze.clone() else {
            return;
        };
        match crate::canvas::analyze_knob_change(knob, value, &view, &mut self.analyze_state) {
            AnalyzeKnobChange::Edits(changes) => self.send_analysis_edits(&changes, merge),
            AnalyzeKnobChange::Clean(clean) => self.send_clean(clean, merge),
            AnalyzeKnobChange::State => {
                self.refresh_slice_keys();
                self.relayout_editors();
            }
            AnalyzeKnobChange::Record(op) => {
                let said = match &mut self.options.document {
                    Some(doc) => doc.analysis_record(op),
                    None => return,
                };
                if let Err(said) = said {
                    self.analyze_says(said);
                }
                self.refresh_analyze();
            }
            AnalyzeKnobChange::Nothing(said) => self.analyze_says(said),
        }
        self.redraw_editor(EditorKind::Analyze);
    }

    fn send_clean(&mut self, clean: fontelle_types::StudyClean, merge: bool) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        if let Err(said) = doc.set_analysis_clean(clean, merge) {
            self.analyze_says(said);
            return;
        }
        if !merge {
            doc.end_gesture();
            self.refresh_title();
        }
        self.refresh_analyze();
        self.redraw_editor(EditorKind::Analyze);
    }

    fn send_markers(&mut self, markers: Vec<fontelle_types::StudyMarker>, merge: bool) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        if let Err(said) = doc.set_analysis_markers(markers, merge) {
            self.analyze_says(said);
            return;
        }
        if !merge {
            doc.end_gesture();
            self.refresh_title();
        }
        self.refresh_analyze();
        self.refresh_slice_keys();
        self.redraw_editor(EditorKind::Analyze);
    }

    fn send_lane_change(&mut self, change: AnalyzeLaneChange) {
        match change {
            AnalyzeLaneChange::Span => self.redraw_editor(EditorKind::Analyze),
            AnalyzeLaneChange::Clean(clean) => self.send_clean(clean, true),
            AnalyzeLaneChange::Markers(markers) => self.send_markers(markers, true),
            AnalyzeLaneChange::Comp(comp) => {
                if let Some(doc) = &mut self.options.document
                    && let Err(said) = doc.set_analysis_comp(comp, true)
                {
                    self.analyze_says(said);
                }
                self.refresh_analyze();
                self.redraw_editor(EditorKind::Analyze);
            }
        }
    }

    /// The Slice page's keyboard preview, asked of the host when the cuts or
    /// the layout change.
    pub(super) fn refresh_slice_keys(&mut self) {
        if self.analyze_state.page != AnalyzePage::Slice {
            return;
        }
        let Some(view) = &self.analyze else {
            return;
        };
        let cuts = crate::canvas::analyze_cuts(view, &self.analyze_state);
        let layout = self.analyze_state.layout;
        if let Some(doc) = &mut self.options.document {
            self.analyze_state.slice_keys = doc.analysis_slice_keys(&cuts, layout);
        }
    }

    fn capture_analyze_noise(&mut self, a: f64, b: f64) {
        let said = match &mut self.options.document {
            Some(doc) => doc.capture_analysis_noise(a, b),
            None => return,
        };
        match said {
            Ok(said) => {
                self.analyze_state.noise_span = Some((a, b));
                self.analyze_state.span = None;
                self.status = said.clone();
                self.show_toast(said, true);
                self.tree.invalidate(TRANSPORT);
            }
            Err(said) => self.analyze_says(said),
        }
        self.refresh_analyze();
        self.refresh_title();
        self.redraw_editor(EditorKind::Analyze);
    }

    fn analyze_take(&mut self, op: AnalyzeTakeOp) {
        let said = match &mut self.options.document {
            Some(doc) => doc.analysis_take(op),
            None => return,
        };
        match said {
            Ok(said) if !said.is_empty() => {
                self.status = said.clone();
                self.show_toast(said, true);
                self.tree.invalidate(TRANSPORT);
            }
            Ok(_) => {}
            Err(said) => self.analyze_says(said),
        }
        self.analyze_fitted = false;
        self.refresh_analyze();
        self.refresh_studio();
        self.refresh_title();
        self.relayout_editors();
        self.redraw_editor(EditorKind::Analyze);
    }

    /// What a card's control does.
    fn analyze_control(&mut self, control: AnalyzeControl) {
        let Some(view) = self.analyze.clone() else {
            return;
        };
        let playhead = self.view.position_sample;
        let anchor = self.analyze_layout.control(control).unwrap_or_default();
        let bounds = self.analyze_bounds();
        let mut clean = view.clean.clone();
        let said: Option<Result<String, String>> = match control {
            AnalyzeControl::Knob(_) => None,
            AnalyzeControl::CaptureNoise => {
                if let Some((a, b)) = self.analyze_state.span {
                    self.capture_analyze_noise(a, b);
                }
                None
            }
            AnalyzeControl::ListenRemoved => {
                self.analyze_state.listen_removed = !self.analyze_state.listen_removed;
                let on = self.analyze_state.listen_removed;
                if let Some(doc) = &mut self.options.document {
                    doc.analysis_listen_removed(on);
                }
                Some(Ok(if on {
                    "Hearing only what the denoiser removes".to_string()
                } else {
                    "Hearing the cleaned audio".to_string()
                }))
            }
            AnalyzeControl::DenoiseOn => {
                clean.denoise.on = !clean.denoise.on;
                self.send_clean(clean, false);
                None
            }
            AnalyzeControl::VoiceDenoise => {
                clean.denoise.voice = !clean.denoise.voice;
                if clean.denoise.voice {
                    clean.denoise.on = true;
                }
                self.send_clean(clean, false);
                None
            }
            AnalyzeControl::TrimToSelection => {
                if let Some((a, b)) = self.analyze_state.span {
                    clean.trim = Some((view.frame_of(a), view.frame_of(b)));
                    self.analyze_state.span = None;
                    self.send_clean(clean, false);
                }
                None
            }
            AnalyzeControl::ResetClean => {
                let noise = clean.denoise.noise.take();
                let mut fresh = fontelle_types::StudyClean::default();
                fresh.denoise.noise = noise;
                self.send_clean(fresh, false);
                Some(Ok("Clean reset: the audio as recorded".to_string()))
            }
            AnalyzeControl::FadeShape => {
                self.open_menu(
                    MenuTarget::AnalyzeFadeShape,
                    anchor.x,
                    anchor.bottom(),
                    bounds,
                );
                None
            }
            AnalyzeControl::AutoSlice => {
                self.open_menu(
                    MenuTarget::AnalyzeAutoSlice,
                    anchor.x,
                    anchor.bottom(),
                    bounds,
                );
                None
            }
            AnalyzeControl::Source => {
                self.open_menu(MenuTarget::AnalyzeSource, anchor.x, anchor.bottom(), bounds);
                None
            }
            AnalyzeControl::UseAsMarkers => {
                let cuts = crate::canvas::analyze_cuts(&view, &self.analyze_state);
                let markers = crate::canvas::markers_from_cuts(&view, &cuts);
                let n = markers.len();
                self.analyze_state.auto = crate::canvas::AutoSlice::Off;
                self.send_markers(markers, false);
                self.relayout_editors();
                Some(Ok(format!(
                    "{n} markers \u{2014} drag one to move it, Del removes it"
                )))
            }
            AnalyzeControl::ClearMarkers => {
                self.send_markers(Vec::new(), false);
                None
            }
            AnalyzeControl::Layout(layout) => {
                self.analyze_state.layout = layout;
                self.refresh_slice_keys();
                None
            }
            AnalyzeControl::Replay => {
                self.analyze_state.replay = !self.analyze_state.replay;
                None
            }
            AnalyzeControl::SendToSampler => {
                let cuts = crate::canvas::analyze_cuts(&view, &self.analyze_state);
                let (layout, replay) = (self.analyze_state.layout, self.analyze_state.replay);
                self.options
                    .document
                    .as_mut()
                    .map(|doc| doc.send_analysis_to_sampler(&cuts, layout, replay, playhead))
            }
            AnalyzeControl::PostFader => {
                let on = view.record.as_ref().is_some_and(|r| r.post_fader);
                self.analyze_record(crate::canvas::AnalyzeRecordOp::PostFader(!on))
            }
            AnalyzeControl::Arm => {
                let armed = view.record.as_ref().is_some_and(|r| r.armed);
                self.analyze_record(crate::canvas::AnalyzeRecordOp::Arm(!armed))
            }
            AnalyzeControl::ArmMode(mode) => {
                let said = self.analyze_record(crate::canvas::AnalyzeRecordOp::Mode(mode));
                self.relayout_editors();
                said
            }
            AnalyzeControl::SendToArrangement => self
                .options
                .document
                .as_mut()
                .map(|doc| doc.send_analysis_to_arrangement(playhead)),
            AnalyzeControl::UseComp => {
                self.analyze_take(AnalyzeTakeOp::UseComp);
                None
            }
            AnalyzeControl::ClearComp => self
                .options
                .document
                .as_mut()
                .map(|doc| doc.set_analysis_comp(Vec::new(), false)),
        };
        match said {
            Some(Ok(said)) if !said.is_empty() => {
                self.status = said.clone();
                self.show_toast(said, false);
                self.tree.invalidate(TRANSPORT);
            }
            Some(Err(said)) => self.analyze_says(said),
            _ => {}
        }
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
        self.refresh_analyze();
        self.refresh_title();
        self.relayout_editors();
        self.redraw_editor(EditorKind::Analyze);
    }

    fn analyze_record(
        &mut self,
        op: crate::canvas::AnalyzeRecordOp,
    ) -> Option<Result<String, String>> {
        let said = self
            .options
            .document
            .as_mut()
            .map(|doc| doc.analysis_record(op));
        self.refresh_analyze();
        said
    }

    // ------------------------------------------------------- typing ---

    fn begin_analyze_typing(&mut self, target: AnalyzeTypingTarget) {
        let Some(view) = &self.analyze else {
            return;
        };
        let text = match target {
            AnalyzeTypingTarget::Knob(knob) => {
                match crate::canvas::analyze_knob_value(knob, view, &self.analyze_state) {
                    Some(value) => knob.display(value),
                    None => return,
                }
            }
            AnalyzeTypingTarget::TakeName(id) => match view.takes.iter().find(|t| t.id == id) {
                Some(take) => take.name.clone(),
                None => return,
            },
        };
        self.analyze_state.typing = Some(AnalyzeTyping { target, text });
        self.status = "Type, then Enter (Esc leaves it as it was)".to_string();
        self.tree.invalidate(TRANSPORT);
        self.redraw_editor(EditorKind::Analyze);
    }

    fn commit_analyze_typing(&mut self) {
        let Some(typing) = self.analyze_state.typing.take() else {
            return;
        };
        match typing.target {
            AnalyzeTypingTarget::Knob(knob) => match knob.parse(&typing.text) {
                Some(value) => self.set_analyze_knob(knob, value, false),
                None => self.analyze_says(format!(
                    "\u{201c}{}\u{201d} is not a value for {}",
                    typing.text,
                    knob.caption().to_lowercase()
                )),
            },
            AnalyzeTypingTarget::TakeName(id) => {
                self.analyze_take(AnalyzeTakeOp::Rename(id, typing.text));
            }
        }
        self.redraw_editor(EditorKind::Analyze);
    }

    /// Keys while something is typed into: they are the field's.
    fn analyze_typing_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let Some(typing) = &mut self.analyze_state.typing else {
            return false;
        };
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => self.commit_analyze_typing(),
            Key::Named(NamedKey::Escape) => self.analyze_state.typing = None,
            Key::Named(NamedKey::Backspace) => {
                typing.text.pop();
            }
            _ => {
                if let Some(text) = &event.text {
                    typing.text.extend(text.chars().filter(|c| !c.is_control()));
                }
            }
        }
        self.redraw_editor(EditorKind::Analyze);
        true
    }

    // ------------------------------------------------------- menus ---

    pub(super) fn analyze_studies_menu(&self) -> Vec<crate::canvas::MenuEntry> {
        let mut entries = vec![crate::canvas::MenuEntry::disabled("Studies in this song")];
        let rows = self
            .options
            .document
            .as_ref()
            .map(|doc| doc.analysis_studies())
            .unwrap_or_default();
        if rows.is_empty() {
            entries.push(crate::canvas::MenuEntry::disabled("  none yet"));
        }
        for row in &rows {
            entries.push(crate::canvas::MenuEntry::new(format!(
                "{} {} \u{2014} {}",
                if row.open { "\u{2713}" } else { "  " },
                row.name,
                row.place
            )));
        }
        entries.push(
            crate::canvas::MenuEntry::new("Record into Analyze Musically\u{2026}").after_rule(),
        );
        entries
    }

    pub(super) fn choose_analyze_studies(&mut self, index: usize) {
        let rows = self
            .options
            .document
            .as_ref()
            .map(|doc| doc.analysis_studies())
            .unwrap_or_default();
        let first = if rows.is_empty() { 2 } else { 1 };
        match index.checked_sub(first) {
            Some(i) if i < rows.len() => self.open_study(rows[i].id),
            Some(i) if i == rows.len() => self.record_into_analysis(),
            _ => {}
        }
    }

    /// Opens the window on a study, from the menu or the browser.
    pub(super) fn open_study(&mut self, id: fontelle_types::StudyId) {
        let said = match &mut self.options.document {
            Some(doc) => doc.open_study_analysis(id),
            None => return,
        };
        self.opened_analyze(said);
    }

    /// *Record into Analyze Musically…*.
    pub(super) fn record_into_analysis(&mut self) {
        let said = match &mut self.options.document {
            Some(doc) => doc.record_into_analysis(),
            None => return,
        };
        let ok = said.is_ok();
        self.opened_analyze(said);
        if ok {
            self.analyze_state.page = AnalyzePage::Record;
            self.relayout_editors();
        }
    }

    /// The Analyze Musically insert's slot: this window, on its study, at
    /// the Record page (Ty, plan §6.1).
    pub(super) fn open_analyze_insert(&mut self, strip: usize, slot: usize) {
        let said = match &mut self.options.document {
            Some(doc) => doc.open_insert_analysis(strip, slot),
            None => return,
        };
        let ok = said.is_ok();
        let empty = self.analyze.is_none();
        self.opened_analyze(said);
        if ok && (empty || self.analyze.as_ref().is_some_and(|v| !v.has_audio)) {
            self.analyze_state.page = AnalyzePage::Record;
            self.relayout_editors();
        }
    }

    /// After the host opened (or refused) a study: the window on it.
    fn opened_analyze(&mut self, said: Result<String, String>) {
        match said {
            Ok(said) => {
                self.status = said;
                self.analyze_state = self.analyze_state.for_another_clip();
                self.analyze_fitted = false;
                self.analyze_revision = u64::MAX;
                self.refresh_analyze();
                self.open_editor(EditorKind::Analyze);
                self.refresh_editors();
            }
            Err(said) => {
                self.status = said.clone();
                self.show_toast(said, false);
            }
        }
        self.tree.invalidate(TRANSPORT);
    }

    pub(super) fn analyze_choice_menu(&self, target: &MenuTarget) -> Vec<crate::canvas::MenuEntry> {
        let tick = |on: bool, word: &str| {
            crate::canvas::MenuEntry::new(format!("{} {word}", if on { "\u{2713}" } else { "  " }))
        };
        let Some(view) = &self.analyze else {
            return Vec::new();
        };
        match target {
            MenuTarget::AnalyzeFadeShape => fontelle_types::StudyFadeShape::ALL
                .iter()
                .map(|shape| tick(view.clean.fade_shape == *shape, shape.label()))
                .collect(),
            MenuTarget::AnalyzeAutoSlice => crate::canvas::AutoSlice::CHOICES
                .iter()
                .enumerate()
                .map(|(i, word)| tick(self.analyze_state.auto.index() == i, word))
                .collect(),
            MenuTarget::AnalyzeSource => {
                let mut entries = Vec::new();
                let record = view.record.clone().unwrap_or_default();
                if let crate::canvas::AnalyzeSource::Insert { track } = &view.source {
                    entries.push(tick(
                        record.input.is_none(),
                        &format!("This track ({track})"),
                    ));
                }
                for input in &record.inputs {
                    entries.push(tick(record.input.as_deref() == Some(input), input));
                }
                if entries.is_empty() {
                    entries.push(crate::canvas::MenuEntry::disabled("No input devices"));
                }
                entries
            }
            MenuTarget::AnalyzeKnob(knob) => vec![
                crate::canvas::MenuEntry::disabled(knob.caption()),
                crate::canvas::MenuEntry::new(format!(
                    "Back to {}",
                    knob.display(knob.default_value())
                )),
                crate::canvas::MenuEntry::new("Type a value\u{2026}"),
            ],
            _ => Vec::new(),
        }
    }

    pub(super) fn choose_analyze_choice(&mut self, target: MenuTarget, index: usize) {
        let Some(view) = self.analyze.clone() else {
            return;
        };
        match target {
            MenuTarget::AnalyzeFadeShape => {
                if let Some(shape) = fontelle_types::StudyFadeShape::ALL.get(index) {
                    let mut clean = view.clean.clone();
                    clean.fade_shape = *shape;
                    self.send_clean(clean, false);
                }
            }
            MenuTarget::AnalyzeAutoSlice => {
                use crate::canvas::AutoSlice;
                self.analyze_state.auto = match index {
                    1 => AutoSlice::Transients { sensitivity: 0.5 },
                    2 => AutoSlice::Notes,
                    3 => AutoSlice::Beats,
                    4 => AutoSlice::Equal { pieces: 8 },
                    _ => AutoSlice::Off,
                };
                self.refresh_slice_keys();
                self.relayout_editors();
            }
            MenuTarget::AnalyzeSource => {
                let insert = matches!(view.source, crate::canvas::AnalyzeSource::Insert { .. });
                let inputs = view.record.map(|r| r.inputs).unwrap_or_default();
                let choice = if insert {
                    match index {
                        0 => Some(None),
                        i => inputs.get(i - 1).cloned().map(Some),
                    }
                } else {
                    inputs.get(index).cloned().map(Some)
                };
                if let Some(input) = choice {
                    match self.analyze_record(crate::canvas::AnalyzeRecordOp::Source(input)) {
                        Some(Ok(said)) => {
                            self.status = said;
                            self.tree.invalidate(TRANSPORT);
                        }
                        Some(Err(said)) => self.analyze_says(said),
                        None => {}
                    }
                    self.relayout_editors();
                }
            }
            MenuTarget::AnalyzeKnob(knob) => match index {
                1 => self.set_analyze_knob(knob, knob.default_value(), false),
                2 => self.begin_analyze_typing(AnalyzeTypingTarget::Knob(knob)),
                _ => {}
            },
            _ => {}
        }
        self.redraw_editor(EditorKind::Analyze);
    }
}
