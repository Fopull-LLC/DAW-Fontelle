//! The piano roll over a vocal track: who sings in the corner, each phrase's
//! render state behind the notes, the words in the notes and what they were
//! sung as beneath them, the pitch the singer sang drawn faintly over it
//! all, and the lyric field while one is being typed.
//!
//! Every string drawn here is shaped in `WindowApp::shape_labels` — at
//! [`LYRIC_STYLE`] and [`PHONEME_STYLE`] — because a string nobody shaped
//! draws as nothing.

use vello::Scene;
use vello::kurbo::{Affine, BezPath, Point, Stroke};
use vello::peniko::Fill;

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, Tick};

use super::{
    RenameMarks, draw_rename_marks, draw_text, draw_text_clipped, fill_rect, fill_rect_rounded,
    stroke_rect_rounded,
};
use crate::canvas::{
    CurveScale, NoteHandle, PhonemeCell, RESEARCH_BADGE, RollView, VocalCurve, VocalStrips,
    VocalTab, VoicebankPageLayout, VoicebankTab, curve_value_y, key_row, note_handles, tick_to_x,
    vibrato_wave,
};
use crate::document::{CatalogueRow, VocalRenderState, VocalView, VoicebankRow};
use crate::layout::Rect;
use crate::text::{Labels, TextStyle};
use crate::theme::{Color, Theme};

/// The words inside the notes.
pub const LYRIC_STYLE: TextStyle = TextStyle::from_tenths(110, 500);
/// The phonemes under them, and the corner's small print.
pub const PHONEME_STYLE: TextStyle = TextStyle::from_tenths(90, 400);
/// The research-only badge.
pub const RESEARCH_TEXT: &str = "RESEARCH ONLY \u{2014} DON'T SHARE";

/// What the roll needs to draw a vocal track's layer.
pub struct RollVocal<'a> {
    pub view: &'a VocalView,
    /// The lyric field, while open: the notes it is for (the first is where
    /// it is drawn), what is typed, and its caret.
    pub editing: Option<(&'a [NoteId], &'a str)>,
    pub marks: Option<RenameMarks>,
    /// Where the corner's singer chip is, as `shape_labels` measured it — a
    /// click there opens the singer picker.
    pub caption: Option<Rect>,
    /// The strips the vocal roll took: the phoneme row, the lane's tabs.
    pub strips: VocalStrips,
    pub tabs: &'a [VocalTab],
    /// The lane the tabs have chosen, `None` for the notes' own.
    pub curve: Option<&'a VocalCurve>,
    /// That lane's scale, and what it holds (a stroke in hand, or the part's).
    pub lane: Option<(CurveScale, &'a fontelle_model::LaneCurve)>,
    pub cells: &'a [PhonemeCell],
    /// The phoneme field, while open: its note, what is typed, suggestions.
    pub phoneme_field: Option<(NoteId, &'a str, &'a [String])>,
    /// Pixels a second around the clip, for glides and vibrato.
    pub px_per_second: f32,
    /// "Open <engine>", when the engine has a window of its own.
    pub engine_chip: Option<Rect>,
    /// "Note ▾" at the end of the tabs: the note menu.
    pub note_button: Option<Rect>,
    /// The singer's portrait, drawn in the corner chip before the name.
    pub portrait: Option<&'a vello::peniko::ImageData>,
}

/// The engine chip's words.
pub fn engine_chip_text(engine: &str, open: bool) -> String {
    if open {
        format!("{engine} \u{2014} open")
    } else {
        format!("Open {engine}")
    }
}

/// The strings the editing layer draws (tabs, phonemes, the lane's scale,
/// the engine chip, suggestions), for `shape_labels` at [`PHONEME_STYLE`].
pub fn edit_strings(vocal: &RollVocal) -> Vec<String> {
    let mut out: Vec<String> = vocal.tabs.iter().map(|t| t.label.clone()).collect();
    out.extend(vocal.tabs.iter().filter_map(|t| t.why.clone()));
    out.extend(vocal.cells.iter().map(|c| c.symbol.clone()));
    if let Some((scale, _)) = &vocal.lane {
        out.extend(scale_labels(scale));
        out.push(scale.name.clone());
    }
    if let Some(engine) = &vocal.view.editor {
        out.push(engine_chip_text(engine, vocal.view.editor_open));
    }
    out.push("Note \u{25be}".to_string());
    if let Some((_, typed, suggestions)) = vocal.phoneme_field {
        out.push(typed.to_string());
        out.extend(suggestions.iter().cloned());
    }
    out
}

