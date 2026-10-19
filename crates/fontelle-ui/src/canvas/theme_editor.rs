//! Making a theme inside the studio (`docs/next-update-plan.md` phase 5):
//! a card docked at the window's right edge with the studio left showing
//! beside it, so every change is seen on the real thing as it is made.
//!
//! Two tabs. **Colours** lists every palette colour by name under a group
//! heading, and a picker — a saturation/value square, a hue strip, an alpha
//! strip and a hex field — edits the one chosen. **Backdrops** picks one of
//! the seven sections and lists its layers top first, with the ways to add
//! one and the chosen layer's own controls; the same picker edits a colour
//! layer's colour or a gradient's stop, and a shader layer opens the code
//! card beside it.
//!
//! A pure view-model (INVARIANT 2): rectangles from counts, a hit from a
//! point, and the picker's arithmetic. The working copy of the theme is the
//! window's; what saving it means is the host's.

use crate::layout::Rect;
use crate::theme::{Color, Metrics};

pub const THEME_EDITOR_TITLE: &str = "Edit theme";
pub const THEME_EDITOR_SAVE: &str = "Save";
pub const THEME_EDITOR_CLOSE: &str = "\u{00d7}";
pub const THEME_EDITOR_TABS: [&str; 2] = ["Colours", "Backdrops"];
/// The three kinds of layer the card can add, in [`ThemeEditorLayout::add`]'s
/// order. Pictures are chosen from the settings page, which has the picker.
pub const THEME_EDITOR_ADD: [&str; 3] = ["+ Colour", "+ Gradient", "+ Shader"];
pub const THEME_EDITOR_OPACITY: &str = "Opacity";
pub const THEME_EDITOR_SPEED: &str = "Speed";
pub const THEME_EDITOR_CODE: &str = "Edit code\u{2026}";
pub const THEME_EDITOR_NO_LAYERS: &str = "Nothing behind this section yet.";
/// Where the picker is, when the chosen layer has no colour of its own.
pub const THEME_EDITOR_NO_COLOUR: &str = "A colour or gradient layer is picked here.";
/// A gradient's two ends, as its stop chips say them.
pub const THEME_EDITOR_STOPS: [&str; 2] = ["From", "To"];

/// Which half of the card is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeEditorTab {
    #[default]
    Colours,
    Backdrops,
}

/// The heading a palette colour is listed under, by its name. Every name
/// the palette has is in one; a new colour without one lands in "Other",
/// still listed.
pub fn palette_group(name: &str) -> &'static str {
    match name {
        "window" | "panel" | "panel_header" | "border" | "ground" | "paper" | "edge" => "Surfaces",
        "text" | "text_muted" | "ink" | "faint" | "caret" => "Text",
        "accent" | "selection" | "playhead" | "meter" | "meter_peak" => "Accents",
        "grid_line_sub" | "grid_line" | "grid_line_strong" => "Grid",
        "note" | "note_selected" | "note_silent" | "key_white" | "key_black" | "key_dead"
        | "row_accidental" | "row_dead" | "row_out_of_scale" | "row_scale_root" => "Piano roll",
        "param_automated" | "modulation" | "mod_envelope" | "mod_lfo" | "mod_macro"
        | "mod_note" | "mod_performance" => "Modulation",
        "singer_voice" | "singer_expression" | "singer_performance" | "singer_output" => "Singer",
        _ => "Other",
    }
}

/// The order the groups are listed in.
pub const PALETTE_GROUPS: [&str; 8] = [
    "Surfaces",
    "Text",
    "Accents",
    "Grid",
    "Piano roll",
    "Modulation",
    "Singer",
    "Other",
];

/// `names` in the order the card lists them: by group, then by name.
pub fn palette_order(names: &[String]) -> Vec<String> {
    let mut out = names.to_vec();
    out.sort_by_key(|n| {
        let group = PALETTE_GROUPS
            .iter()
            .position(|g| *g == palette_group(n))
            .unwrap_or(PALETTE_GROUPS.len());
        (group, n.clone())
    });
    out
}

