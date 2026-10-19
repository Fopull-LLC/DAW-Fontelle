//! The studio's window with no window behind it: presses, drags and
//! releases delivered through the same functions a mouse's events reach
//! (`editor_pointer_moved`, `editor_pressed`, `press`, …), so a test can
//! see what a click on a button *does* — not only that the layout puts the
//! button there. No event loop, no surface, no GPU: nothing is drawn.

use super::{EditorKind, MouseButton, WindowApp, WindowOptions};
use crate::canvas::{SingerPanelLayout, VoicebankTab};
use crate::document::StudioHost;
use crate::layout::{Rect, editor_window_layout};
use crate::theme::Theme;

/// See the module doc.
pub struct Windowless {
    app: WindowApp,
}

impl Windowless {
    /// The studio over `document`, its window `size` points across.
    pub fn new(document: Box<dyn StudioHost>, theme: Theme, size: (u32, u32)) -> Self {
        let mut app = WindowApp::new(WindowOptions {
            document: Some(document),
            theme,
            size,
            ..WindowOptions::default()
        });
        app.refresh_studio();
        Self { app }
    }

    /// The same, with `host` as the transport the bar drives.
    pub fn with_host(mut self, host: Box<dyn crate::transport::TransportHost>) -> Self {
        self.app.options.host = Some(host);
        self.app.view = self
            .app
            .options
            .host
            .as_mut()
            .map(|h| h.view())
            .unwrap_or_default();
        self
    }

    /// The arrangement's ruler, where a press moves the time marker.
    pub fn timeline_ruler(&self) -> Rect {
        self.app.timeline_layout.ruler
    }

    /// The selected channel's instrument panel, open as its window would be
    /// at `size` points.
    pub fn open_instrument_panel(&mut self, size: (f32, f32)) {
        let body = editor_window_layout(size.0, size.1, &self.app.options.theme.metrics).body;
        self.app
            .windowless
            .retain(|(kind, _)| *kind != EditorKind::Instrument);
        self.app.windowless.push((EditorKind::Instrument, body));
        self.settle();
    }

    /// The singer panel as it is laid out now.
    pub fn singer_layout(&self) -> &SingerPanelLayout {
        &self.app.singer_layout
    }

    /// A left click at `(x, y)` in the instrument panel's window.
    pub fn click_instrument_panel(&mut self, x: f32, y: f32) {
        self.app.editor_pointer_moved(EditorKind::Instrument, x, y);
        self.app
            .editor_pressed(EditorKind::Instrument, MouseButton::Left);
        self.app.editor_released(EditorKind::Instrument);
        self.settle();
    }

    /// A left-button drag in the instrument panel's window, `from` to `to`.
    pub fn drag_instrument_panel(&mut self, from: (f32, f32), to: (f32, f32)) {
        let kind = EditorKind::Instrument;
        self.app.editor_pointer_moved(kind, from.0, from.1);
        self.app.editor_pressed(kind, MouseButton::Left);
        for step in 1..=8 {
            let t = step as f32 / 8.0;
            self.app.editor_pointer_moved(
                kind,
                from.0 + (to.0 - from.0) * t,
                from.1 + (to.1 - from.1) * t,
            );
        }
        self.app.editor_released(kind);
        self.settle();
    }

    /// A left click at `(x, y)` in the studio's own window.
    pub fn click_studio(&mut self, x: f32, y: f32) {
        self.app.cursor = (x, y);
        self.app.press(winit::event::MouseButton::Left, x, y);
        self.app.studio_released();
        self.settle();
    }

    /// `button` going down at `(x, y)` in the studio's own window, and
    /// staying down.
    pub fn press_studio(&mut self, button: winit::event::MouseButton, x: f32, y: f32) {
        self.app.cursor = (x, y);
        self.app.press(button, x, y);
        self.settle();
    }

    /// `button` coming up in the studio's window, wherever the pointer is.
    pub fn release_studio(&mut self, button: winit::event::MouseButton) {
        self.app.released(button);
        self.settle();
    }

    /// The pointer moving to `(x, y)` in the studio's window.
    pub fn move_studio(&mut self, x: f32, y: f32) {
        self.app.pointer_moved(x, y);
    }

    /// One frame `dt` seconds after the last: what moves on its own moves.
    pub fn frame(&mut self, dt: f32) {
        self.app.frame_motion(dt);
        self.settle();
    }