fn number(v: f32) -> String {
    if v.fract().abs() < 1e-3 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

fn scale_labels(scale: &CurveScale) -> [String; 2] {
    let unit = if scale.unit.is_empty() {
        String::new()
    } else {
        format!(" {}", scale.unit)
    };
    [
        format!("{}{unit}", number(scale.max)),
        format!("{}{unit}", number(scale.min)),
    ]
}

/// How much wider the corner chip is with a portrait in it.
pub const CAPTION_PORTRAIT: f32 = 20.0;

/// The corner chip: where it goes for a caption `width` wide (add
/// [`CAPTION_PORTRAIT`] when the singer has a portrait).
pub fn caption_rect(grid: Rect, width: f32) -> Rect {
    let w = width + 16.0;
    Rect::new(grid.right() - w - 8.0, grid.y + 6.0, w, 20.0)
}

/// The lines the corner says under the chip: the research badge, why it is
/// silent, how many words were guessed.
pub fn corner_lines(view: &VocalView) -> Vec<String> {
    let mut lines = Vec::new();
    if view.research_only {
        lines.push(RESEARCH_TEXT.to_string());
    }
    if let Some(message) = &view.message {
        lines.push(message.clone());
    }
    if !view.guessed.is_empty() {
        lines.push(guessed_line(view.guessed.len()));
    }
    lines
}

pub fn guessed_line(count: usize) -> String {
    match count {
        1 => "1 word's pronunciation was guessed \u{2014} write it as [phonemes] to be sure"
            .to_string(),
        n => format!(
            "{n} words' pronunciations were guessed \u{2014} write them as [phonemes] to be sure"
        ),
    }
}

fn span_x(v: &RollView, grid: Rect, start: Tick, end: Tick) -> Option<(f32, f32)> {
    let x0 = tick_to_x(v, grid, start).max(grid.x);
    let x1 = tick_to_x(v, grid, end).min(grid.right());
    (x1 > x0).then_some((x0, x1))
}

/// Behind the notes: each phrase's state. One still being sung is hatched
/// across the grid (it is silence until it lands); a quick take playing
/// while the final one is sung gets a thin strip along the top; a failed
/// one a strip in the warning colour.
pub fn draw_phrase_bands(
    scene: &mut Scene,
    theme: &Theme,
    v: &RollView,
    grid: Rect,
    vocal: &RollVocal,
) {
    let p = &theme.palette;
    for phrase in &vocal.view.phrases {
        let Some((x0, x1)) = span_x(v, grid, phrase.start, phrase.end) else {
            continue;
        };
        match phrase.state {
            VocalRenderState::Waiting => {
                fill_rect(
                    scene,
                    Rect::new(x0, grid.y, x1 - x0, grid.height),
                    p.accent.with_alpha(14),
                );
                let mut hatch = BezPath::new();
                let step = 10.0;
                let mut x = x0 - grid.height;
                while x < x1 {
                    // A diagonal from the bottom-left up to the right,
                    // clipped to the band by hand (cheaper than a layer).
                    let (mut ax, mut ay) = (x, grid.bottom());
                    let (mut bx, mut by) = (x + grid.height, grid.y);
                    if ax < x0 {
                        ay -= x0 - ax;
                        ax = x0;
                    }
                    if bx > x1 {
                        by += bx - x1;
                        bx = x1;
                    }
                    if ax < bx {
                        hatch.move_to(Point::new(ax as f64, ay as f64));
                        hatch.line_to(Point::new(bx as f64, by as f64));
                    }
                    x += step;
                }
                scene.stroke(
                    &Stroke::new(1.0),
                    Affine::IDENTITY,
                    p.accent.with_alpha(34).to_peniko(),
                    None,
                    &hatch,
                );
            }
            VocalRenderState::Preview => fill_rect(
                scene,
                Rect::new(x0, grid.y, x1 - x0, 3.0),
                p.accent.with_alpha(110),
            ),
            VocalRenderState::Final => fill_rect(
                scene,
                Rect::new(x0, grid.y, x1 - x0, 2.0),
                p.accent.with_alpha(45),
            ),
            VocalRenderState::Failed => fill_rect(
                scene,
                Rect::new(x0, grid.y, x1 - x0, 3.0),
                p.meter_peak.with_alpha(200),
            ),
        }
    }
}

/// Over the notes: their words, what they were sung as, the sung pitch,
/// the lyric field and the corner.
#[allow(clippy::too_many_arguments)]
pub fn draw_vocal_overlay(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    v: &RollView,
    grid: Rect,
    notes: &Arena<NoteId, Note>,
    selection: &[NoteId],
    vocal: &RollVocal,
) {
    let p = &theme.palette;
    let editing_first = vocal.editing.and_then(|(ids, _)| ids.first().copied());

    // The pitch the singer sang: faint, so the notes stay what you read.
    for line in &vocal.view.pitch {
        let mut path = BezPath::new();
        let mut started = false;
        for (tick, key) in line {
            let x = tick_to_x(v, grid, *tick);
            let base = key.floor().clamp(0.0, 127.0) as u8;
            let row = key_row(v, grid, base);
            let y = row.y + row.height / 2.0 - (key - f32::from(base)) * row.height;
            let point = Point::new(x as f64, y as f64);
            if started {
                path.line_to(point);
            } else {
                path.move_to(point);
                started = true;
            }
        }
        if started {
            scene.push_layer(
                Fill::NonZero,
                vello::peniko::BlendMode::default(),
                1.0,
                Affine::IDENTITY,
                &vello::kurbo::Rect::new(
                    grid.x as f64,
                    grid.y as f64,
                    grid.right() as f64,
                    grid.bottom() as f64,
                ),
            );
            scene.stroke(
                &Stroke::new(1.5),
                Affine::IDENTITY,
                p.text.with_alpha(120).to_peniko(),
                None,
                &path,
            );
            scene.pop_layer();
        }
    }

    // Each note's word inside it, and its phonemes small beneath.
    for (id, note) in notes.iter() {
        if Some(id) == editing_first {
            continue;
        }
        let Some((x0, x1)) = span_x(v, grid, note.start, note.start + note.length) else {
            continue;
        };
        let row = key_row(v, grid, note.key);
        if row.bottom() < grid.y || row.y > grid.bottom() {
            continue;
        }
        let block = Rect::new(x0, row.y, x1 - x0, row.height).intersection(&grid);
        if block.width < 10.0 || block.is_empty() {
            continue;
        }
        let lyric = if note.lyric.is_empty() {
            vocal.view.default_lyric.as_str()
        } else {
            note.lyric.as_str()
        };
        let selected = selection.contains(&id);
        let ink = if selected { p.window } else { p.text };
        if let Some(text) = labels.get_styled(lyric, LYRIC_STYLE) {
            let y = row.y + ((row.height - text.height) / 2.0).max(0.0);
            draw_text_clipped(scene, text, block.inset(1.0), x0 + 4.0, y, ink);
            if vocal.view.guessed.contains(&id) {
                // A dotted underline in the warning colour: this word was
                // guessed at, and brackets would make sure of it.
                let w = text.width.min(block.width - 6.0);
                let under = (row.bottom() - 2.5).min(grid.bottom());
                let mut x = x0 + 4.0;
                while x < x0 + 4.0 + w {
                    fill_rect(scene, Rect::new(x, under, 2.0, 1.5), RESEARCH_BADGE_COLOR);
                    x += 4.0;
                }
            }
        }
        if let Some((_, phonemes)) = vocal.view.phonemes.iter().find(|(n, _)| *n == id)
            && let Some(text) = labels.get_styled(phonemes, PHONEME_STYLE)
        {
            let room = Rect::new(
                x0,
                row.bottom() + 1.0,
                (x1 - x0).max(text.width + 4.0),
                text.height + 2.0,
            )
            .intersection(&grid);
            draw_text_clipped(
                scene,
                text,
                room,
                x0 + 2.0,
                row.bottom() + 1.0,
                p.text_muted,
            );
        }
    }

    // The lyric field, over the note it is for.
    if let (Some((ids, typed)), Some(first)) = (vocal.editing, editing_first)
        && let Some(note) = notes.get(first)
    {
        let row = key_row(v, grid, note.key);
        let x0 = tick_to_x(v, grid, note.start).max(grid.x);
        let x1 = tick_to_x(v, grid, note.start + note.length);
        let text = labels.get_styled(typed, LYRIC_STYLE);
        let text_w = text.map_or(0.0, |t| t.width);
        let min = if ids.len() > 1 { 220.0 } else { 72.0 };
        let w = (text_w + 14.0).max(x1 - x0).max(min);
        let field = Rect::new(
            x0.min(grid.right() - w).max(grid.x),
            row.y - 3.0,
            w,
            row.height.max(16.0) + 6.0,
        );
        fill_rect_rounded(scene, field, 3.0, p.panel_header);
        stroke_rect_rounded(scene, field, 3.0, 1.5, p.accent);
        let text_x = field.x + 6.0;
        if let Some(text) = text {
            let y = field.y + ((field.height - text.height) / 2.0).max(0.0);
            draw_text_clipped(scene, text, field.inset(2.0), text_x, y, p.text);
        }
        draw_rename_marks(scene, theme, field, text_x, text_w, vocal.marks);
    }

    // The corner: who sings (a chip — click it to choose), then the
    // research badge, why it is silent, words guessed.
    if let Some(chip) = vocal.caption
        && let Some(text) = labels.get_styled(&vocal.view.caption, LYRIC_STYLE)
    {
        fill_rect_rounded(scene, chip, 4.0, p.panel_header.with_alpha(235));
        stroke_rect_rounded(scene, chip, 4.0, 1.0, p.border);
        let mut text_x = chip.x + 8.0;
        if let Some(image) = vocal.portrait {
            let face = Rect::new(
                chip.x + 2.0,
                chip.y + 2.0,
                chip.height - 4.0,
                chip.height - 4.0,
            );
            draw_portrait(scene, labels, Some(image), face, "");
            text_x = face.right() + 6.0;
        }
        draw_text(
            scene,
            text,
            text_x,
            chip.y + ((chip.height - text.height) / 2.0).max(0.0),
            p.text,
        );
        let mut y = chip.bottom() + 4.0;
        for line in corner_lines(vocal.view) {
            let Some(text) = labels.get_styled(&line, PHONEME_STYLE) else {
                continue;
            };
            let plate = Rect::new(
                chip.right() - text.width - 12.0,
                y,
                text.width + 12.0,
                text.height + 6.0,
            );
            let (fill, ink) = if line == RESEARCH_TEXT {
                (RESEARCH_BADGE_COLOR, Color::rgba(0x1a, 0x10, 0x05, 0xff))
            } else {
                (p.panel_header.with_alpha(235), p.text_muted)
            };
            fill_rect_rounded(scene, plate, 3.0, fill);
            draw_text(scene, text, plate.x + 6.0, plate.y + 3.0, ink);
            y = plate.bottom() + 3.0;
        }
    }
}

const RESEARCH_BADGE_COLOR: Color = Color::rgba(
    RESEARCH_BADGE[0],
    RESEARCH_BADGE[1],
    RESEARCH_BADGE[2],
    RESEARCH_BADGE[3],
);

/// The strings the layer draws, for `shape_labels`: each note's word (or
/// the language's syllable), the phonemes, the corner, the field.
pub fn strings(
    view: &VocalView,
    notes: &Arena<NoteId, Note>,
    typed: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    let mut lyric_style = vec![view.caption.clone(), view.default_lyric.clone()];
    for note in notes.values() {
        if !note.lyric.is_empty() {
            lyric_style.push(note.lyric.clone());
        }
    }
    if let Some(typed) = typed {
        lyric_style.push(typed.to_string());
    }
    let mut small = corner_lines(view);
    small.extend(view.phonemes.iter().map(|(_, p)| p.clone()));
    (lyric_style, small)
}

/// On an arrangement block: the stretches of a vocal clip still being sung,
/// dimmed and hatched — silence there until they land. `spans` are in the
/// clip's own ticks; the block is the whole clip.
pub fn draw_clip_rendering(
    scene: &mut Scene,
    theme: &Theme,
    grid: Rect,
    block: Rect,
    clip_length: Tick,
    spans: &[(Tick, Tick)],
) {
    if clip_length <= 0 {
        return;
    }
    let p = &theme.palette;
    let scale = block.width / clip_length as f32;
    for (from, to) in spans {
        let x0 = (block.x + *from as f32 * scale).max(block.x);
        let x1 = (block.x + *to as f32 * scale).min(block.right());
        let band = Rect::new(x0, block.y, x1 - x0, block.height).intersection(&grid);
        if band.is_empty() {
            continue;
        }
        fill_rect(scene, band, p.window.with_alpha(90));
        let mut hatch = BezPath::new();
        let mut x = band.x - band.height;
        while x < band.right() {
            let (mut ax, mut ay) = (x, band.bottom());
            let (mut bx, mut by) = (x + band.height, band.y);
            if ax < band.x {
                ay -= band.x - ax;
                ax = band.x;
            }
            if bx > band.right() {
                by += bx - band.right();
                bx = band.right();
            }
            if ax < bx {
                hatch.move_to(Point::new(ax as f64, ay as f64));
                hatch.line_to(Point::new(bx as f64, by as f64));
            }
            x += 6.0;
        }
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            p.text.with_alpha(60).to_peniko(),
            None,
            &hatch,
        );
    }
}

