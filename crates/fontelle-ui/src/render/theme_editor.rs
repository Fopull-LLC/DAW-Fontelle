//! The theme editor's two cards, painted. Where everything goes is
//! `canvas::theme_editor`'s; what is chosen is the window's
//! [`ThemeEditor`]. The theme handed in is the working copy, since the
//! window wears it while the card is open — so the card itself previews the
//! colours it is editing.

use vello::Scene;
use vello::kurbo::{Affine, Point, Rect as KRect};
use vello::peniko::{BlendMode, Brush, Fill, Gradient};

use super::{
    draw_line, draw_text_clipped, draw_welcome_button, fill_rect, fill_rect_rounded,
    stroke_rect_rounded,
};
use crate::app::theme_editor::{LayerKind, ThemeEditor, layer_caption};
use crate::canvas::{
    SHADER_EDITOR_DONE, SHADER_EDITOR_TITLE, THEME_EDITOR_ADD, THEME_EDITOR_CLOSE,
    THEME_EDITOR_CODE, THEME_EDITOR_NO_COLOUR, THEME_EDITOR_NO_LAYERS, THEME_EDITOR_OPACITY,
    THEME_EDITOR_SAVE, THEME_EDITOR_SPEED, THEME_EDITOR_STOPS, THEME_EDITOR_TABS,
    THEME_EDITOR_TITLE, ThemeEditorTab,
};
use crate::layout::Rect;
use crate::text::Labels;
use crate::theme::{BackdropPanel, Color, Theme};

/// The layer rows' three buttons.
const LAYER_UP: &str = "\u{2191}";
const LAYER_DOWN: &str = "\u{2193}";
const LAYER_REMOVE: &str = "\u{00d7}";

/// Every string the cards draw, for the window to shape first.
pub fn theme_editor_words(ed: &ThemeEditor) -> Vec<String> {
    let mut words: Vec<String> = [
        THEME_EDITOR_TITLE,
        THEME_EDITOR_SAVE,
        THEME_EDITOR_CLOSE,
        THEME_EDITOR_OPACITY,
        THEME_EDITOR_SPEED,
        THEME_EDITOR_CODE,
        THEME_EDITOR_NO_LAYERS,
        THEME_EDITOR_NO_COLOUR,
        LAYER_UP,
        LAYER_DOWN,
        LAYER_REMOVE,
        SHADER_EDITOR_TITLE,
        SHADER_EDITOR_DONE,
    ]
    .into_iter()
    .chain(THEME_EDITOR_TABS)
    .chain(THEME_EDITOR_ADD)
    .chain(THEME_EDITOR_STOPS)
    .map(str::to_string)
    .collect();
    words.push(title(ed));
    words.extend(ed.names.iter().map(|n| crate::canvas::palette_label(n)));
    words.extend(crate::canvas::PALETTE_GROUPS.iter().map(|g| g.to_string()));
    words.extend(BackdropPanel::ALL.iter().map(|p| p.label().to_string()));
    words.extend(ed.stack().iter().map(layer_caption));
    words.push(hex_words(ed));
    if let Some(code) = &ed.code {
        words.push(code.problem_words());
    }
    words
}

fn title(ed: &ThemeEditor) -> String {
    format!("{THEME_EDITOR_TITLE} \u{00b7} {}", ed.working.name)
}

/// What the hex field shows: what is typed, or the colour.
fn hex_words(ed: &ThemeEditor) -> String {
    match (&ed.hex, ed.colour()) {
        (Some(entry), _) => entry.text().to_string(),
        (None, Some(c)) => c.to_hex(),
        (None, None) => String::new(),
    }
}

fn krect(r: Rect) -> KRect {
    KRect::new(r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64)
}

fn fill_gradient(scene: &mut Scene, r: Rect, from: Point, to: Point, stops: &[(f32, Color)]) {
    if r.is_empty() {
        return;
    }
    let gradient = Gradient::new_linear(from, to).with_stops(
        stops
            .iter()
            .map(|(at, c)| (*at, c.to_peniko()))
            .collect::<Vec<_>>()
            .as_slice(),
    );
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        &Brush::Gradient(gradient),
        None,
        &krect(r),
    );
}