/// A colour's name as the card writes it: `text_muted` → "Text muted".
pub fn palette_label(name: &str) -> String {
    let spaced = name.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The colour at (`x`, `y`) in `square` at hue `hue` and alpha `alpha`:
/// saturation grows left to right, value bottom to top.
pub fn picker_colour(square: Rect, hue: f32, alpha: f32, x: f32, y: f32) -> Color {
    let s = ((x - square.x) / square.width.max(1.0)).clamp(0.0, 1.0);
    let v = 1.0 - ((y - square.y) / square.height.max(1.0)).clamp(0.0, 1.0);
    Color::from_hsva(hue, s, v, alpha)
}

/// Where `color` sits in `square` — the picker's ring.
pub fn picker_point(square: Rect, color: Color) -> (f32, f32) {
    let [_, s, v, _] = color.to_hsva();
    (
        square.x + s * square.width,
        square.y + (1.0 - v) * square.height,
    )
}

/// The hue at `y` down the hue strip: red at the top, round to red again.
pub fn hue_at(strip: Rect, y: f32) -> f32 {
    ((y - strip.y) / strip.height.max(1.0)).clamp(0.0, 1.0) * 360.0
}

/// The alpha at `x` across the alpha strip.
pub fn alpha_at(strip: Rect, x: f32) -> f32 {
    ((x - strip.x) / strip.width.max(1.0)).clamp(0.0, 1.0)
}

/// A slider's value, 0 to 1, at `x` across it.
pub fn slider_at(slider: Rect, x: f32) -> f32 {
    alpha_at(slider, x)
}

/// One layer's row: the row itself, and its three buttons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerRowLayout {
    pub row: Rect,
    pub up: Rect,
    pub down: Rect,
    pub remove: Rect,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeEditorLayout {
    pub frame: Rect,
    pub title: Rect,
    pub save: Rect,
    pub close: Rect,
    pub tabs: [Rect; 2],
    pub square: Rect,
    pub hue: Rect,
    pub alpha: Rect,
    pub hex: Rect,
    /// Beside the hex field: the colour being edited, as it is.
    pub swatch: Rect,
    /// Colours: where the rows show, which they are clipped to.
    pub list: Rect,
    /// Colours: one per palette colour, in [`palette_order`], scrolled.
    pub rows: Vec<Rect>,
    /// Colours: the group headings, by the row each sits over.
    pub headings: Vec<(usize, Rect)>,
    /// Backdrops: one chip per [`BackdropPanel::ALL`](crate::theme::BackdropPanel::ALL).
    pub sections: Vec<Rect>,
    /// Backdrops: the section's layers, top first.
    pub layers: Vec<LayerRowLayout>,
    /// Backdrops: where [`THEME_EDITOR_NO_LAYERS`] goes when there are none.
    pub empty: Rect,
    /// Backdrops: [`THEME_EDITOR_ADD`]'s three.
    pub add: Vec<Rect>,
    /// Backdrops: the chosen layer's controls. Laid out always; which are
    /// drawn and pressable depends on its kind.
    pub opacity: Rect,
    pub speed: Rect,
    pub stops: [Rect; 2],
    pub code: Rect,
}

/// What a point on the card is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeEditorHit {
    Close,
    Save,
    Tab(ThemeEditorTab),
    Square,
    Hue,
    Alpha,
    Hex,
    Row(usize),
    Section(usize),
    Layer(usize),
    LayerUp(usize),
    LayerDown(usize),
    RemoveLayer(usize),
    Add(usize),
    Opacity,
    Speed,
    Stop(usize),
    Code,
    /// On the card, on nothing: swallowed, so it reaches nothing under it.
    Card,
}

/// The card for `window`, unscrolled. See [`theme_editor_layout_scrolled`].
pub fn theme_editor_layout(
    window: Rect,
    metrics: &Metrics,
    tab: ThemeEditorTab,
    colours: usize,
    layers: usize,
) -> ThemeEditorLayout {
    theme_editor_layout_scrolled(window, metrics, tab, colours, layers, 0.0, &[])
}