// ------------------------------------------------------------- editing

fn polyline(points: &[(f32, f32)]) -> BezPath {
    let mut path = BezPath::new();
    for (i, (x, y)) in points.iter().enumerate() {
        let p = Point::new(f64::from(*x), f64::from(*y));
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path
}

fn clipped(scene: &mut Scene, rect: Rect, draw: impl FnOnce(&mut Scene)) {
    scene.push_layer(
        Fill::NonZero,
        vello::peniko::BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &vello::kurbo::Rect::new(
            f64::from(rect.x),
            f64::from(rect.y),
            f64::from(rect.right()),
            f64::from(rect.bottom()),
        ),
    );
    draw(scene);
    scene.pop_layer();
}

/// The tabs across the lane, the chosen one lit, the ignored ones greyed.
pub fn draw_vocal_tabs(scene: &mut Scene, theme: &Theme, labels: &Labels, vocal: &RollVocal) {
    let strip = vocal.strips.tabs;
    if strip.is_empty() {
        return;
    }
    let p = &theme.palette;
    fill_rect(scene, strip, p.panel_header);
    fill_rect(
        scene,
        Rect::new(strip.x, strip.bottom() - 1.0, strip.width, 1.0),
        p.border,
    );
    for tab in vocal.tabs {
        let on = tab.curve.as_ref() == vocal.curve;
        let (fill, ink) = match (on, tab.honoured) {
            (true, _) => (p.accent, p.window),
            (false, true) => (p.panel, p.text),
            (false, false) => (p.panel.with_alpha(110), p.text_muted.with_alpha(150)),
        };
        fill_rect_rounded(scene, tab.rect, 3.0, fill);
        if let Some(text) = labels.get_styled(&tab.label, PHONEME_STYLE) {
            draw_text_clipped(
                scene,
                text,
                tab.rect,
                tab.rect.x + 7.0,
                tab.rect.y + ((tab.rect.height - text.height) / 2.0).max(0.0),
                ink,
            );
        }
        if !tab.honoured {
            // A slash through it: drawn and kept, not sung by this voice.
            fill_rect(
                scene,
                Rect::new(
                    tab.rect.x + 3.0,
                    tab.rect.y + tab.rect.height / 2.0,
                    tab.rect.width - 6.0,
                    1.0,
                ),
                p.text_muted.with_alpha(140),
            );
        }
    }
}

/// The chosen lane: its neutral line, the curve filled down to it, its
/// range at the side, and greyed with why when the voice ignores it.
#[allow(clippy::too_many_arguments)]
pub fn draw_vocal_lane(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    v: &RollView,
    grid: Rect,
    plot: Rect,
    keys: Rect,
    vocal: &RollVocal,
) {
    let Some((scale, curve)) = &vocal.lane else {
        return;
    };
    if plot.is_empty() {
        return;
    }
    let p = &theme.palette;
    fill_rect(scene, keys, p.panel_header);
    fill_rect(scene, plot, p.row_accidental);
    let neutral_y = curve_value_y(scale, plot, scale.neutral);
    fill_rect(
        scene,
        Rect::new(plot.x, neutral_y, plot.width, 1.0),
        p.grid_line_strong,
    );
    let [top, bottom] = scale_labels(scale);
    for (text, y) in [(top, plot.y + 2.0), (bottom, plot.bottom() - 13.0)] {
        if let Some(t) = labels.get_styled(&text, PHONEME_STYLE) {
            draw_text_clipped(scene, t, keys, keys.x + 4.0, y, p.text_muted);
        }
    }
    if let Some(t) = labels.get_styled(&scale.name, PHONEME_STYLE) {
        draw_text_clipped(
            scene,
            t,
            keys,
            keys.x + 4.0,
            (neutral_y - t.height / 2.0).clamp(plot.y + 14.0, plot.bottom() - 26.0),
            p.text,
        );
    }
    // The curve, every two pixels, between the plot's ends.
    let mut points = Vec::new();
    let mut x = plot.x;
    while x <= plot.right() {
        let tick = crate::canvas::x_to_tick(v, grid, x);
        points.push((x, curve_value_y(scale, plot, curve.at(tick, scale.neutral))));
        x += 2.0;
    }
    let honoured = vocal
        .tabs
        .iter()
        .find(|t| t.curve.as_ref() == vocal.curve)
        .is_none_or(|t| t.honoured);
    let ink = if honoured { p.accent } else { p.text_muted };
    clipped(scene, plot, |scene| {
        let mut area = polyline(&points);
        if let (Some(first), Some(last)) = (points.first(), points.last()) {
            area.line_to(Point::new(f64::from(last.0), f64::from(neutral_y)));
            area.line_to(Point::new(f64::from(first.0), f64::from(neutral_y)));
            area.close_path();
        }
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            ink.with_alpha(50).to_peniko(),
            None,
            &area,
        );
        scene.stroke(
            &Stroke::new(1.5),
            Affine::IDENTITY,
            ink.to_peniko(),
            None,
            &polyline(&points),
        );
    });
    if !honoured
        && let Some(why) = vocal
            .tabs
            .iter()
            .find(|t| t.curve.as_ref() == vocal.curve)
            .and_then(|t| t.why.as_deref())
        && let Some(t) = labels.get_styled(why, PHONEME_STYLE)
    {
        let plate = Rect::new(plot.x + 8.0, plot.y + 6.0, t.width + 12.0, t.height + 6.0);
        fill_rect_rounded(scene, plate, 3.0, p.panel_header.with_alpha(230));
        draw_text(scene, t, plate.x + 6.0, plate.y + 3.0, p.text_muted);
    }
}

