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
use crate::canvas::{RESEARCH_BADGE, RollView, key_row, tick_to_x};
use crate::document::{VocalRenderState, VocalView};
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
}

/// The corner chip: where it goes for a caption `width` wide.
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
        draw_text(
            scene,
            text,
            chip.x + 8.0,
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