/// The card, with the colour list scrolled down by `scroll` points and a
/// group heading over each of `heading_rows`' rows.
pub fn theme_editor_layout_scrolled(
    window: Rect,
    metrics: &Metrics,
    tab: ThemeEditorTab,
    colours: usize,
    layers: usize,
    scroll: f32,
    heading_rows: &[usize],
) -> ThemeEditorLayout {
    let margin = metrics.panel_margin;
    let pad = metrics.panel_padding.max(6.0);
    let row = metrics.row_height.max(18.0);
    let gap = 6.0;
    let top = window.y + metrics.transport_bar_height + margin;
    let width = (window.width * 0.3)
        .clamp(300.0, 380.0)
        .min(window.width - 2.0 * margin);
    let frame = Rect::new(
        window.right() - margin - width,
        top,
        width,
        (window.bottom() - margin - top).max(0.0),
    );
    let inner = frame.inset(pad);

    let close = Rect::new(inner.right() - row, inner.y, row, row);
    let save = Rect::new(close.x - gap - 64.0, inner.y, 64.0, row);
    let title = Rect::new(inner.x, inner.y, (save.x - gap - inner.x).max(0.0), row);
    let mut y = inner.y + row + gap;

    let half = (inner.width - gap) / 2.0;
    let tabs = [
        Rect::new(inner.x, y, half, row),
        Rect::new(inner.x + half + gap, y, half, row),
    ];
    y += row + gap * 2.0;

    let strip = 16.0;
    let square_h = (frame.height * 0.2).clamp(90.0, 150.0);
    let square = Rect::new(inner.x, y, inner.width - strip - gap, square_h);
    let hue = Rect::new(square.right() + gap, y, strip, square_h);
    y += square_h + gap;
    let alpha = Rect::new(inner.x, y, square.width, 12.0);
    y += 12.0 + gap;
    let hex = Rect::new(inner.x, y, 110.0_f32.min(inner.width / 2.0), row);
    let swatch = Rect::new(hex.right() + gap, y, row * 2.0, row);
    y += row + gap * 2.0;

    let mut layout = ThemeEditorLayout {
        frame,
        title,
        save,
        close,
        tabs,
        square,
        hue,
        alpha,
        hex,
        swatch,
        list: Rect::default(),
        rows: Vec::new(),
        headings: Vec::new(),
        sections: Vec::new(),
        layers: Vec::new(),
        empty: Rect::default(),
        add: Vec::new(),
        opacity: Rect::default(),
        speed: Rect::default(),
        stops: [Rect::default(); 2],
        code: Rect::default(),
    };

    match tab {
        ThemeEditorTab::Colours => {
            let list = Rect::new(inner.x, y, inner.width, (inner.bottom() - y).max(0.0));
            let mut at = list.y - scroll;
            for i in 0..colours {
                if heading_rows.contains(&i) {
                    layout
                        .headings
                        .push((i, Rect::new(list.x, at, list.width, row)));
                    at += row;
                }
                layout.rows.push(Rect::new(list.x, at, list.width, row));
                at += row;
            }
            layout.list = list;
        }
        ThemeEditorTab::Backdrops => {
            // The seven sections in two rows: four, then three.
            let chip_w = (inner.width - 3.0 * gap) / 4.0;
            for i in 0..crate::theme::BackdropPanel::ALL.len() {
                let (r, c) = (i / 4, i % 4);
                layout.sections.push(Rect::new(
                    inner.x + c as f32 * (chip_w + gap),
                    y + r as f32 * (row + gap),
                    chip_w,
                    row,
                ));
            }
            y += 2.0 * (row + gap) + gap;
            for i in 0..layers {
                let r = Rect::new(inner.x, y + i as f32 * row, inner.width, row);
                let remove = Rect::new(r.right() - row, r.y, row, row);
                let down = Rect::new(remove.x - row, r.y, row, row);
                let up = Rect::new(down.x - row, r.y, row, row);
                layout.layers.push(LayerRowLayout {
                    row: Rect::new(r.x, r.y, (up.x - r.x).max(0.0), row),
                    up,
                    down,
                    remove,
                });
            }
            // An empty section still has a line saying so.
            if layers == 0 {
                layout.empty = Rect::new(inner.x, y, inner.width, row);
            }
            y += layers.max(1) as f32 * row + gap;
            let add_w = (inner.width - 2.0 * gap) / 3.0;
            layout.add = (0..3)
                .map(|i| Rect::new(inner.x + i as f32 * (add_w + gap), y, add_w, row))
                .collect();
            y += row + gap * 2.0;
            let label = 64.0;
            layout.opacity = Rect::new(inner.x + label, y, inner.width - label, row);
            y += row + gap;
            layout.speed = Rect::new(inner.x + label, y, inner.width - label, row);
            y += row + gap;
            layout.stops = [
                Rect::new(inner.x, y, half, row),
                Rect::new(inner.x + half + gap, y, half, row),
            ];
            layout.code = Rect::new(inner.x, y, inner.width, row);
        }
    }
    layout
}