fn across(r: Rect) -> (Point, Point) {
    (
        Point::new(r.x as f64, r.y as f64),
        Point::new(r.right() as f64, r.y as f64),
    )
}

fn down(r: Rect) -> (Point, Point) {
    (
        Point::new(r.x as f64, r.y as f64),
        Point::new(r.x as f64, r.bottom() as f64),
    )
}

/// A checkerboard under anything see-through, so alpha reads as alpha.
fn checker(scene: &mut Scene, r: Rect) {
    fill_rect(scene, r, Color::rgb(0xcc, 0xcc, 0xcc));
    let cell = 6.0;
    let mut y = r.y;
    let mut odd_row = false;
    while y < r.bottom() {
        let mut x = r.x + if odd_row { cell } else { 0.0 };
        while x < r.right() {
            let c = Rect::new(x, y, cell, cell).intersection(&r);
            fill_rect(scene, c, Color::rgb(0x99, 0x99, 0x99));
            x += cell * 2.0;
        }
        y += cell;
        odd_row = !odd_row;
    }
}

/// A chip: lit when chosen.
fn chip(scene: &mut Scene, theme: &Theme, labels: &Labels, word: &str, r: Rect, on: bool) {
    let p = &theme.palette;
    if on {
        fill_rect_rounded(scene, r, r.height / 2.0, p.accent.with_alpha(0x55));
    }
    stroke_rect_rounded(
        scene,
        r,
        r.height / 2.0,
        1.0,
        if on { p.accent } else { p.border },
    );
    if let Some(shaped) = labels.get(word) {
        draw_text_clipped(
            scene,
            shaped,
            r,
            r.x + (r.width - shaped.width) / 2.0,
            r.y + (r.height - shaped.height) / 2.0,
            if on { p.text } else { p.text_muted },
        );
    }
}

fn centred(scene: &mut Scene, labels: &Labels, word: &str, r: Rect, ink: Color) {
    if let Some(shaped) = labels.get(word) {
        draw_text_clipped(
            scene,
            shaped,
            r,
            r.x + (r.width - shaped.width) / 2.0,
            r.y + (r.height - shaped.height) / 2.0,
            ink,
        );
    }
}

fn slider(scene: &mut Scene, theme: &Theme, labels: &Labels, word: &str, r: Rect, value: f32) {
    let p = &theme.palette;
    let label = Rect::new(r.x - 64.0, r.y, 60.0, r.height);
    draw_line(scene, labels, word, label, p.text_muted);
    let track = Rect::new(r.x, r.y + r.height / 2.0 - 2.0, r.width, 4.0);
    fill_rect_rounded(scene, track, 2.0, p.border);
    let filled = Rect::new(track.x, track.y, track.width * value.clamp(0.0, 1.0), 4.0);
    fill_rect_rounded(scene, filled, 2.0, p.accent);
    let knob = 12.0;
    let kx = r.x + r.width * value.clamp(0.0, 1.0) - knob / 2.0;
    fill_rect_rounded(
        scene,
        Rect::new(kx, r.y + (r.height - knob) / 2.0, knob, knob),
        knob / 2.0,
        p.text,
    );
}