/// What each note was sung as, phoneme by phoneme, where each starts.
pub fn draw_phoneme_row(scene: &mut Scene, theme: &Theme, labels: &Labels, vocal: &RollVocal) {
    let row = vocal.strips.phonemes;
    if row.is_empty() {
        return;
    }
    let p = &theme.palette;
    fill_rect(scene, row, p.panel_header);
    fill_rect(scene, Rect::new(row.x, row.y, row.width, 1.0), p.border);
    let timing = vocal.view.capabilities.phoneme_timing;
    clipped(scene, row, |scene| {
        for cell in vocal.cells {
            let edge = Rect::new(cell.rect.x, row.y + 2.0, 1.5, row.height - 4.0);
            fill_rect(scene, edge, if timing { p.accent } else { p.border });
            if let Some(t) = labels.get_styled(&cell.symbol, PHONEME_STYLE) {
                draw_text_clipped(
                    scene,
                    t,
                    cell.rect,
                    cell.rect.x + 3.0,
                    row.y + ((row.height - t.height) / 2.0).max(0.0),
                    p.text,
                );
            }
        }
    });
    // The phoneme field, over its note's cells.
    if let Some((note, typed, suggestions)) = vocal.phoneme_field {
        let x = vocal
            .cells
            .iter()
            .find(|c| c.note == note)
            .map_or(row.x, |c| c.rect.x);
        let text = labels.get_styled(typed, PHONEME_STYLE);
        let w = text.map_or(0.0, |t| t.width).max(90.0) + 12.0;
        let field = Rect::new(
            x.min(row.right() - w).max(row.x),
            row.y - 4.0,
            w,
            row.height + 6.0,
        );
        fill_rect_rounded(scene, field, 3.0, p.panel_header);
        stroke_rect_rounded(scene, field, 3.0, 1.5, p.accent);
        if let Some(t) = text {
            draw_text(
                scene,
                t,
                field.x + 6.0,
                field.y + ((field.height - t.height) / 2.0).max(0.0),
                p.text,
            );
        }
        // Suggestions from the voice's own phonemes, above it.
        let mut sx = field.x;
        for s in suggestions {
            let Some(t) = labels.get_styled(s, PHONEME_STYLE) else {
                continue;
            };
            let chip = Rect::new(sx, field.y - t.height - 8.0, t.width + 10.0, t.height + 5.0);
            fill_rect_rounded(scene, chip, 3.0, p.panel);
            stroke_rect_rounded(scene, chip, 3.0, 1.0, p.border);
            draw_text(scene, t, chip.x + 5.0, chip.y + 2.5, p.text_muted);
            sx = chip.right() + 4.0;
        }
    }
}

