//! The theme editor's half of `WindowApp` (next update, phase 5): the card
//! beside the studio, and the code card for a shader layer.
//!
//! The window edits a **working copy** of the theme and wears it after every
//! change, so the studio beside the card is the preview. Nothing reaches the
//! host until Save, which keeps it through
//! [`save_edited_theme`](crate::document::DocumentHost::save_edited_theme) —
//! a copy for a built-in look — and Close without saving wears the host's
//! look again. Where things are is `canvas::theme_editor`'s, which is pure
//! and tested; this is the plumbing.
//!
//! A shader is text here. A node graph that writes the same text is the
//! later step the plan leaves room for: whatever builds the WGSL, the layer
//! keeps WGSL, so a graph-made shader is still one file and still opens
//! here as code.

use super::*;

use crate::backdrop::{ShaderProblem, shader_problem};
use crate::canvas::{
    NotepadRow, ShaderEditorHit, ShaderEditorLayout, TextEntry, ThemeEditorHit, ThemeEditorLayout,
    ThemeEditorTab,
};
use crate::layout::Rect;
use crate::theme::{BackdropPanel, Blend, Color, GradientLayer, Layer, ShaderLayer};

/// What *+ Shader* starts with: a gradient between the layer's first two
/// colours, with the uniforms named so the next step is obvious.
pub const SHADER_STARTER: &str = "\
// uv runs 0 to 1 across the window, px is in points.
// bd.time, bd.beat and bd.level move; bd.color0 to bd.color3 are the
// layer's colours (the theme's own until set).
fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let wave = 0.5 + 0.5 * sin(uv.x * 6.0 + bd.time * 0.5);
    let c = mix(bd.color2.rgb, bd.color0.rgb, uv.y * 0.6 * wave);
    return vec4<f32>(c, 1.0);
}
";

/// The code card's text size, against the theme's own.
const CODE_TEXT_SCALE: f32 = 0.95;

/// What a pointer held on the card is moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerDrag {
    Square,
    Hue,
    Alpha,
    Opacity,
    Speed,
}

/// A layer's kind, as the card asks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Picture,
    Shader,
    Gradient,
    Solid,
}

impl LayerKind {
    fn of(layer: &Layer) -> Self {
        match layer {
            Layer::Image(_) => Self::Picture,
            Layer::Shader(_) => Self::Shader,
            Layer::Gradient(_) => Self::Gradient,
            Layer::Solid { .. } => Self::Solid,
        }
    }
}

/// How a layer's row names it.
pub fn layer_caption(layer: &Layer) -> String {
    match layer {
        Layer::Image(_) => "Picture".to_string(),
        Layer::Shader(s) => match s.shader.strip_prefix("builtin:") {
            Some(name) => format!("Shader \u{00b7} {name}"),
            None => "Shader \u{00b7} your own".to_string(),
        },
        Layer::Gradient(g) if g.radial => "Gradient \u{00b7} radial".to_string(),
        Layer::Gradient(_) => "Gradient".to_string(),
        Layer::Solid { .. } => "Colour".to_string(),
    }
}

/// The code card.
#[derive(Debug)]
pub struct CodeCard {
    /// The layer it writes: its section, and its place in the stack.
    pub target: (BackdropPanel, usize),
    pub entry: TextEntry,
    pub rows: Vec<NotepadRow>,
    pub scroll: usize,
    pub problem: Option<ShaderProblem>,
    pub layout: ShaderEditorLayout,
    pub advance: f32,
    pub line_height: f32,
    pub text_px: f32,
    pub columns: usize,
    pub lines: usize,
}

impl CodeCard {
    /// The line under the text, as it is drawn.
    pub fn problem_words(&self) -> String {
        match &self.problem {
            None => crate::canvas::SHADER_EDITOR_FINE.to_string(),
            Some(ShaderProblem {
                line: Some(n),
                message,
            }) => format!("Line {n}: {message}"),
            Some(ShaderProblem {
                line: None,
                message,
            }) => message.clone(),
        }
    }
}