pub(super) fn draw_theme_editor(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    ed: &ThemeEditor,
    caret_on: bool,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &ed.layout;
    if l.frame.is_empty() {
        return;
    }
    // Solid whatever the theme's see-through panels say: words over words
    // are not readable.
    fill_rect_rounded(scene, l.frame, m.corner_radius, p.panel.with_alpha(0xff));
    stroke_rect_rounded(scene, l.frame, m.corner_radius, 1.0, p.border);
    draw_line(scene, labels, &title(ed), l.title, p.text);
    draw_welcome_button(scene, theme, labels, THEME_EDITOR_SAVE, l.save, true);
    centred(scene, labels, THEME_EDITOR_CLOSE, l.close, p.text_muted);
    for (i, (tab, word)) in [ThemeEditorTab::Colours, ThemeEditorTab::Backdrops]
        .into_iter()
        .zip(THEME_EDITOR_TABS)
        .enumerate()
    {
        chip(scene, theme, labels, word, l.tabs[i], ed.tab == tab);
    }

    // The picker, live only while there is a colour to edit.
    let colour = ed.colour();
    if let Some(c) = colour {
        let pure = Color::from_hsva(ed.hue, 1.0, 1.0, 1.0);
        fill_rect(scene, l.square, pure);
        let (a, b) = across(l.square);
        fill_gradient(
            scene,
            l.square,
            a,
            b,
            &[
                (0.0, Color::rgb(255, 255, 255)),
                (1.0, Color::rgba(255, 255, 255, 0)),
            ],
        );
        let (a, b) = down(l.square);
        fill_gradient(
            scene,
            l.square,
            a,
            b,
            &[(0.0, Color::rgba(0, 0, 0, 0)), (1.0, Color::rgb(0, 0, 0))],
        );
        let (x, y) = crate::canvas::picker_point(l.square, c);
        let ring = Rect::new(x - 5.0, y - 5.0, 10.0, 10.0);
        stroke_rect_rounded(scene, ring, 5.0, 2.0, Color::rgb(255, 255, 255));
        stroke_rect_rounded(scene, ring.inset(-1.5), 6.5, 1.0, Color::rgb(0, 0, 0));

        let stops: Vec<(f32, Color)> = (0..=6)
            .map(|i| {
                let at = i as f32 / 6.0;
                (at, Color::from_hsva(at * 360.0, 1.0, 1.0, 1.0))
            })
            .collect();
        let (a, b) = down(l.hue);
        fill_gradient(scene, l.hue, a, b, &stops);
        let hy = l.hue.y + ed.hue / 360.0 * l.hue.height;
        fill_rect(
            scene,
            Rect::new(l.hue.x - 2.0, hy - 1.5, l.hue.width + 4.0, 3.0),
            Color::rgb(255, 255, 255),
        );

        checker(scene, l.alpha);
        let (a, b) = across(l.alpha);
        fill_gradient(
            scene,
            l.alpha,
            a,
            b,
            &[(0.0, c.with_alpha(0)), (1.0, c.with_alpha(0xff))],
        );
        let ax = l.alpha.x + f32::from(c.0[3]) / 255.0 * l.alpha.width;
        fill_rect(
            scene,
            Rect::new(ax - 1.5, l.alpha.y - 2.0, 3.0, l.alpha.height + 4.0),
            p.text,
        );

        checker(scene, l.swatch);
        fill_rect(scene, l.swatch, c);
        stroke_rect_rounded(scene, l.swatch, 2.0, 1.0, p.border);
    } else {
        // Nothing to pick: one line where the picker would be, rather than
        // a picker that does nothing.
        let at = Rect::new(
            l.square.x,
            l.square.y,
            l.hue.right() - l.square.x,
            l.square.height,
        );
        centred(scene, labels, THEME_EDITOR_NO_COLOUR, at, p.text_muted);
        return draw_tabs_below(scene, theme, labels, ed, caret_on);
    }
    fill_rect_rounded(scene, l.hex, m.corner_radius, p.window.with_alpha(0xff));
    stroke_rect_rounded(
        scene,
        l.hex,
        m.corner_radius,
        1.0,
        if ed.hex.is_some() { p.accent } else { p.border },
    );
    let hex = hex_words(ed);
    let inside = l.hex.inset(4.0);
    draw_line(scene, labels, &hex, inside, p.text);
    if let Some(entry) = &ed.hex
        && caret_on
    {
        let width = labels.get(&hex).map_or(0.0, |t| t.width);
        // Typed text is short: the caret sits at its end, which is where a
        // field opened selected-all puts it once anything is typed.
        let x = if entry.selection().is_some() {
            inside.x
        } else {
            inside.x + width + 1.0
        };
        fill_rect(scene, Rect::new(x, inside.y, 1.5, inside.height), p.accent);
    }
    draw_tabs_below(scene, theme, labels, ed, caret_on);
}