/// The pitch the notes, glides, vibrato and bend make — the line a bend is
/// drawn against — over the notes.
pub fn draw_pitch_line(
    scene: &mut Scene,
    theme: &Theme,
    v: &RollView,
    grid: Rect,
    view: &VocalView,
) {
    let p = &theme.palette;
    for line in &view.pitch_line {
        let points: Vec<(f32, f32)> = line
            .iter()
            .map(|(tick, key)| {
                let base = key.floor().clamp(0.0, 127.0) as u8;
                let row = key_row(v, grid, base);
                (
                    tick_to_x(v, grid, *tick),
                    row.y + row.height / 2.0 - (key - f32::from(base)) * row.height,
                )
            })
            .collect();
        if points.len() < 2 {
            continue;
        }
        clipped(scene, grid, |scene| {
            scene.stroke(
                &Stroke::new(1.25),
                Affine::IDENTITY,
                p.accent.with_alpha(170).to_peniko(),
                None,
                &polyline(&points),
            );
        });
    }
}

/// Each note's vibrato as a wave, and the selected notes' handles.
pub fn draw_note_expression(
    scene: &mut Scene,
    theme: &Theme,
    v: &RollView,
    grid: Rect,
    notes: &Arena<NoteId, Note>,
    selection: &[NoteId],
    vocal: &RollVocal,
) {
    let p = &theme.palette;
    let caps = &vocal.view.capabilities;
    clipped(scene, grid, |scene| {
        for (id, note) in notes.iter() {
            let row = key_row(v, grid, note.key);
            let x0 = tick_to_x(v, grid, note.start);
            let x1 = tick_to_x(v, grid, note.start + note.length);
            if x1 < grid.x || x0 > grid.right() || row.bottom() < grid.y || row.y > grid.bottom() {
                continue;
            }
            let rect = Rect::new(x0, row.y, x1 - x0, row.height);
            let voice = note.voice.as_deref();
            if let Some(vib) = voice.and_then(|v| v.vibrato) {
                let wave = vibrato_wave(rect, &vib, vocal.px_per_second);
                if wave.len() > 1 {
                    scene.stroke(
                        &Stroke::new(1.25),
                        Affine::IDENTITY,
                        p.window.with_alpha(200).to_peniko(),
                        None,
                        &polyline(&wave),
                    );
                }
            }
            if !selection.contains(&id) {
                continue;
            }
            for (handle, r) in note_handles(rect, voice, vocal.px_per_second, caps.portamento) {
                let fill = match handle {
                    NoteHandle::Glide => p.text,
                    _ => p.accent,
                };
                if handle == NoteHandle::Glide {
                    // The glide's run, from its start to the note's head.
                    fill_rect(
                        scene,
                        Rect::new(
                            r.x + r.width / 2.0,
                            r.y + r.height / 2.0,
                            (x0 - r.x - r.width / 2.0).max(0.0),
                            1.0,
                        ),
                        p.text_muted,
                    );
                }
                fill_rect_rounded(scene, r, 2.0, fill);
                stroke_rect_rounded(scene, r, 2.0, 1.0, p.window);
            }
        }
    });
}

/// "Note ▾" at the end of the tabs.
pub fn draw_note_button(scene: &mut Scene, theme: &Theme, labels: &Labels, vocal: &RollVocal) {
    let Some(rect) = vocal.note_button else {
        return;
    };
    let p = &theme.palette;
    fill_rect_rounded(scene, rect, 3.0, p.panel);
    stroke_rect_rounded(scene, rect, 3.0, 1.0, p.border);
    if let Some(t) = labels.get_styled("Note \u{25be}", PHONEME_STYLE) {
        draw_text(
            scene,
            t,
            rect.x + 7.0,
            rect.y + ((rect.height - t.height) / 2.0).max(0.0),
            p.text,
        );
    }
}

/// The engine chip beside the singer chip.
pub fn draw_engine_chip(scene: &mut Scene, theme: &Theme, labels: &Labels, vocal: &RollVocal) {
    let (Some(chip), Some(engine)) = (vocal.engine_chip, vocal.view.editor.as_deref()) else {
        return;
    };
    let p = &theme.palette;
    let text = engine_chip_text(engine, vocal.view.editor_open);
    let fill = if vocal.view.editor_open {
        p.accent.with_alpha(90)
    } else {
        p.accent
    };
    fill_rect_rounded(scene, chip, 4.0, fill);
    if let Some(t) = labels.get_styled(&text, PHONEME_STYLE) {
        draw_text(
            scene,
            t,
            chip.x + 8.0,
            chip.y + ((chip.height - t.height) / 2.0).max(0.0),
            p.window,
        );
    }
}

/// Where the engine chip goes: left of the singer chip.
pub fn engine_chip_rect(caption: Rect, width: f32) -> Rect {
    let w = width + 16.0;
    Rect::new(caption.x - w - 6.0, caption.y, w, caption.height)
}

// ------------------------------------------------------------ the page

/// What the Voicebanks page shows.
pub struct VoicebankPage<'a> {
    pub layout: &'a VoicebankPageLayout,
    pub tab: VoicebankTab,
    pub installed: &'a [VoicebankRow],
    pub catalogue: &'a [CatalogueRow],
    pub folder: &'a str,
}

/// An installed row's folder button and sample button.
pub const PAGE_FOLDER: &str = "Folder";
pub const PAGE_SAMPLE: &str = "\u{25b6} Sample";

/// The page's strings, for `shape_labels` at [`PHONEME_STYLE`] (and the
/// title at [`LYRIC_STYLE`]).
pub fn page_strings(page: &VoicebankPage) -> Vec<String> {
    let mut out: Vec<String> = [
        "Voicebanks",
        "Installed",
        "Find singers",
        "Install from file\u{2026}",
        "Open folder",
        "\u{2715}",
        "Uninstall",
        "Install",
        "Get it",
        "Waiting\u{2026}",
        "Read terms\u{2026}",
        "Terms agreed",
        "No terms",
        "Drop a voicebank's .zip, .7z, .uar, .oudep or folder anywhere on the window to install it.",
        "No catalogue entries.",
        PAGE_FOLDER,
        PAGE_SAMPLE,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    out.push(page.folder.to_string());
    for r in page.installed {
        out.push(r.name.clone());
        out.push(installed_line(r));
        out.push(initial(&r.name));
    }
    for r in page.catalogue {
        out.push(r.name.clone());
        out.push(catalogue_line(r));
        out.push(r.terms_summary.clone());
        out.push(initial(&r.name));
    }
    out
}

fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}

