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
    fn settle(&mut self) {
        if let Some(doc) = &mut self.app.options.document {
            doc.pump();
        }
        self.app.refresh_studio();
        self.app.relayout_editors();
    }
}
