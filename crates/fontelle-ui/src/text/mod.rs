//! Shaping, layout and font fallback via `cosmic-text` (TDD §16.2), turned into
//! something `vello` can draw.
//!
//! The seam matters: shaping is a solved problem we do not want to own, and
//! *positioning* is ours and is arithmetic — so the shaper's output is
//! converted here into glyph runs, and that conversion is tested without a GPU
//! (`docs/first-usable-plan.md` §2.5).

use std::collections::HashMap;

use cosmic_text::{Attrs, Buffer, Family, FontSystem, Shaping, Wrap};

use crate::theme::FontTokens;

/// Glyphs that share a font and a size — exactly one `vello` draw call.
///
/// Grouping here rather than at the call site keeps a line of chrome text to a
/// single `draw_glyphs` when it is all one face, which it almost always is.
#[derive(Clone)]
pub struct GlyphRun {
    pub font: vello::peniko::FontData,
    pub font_size: f32,
    /// Positions are relative to the layout's top-left, with `y` on the
    /// baseline and increasing downwards — the same convention `vello` draws
    /// in, so the caller only has to translate.
    pub glyphs: Vec<vello::Glyph>,
}

/// A laid-out piece of text.
#[derive(Clone, Default)]
pub struct TextLayout {
    pub runs: Vec<GlyphRun>,
    /// The widest line.
    pub width: f32,
    /// Top of the first line to bottom of the last.
    pub height: f32,
}

impl TextLayout {
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub fn glyph_count(&self) -> usize {
        self.runs.iter().map(|r| r.glyphs.len()).sum()
    }
}

/// Owns the font database. Expensive to construct — it scans the system font
/// directories — so there is one per window, not one per string.
pub struct TextContext {
    pub font_system: FontSystem,
}

impl TextContext {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
        }
    }

    /// Shapes `text` in `font`, wrapping at `max_width` when one is given.
    pub fn layout(&mut self, text: &str, font: &FontTokens, max_width: Option<f32>) -> TextLayout {
        let metrics = cosmic_text::Metrics::new(font.size, font.size * font.line_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        // No width means no wrapping — otherwise `cosmic-text`'s default word
        // wrap folds a label at whatever width the buffer happens to have.
        buffer.set_wrap(
            &mut self.font_system,
            if max_width.is_some() {
                Wrap::Word
            } else {
                Wrap::None
            },
        );
        buffer.set_size(&mut self.font_system, max_width, None);
        buffer.set_text(
            &mut self.font_system,
            text,
            &Attrs::new().family(family(&font.family)),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut layout = TextLayout::default();
        // What the run in progress is made of, so grouping is an id comparison
        // rather than a font lookup per glyph.
        let mut current: Option<(cosmic_text::fontdb::ID, cosmic_text::fontdb::Weight, f32)> = None;

        for run in buffer.layout_runs() {
            layout.width = layout.width.max(run.line_w);
            layout.height = layout.height.max(run.line_top + run.line_height);

            for glyph in run.glyphs {
                // The same arithmetic `LayoutGlyph::physical` does, minus the
                // rounding: we hand vello subpixel positions and let it decide.
                let x = glyph.x + glyph.font_size * glyph.x_offset;
                let y = run.line_y + glyph.y - glyph.font_size * glyph.y_offset;

                // Extend the run in progress when the face and size match, so
                // one script at one size stays one draw call. A fallback glyph
                // (a CJK name inside a Latin label) starts a new run in place,
                // which keeps draw order — and therefore overlap — correct.
                if current != Some((glyph.font_id, glyph.font_weight, glyph.font_size)) {
                    let Some(font) = self.font_system.get_font(glyph.font_id, glyph.font_weight)
                    else {
                        // A shaped glyph whose face has gone is not worth
                        // failing a frame over; drop it and draw the rest.
                        continue;
                    };
                    layout.runs.push(GlyphRun {
                        font: font.as_peniko(),
                        font_size: glyph.font_size,
                        glyphs: Vec::new(),
                    });
                    current = Some((glyph.font_id, glyph.font_weight, glyph.font_size));
                }
                layout
                    .runs
                    .last_mut()
                    .expect("a run was just pushed or matched")
                    .glyphs
                    .push(vello::Glyph {
                        id: u32::from(glyph.glyph_id),
                        x,
                        y,
                    });
            }
        }

        if layout.runs.is_empty() {
            // Nothing was drawn, so it occupies nothing — a blank label must
            // not reserve a line's worth of height.
            layout.width = 0.0;
            layout.height = 0.0;
        }
        layout
    }
}

impl Default for TextContext {
    fn default() -> Self {
        Self::new()
    }
}

/// A theme's family name, resolved the way CSS does it: the generic names mean
/// "whatever this machine has", anything else is a face to look up.
fn family(name: &str) -> Family<'_> {
    match name {
        "sans-serif" => Family::SansSerif,
        "serif" => Family::Serif,
        "monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        other => Family::Name(other),
    }
}

/// Shaped strings, kept so a label is shaped once rather than once a frame.
///
/// The chrome is now full of text that comes from data rather than from the
/// theme — bar numbers, key names, channel names, soundfont names, preset
/// names — and [`draw_window`](crate::render::draw_window) is a pure function
/// that cannot shape anything. So the window shapes what it is about to draw
/// into this and hands it over by reference.
///
/// Bounded rather than unbounded: scrolling a big soundfont collection would
/// otherwise accumulate a shaped layout per file ever seen. Over the cap it is
/// emptied wholesale, which costs one frame of re-shaping and never grows.
#[derive(Default)]
pub struct Labels {
    shaped: HashMap<String, TextLayout>,
}

/// How many shaped strings to keep. A screenful of every panel at once is well
/// under a hundred.
const LABEL_CAP: usize = 512;

impl Labels {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shapes `text` if it has not been shaped already.
    pub fn ensure(&mut self, text: &str, font: &FontTokens, context: &mut TextContext) {
        if self.shaped.contains_key(text) {
            return;
        }
        if self.shaped.len() >= LABEL_CAP {
            self.shaped.clear();
        }
        let layout = context.layout(text, font, None);
        self.shaped.insert(text.to_string(), layout);
    }

    /// The shaped form, or `None` when nobody asked for it this frame — which
    /// draws as nothing rather than as a panic.
    pub fn get(&self, text: &str) -> Option<&TextLayout> {
        self.shaped.get(text)
    }

    pub fn len(&self) -> usize {
        self.shaped.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shaped.is_empty()
    }
}
