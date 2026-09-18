//! Shaping, layout and font fallback via `cosmic-text` (TDD §16.2), turned into
//! something `vello` can draw.
//!
//! The seam matters: shaping is a solved problem we do not want to own, and
//! *positioning* is ours and is arithmetic — so the shaper's output is
//! converted here into glyph runs, and that conversion is tested without a GPU
//! (`docs/first-usable-plan.md` §2.5).

use std::collections::HashMap;

pub use cosmic_text::Weight;
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Shaping, Wrap};

use crate::theme::FontTokens;

/// A size and a weight to shape a string at — the bridge's type scale
/// (`docs/flopsynth-next.md` §3.1, principle 11): 15 px Medium headings,
/// 12 px Regular captions, 12 px Medium values, each times the window's
/// scale. The size is kept in tenths of a pixel so a style can be a map
/// key, and the line is the size — the bridge's captions sit in a band of
/// their own size, and a line taller than its glyphs would be clipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextStyle {
    size_tenths: u32,
    weight: u16,
}

impl TextStyle {
    pub fn new(size_px: f32, weight: Weight) -> Self {
        Self {
            size_tenths: (size_px.max(1.0) * 10.0).round() as u32,
            weight: weight.0,
        }
    }

    pub fn size(self) -> f32 {
        self.size_tenths as f32 / 10.0
    }

    pub fn weight(self) -> Weight {
        Weight(self.weight)
    }
}

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
        self.layout_weighted(text, font, max_width, Weight::NORMAL)
    }

    /// [`layout`](Self::layout) at `weight` — the face's Medium or Bold
    /// where it has one; `cosmic-text` picks the nearest it finds.
    pub fn layout_weighted(
        &mut self,
        text: &str,
        font: &FontTokens,
        max_width: Option<f32>,
        weight: Weight,
    ) -> TextLayout {
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
            &Attrs::new().family(family(&font.family)).weight(weight),
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
/// otherwise accumulate a shaped layout per file ever seen. Over the cap,
/// **what goes is what the current frame has not asked for** — never a string
/// shaped for the frame being built.
///
/// That last clause is the reported bug. The cache used to be emptied
/// wholesale the moment it filled, which is to say in the middle of a frame's
/// shaping pass: every string shaped *before* that moment was gone by the
/// time the renderer looked it up and drew as nothing, for one frame, and a
/// window whose working set sat near the cap did it on nearly every frame.
/// *"The text constantly keeps flickering while trying to navigate the
/// app."* The window says where a frame begins ([`Labels::begin_frame`]),
/// every string carries the frame that last asked for it, and a frame that
/// wants more than the cap simply keeps all of it.
#[derive(Default)]
pub struct Labels {
    shaped: HashMap<String, Shaped>,
    /// The same strings at [`SMALL_LABEL`] of the chrome's size, kept apart
    /// so that a caption and a heading spelt the same way can both be drawn.
    small: HashMap<String, Shaped>,
    /// Strings shaped at a [`TextStyle`] of the caller's — the bridge's
    /// three sizes at its scale — keyed by the style too, so a card's name
    /// and its captions spelt alike are two shaped strings.
    styled: HashMap<(String, TextStyle), Shaped>,
    /// Which frame is being shaped. Bumped by [`Labels::begin_frame`].
    frame: u64,
}

/// A layout and the frame that last asked for it.
struct Shaped {
    frame: u64,
    layout: TextLayout,
}

/// How much smaller a small label is than the chrome's text.
///
/// Flopsynth's knob captions are drawn at this size: a hundred and thirty
/// controls do not fit on one page at thirteen pixels, and "mod from" in a
/// fifty-pixel cell does not either. Eleven pixels at the default size —
/// what every synthesiser's panel is captioned in.
pub const SMALL_LABEL: f32 = 0.85;

/// How many shaped strings to keep before the ones no longer on screen are
/// let go. A screenful of every panel at once is a few hundred; the roll's
/// key names, a browser page of names and details, and an editor window's
/// captions and read-outs all count.
const LABEL_CAP: usize = 512;

impl Labels {
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks the start of a frame's shaping pass.
    ///
    /// Everything shaped after this and before the next call is *this
    /// frame's* and survives the cache filling; everything older is what a
    /// full cache lets go of.
    pub fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Shapes `text` if it has not been shaped already.
    pub fn ensure(&mut self, text: &str, font: &FontTokens, context: &mut TextContext) {
        let frame = self.frame;
        if let Some(entry) = self.shaped.get_mut(text) {
            entry.frame = frame;
            return;
        }
        if self.shaped.len() >= LABEL_CAP {
            self.shaped.retain(|_, entry| entry.frame == frame);
        }
        let layout = context.layout(text, font, None);
        self.shaped
            .insert(text.to_string(), Shaped { frame, layout });
    }

    /// Shapes `text` at [`SMALL_LABEL`] of `font`'s size, if it has not been
    /// already. Asked back with [`Labels::get_small`].
    pub fn ensure_small(&mut self, text: &str, font: &FontTokens, context: &mut TextContext) {
        let frame = self.frame;
        if let Some(entry) = self.small.get_mut(text) {
            entry.frame = frame;
            return;
        }
        if self.small.len() >= LABEL_CAP {
            self.small.retain(|_, entry| entry.frame == frame);
        }
        let small = FontTokens {
            family: font.family.clone(),
            size: font.size * SMALL_LABEL,
            line_height: font.line_height,
        };
        let layout = context.layout(text, &small, None);
        self.small
            .insert(text.to_string(), Shaped { frame, layout });
    }

    /// The small form of `text`, or `None` when nobody shaped it small.
    pub fn get_small(&self, text: &str) -> Option<&TextLayout> {
        self.small.get(text).map(|entry| &entry.layout)
    }

    /// Shapes `text` at `style` — its size, in `font`'s family, at its
    /// weight, with the line as tall as the size — if it has not been
    /// already. Asked back with [`Labels::get_styled`].
    pub fn ensure_styled(
        &mut self,
        text: &str,
        font: &FontTokens,
        style: TextStyle,
        context: &mut TextContext,
    ) {
        let frame = self.frame;
        let key = (text.to_string(), style);
        if let Some(entry) = self.styled.get_mut(&key) {
            entry.frame = frame;
            return;
        }
        if self.styled.len() >= LABEL_CAP {
            self.styled.retain(|_, entry| entry.frame == frame);
        }
        let sized = FontTokens {
            family: font.family.clone(),
            size: style.size(),
            line_height: 1.0,
        };
        let layout = context.layout_weighted(text, &sized, None, style.weight());
        self.styled.insert(key, Shaped { frame, layout });
    }

    /// The form of `text` at `style`, or `None` when nobody shaped it so.
    pub fn get_styled(&self, text: &str, style: TextStyle) -> Option<&TextLayout> {
        self.styled
            .get(&(text.to_string(), style))
            .map(|entry| &entry.layout)
    }

    /// The shaped form, or `None` when nobody asked for it this frame — which
    /// draws as nothing rather than as a panic.
    pub fn get(&self, text: &str) -> Option<&TextLayout> {
        self.shaped.get(text).map(|entry| &entry.layout)
    }

    pub fn len(&self) -> usize {
        self.shaped.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shaped.is_empty()
    }
}