/// What is under the picker: the tab's own half, and the code card.
fn draw_tabs_below(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    ed: &ThemeEditor,
    caret_on: bool,
) {
    match ed.tab {
        ThemeEditorTab::Colours => draw_colours(scene, theme, labels, ed),
        ThemeEditorTab::Backdrops => draw_backdrops(scene, theme, labels, ed),
    }

    if let Some(code) = &ed.code {
        draw_code_card(scene, theme, labels, code, caret_on);
    }
}

fn draw_colours(scene: &mut Scene, theme: &Theme, labels: &Labels, ed: &ThemeEditor) {
    let p = &theme.palette;
    let l = &ed.layout;
    if l.list.is_empty() {
        return;
    }
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &krect(l.list),
    );
    for (row, rect) in &l.headings {
        if !rect.intersects(&l.list) {
            continue;
        }
        let group = crate::canvas::palette_group(&ed.names[*row]);
        draw_line(scene, labels, group, *rect, p.text_muted);
    }
    for (i, rect) in l.rows.iter().enumerate() {
        if !rect.intersects(&l.list) {
            continue;
        }
        let name = &ed.names[i];
        if i == ed.token {
            fill_rect_rounded(scene, *rect, 3.0, p.accent.with_alpha(0x40));
        }
        let sw = Rect::new(
            rect.x + 8.0,
            rect.y + 3.0,
            rect.height - 6.0,
            rect.height - 6.0,
        );
        if let Some(c) = ed.working.palette.get(name) {
            checker(scene, sw);
            fill_rect(scene, sw, c);
        }
        stroke_rect_rounded(scene, sw, 2.0, 1.0, p.border);
        let words = Rect::new(
            sw.right() + 8.0,
            rect.y,
            rect.right() - sw.right() - 8.0,
            rect.height,
        );
        draw_line(
            scene,
            labels,
            &crate::canvas::palette_label(name),
            words,
            p.text,
        );
    }
    scene.pop_layer();
}

fn draw_backdrops(scene: &mut Scene, theme: &Theme, labels: &Labels, ed: &ThemeEditor) {
    let p = &theme.palette;
    let l = &ed.layout;
    for (i, (r, panel)) in l.sections.iter().zip(BackdropPanel::ALL).enumerate() {
        chip(scene, theme, labels, panel.label(), *r, i == ed.section);
    }
    if !l.empty.is_empty() {
        draw_line(scene, labels, THEME_EDITOR_NO_LAYERS, l.empty, p.text_muted);
    }
    let chosen_row = ed.layer.map(|k| ed.row_of(k));
    for (row, layer) in l.layers.iter().enumerate() {
        let Some(index) = ed.index_of_row(row) else {
            continue;
        };
        let whole = layer.row.union(&layer.remove);
        if chosen_row == Some(row) {
            fill_rect_rounded(scene, whole, 3.0, p.accent.with_alpha(0x40));
        }
        let caption = layer_caption(&ed.stack()[index]);
        let words = Rect::new(
            layer.row.x + 8.0,
            layer.row.y,
            layer.row.width - 8.0,
            layer.row.height,
        );
        draw_line(scene, labels, &caption, words, p.text);
        centred(scene, labels, LAYER_UP, layer.up, p.text_muted);
        centred(scene, labels, LAYER_DOWN, layer.down, p.text_muted);
        centred(scene, labels, LAYER_REMOVE, layer.remove, p.text_muted);
    }
    for (r, word) in l.add.iter().zip(THEME_EDITOR_ADD) {
        draw_welcome_button(scene, theme, labels, word, *r, false);
    }
    let Some(kind) = ed.chosen_kind() else {
        return;
    };
    if kind != LayerKind::Solid {
        slider(
            scene,
            theme,
            labels,
            THEME_EDITOR_OPACITY,
            l.opacity,
            ed.opacity(),
        );
    }
    match kind {
        LayerKind::Shader => {
            slider(
                scene,
                theme,
                labels,
                THEME_EDITOR_SPEED,
                l.speed,
                ed.speed() / 4.0,
            );
            draw_welcome_button(
                scene,
                theme,
                labels,
                THEME_EDITOR_CODE,
                l.code,
                ed.code.is_some(),
            );
        }
        LayerKind::Gradient => {
            for (i, (r, word)) in l.stops.iter().zip(THEME_EDITOR_STOPS).enumerate() {
                chip(scene, theme, labels, word, *r, i == ed.stop);
            }
        }
        LayerKind::Picture | LayerKind::Solid => {}
    }
}