/// What (`x`, `y`) is on the card, or `None` off it.
pub fn theme_editor_hit(l: &ThemeEditorLayout, x: f32, y: f32) -> Option<ThemeEditorHit> {
    if !l.frame.contains(x, y) {
        return None;
    }
    let fixed = [
        (l.close, ThemeEditorHit::Close),
        (l.save, ThemeEditorHit::Save),
        (l.tabs[0], ThemeEditorHit::Tab(ThemeEditorTab::Colours)),
        (l.tabs[1], ThemeEditorHit::Tab(ThemeEditorTab::Backdrops)),
        (l.square, ThemeEditorHit::Square),
        (l.hue, ThemeEditorHit::Hue),
        (l.alpha, ThemeEditorHit::Alpha),
        (l.hex, ThemeEditorHit::Hex),
    ];
    if let Some((_, hit)) = fixed.iter().find(|(r, _)| r.contains(x, y)) {
        return Some(*hit);
    }
    if l.list.contains(x, y)
        && let Some(i) = l.rows.iter().position(|r| r.contains(x, y))
    {
        return Some(ThemeEditorHit::Row(i));
    }
    if let Some(i) = l.sections.iter().position(|r| r.contains(x, y)) {
        return Some(ThemeEditorHit::Section(i));
    }
    for (i, layer) in l.layers.iter().enumerate() {
        if layer.remove.contains(x, y) {
            return Some(ThemeEditorHit::RemoveLayer(i));
        }
        if layer.up.contains(x, y) {
            return Some(ThemeEditorHit::LayerUp(i));
        }
        if layer.down.contains(x, y) {
            return Some(ThemeEditorHit::LayerDown(i));
        }
        if layer.row.contains(x, y) {
            return Some(ThemeEditorHit::Layer(i));
        }
    }
    if let Some(i) = l.add.iter().position(|r| r.contains(x, y)) {
        return Some(ThemeEditorHit::Add(i));
    }
    // The opacity and speed rows take their label's width too: the whole
    // line is the slider's to grab.
    let line = |r: Rect| Rect::new(l.frame.x, r.y, l.frame.width, r.height);
    if !l.opacity.is_empty() && line(l.opacity).contains(x, y) {
        return Some(ThemeEditorHit::Opacity);
    }
    if !l.speed.is_empty() && line(l.speed).contains(x, y) {
        return Some(ThemeEditorHit::Speed);
    }
    if let Some(i) = l.stops.iter().position(|r| r.contains(x, y)) {
        return Some(ThemeEditorHit::Stop(i));
    }
    if l.code.contains(x, y) {
        return Some(ThemeEditorHit::Code);
    }
    Some(ThemeEditorHit::Card)
}

pub const SHADER_EDITOR_TITLE: &str = "Shader";
pub const SHADER_EDITOR_DONE: &str = "Done";
pub const SHADER_EDITOR_FINE: &str = "Compiles \u{2014} showing behind the studio.";

/// The code card: the studio's side of the window, beside the theme card.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShaderEditorLayout {
    pub frame: Rect,
    pub title: Rect,
    pub done: Rect,
    /// The monospace sheet the WGSL is written on.
    pub text: Rect,
    /// One line under it: the compile error, or that it compiles.
    pub problem: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderEditorHit {
    Done,
    Text,
    Card,
}

/// The code card for `window`, left of the theme card at `beside`. It
/// leaves the top of the studio showing: the arrangement's backdrop is the
/// one most themes are made for.
pub fn shader_editor_layout(window: Rect, beside: Rect, metrics: &Metrics) -> ShaderEditorLayout {
    let margin = metrics.panel_margin;
    let pad = metrics.panel_padding.max(6.0);
    let row = metrics.row_height.max(18.0);
    let right = beside.x - margin;
    let width = ((right - window.x) * 0.6)
        .clamp(260.0, 720.0)
        .min(right - window.x - margin);
    let top = beside.y + beside.height * 0.35;
    let frame = Rect::new(
        right - width,
        top,
        width.max(0.0),
        (beside.bottom() - top).max(0.0),
    );
    let inner = frame.inset(pad);
    let done = Rect::new(inner.right() - 64.0, inner.y, 64.0, row);
    let title = Rect::new(inner.x, inner.y, (done.x - 6.0 - inner.x).max(0.0), row);
    let problem = Rect::new(inner.x, inner.bottom() - row, inner.width, row);
    let text_top = inner.y + row + 6.0;
    let text = Rect::new(
        inner.x,
        text_top,
        inner.width,
        (problem.y - 6.0 - text_top).max(0.0),
    );
    ShaderEditorLayout {
        frame,
        title,
        done,
        text,
        problem,
    }
}

pub fn shader_editor_hit(l: &ShaderEditorLayout, x: f32, y: f32) -> Option<ShaderEditorHit> {
    if !l.frame.contains(x, y) {
        return None;
    }
    Some(if l.done.contains(x, y) {
        ShaderEditorHit::Done
    } else if l.text.contains(x, y) {
        ShaderEditorHit::Text
    } else {
        ShaderEditorHit::Card
    })
}