fn installed_line(r: &VoicebankRow) -> String {
    let mut line = r.kind.clone();
    if !r.languages.is_empty() {
        let langs: Vec<String> = r
            .languages
            .iter()
            .map(|l| crate::canvas::language_name(l))
            .collect();
        line.push_str(&format!(" \u{b7} {}", langs.join(", ")));
    }
    if let Some(a) = &r.author {
        line.push_str(&format!(" \u{b7} by {a}"));
    }
    line
}

fn catalogue_line(r: &CatalogueRow) -> String {
    let langs: Vec<String> = r
        .languages
        .iter()
        .map(|l| crate::canvas::language_name(l))
        .collect();
    format!(
        "{} \u{b7} {} \u{b7} commercial use: {} \u{b7} credit: {} \u{b7} AI training: {}",
        r.kind,
        langs.join(", "),
        r.commercial,
        r.credit,
        r.ai_training
    )
}

fn button(scene: &mut Scene, theme: &Theme, labels: &Labels, rect: Rect, text: &str, strong: bool) {
    let p = &theme.palette;
    fill_rect_rounded(
        scene,
        rect,
        4.0,
        if strong { p.accent } else { p.panel_header },
    );
    stroke_rect_rounded(scene, rect, 4.0, 1.0, p.border);
    if let Some(t) = labels.get_styled(text, PHONEME_STYLE) {
        draw_text_clipped(
            scene,
            t,
            rect,
            rect.x + ((rect.width - t.width) / 2.0).max(4.0),
            rect.y + ((rect.height - t.height) / 2.0).max(0.0),
            if strong { p.window } else { p.text },
        );
    }
}

/// A colour from a name, for a bank with no portrait.
fn avatar(name: &str) -> Color {
    let h = name
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    Color::rgba(
        0x40 + (h & 0x7f) as u8,
        0x40 + ((h >> 8) & 0x7f) as u8,
        0x60 + ((h >> 16) & 0x7f) as u8,
        0xff,
    )
}

pub fn draw_voicebank_page(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    portraits: &crate::portrait::Portraits,
    page: &VoicebankPage,
) {
    let p = &theme.palette;
    let l = page.layout;
    fill_rect_rounded(scene, l.frame, 8.0, p.panel);
    stroke_rect_rounded(scene, l.frame, 8.0, 1.0, p.border);
    if let Some(t) = labels.get_styled("Voicebanks", LYRIC_STYLE) {
        draw_text(scene, t, l.title.x, l.title.y + 2.0, p.text);
    }
    button(scene, theme, labels, l.close, "\u{2715}", false);
    for (tab, rect) in &l.tabs {
        let name = match tab {
            VoicebankTab::Installed => "Installed",
            VoicebankTab::Find => "Find singers",
        };
        button(scene, theme, labels, *rect, name, *tab == page.tab);
    }
    button(
        scene,
        theme,
        labels,
        l.install,
        "Install from file\u{2026}",
        false,
    );
    button(scene, theme, labels, l.folder, "Open folder", false);
    let count = match page.tab {
        VoicebankTab::Installed => page.installed.len(),
        VoicebankTab::Find => page.catalogue.len(),
    };
    if count == 0 {
        let hint = match page.tab {
            VoicebankTab::Installed => {
                "Drop a voicebank's .zip, .7z, .uar, .oudep or folder anywhere on the window to install it."
            }
            VoicebankTab::Find => "No catalogue entries.",
        };
        if let Some(t) = labels.get_styled(hint, PHONEME_STYLE) {
            draw_text(scene, t, l.list.x + 4.0, l.list.y + 8.0, p.text_muted);
        }
        return;
    }
    clipped(scene, l.list, |scene| {
        for row in &l.rows {
            fill_rect_rounded(scene, row.frame, 5.0, p.panel_header);
            let (name, line, summary, terms, action, waiting, portrait, sample) = match page.tab {
                VoicebankTab::Installed => {
                    let Some(r) = page.installed.get(row.index) else {
                        continue;
                    };
                    let terms = match (&r.terms, r.terms_agreed) {
                        (None, _) => "No terms",
                        (Some(_), true) => "Terms agreed",
                        (Some(_), false) => "Read terms\u{2026}",
                    };
                    (
                        r.name.clone(),
                        installed_line(r),
                        None,
                        terms,
                        "Uninstall",
                        false,
                        r.portrait.as_deref(),
                        r.sample.is_some(),
                    )
                }
                VoicebankTab::Find => {
                    let Some(r) = page.catalogue.get(row.index) else {
                        continue;
                    };
                    let action = if r.waiting {
                        "Waiting\u{2026}"
                    } else if r.direct {
                        "Install"
                    } else {
                        "Get it"
                    };
                    (
                        r.name.clone(),
                        catalogue_line(r),
                        Some(r.terms_summary.clone()),
                        "Read terms\u{2026}",
                        action,
                        r.waiting,
                        None,
                        false,
                    )
                }
            };
            let image = portrait.and_then(|path| portraits.peek(path));
            draw_portrait(scene, labels, image, row.portrait, &name);
            if let Some(t) = labels.get_styled(&name, PHONEME_STYLE) {
                draw_text_clipped(scene, t, row.text, row.text.x, row.text.y + 2.0, p.text);
            }
            if let Some(t) = labels.get_styled(&line, PHONEME_STYLE) {
                draw_text_clipped(
                    scene,
                    t,
                    row.text,
                    row.text.x,
                    row.text.y + 20.0,
                    p.text_muted,
                );
            }
            if let Some(summary) = summary
                && let Some(t) = labels.get_styled(&summary, PHONEME_STYLE)
            {
                draw_text_clipped(
                    scene,
                    t,
                    row.text,
                    row.text.x,
                    row.text.y + 38.0,
                    p.text_muted,
                );
            }
            button(scene, theme, labels, row.terms, terms, false);
            if !row.reveal.is_empty() {
                button(scene, theme, labels, row.reveal, PAGE_FOLDER, false);
            }
            if sample && !row.sample.is_empty() {
                button(scene, theme, labels, row.sample, PAGE_SAMPLE, false);
            }
            // Uninstalling is not the thing to make loud.
            let strong = !waiting && page.tab == VoicebankTab::Find;
            button(scene, theme, labels, row.action, action, strong);
        }
    });
}

// ------------------------------------------------------------ portraits