    /// The arrangement's area, and where its view is scrolled to in time.
    pub fn timeline_frame(&self) -> Rect {
        self.app.layout.timeline.frame
    }

    pub fn timeline_scroll_tick(&self) -> fontelle_types::Tick {
        self.app.timeline.view.scroll_tick
    }

    /// How many clips the arrangement shows.
    pub fn clip_count(&self) -> usize {
        self.app.clips.len()
    }

    /// The window idling for `time`, as its loop does between events: the
    /// document's work done and the view read again, every few
    /// milliseconds.
    pub fn idle(&mut self, time: std::time::Duration) {
        let until = std::time::Instant::now() + time;
        while std::time::Instant::now() < until {
            self.settle();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Which tab of the voicebanks page is open, if it is.
    pub fn voicebank_page(&self) -> Option<VoicebankTab> {
        self.app.vocal_edit.page.as_ref().map(|p| p.tab)
    }

    /// Closes the voicebanks page and the singer picker.
    pub fn close_pages(&mut self) {
        self.app.vocal_edit.page = None;
        self.app.close_singer_picker();
    }

    /// The singer picker's cards, while it is open.
    pub fn picker_cards(&self) -> Vec<Rect> {
        self.app
            .vocal_edit
            .picker
            .as_ref()
            .and_then(|p| p.layout.as_ref())
            .map(|l| l.cards.iter().map(|c| c.frame).collect())
            .unwrap_or_default()
    }

    /// What the status line says.
    pub fn status(&self) -> &str {
        &self.app.status
    }

    pub fn document(&self) -> &dyn StudioHost {
        self.app
            .options
            .document
            .as_deref()
            .expect("a windowless studio has its document")
    }

    // --- the roll's smart chords ---

    /// Where a roll toolbar chip is.
    pub fn roll_chip(&self, control: crate::canvas::RollControl) -> Option<Rect> {
        self.app
            .roll_bar
            .items
            .iter()
            .find(|(c, _)| *c == control)
            .map(|(_, r)| *r)
    }

    /// The middle of the roll's row for `key` at `tick`, on screen.
    pub fn roll_point(&self, tick: fontelle_types::Tick, key: u8) -> (f32, f32) {
        let grid = self.app.roll_layout.grid;
        let row = crate::canvas::key_row(&self.app.roll.view, grid, key);
        (
            crate::canvas::tick_to_x(&self.app.roll.view, grid, tick) + 3.0,
            row.y + row.height / 2.0,
        )
    }

    /// The chord strip, while it is open.
    pub fn chord_strip(&self) -> Option<crate::canvas::ChordStripLayout> {
        self.app.chord_strip_layout()
    }

    /// The chips' chord names, while the strip is open.
    pub fn chord_names(&self) -> Vec<String> {
        self.app
            .chord_strip
            .as_ref()
            .map(|s| s.suggestions.iter().map(|x| x.chord.label()).collect())
            .unwrap_or_default()
    }

    /// The keys the strip has sounding.
    pub fn chord_sounding(&self) -> &[u8] {
        &self.app.chord_sounding
    }

    /// The wheel turned `dy` notches with the pointer at `(x, y)`.
    pub fn wheel_studio(&mut self, x: f32, y: f32, dy: f32) {
        self.app.cursor = (x, y);
        self.app.scroll_roll(0.0, dy);
        self.app.glide_views(10.0);
    }

    /// The arrangement's ruler and grid, and its view.
    pub fn timeline_ruler_and_view(&self) -> (Rect, Rect, crate::canvas::TimelineView) {
        (
            self.app.timeline_layout.ruler,
            self.app.timeline_layout.grid,
            self.app.timeline.view,
        )
    }

    /// The roll's ruler and grid, and its view.
    pub fn roll_ruler_and_view(&self) -> (Rect, Rect, crate::canvas::RollView) {
        (
            self.app.roll_layout.ruler,
            self.app.roll_layout.grid,
            self.app.roll.view,
        )
    }

    /// How many flecks of deleted notes are in the air.
    pub fn flecks(&self) -> usize {
        self.app.particles.flecks().len()
    }

    /// The transport bar's signature box.
    pub fn signature_box(&self) -> Rect {
        self.app.bar.signature
    }

    /// The change chosen on the ruler, by its bar.
    pub fn meter_selected(&self) -> Option<u32> {
        self.app.meter_selected
    }

    /// Row `lane`'s header on the arrangement.
    pub fn lane_header(&self, lane: usize) -> Rect {
        let top =
            crate::canvas::lane_to_y(&self.app.timeline.view, self.app.timeline_layout.grid, lane);
        Rect::new(
            self.app.timeline_layout.headers.x,
            top,
            self.app.timeline_layout.headers.width,
            self.app.timeline.view.lane_height,
        )
    }

    /// Holds (or lets go of) Shift and Ctrl for the presses that follow.
    pub fn hold(&mut self, shift: bool, ctrl: bool) {
        let mut m = winit::keyboard::ModifiersState::empty();
        m.set(winit::keyboard::ModifiersState::SHIFT, shift);
        m.set(winit::keyboard::ModifiersState::CONTROL, ctrl);
        self.app.modifiers = m;
    }

    /// The rows a shift- or ctrl-click has chosen together.
    pub fn lane_set(&self) -> &[usize] {
        &self.app.lane_set
    }

    /// The key the single-note audition is sounding, if any.
    pub fn auditioning(&self) -> Option<u8> {
        self.app.audition.key()
    }

    /// The open menu's rows, and what each says.
    pub fn menu_rows(&self) -> Vec<(Rect, String)> {
        self.app
            .menu
            .as_ref()
            .map(|(_, m)| {
                m.rows
                    .iter()
                    .copied()
                    .zip(m.entries.iter().map(|e| e.label.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The pointer's move, and the hover it brings.
    pub fn hover_studio(&mut self, x: f32, y: f32) {
        self.app.pointer_moved(x, y);
        self.app.update_hover();
    }

    pub fn document_mut(&mut self) -> &mut dyn StudioHost {
        self.app
            .options
            .document
            .as_deref_mut()
            .expect("a windowless studio has its document")
    }

    /// What a frame does before it is drawn: the document's work, and the
    /// window's view of it read again.
    /// The song's meter, as the window last read it.
    pub fn meter(&self) -> &fontelle_types::Meter {
        &self.app.meter
    }

    /// The signature flags on the arrangement's ruler, by the bar each
    /// starts.
    pub fn meter_flags(&self) -> Vec<(u32, Rect)> {
        crate::canvas::meter_flags(
            &self.app.timeline.view,
            self.app.timeline_layout.grid,
            self.app.timeline_layout.ruler,
            &self.app.meter,
        )
    }

    /// An arrow key: `dx` across, `dy` down, with whatever [`hold`](Self::hold)
    /// says is held.
    pub fn press_key_arrow(&mut self, dx: i32, dy: i32) {
        self.app.arrow(dx, dy);
        self.settle();
    }

    /// Whether a typed prompt is open.
    pub fn prompt_open(&self) -> bool {
        matches!(&self.app.menu, Some((super::MenuTarget::NameProject(_), _)))
    }

    /// Types `text` into the open prompt and presses Enter.
    pub fn answer_prompt(&mut self, text: &str) {
        let Some((target, _)) = self.app.menu.clone() else {
            panic!("no prompt is open");
        };
        self.app.menu_filter.set(text.to_string());
        self.app.choose_menu(&target, 0);
        self.settle();
    }

    /// The start menu put up, as a launch puts it up.
    pub fn open_welcome(&mut self) {
        self.app.open_welcome();
        self.settle();
    }

    /// The start menu's card, while it is up.
    pub fn welcome_layout(&self) -> Option<crate::canvas::WelcomeLayout> {
        self.app.welcome.as_ref().map(|w| w.layout.clone())
    }

    /// The guide, as `?` opens it.
    pub fn open_help(&mut self) {
        self.app.open_help();
        self.settle();
    }

    /// The guide's card as it is laid out now.
    pub fn help_layout(&self) -> crate::canvas::HelpLayout {
        self.app.help_layout()
    }

    /// The settings page, on the section headed `title`.
    pub fn open_settings(&mut self, title: &str) {
        self.app.open_settings_page_at(title);
    }

    pub fn settings_page_open(&self) -> bool {
        self.app.settings_page.is_some()
    }

    /// A press on settings row `index`, as a click on its control makes.
    pub fn press_setting(&mut self, index: usize) {
        self.app.press_setting_row(index, false);
        self.settle();
    }

    /// The theme editor's card, while it is open.
    pub fn theme_editor(&self) -> Option<crate::canvas::ThemeEditorLayout> {
        self.app.theme_editor.as_ref().map(|ed| ed.layout.clone())
    }

    /// The card's colour rows with the palette colour each is.
    pub fn theme_editor_rows(&self) -> Vec<(Rect, String)> {
        self.app.theme_editor.as_ref().map_or_else(Vec::new, |ed| {
            ed.layout
                .rows
                .iter()
                .copied()
                .zip(ed.names.iter().cloned())
                .filter(|(r, _)| r.intersection(&ed.layout.list) == *r)
                .collect()
        })
    }

    /// The look the window is wearing now.
    pub fn window_theme(&self) -> &Theme {
        &self.app.options.theme
    }

    /// The shader code card, while it is open.
    pub fn shader_editor(&self) -> Option<crate::canvas::ShaderEditorLayout> {
        self.app
            .theme_editor
            .as_ref()
            .and_then(|ed| ed.code.as_ref())
            .map(|code| code.layout)
    }

    /// What the code card says is wrong, or `None` while it compiles.
    pub fn shader_problem(&self) -> Option<String> {
        let code = self.app.theme_editor.as_ref()?.code.as_ref()?;
        code.problem.as_ref()?;
        Some(code.problem_words())
    }

    /// `text` typed, a key a character, `\n` as Enter and `\u{8}` as
    /// Backspace — to whatever has the
    /// theme editor's keyboard.
    pub fn type_text(&mut self, text: &str) {
        use winit::keyboard::{Key, NamedKey};
        for c in text.chars() {
            let key = if c == '\n' {
                Key::Named(NamedKey::Enter)
            } else if c == '\u{8}' {
                Key::Named(NamedKey::Backspace)
            } else {
                Key::Character(c.to_string().into())
            };
            self.app.theme_editor_key(&key);
        }
        self.settle();
    }

    /// One turn of the shared song, and the window's view of it read again —
    /// what each pass of the window's loop does while a song is shared.
    pub fn pump_session(&mut self) {
        if let Some(doc) = &mut self.app.options.document {
            doc.pump_session();
        }
        self.app.read_session_now();
    }

    /// The Share panel, opened as its button opens it.
    pub fn open_share_panel(&mut self) {
        self.app.share_open = true;
        self.app.read_session_now();
    }

    /// Where the Share panel's parts are, as the window lays it out now.
    pub fn share_panel(&self) -> crate::canvas::SharePanelLayout {
        self.app.share_panel_layout()
    }

    /// The question a session has up — a card with its answers — if any.
    pub fn question(&self) -> Option<crate::document::SessionQuestion> {
        self.app.session_question.clone()
    }

    /// Answers the card's `n`th button, as a click on it does.
    pub fn answer_question(&mut self, n: usize) {
        self.app.answer_session_question(n);
    }

    /// `text` typed into the open prompt a key at a time: `\n` is Enter,
    /// `\u{1b}` Escape, `\u{8}` Backspace.
    pub fn type_into_prompt(&mut self, text: &str) {
        use winit::keyboard::{Key, NamedKey};
        for c in text.chars() {
            let key = match c {
                '\n' => Key::Named(NamedKey::Enter),
                '\u{1b}' => Key::Named(NamedKey::Escape),
                '\u{8}' => Key::Named(NamedKey::Backspace),
                c => Key::Character(c.to_string().into()),
            };
            self.app.menu_filter_logical_key(&key);
        }
    }

    /// What the open prompt's field shows, if a prompt is open.
    pub fn prompt_field(&self) -> Option<String> {
        self.app
            .text_field()
            .map(|field| field.entry.text().to_string())
    }

    /// The settings page's rows as drawn: each row's name and the caption
    /// on its button (empty for a row that is not one).
    pub fn setting_labels(&self) -> Vec<(String, String)> {
        let doc = self.document();
        doc.settings()
            .into_iter()
            .zip(doc.setting_controls())
            .map(|(entry, control)| {
                let caption = match control {
                    crate::canvas::SettingControl::Button { caption } => caption,
                    _ => String::new(),
                };
                (entry.name, caption)
            })
            .collect()
    }

    fn settle(&mut self) {
        if let Some(doc) = &mut self.app.options.document {
            doc.pump();
        }
        self.app.refresh_studio();
        self.app.relayout_editors();
    }
}
