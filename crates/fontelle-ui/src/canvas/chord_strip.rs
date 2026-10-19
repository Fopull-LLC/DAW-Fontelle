//! The smart-chord strip: chips of the chords a placed note could carry,
//! just above the note, with a row of controls over them that reshape every
//! chip at once — rooted or containing, how far the chords reach, how they
//! are voiced, which tone is at the bottom — and a way to type one.
//!
//! Ty, 2026-10-18: beginners need *"show me chords that sound good here"*,
//! people who know the theory need *"let me build exactly the chord in my
//! head, fast"*. The chips are the first; the controls and the typed name
//! the second. What a chip means is `fontelle_types::chord`; this is only
//! where everything goes and what a press lands on.

use fontelle_types::chord::{Extension, Inversion, Suggest, Suggestion, Voicing};

use crate::layout::Rect;
use crate::theme::Metrics;

/// The strip, laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct ChordStripLayout {
    pub frame: Rect,
    /// One per chord, in the order offered.
    pub chips: Vec<Rect>,
    /// Rooted / Containing.
    pub mode: Rect,
    pub extension: Rect,
    pub voicing: Rect,
    pub inversion: Rect,
    /// *Type…*: a chord by name or numeral.
    pub type_it: Rect,
}

/// What a press on the strip lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordStripHit {
    Chip(usize),
    Mode,
    Extension,
    Voicing,
    Inversion,
    Type,
    /// The strip's own frame, between the parts.
    Inside,
}

const CHIP_WIDTH: f32 = 58.0;
const GAP: f32 = 4.0;
const CONTROLS: [f32; 5] = [80.0, 52.0, 64.0, 66.0, 70.0];

/// The words on the strip's controls.
pub fn suggest_label(suggest: Suggest) -> &'static str {
    match suggest {
        Suggest::Rooted => "Rooted",
        Suggest::Containing => "Contains",
    }
}

pub const TYPE_CHORD: &str = "Type\u{2026}";

/// Every caption the strip's controls can show, for the window to shape.
pub fn chord_strip_words() -> Vec<&'static str> {
    let mut words = vec![
        suggest_label(Suggest::Rooted),
        suggest_label(Suggest::Containing),
        TYPE_CHORD,
    ];
    words.extend(Extension::ALL.map(Extension::label));
    words.extend(Voicing::ALL.map(Voicing::label));
    words.extend(Inversion::ALL.map(Inversion::label));
    words
}

/// The strip for a note drawn at `note`, with `chips` chords, inside `grid`:
/// just above the note, or just under it where there is no room above, and
/// slid sideways to stay in the grid.
pub fn chord_strip_layout(
    grid: Rect,
    note: Rect,
    chips: usize,
    metrics: &Metrics,
) -> ChordStripLayout {
    let row = metrics.row_height.max(1.0);
    let pad = 4.0;
    let control_h = (row * 0.9).round();
    let chip_h = (row * 1.6).round();
    let chips_w = chips as f32 * CHIP_WIDTH + chips.saturating_sub(1) as f32 * GAP;
    let controls_w = CONTROLS.iter().sum::<f32>() + GAP * (CONTROLS.len() - 1) as f32;
    let width = (chips_w.max(controls_w) + 2.0 * pad).min(grid.width.max(0.0));
    let height = (pad * 3.0 + control_h + chip_h).min(grid.height.max(0.0));
    let x = note.x.min(grid.right() - width).max(grid.x);
    let above = note.y - 4.0 - height;
    let y = if above >= grid.y {
        above
    } else {
        (note.bottom() + 4.0)
            .min(grid.bottom() - height)
            .max(grid.y)
    };
    let frame = Rect::new(x, y, width, height);

    let mut at = frame.x + pad;
    let controls_y = frame.y + pad;
    let mut controls = CONTROLS.iter().map(|w| {
        let r = Rect::new(at, controls_y, *w, control_h)
            .intersection(&frame)
            .clamped();
        at += w + GAP;
        r
    });
    let mode = controls.next().unwrap_or(Rect::ZERO);
    let extension = controls.next().unwrap_or(Rect::ZERO);
    let voicing = controls.next().unwrap_or(Rect::ZERO);
    let inversion = controls.next().unwrap_or(Rect::ZERO);
    let type_it = controls.next().unwrap_or(Rect::ZERO);
    let chips_y = controls_y + control_h + pad;
    let chips = (0..chips)
        .map(|i| {
            Rect::new(
                frame.x + pad + i as f32 * (CHIP_WIDTH + GAP),
                chips_y,
                CHIP_WIDTH,
                chip_h,
            )
            .intersection(&frame)
            .clamped()
        })
        .collect();
    ChordStripLayout {
        frame,
        chips,
        mode,
        extension,
        voicing,
        inversion,
        type_it,
    }
}

pub fn chord_strip_hit(layout: &ChordStripLayout, x: f32, y: f32) -> Option<ChordStripHit> {
    if !layout.frame.contains(x, y) {
        return None;
    }
    if let Some(i) = layout.chips.iter().position(|c| c.contains(x, y)) {
        return Some(ChordStripHit::Chip(i));
    }
    let controls = [
        (layout.mode, ChordStripHit::Mode),
        (layout.extension, ChordStripHit::Extension),
        (layout.voicing, ChordStripHit::Voicing),
        (layout.inversion, ChordStripHit::Inversion),
        (layout.type_it, ChordStripHit::Type),
    ];
    Some(
        controls
            .into_iter()
            .find(|(r, _)| r.contains(x, y))
            .map_or(ChordStripHit::Inside, |(_, hit)| hit),
    )
}

/// A chip's two lines: the chord's name, and under it its job in the key
/// in small type — with a dot when it is borrowed from outside the key.
pub fn chord_chip_caption(suggestion: &Suggestion) -> (String, String) {
    let roman = suggestion.roman.clone().unwrap_or_default();
    let small = if suggestion.borrowed {
        if roman.is_empty() {
            "\u{2022}".to_string()
        } else {
            format!("{roman} \u{2022}")
        }
    } else {
        roman
    };
    (suggestion.chord.label(), small)
}

/// The next value of a control, pressed.
pub fn next_extension(e: Extension) -> Extension {
    let all = Extension::ALL;
    all[(all.iter().position(|x| *x == e).unwrap_or(0) + 1) % all.len()]
}

pub fn next_voicing(v: Voicing) -> Voicing {
    let all = Voicing::ALL;
    all[(all.iter().position(|x| *x == v).unwrap_or(0) + 1) % all.len()]
}

pub fn next_inversion(i: Inversion) -> Inversion {
    let all = Inversion::ALL;
    all[(all.iter().position(|x| *x == i).unwrap_or(0) + 1) % all.len()]
}