/// The theme editor, while it is open.
#[derive(Debug)]
pub struct ThemeEditor {
    pub working: Theme,
    pub tab: ThemeEditorTab,
    /// Every palette colour's name, in the order the card lists them.
    pub names: Vec<String>,
    /// The colour chosen on the Colours tab, by its place in `names`.
    pub token: usize,
    /// The section chosen on the Backdrops tab, in [`BackdropPanel::ALL`].
    pub section: usize,
    /// The chosen layer, by its place in the section's stack (bottom first).
    pub layer: Option<usize>,
    /// A gradient's chosen end.
    pub stop: usize,
    /// The picker's hue, kept apart from the colour so a grey keeps the hue
    /// it was given when its saturation comes back.
    pub hue: f32,
    pub scroll: f32,
    /// The hex field, while it is being typed in.
    pub hex: Option<TextEntry>,
    pub drag: Option<PickerDrag>,
    pub layout: ThemeEditorLayout,
    pub code: Option<CodeCard>,
}

impl ThemeEditor {
    pub fn panel(&self) -> BackdropPanel {
        BackdropPanel::ALL[self.section.min(BackdropPanel::ALL.len() - 1)]
    }

    pub fn stack(&self) -> &[Layer] {
        self.working.backdrops.layers(self.panel())
    }

    /// A stack index as the card's row: the rows list the top first.
    pub fn row_of(&self, index: usize) -> usize {
        self.stack().len().saturating_sub(1 + index)
    }

    pub fn index_of_row(&self, row: usize) -> Option<usize> {
        let n = self.stack().len();
        (row < n).then(|| n - 1 - row)
    }

    pub fn chosen_layer(&self) -> Option<&Layer> {
        self.stack().get(self.layer?)
    }

    pub fn chosen_kind(&self) -> Option<LayerKind> {
        self.chosen_layer().map(LayerKind::of)
    }

    /// Where a group heading goes: over the first row of each group.
    pub fn heading_rows(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut last = "";
        for (i, name) in self.names.iter().enumerate() {
            let group = crate::canvas::palette_group(name);
            if group != last {
                out.push(i);
                last = group;
            }
        }
        out
    }

    /// The colour the picker is editing, if the card is on one.
    pub fn colour(&self) -> Option<Color> {
        match self.tab {
            ThemeEditorTab::Colours => self.working.palette.get(self.names.get(self.token)?),
            ThemeEditorTab::Backdrops => match self.chosen_layer()? {
                Layer::Solid { color } => Some(*color),
                Layer::Gradient(g) => g.stops.get(self.stop).map(|s| s.1),
                _ => None,
            },
        }
    }