/// A portrait filling `rect` (cropped to its shape, rounded), or, with none,
/// a colour from the name and its initial.
pub fn draw_portrait(
    scene: &mut Scene,
    labels: &Labels,
    image: Option<&vello::peniko::ImageData>,
    rect: Rect,
    name: &str,
) {
    if rect.is_empty() {
        return;
    }
    let radius = (rect.width.min(rect.height) * 0.12).clamp(2.0, 8.0);
    match image.filter(|i| i.width > 0 && i.height > 0) {
        Some(image) => {
            // Cover: the shorter side fills, the longer is cropped, centred.
            let scale = (rect.width / image.width as f32).max(rect.height / image.height as f32);
            let w = image.width as f32 * scale;
            let h = image.height as f32 * scale;
            let x = rect.x + (rect.width - w) / 2.0;
            let y = rect.y + (rect.height - h) / 2.0;
            let brush = vello::peniko::ImageBrush {
                image: image.clone(),
                sampler: vello::peniko::ImageSampler::new()
                    .with_quality(vello::peniko::ImageQuality::Medium),
            };
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                &brush,
                Some(
                    Affine::translate((f64::from(x), f64::from(y)))
                        * Affine::scale(f64::from(scale)),
                ),
                &super::rounded(rect, radius),
            );
        }
        None => {
            fill_rect_rounded(scene, rect, radius, avatar(name));
            if let Some(t) = labels.get_styled(&initial(name), PHONEME_STYLE) {
                draw_text(
                    scene,
                    t,
                    rect.x + (rect.width - t.width) / 2.0,
                    rect.y + (rect.height - t.height) / 2.0,
                    Color::rgba(0xff, 0xff, 0xff, 0xff),
                );
            }
        }
    }
}

// --------------------------------------------------------- the picker

/// What the singer picker shows.
pub struct SingerPickerView<'a> {
    pub layout: &'a crate::canvas::SingerPickerLayout,
    pub singers: &'a [crate::document::SingerChoice],
    /// What is typed in the search.
    pub query: &'a str,
    /// The focused card (an index into the layout's cards) and which of its
    /// language chips Tab has reached.
    pub focus: Option<usize>,
    pub language: usize,
    /// Who sings the track now.
    pub current: Option<&'a fontelle_model::VocalVoice>,
    pub problems: &'a [String],
}

/// The Voicebanks page and the singer picker, over the whole window
/// (either can be opened with no vocal track in the roll).
pub struct VocalOverlay<'a> {
    pub page: Option<VoicebankPage<'a>>,
    pub picker: Option<SingerPickerView<'a>>,
    pub portraits: &'a crate::portrait::Portraits,
}

pub const PICKER_TITLE: &str = "Choose a singer";
pub const PICKER_SEARCH_HINT: &str = "Search \u{2014} name, language, UTAU, DiffSinger, engine";
pub const PICKER_BANKS: &str = "Fontelle Singer \u{2014} free voicebanks";
pub const PICKER_ENGINES: &str = "Singer engines";
pub const PICKER_NONE: &str = "No singer matches the search.";
pub const PICKER_EMPTY: &str =
    "No singer is installed yet \u{2014} Voicebanks & Find singers has free ones.";

fn kind_badge(s: &crate::document::SingerChoice) -> &'static str {
    match s.kind {
        crate::document::SingerKind::Utau => "UTAU",
        crate::document::SingerKind::DiffSinger => "DiffSinger",
        crate::document::SingerKind::Engine => "Engine",
    }
}

fn terms_badge(s: &crate::document::SingerChoice) -> Option<&'static str> {
    match (&s.terms, s.terms_agreed) {
        (Some(_), false) => Some("Terms to read"),
        (Some(_), true) => Some("Terms agreed"),
        (None, _) => None,
    }
}

const RESEARCH_SHORT: &str = "Research only";
const SINGING: &str = "Singing";