fn draw_code_card(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    code: &crate::app::theme_editor::CodeCard,
    caret_on: bool,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &code.layout;
    if l.frame.is_empty() {
        return;
    }
    fill_rect_rounded(scene, l.frame, m.corner_radius, p.panel.with_alpha(0xff));
    stroke_rect_rounded(scene, l.frame, m.corner_radius, 1.0, p.border);
    draw_line(scene, labels, SHADER_EDITOR_TITLE, l.title, p.text);
    draw_welcome_button(scene, theme, labels, SHADER_EDITOR_DONE, l.done, false);
    fill_rect_rounded(scene, l.text, m.corner_radius, p.window.with_alpha(0xff));

    let text = code.entry.text();
    let x_of = |column: usize| l.text.x + column as f32 * code.advance;
    let last = (code.scroll + code.lines).min(code.rows.len());
    // The line a problem points at, washed in the alarm colour.
    let bad_row = code
        .problem
        .as_ref()
        .and_then(|pr| pr.line)
        .and_then(|line| {
            let start = text
                .split_inclusive('\n')
                .take(line - 1)
                .map(str::len)
                .sum::<usize>();
            code.rows.iter().position(|r| r.contains(start))
        });
    let selection = code.entry.selection();
    for row in code.scroll..last {
        let r = code.rows[row];
        let top = l.text.y + (row - code.scroll) as f32 * code.line_height;
        let band = Rect::new(l.text.x, top, l.text.width, code.line_height);
        if bad_row == Some(row) {
            fill_rect(scene, band, p.meter_peak.with_alpha(0x33));
        }
        if let Some((from, to)) = selection {
            let (from, to) = (from.max(r.from), to.min(r.to));
            if from < to {
                let start = text[r.from..from].chars().count();
                let width = text[from..to].chars().count();
                let wash = Rect::new(
                    x_of(start),
                    top,
                    width as f32 * code.advance,
                    code.line_height,
                )
                .intersection(&l.text);
                fill_rect(scene, wash, p.accent.with_alpha(0x44));
            }
        }
        let line = &text[r.from.min(text.len())..r.to.min(text.len())];
        if let Some(shaped) = labels.get_mono(line, code.text_px) {
            draw_text_clipped(
                scene,
                shaped,
                l.text,
                l.text.x,
                top + (code.line_height - code.text_px) / 2.0,
                p.text,
            );
        }
    }
    if caret_on && selection.is_none() {
        let caret = code.entry.caret();
        let row = crate::canvas::notepad_row_of(&code.rows, caret);
        if (code.scroll..last).contains(&row) {
            let column = crate::canvas::notepad_column_of(text, &code.rows, caret);
            let top = l.text.y + (row - code.scroll) as f32 * code.line_height;
            fill_rect(
                scene,
                Rect::new(x_of(column), top + 2.0, 1.5, code.line_height - 4.0),
                p.accent,
            );
        }
    }
    let ink = if code.problem.is_some() {
        p.meter_peak
    } else {
        p.text_muted
    };
    draw_line(scene, labels, &code.problem_words(), l.problem, ink);
}