    fn set_colour(&mut self, colour: Color) {
        match self.tab {
            ThemeEditorTab::Colours => {
                if let Some(name) = self.names.get(self.token).cloned() {
                    let _ = self.working.palette.set(&name, colour);
                }
            }
            ThemeEditorTab::Backdrops => {
                let (panel, stop) = (self.panel(), self.stop);
                let Some(index) = self.layer else { return };
                match self.working.backdrops.layers_mut(panel).get_mut(index) {
                    Some(Layer::Solid { color }) => *color = colour,
                    Some(Layer::Gradient(g)) => {
                        if let Some(s) = g.stops.get_mut(stop) {
                            s.1 = colour;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// The chosen layer's strength, 0 to 1.
    pub fn opacity(&self) -> f32 {
        match self.chosen_layer() {
            Some(Layer::Image(b)) => b.opacity,
            Some(Layer::Shader(s)) => s.opacity,
            Some(Layer::Gradient(g)) => g.opacity,
            _ => 1.0,
        }
    }

    /// A shader's speed, as the slider shows it: 0 to 4 across.
    pub fn speed(&self) -> f32 {
        match self.chosen_layer() {
            Some(Layer::Shader(s)) => s.speed,
            _ => 1.0,
        }
    }

    /// Takes the hue from the colour now being edited, unless it has none.
    fn take_hue(&mut self) {
        if let Some(c) = self.colour() {
            let [h, s, v, _] = c.to_hsva();
            if s > 0.0 && v > 0.0 {
                self.hue = h;
            }
        }
    }
}

/// Speed's slider runs from still to four times.
const MOST_SPEED: f32 = 4.0;

impl WindowApp {
    /// Settings ▸ Appearance ▸ *Edit this theme…*: the card, over a working
    /// copy of the look the window wears.
    pub(super) fn open_theme_editor(&mut self) {
        if self.theme_editor.is_some() {
            return;
        }
        let working = self.options.theme.clone();
        let names = crate::canvas::palette_order(&working.palette.names());
        let token = names.iter().position(|n| n == "accent").unwrap_or(0);
        let mut editor = ThemeEditor {
            working,
            tab: ThemeEditorTab::Colours,
            names,
            token,
            section: 0,
            layer: None,
            stop: 0,
            hue: 0.0,
            scroll: 0.0,
            hex: None,
            drag: None,
            layout: ThemeEditorLayout::default(),
            code: None,
        };
        editor.take_hue();
        editor.layer = editor.stack().len().checked_sub(1);
        self.theme_editor = Some(editor);
        // The menu that opened it, and the page it was on, are done with:
        // the page covers the studio, which is the preview.
        self.menu = None;
        self.close_settings_page();
        self.relayout_theme_editor();
    }

    /// Where everything on the two cards is, after any change.
    pub(super) fn relayout_theme_editor(&mut self) {
        let window = self.layout.window;
        let theme = self.options.theme.clone();
        let Some(ed) = self.theme_editor.as_mut() else {
            return;
        };
        let headings = ed.heading_rows();
        let layers = ed.stack().len();
        // Unscrolled first, to know how far the list can go.
        let plain = crate::canvas::theme_editor_layout_scrolled(
            window,
            &theme.metrics,
            ed.tab,
            ed.names.len(),
            layers,
            0.0,
            &headings,
        );
        let content = plain.rows.last().map_or(0.0, |r| r.bottom() - plain.list.y);
        ed.scroll = ed.scroll.clamp(0.0, (content - plain.list.height).max(0.0));
        ed.layout = crate::canvas::theme_editor_layout_scrolled(
            window,
            &theme.metrics,
            ed.tab,
            ed.names.len(),
            layers,
            ed.scroll,
            &headings,
        );
        let beside = ed.layout.frame;
        if let Some(code) = ed.code.as_mut() {
            code.text_px = (theme.font.size * CODE_TEXT_SCALE).round().max(9.0);
            code.advance = self
                .text
                .layout(
                    "0",
                    &crate::theme::FontTokens {
                        family: "monospace".to_string(),
                        size: code.text_px,
                        line_height: 1.0,
                        display: None,
                    },
                    None,
                )
                .width
                .max(1.0);
            code.line_height = (code.text_px * crate::canvas::NOTEPAD_LEADING).round();
            code.layout = crate::canvas::shader_editor_layout(window, beside, &theme.metrics);
            code.columns = crate::canvas::notepad_columns(code.layout.text.width, code.advance);
            code.lines = (code.layout.text.height / code.line_height)
                .floor()
                .max(1.0) as usize;
            code.rows = crate::canvas::notepad_rows(code.entry.text(), code.columns);
            let caret_row = crate::canvas::notepad_row_of(&code.rows, code.entry.caret());
            code.scroll = crate::canvas::notepad_scroll_to(caret_row, code.lines, code.scroll);
        }
        self.tree.invalidate_rect(window);
    }

    /// Wears the working copy: the studio beside the card is the preview.
    fn wear_working_theme(&mut self) {
        let Some(theme) = self.theme_editor.as_ref().map(|ed| ed.working.clone()) else {
            return;
        };
        self.apply_theme(theme);
        self.relayout_theme_editor();
    }

    /// Shuts the card. Unsaved changes go with it: the host's look is worn
    /// again.
    pub(super) fn close_theme_editor(&mut self) {
        if self.theme_editor.take().is_none() {
            return;
        }
        if let Some(theme) = self
            .options
            .document
            .as_ref()
            .and_then(|d| d.studio_theme())
        {
            self.apply_theme(theme);
        }
        self.tree.invalidate_rect(self.layout.window);
    }

    fn save_theme_editor(&mut self) {
        let Some(theme) = self.theme_editor.as_ref().map(|ed| ed.working.clone()) else {
            return;
        };
        let Some(doc) = self.options.document.as_mut() else {
            return;
        };
        match doc.save_edited_theme(theme) {
            Ok(said) => {
                // The look saved may be a copy under a new name; the card
                // goes on editing that.
                if let Some(saved) = doc.studio_theme()
                    && let Some(ed) = self.theme_editor.as_mut()
                {
                    ed.working.name = saved.name;
                }
                self.show_toast(said, false);
            }
            Err(why) => self.show_toast(why, false),
        }
        self.refresh_theme();
        self.wear_working_theme();
    }

    /// A press on either card. Whether it was on one.
    pub(super) fn press_theme_editor(&mut self, x: f32, y: f32) -> bool {
        let Some(ed) = self.theme_editor.as_ref() else {
            return false;
        };
        if let Some(code) = &ed.code
            && let Some(hit) = crate::canvas::shader_editor_hit(&code.layout, x, y)
        {
            match hit {
                ShaderEditorHit::Done => self.close_code_card(),
                ShaderEditorHit::Text => self.place_code_caret(x, y),
                ShaderEditorHit::Card => {}
            }
            return true;
        }
        let Some(hit) = crate::canvas::theme_editor_hit(&ed.layout, x, y) else {
            return false;
        };
        // A press anywhere else on the card ends typing in the hex field,
        // keeping what was typed if it is a colour.
        if hit != ThemeEditorHit::Hex {
            self.commit_hex();
        }
        self.theme_editor_hit(hit, x, y);
        self.relayout_theme_editor();
        true
    }

    fn theme_editor_hit(&mut self, hit: ThemeEditorHit, x: f32, y: f32) {
        match hit {
            ThemeEditorHit::Close => return self.close_theme_editor(),
            ThemeEditorHit::Save => return self.save_theme_editor(),
            ThemeEditorHit::Card => return,
            _ => {}
        }
        let Some(ed) = self.theme_editor.as_mut() else {
            return;
        };
        let mut changed = false;
        match hit {
            ThemeEditorHit::Tab(tab) => {
                ed.tab = tab;
                ed.hex = None;
                ed.take_hue();
            }
            ThemeEditorHit::Square
            | ThemeEditorHit::Hue
            | ThemeEditorHit::Alpha
            | ThemeEditorHit::Opacity
            | ThemeEditorHit::Speed => {
                ed.drag = Some(match hit {
                    ThemeEditorHit::Square => PickerDrag::Square,
                    ThemeEditorHit::Hue => PickerDrag::Hue,
                    ThemeEditorHit::Alpha => PickerDrag::Alpha,
                    ThemeEditorHit::Opacity => PickerDrag::Opacity,
                    _ => PickerDrag::Speed,
                });
                changed = drag_to(ed, x, y);
            }
            ThemeEditorHit::Hex => {
                if ed.hex.is_none()
                    && let Some(c) = ed.colour()
                {
                    let mut entry = TextEntry::new(c.to_hex());
                    entry.select_all();
                    ed.hex = Some(entry);
                }
            }
            ThemeEditorHit::Row(i) => {
                ed.token = i.min(ed.names.len().saturating_sub(1));
                ed.take_hue();
            }
            ThemeEditorHit::Section(i) => {
                ed.section = i;
                ed.layer = ed.stack().len().checked_sub(1);
                ed.stop = 0;
                ed.take_hue();
            }
            ThemeEditorHit::Layer(row) => {
                ed.layer = ed.index_of_row(row);
                ed.stop = 0;
                ed.take_hue();
            }
            ThemeEditorHit::LayerUp(row) | ThemeEditorHit::LayerDown(row) => {
                let up = matches!(hit, ThemeEditorHit::LayerUp(_));
                if let Some(k) = ed.index_of_row(row) {
                    let panel = ed.panel();
                    let stack = ed.working.backdrops.layers_mut(panel);
                    let other = if up { k + 1 } else { k.wrapping_sub(1) };
                    if other < stack.len() {
                        stack.swap(k, other);
                        ed.layer = Some(other);
                        changed = true;
                    }
                }
            }
            ThemeEditorHit::RemoveLayer(row) => {
                if let Some(k) = ed.index_of_row(row) {
                    let panel = ed.panel();
                    ed.working.backdrops.layers_mut(panel).remove(k);
                    let n = ed.stack().len();
                    ed.layer = n.checked_sub(1).map(|top| k.min(top));
                    // The code card was writing it.
                    if ed.code.as_ref().is_some_and(|c| c.target == (panel, k)) {
                        ed.code = None;
                    }
                    changed = true;
                }
            }
            ThemeEditorHit::Add(kind) => {
                let p = &ed.working.palette;
                let layer = match kind {
                    0 => Layer::Solid {
                        color: p.accent.with_alpha(0x40),
                    },
                    1 => Layer::Gradient(GradientLayer {
                        stops: vec![
                            (0.0, p.accent.with_alpha(0x60)),
                            (1.0, p.window.with_alpha(0)),
                        ],
                        angle: 90.0,
                        radial: false,
                        opacity: 1.0,
                        blend: Blend::Normal,
                    }),
                    _ => Layer::Shader(ShaderLayer::new(SHADER_STARTER)),
                };
                let panel = ed.panel();
                let stack = ed.working.backdrops.layers_mut(panel);
                stack.push(layer);
                ed.layer = Some(stack.len() - 1);
                ed.stop = 0;
                ed.take_hue();
                changed = true;
            }
            ThemeEditorHit::Stop(i) => {
                if ed.chosen_kind() == Some(LayerKind::Gradient) {
                    ed.stop = i;
                    ed.take_hue();
                }
            }
            ThemeEditorHit::Code => {
                if ed.chosen_kind() == Some(LayerKind::Shader) {
                    self.open_code_card();
                    return;
                }
            }
            ThemeEditorHit::Close | ThemeEditorHit::Save | ThemeEditorHit::Card => {}
        }
        if changed {
            self.wear_working_theme();
        }
    }

    /// The pointer moving with a button held on the picker or a slider.
    /// Whether the card had the drag.
    pub(super) fn drag_theme_editor(&mut self, x: f32, y: f32) -> bool {
        let Some(ed) = self.theme_editor.as_mut() else {
            return false;
        };
        if ed.drag.is_none() {
            return false;
        }
        if drag_to(ed, x, y) {
            self.wear_working_theme();
        }
        true
    }

    /// The button coming up: whatever the card was dragging is let go.
    /// Whether it had one.
    pub(super) fn release_theme_editor(&mut self) -> bool {
        self.theme_editor
            .as_mut()
            .and_then(|ed| ed.drag.take())
            .is_some()
    }

    /// The wheel over a card: the colour list or the code scrolls. Whether
    /// it was over one.
    pub(super) fn wheel_theme_editor(&mut self, dy: f32) -> bool {
        let (x, y) = self.cursor;
        let Some(ed) = self.theme_editor.as_mut() else {
            return false;
        };
        if let Some(code) = ed.code.as_mut()
            && code.layout.frame.contains(x, y)
        {
            let most = code.rows.len().saturating_sub(code.lines);
            code.scroll = (code.scroll as f32 - dy * 3.0).clamp(0.0, most as f32) as usize;
            self.tree.invalidate_rect(self.layout.window);
            return true;
        }
        if !ed.layout.frame.contains(x, y) {
            return false;
        }
        if ed.layout.list.contains(x, y) {
            ed.scroll -= dy * ed.layout.rows.first().map_or(20.0, |r| r.height) * 3.0;
            self.relayout_theme_editor();
        }
        true
    }

    /// What the cards make of a key. Whether they took it.
    pub(super) fn theme_editor_key(&mut self, key: &winit::keyboard::Key) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();
        let Some(ed) = self.theme_editor.as_mut() else {
            return false;
        };
        if ed.code.is_some() {
            return self.code_card_key(key, ctrl, shift);
        }
        if let Some(entry) = ed.hex.as_mut() {
            match key {
                Key::Named(NamedKey::Enter) => self.commit_hex(),
                Key::Named(NamedKey::Escape) => ed.hex = None,
                key => {
                    let mut clipboard = std::mem::take(&mut self.clipboard_text);
                    crate::canvas::text_key(entry, key, ctrl, shift, &mut clipboard);
                    self.clipboard_text = clipboard;
                }
            }
            self.tree.invalidate_rect(self.layout.window);
            return true;
        }
        false
    }

    /// The hex field's text, kept when it is a colour.
    fn commit_hex(&mut self) {
        let Some(ed) = self.theme_editor.as_mut() else {
            return;
        };
        let Some(entry) = ed.hex.take() else {
            return;
        };
        let text = entry.text().trim();
        let text = if text.starts_with('#') {
            text.to_string()
        } else {
            format!("#{text}")
        };
        if let Ok(c) = Color::parse(&text) {
            ed.set_colour(c);
            ed.take_hue();
            self.wear_working_theme();
        }
    }

    fn open_code_card(&mut self) {
        let Some(ed) = self.theme_editor.as_mut() else {
            return;
        };
        let panel = ed.panel();
        let Some(index) = ed.layer else { return };
        let Some(Layer::Shader(s)) = ed.stack().get(index) else {
            return;
        };
        let body = crate::backdrop::source_of(&s.shader)
            .map(|b| b.into_owned())
            .unwrap_or_default();
        let mut entry = TextEntry::new(body);
        entry.place(0, false);
        let problem = shader_problem(entry.text());
        ed.code = Some(CodeCard {
            target: (panel, index),
            entry,
            rows: Vec::new(),
            scroll: 0,
            problem,
            layout: crate::canvas::shader_editor_layout(
                Rect::default(),
                Rect::default(),
                &self.options.theme.metrics,
            ),
            advance: 1.0,
            line_height: 1.0,
            text_px: 12.0,
            columns: 1,
            lines: 1,
        });
        self.relayout_theme_editor();
    }

    fn close_code_card(&mut self) {
        if let Some(ed) = self.theme_editor.as_mut() {
            ed.code = None;
        }
        self.relayout_theme_editor();
    }

    fn place_code_caret(&mut self, x: f32, y: f32) {
        let Some(code) = self.theme_editor.as_mut().and_then(|ed| ed.code.as_mut()) else {
            return;
        };
        let text = code.layout.text;
        let row = code.scroll + ((y - text.y) / code.line_height).max(0.0) as usize;
        let column = ((x - text.x) / code.advance).round().max(0.0) as usize;
        let row = row.min(code.rows.len().saturating_sub(1));
        let at = crate::canvas::notepad_index_of(code.entry.text(), &code.rows, row, column);
        code.entry.place(at, false);
        self.caret_on = true;
        self.caret_phase = 0.0;
        self.tree.invalidate_rect(self.layout.window);
    }

    /// A key on the code card: the notepad's editing, and the text checked
    /// after every edit — worn at once when it compiles.
    fn code_card_key(&mut self, key: &winit::keyboard::Key, ctrl: bool, shift: bool) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let Some(ed) = self.theme_editor.as_mut() else {
            return false;
        };
        let Some(code) = ed.code.as_mut() else {
            return false;
        };
        let mut edited = false;
        match key {
            Key::Named(NamedKey::Escape) => {
                ed.code = None;
                self.relayout_theme_editor();
                return true;
            }
            // A new line keeps the one above's indent: code is written
            // indented, and retyping it each line is the editor's job.
            Key::Named(NamedKey::Enter) => {
                let text = code.entry.text();
                let start = text[..code.entry.caret()].rfind('\n').map_or(0, |i| i + 1);
                let indent: String = text[start..].chars().take_while(|c| *c == ' ').collect();
                code.entry.insert(&format!("\n{indent}"));
                edited = true;
            }
            Key::Named(NamedKey::Tab) => {
                if shift {
                    crate::canvas::notepad_outdent(&mut code.entry);
                } else {
                    crate::canvas::notepad_indent(&mut code.entry);
                }
                edited = true;
            }
            Key::Named(NamedKey::ArrowUp) => {
                let goal = crate::canvas::notepad_column_of(
                    code.entry.text(),
                    &code.rows,
                    code.entry.caret(),
                );
                crate::canvas::notepad_step_row(&mut code.entry, &code.rows, -1, goal, shift);
            }
            Key::Named(NamedKey::ArrowDown) => {
                let goal = crate::canvas::notepad_column_of(
                    code.entry.text(),
                    &code.rows,
                    code.entry.caret(),
                );
                crate::canvas::notepad_step_row(&mut code.entry, &code.rows, 1, goal, shift);
            }
            Key::Named(NamedKey::Home) if !ctrl => {
                crate::canvas::notepad_line_home(&mut code.entry, &code.rows, shift)
            }
            Key::Named(NamedKey::End) if !ctrl => {
                crate::canvas::notepad_line_end(&mut code.entry, &code.rows, shift)
            }
            Key::Character(c) if ctrl && c.eq_ignore_ascii_case("v") => {
                let pasted = crate::canvas::notepad_paste_text(&self.clipboard_text.paste_raw());
                if !pasted.is_empty() {
                    code.entry.insert(&pasted);
                    edited = true;
                }
            }
            key => {
                let mut clipboard = std::mem::take(&mut self.clipboard_text);
                let what =
                    crate::canvas::text_key(&mut code.entry, key, ctrl, shift, &mut clipboard);
                self.clipboard_text = clipboard;
                match what {
                    crate::canvas::TextKey::Edited => edited = true,
                    crate::canvas::TextKey::Moved => {}
                    // Undo, save and the rest stay the program's.
                    crate::canvas::TextKey::Ignored => return !ctrl,
                }
            }
        }
        if edited {
            code.problem = shader_problem(code.entry.text());
            if code.problem.is_none() {
                let (panel, index) = code.target;
                let text = code.entry.text().to_string();
                if let Some(Layer::Shader(s)) =
                    ed.working.backdrops.layers_mut(panel).get_mut(index)
                {
                    s.shader = text;
                }
                self.caret_on = true;
                self.caret_phase = 0.0;
                self.wear_working_theme();
                return true;
            }
        }
        self.caret_on = true;
        self.caret_phase = 0.0;
        self.relayout_theme_editor();
        true
    }
}

/// The picker or a slider moved to (`x`, `y`). Whether the theme changed.
fn drag_to(ed: &mut ThemeEditor, x: f32, y: f32) -> bool {
    let Some(drag) = ed.drag else {
        return false;
    };
    let l = &ed.layout;
    match drag {
        PickerDrag::Square | PickerDrag::Hue | PickerDrag::Alpha => {
            let Some(c) = ed.colour() else {
                return false;
            };
            let [_, s, v, a] = c.to_hsva();
            let next = match drag {
                PickerDrag::Square => crate::canvas::picker_colour(l.square, ed.hue, a, x, y),
                PickerDrag::Hue => {
                    ed.hue = crate::canvas::hue_at(l.hue, y);
                    Color::from_hsva(ed.hue, s, v, a)
                }
                _ => Color::from_hsva(ed.hue, s, v, crate::canvas::alpha_at(l.alpha, x)),
            };
            if next == c {
                return false;
            }
            ed.set_colour(next);
            true
        }
        PickerDrag::Opacity | PickerDrag::Speed => {
            let value = crate::canvas::slider_at(
                if drag == PickerDrag::Opacity {
                    l.opacity
                } else {
                    l.speed
                },
                x,
            );
            let panel = ed.panel();
            let Some(index) = ed.layer else { return false };
            let Some(layer) = ed.working.backdrops.layers_mut(panel).get_mut(index) else {
                return false;
            };
            match (drag, layer) {
                (PickerDrag::Opacity, Layer::Image(b)) => b.opacity = value,
                (PickerDrag::Opacity, Layer::Shader(s)) => s.opacity = value,
                (PickerDrag::Opacity, Layer::Gradient(g)) => g.opacity = value,
                (PickerDrag::Speed, Layer::Shader(s)) => s.speed = value * MOST_SPEED,
                _ => return false,
            }
            true
        }
    }
}