/// The picker's strings, for `shape_labels` at [`PHONEME_STYLE`] (the title
/// at [`LYRIC_STYLE`]).
pub fn picker_strings(p: &SingerPickerView) -> Vec<String> {
    let mut out: Vec<String> = [
        PICKER_SEARCH_HINT,
        PICKER_BANKS,
        PICKER_ENGINES,
        PICKER_NONE,
        PICKER_EMPTY,
        "No singer",
        "Install a voicebank\u{2026}",
        "Voicebanks & Find singers\u{2026}",
        "\u{2715}",
        RESEARCH_SHORT,
        SINGING,
        "Terms to read",
        "Terms agreed",
        "UTAU",
        "DiffSinger",
        "Engine",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    out.push(p.query.to_string());
    for card in &p.layout.cards {
        let Some(s) = p.singers.get(card.singer) else {
            continue;
        };
        out.push(s.name.clone());
        out.push(initial(&s.name));
        if !s.kind.is_voicebank() {
            out.push(s.engine_name.clone());
        }
        for (tag, _) in &card.languages {
            out.push(tag.to_uppercase());
        }
    }
    for problem in p.problems {
        out.push(format!("Not loaded: {problem}"));
    }
    out
}

fn chip(scene: &mut Scene, labels: &Labels, rect: Rect, text: &str, fill: Color, ink: Color) {
    fill_rect_rounded(scene, rect, 3.0, fill);
    if let Some(t) = labels.get_styled(text, PHONEME_STYLE) {
        draw_text_clipped(
            scene,
            t,
            rect,
            rect.x + ((rect.width - t.width) / 2.0).max(2.0),
            rect.y + ((rect.height - t.height) / 2.0).max(0.0),
            ink,
        );
    }
}

pub fn draw_singer_picker(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    portraits: &crate::portrait::Portraits,
    picker: &SingerPickerView,
) {
    let p = &theme.palette;
    let l = picker.layout;
    fill_rect_rounded(scene, l.frame, 8.0, p.panel);
    stroke_rect_rounded(scene, l.frame, 8.0, 1.0, p.border);
    if let Some(t) = labels.get_styled(PICKER_TITLE, LYRIC_STYLE) {
        draw_text(scene, t, l.title.x, l.title.y + 2.0, p.text);
    }
    button(scene, theme, labels, l.close, "\u{2715}", false);
    // The search field.
    fill_rect_rounded(scene, l.search, 4.0, p.window);
    stroke_rect_rounded(scene, l.search, 4.0, 1.0, p.accent);
    let (shown, ink) = if picker.query.is_empty() {
        (PICKER_SEARCH_HINT, p.text_muted)
    } else {
        (picker.query, p.text)
    };
    if let Some(t) = labels.get_styled(shown, PHONEME_STYLE) {
        let y = l.search.y + ((l.search.height - t.height) / 2.0).max(0.0);
        draw_text_clipped(scene, t, l.search.inset(2.0), l.search.x + 8.0, y, ink);
        if !picker.query.is_empty() {
            let caret = Rect::new(
                l.search.x + 9.0 + t.width,
                l.search.y + 6.0,
                1.5,
                l.search.height - 12.0,
            );
            fill_rect(scene, caret, p.accent);
        }
    } else if picker.query.is_empty() {
        // Nothing shaped yet: still a field.
    }
    if l.cards.is_empty() {
        let hint = if picker.singers.is_empty() {
            PICKER_EMPTY
        } else {
            PICKER_NONE
        };
        if let Some(t) = labels.get_styled(hint, PHONEME_STYLE) {
            draw_text(scene, t, l.list.x + 4.0, l.list.y + 8.0, p.text_muted);
        }
    }
    clipped(scene, l.list, |scene| {
        for (group, rect) in &l.groups {
            let text = match group {
                crate::canvas::PickerGroup::Voicebanks => PICKER_BANKS,
                crate::canvas::PickerGroup::Engines => PICKER_ENGINES,
            };
            if let Some(t) = labels.get_styled(text, PHONEME_STYLE) {
                draw_text(scene, t, rect.x + 2.0, rect.y + 4.0, p.text_muted);
            }
            fill_rect(
                scene,
                Rect::new(rect.x, rect.bottom() - 3.0, rect.width, 1.0),
                p.border,
            );
        }
        for (k, card) in l.cards.iter().enumerate() {
            if card.frame.bottom() < l.list.y || card.frame.y > l.list.bottom() {
                continue;
            }
            let Some(s) = picker.singers.get(card.singer) else {
                continue;
            };
            let focused = picker.focus == Some(k);
            let singing = picker
                .current
                .is_some_and(|c| c.engine == s.engine && c.singer == s.id);
            fill_rect_rounded(scene, card.frame, 6.0, p.panel_header);
            stroke_rect_rounded(
                scene,
                card.frame,
                6.0,
                if focused { 2.0 } else { 1.0 },
                if focused || singing {
                    p.accent
                } else {
                    p.border
                },
            );
            let image = s.image.as_deref().and_then(|path| portraits.peek(path));
            draw_portrait(scene, labels, image, card.portrait, &s.name);
            let text_x = card.portrait.right() + 10.0;
            let text = Rect::new(
                text_x,
                card.frame.y + 8.0,
                (card.frame.right() - text_x - 8.0).max(0.0),
                card.portrait.height + 4.0,
            );
            if let Some(t) = labels.get_styled(&s.name, PHONEME_STYLE) {
                draw_text_clipped(scene, t, text, text.x, text.y + 2.0, p.text);
            }
            // The kind, as a badge; an engine's own name under it.
            let badge = kind_badge(s);
            let bw = labels
                .get_styled(badge, PHONEME_STYLE)
                .map_or(40.0, |t| t.width + 10.0);
            let badge_rect = Rect::new(text.x, text.y + 20.0, bw.min(text.width), 16.0);
            let kind_fill = match s.kind {
                crate::document::SingerKind::Utau => Color::rgba(0x3d, 0x7e, 0xc9, 0xff),
                crate::document::SingerKind::DiffSinger => Color::rgba(0x7a, 0x52, 0xc8, 0xff),
                crate::document::SingerKind::Engine => Color::rgba(0x4f, 0x8a, 0x5b, 0xff),
            };
            chip(
                scene,
                labels,
                badge_rect,
                badge,
                kind_fill,
                Color::rgba(0xff, 0xff, 0xff, 0xff),
            );
            let mut line_y = badge_rect.bottom() + 4.0;
            if !s.kind.is_voicebank()
                && let Some(t) = labels.get_styled(&s.engine_name, PHONEME_STYLE)
            {
                draw_text_clipped(scene, t, text, text.x, line_y, p.text_muted);
                line_y += 16.0;
            }
            if singing {
                if let Some(t) = labels.get_styled(SINGING, PHONEME_STYLE) {
                    draw_text_clipped(scene, t, text, text.x, line_y, p.accent);
                }
            } else if let Some(terms) = terms_badge(s)
                && let Some(t) = labels.get_styled(terms, PHONEME_STYLE)
            {
                let ink = if s.terms_agreed {
                    p.text_muted
                } else {
                    RESEARCH_BADGE_COLOR
                };
                draw_text_clipped(scene, t, text, text.x, line_y, ink);
            }
            // Languages, a chip each; research-only at the card's corner.
            for (j, (tag, rect)) in card.languages.iter().enumerate() {
                let lit = focused && picker.language == j;
                let current = singing && picker.current.is_some_and(|c| &c.language == tag);
                let (fill, ink) = if lit || current {
                    (p.accent, p.window)
                } else {
                    (p.window, p.text)
                };
                chip(scene, labels, *rect, &tag.to_uppercase(), fill, ink);
            }
            if s.research_only {
                let w = labels
                    .get_styled(RESEARCH_SHORT, PHONEME_STYLE)
                    .map_or(80.0, |t| t.width + 10.0);
                let r = Rect::new(
                    card.frame.right() - w - 8.0,
                    card.frame.bottom() - 28.0,
                    w,
                    18.0,
                );
                chip(
                    scene,
                    labels,
                    r,
                    RESEARCH_SHORT,
                    RESEARCH_BADGE_COLOR,
                    Color::rgba(0x1a, 0x10, 0x05, 0xff),
                );
            }
        }
    });
    for (rect, problem) in l.problems.iter().zip(picker.problems) {
        if let Some(t) = labels.get_styled(&format!("Not loaded: {problem}"), PHONEME_STYLE) {
            draw_text_clipped(scene, t, *rect, rect.x, rect.y + 2.0, p.text_muted);
        }
    }
    button(
        scene,
        theme,
        labels,
        l.nobody,
        "No singer",
        picker.current.is_none(),
    );
    button(
        scene,
        theme,
        labels,
        l.install,
        "Install a voicebank\u{2026}",
        false,
    );
    button(
        scene,
        theme,
        labels,
        l.manage,
        "Voicebanks & Find singers\u{2026}",
        false,
    );
}

/// The overlay: the page, then the picker over it.
pub fn draw_vocal_overlays(scene: &mut Scene, theme: &Theme, labels: &Labels, o: &VocalOverlay) {
    if let Some(page) = &o.page {
        draw_voicebank_page(scene, theme, labels, o.portraits, page);
    }
    if let Some(picker) = &o.picker {
        draw_singer_picker(scene, theme, labels, o.portraits, picker);
    }
}
