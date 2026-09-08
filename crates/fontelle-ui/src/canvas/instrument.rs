//! The instrument editor: Fontelle's own soundfont player, opened up.
//!
//! The product thesis (TDD §7.2) is that an SF2 file supplies **defaults** and
//! the user owns every parameter afterwards. Until this panel existed there was
//! no way to exercise that at all — an imported preset played exactly what the
//! file said and nothing could be changed, which is the one thing Fontelle is
//! for.
//!
//! # Why this file is so short
//!
//! It knows nothing about a `Patch`. It draws a list of groups of parameters,
//! each carrying a **normalised** value, a caption, and its own
//! [`ParamAddress`] — §8.2's addressing scheme, which is also what automation,
//! MIDI learn and the plugin export will address these by, so there is one
//! scheme rather than four. Turning an address and a 0..1 into a change to a
//! patch is `fontelle-app`'s job, on the other side of `StudioHost`.
//!
//! Geometry and hit-testing only, and both pure, for the reason §2.5 gives.

use fontelle_types::ParamAddress;

use crate::layout::Rect;
use crate::theme::Metrics;

/// What kind of control a parameter gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamKind {
    /// A continuous value, dragged vertically.
    Knob,
    /// Off or on, clicked.
    Switch,
    /// One of a fixed list, clicked to step through them.
    Choice(Vec<String>),
}

/// The two controls on this panel that belong to the **channel** rather than to
/// its patch: its level and its place in the stereo field.
///
/// Named here, in the crate that draws the panel, because the window has to
/// recognise them — they are the two a right-click can turn into an automation
/// lane (see [`fontelle_types::ParamTarget`]), and the patch's own knobs are
/// not addressable yet. The layer that maps an address onto a `Patch` is
/// `fontelle-app`, and it uses these same two constants rather than spelling
/// them again.
///
/// **INVARIANT 7:** these strings never change. They are what a saved project's
/// automation names.
pub const MIXER_GAIN: &str = "mixer/gain";
pub const MIXER_PAN: &str = "mixer/pan";

/// One control on the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentParam {
    /// §8.2's stable address. The window hands this straight back when the
    /// control is moved, so nothing here has to know what it means.
    pub address: ParamAddress,
    pub label: String,
    /// **Normalised**, 0..=1. What the knob draws and what a drag writes; the
    /// mapping to hertz or seconds belongs with the patch, not with the dial.
    pub value: f32,
    /// The value in its own units, for the read-out under the control — "1.2
    /// kHz", "off", "Normal". A knob whose number you cannot read is a knob
    /// you cannot set.
    pub display: String,
    pub kind: ParamKind,
    /// Whether an automation lane has taken this control over, so it can be
    /// drawn with the ring TDD §12.2 asks for.
    ///
    /// Set by [`InstrumentView::mark_automated`] after the view is built,
    /// rather than by whatever builds it: the caller knows which addresses are
    /// automated as one set, and asking the document per parameter would
    /// rebuild that set once per knob — forty-nine times for an EQ.
    pub automated: bool,
}

/// A named row of controls — the filter, the amp envelope, the voice.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentGroup {
    pub name: String,
    pub params: Vec<InstrumentParam>,
}

/// Everything the panel shows.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentView {
    /// What the channel is playing, for the panel's heading.
    pub title: String,
    /// What this insert's **detector** can be pointed at: "no key" first, then
    /// one entry per mixer strip (`docs/effects-catalogue.md` §2.1).
    ///
    /// Empty for every panel whose effect has no detector, which is every
    /// instrument panel and nine of the eleven effects — see
    /// [`EffectKind::takes_key`](fontelle_types::EffectKind::takes_key).
    ///
    /// A row of chips rather than a knob for the reason `EffectSlot::key`
    /// gives: a track is not a float with a fixed range, so it cannot be a
    /// `ParamSpec` without inventing a second addressing scheme.
    pub keys: Vec<String>,
    /// Which of [`keys`](Self::keys) is chosen. Always `Some` when `keys` is
    /// non-empty, because "no key" is one of them.
    pub key: Option<usize>,
    pub groups: Vec<InstrumentGroup>,
}

impl InstrumentView {
    /// Flags every control an automation lane owns, and **clears the rest**.
    ///
    /// Clearing matters as much as setting: the panel is rebuilt from the
    /// document whenever the revision moves, so this runs again after a lane
    /// is deleted, and a ring left on a knob nothing owns any more is worse
    /// than no ring — it is a ring that lies.
    pub fn mark_automated(&mut self, is_automated: impl Fn(&ParamAddress) -> bool) {
        for group in &mut self.groups {
            for param in &mut group.params {
                param.automated = is_automated(&param.address);
            }
        }
    }
}

impl InstrumentView {
    pub fn param(&self, group: usize, param: usize) -> Option<&InstrumentParam> {
        self.groups.get(group)?.params.get(param)
    }
}

/// Where every control is.
#[derive(Debug, Clone, PartialEq)]
pub struct InstrumentLayout {
    pub body: Rect,
    /// The instrument's name, drawn as a **field** across the top rather than
    /// as a bar.
    ///
    /// *"the name section in the instrument window for the soundfont player
    /// should be kind of like field instead of a bar and i should be able to
    /// drag soundfonts into it from the soundfonts window to also assign a
    /// soundfont."* A field is a thing you can put something *in*, and that is
    /// exactly what this one is for: it is the drop target.
    pub name: Rect,
    /// One chip per choosable key, under the presets. Same shape and the same
    /// rules — see [`InstrumentView::keys`].
    pub keys: Vec<(usize, Rect)>,
    /// One heading strip per group, in order.
    pub headings: Vec<(usize, Rect)>,
    /// Every control, as `(group, param, cell)`.
    pub cells: Vec<(usize, usize, Rect)>,
    /// How tall the whole thing came out, so a scrollbar can be drawn and a
    /// scroll offset clamped once there are enough parameters to need one.
    pub content_height: f32,
}

impl InstrumentLayout {
    /// Where one control's cell is, by its place in the view.
    ///
    /// What a drop-down hangs under: the list appears where the control is,
    /// not where the pointer happened to be, which is what a drop-down is.
    pub fn control(&self, group: usize, param: usize) -> Option<Rect> {
        self.cells
            .iter()
            .find(|(g, p, _)| *g == group && *p == param)
            .map(|(_, _, cell)| *cell)
    }
}

/// How wide one control's cell is: a dial, its name, and its read-out.
pub const CELL_WIDTH: f32 = 92.0;

/// And how tall. Enough for the dial, the caption over it and the value under.
pub const CELL_HEIGHT: f32 = 76.0;

/// Between cells, and around them.
const GAP: f32 = 6.0;

/// How wide a chip on the key row is, and how tall.
///
/// Wide enough for "12-bit sampler" at the panel's own font, which is the
/// longest name any effect ships; a chip whose name is clipped is a chip
/// nobody can choose on purpose.
pub const CHIP_WIDTH: f32 = 104.0;
pub const CHIP_HEIGHT: f32 = 22.0;

pub fn instrument_layout(body: Rect, metrics: &Metrics, view: &InstrumentView) -> InstrumentLayout {
    let mut headings = Vec::new();
    let mut cells = Vec::new();
    let mut keys = Vec::new();
    if body.is_empty() {
        return InstrumentLayout {
            body,
            name: Rect::ZERO,
            keys,
            headings,
            cells,
            content_height: 0.0,
        };
    }

    // How many cells fit across, at least one — a panel too narrow for a cell
    // gets a clipped column rather than a division by zero.
    let per_row = (((body.width + GAP) / (CELL_WIDTH + GAP)).floor() as usize).max(1);
    let mut y = body.y;

    // The name field, across the top and above everything: it says what this
    // instrument *is*, and it is where a soundfont dragged out of the browser
    // lands.
    let name = Rect::new(body.x, y, body.width, metrics.row_height).clamped();
    y += metrics.row_height + GAP;

    // The chip rows, above everything, and only when there is one of each: a
    // panel with neither has to lay out exactly as it did before they existed.
    keys = chip_row(body, view.keys.len(), &mut y);

    for (index, group) in view.groups.iter().enumerate() {
        let heading = Rect::new(body.x, y, body.width, metrics.row_height).clamped();
        headings.push((index, heading));
        y += metrics.row_height;

        for (n, _) in group.params.iter().enumerate() {
            let column = n % per_row;
            let row = n / per_row;
            let cell = Rect::new(
                body.x + column as f32 * (CELL_WIDTH + GAP),
                y + row as f32 * (CELL_HEIGHT + GAP),
                CELL_WIDTH,
                CELL_HEIGHT,
            );
            // Clipped to the panel's width so a cell can never run off the
            // right-hand edge, whatever `per_row` rounded to.
            cells.push((
                index,
                n,
                Rect::new(
                    cell.x,
                    cell.y,
                    cell.width.min((body.right() - cell.x).max(0.0)),
                    cell.height,
                )
                .clamped(),
            ));
        }
        let rows = group.params.len().div_ceil(per_row);
        y += rows as f32 * (CELL_HEIGHT + GAP) + GAP;
    }

    InstrumentLayout {
        body,
        name,
        keys,
        headings,
        cells,
        content_height: (y - body.y).max(0.0),
    }
}

/// Lays `count` chips across `body` from `y` down, wrapping, and advances `y`
/// past them.
///
/// One implementation for both rows: presets and keys are different lists of
/// different things, but "a row of clickable words above the controls" is one
/// shape, and two copies of it is one to get wrong the day a third row wants
/// the same shape.
fn chip_row(body: Rect, count: usize, y: &mut f32) -> Vec<(usize, Rect)> {
    if count == 0 {
        return Vec::new();
    }
    let across = (((body.width + GAP) / (CHIP_WIDTH + GAP)).floor() as usize).max(1);
    let chips = (0..count)
        .map(|index| {
            let chip = Rect::new(
                body.x + (index % across) as f32 * (CHIP_WIDTH + GAP),
                *y + (index / across) as f32 * (CHIP_HEIGHT + GAP),
                CHIP_WIDTH,
                CHIP_HEIGHT,
            );
            (
                index,
                // Clipped to the panel's width so a chip can never run off the
                // right-hand edge, whatever `across` rounded to.
                Rect::new(
                    chip.x,
                    chip.y,
                    chip.width.min((body.right() - chip.x).max(0.0)),
                    chip.height,
                )
                .clamped(),
            )
        })
        .collect();
    *y += count.div_ceil(across) as f32 * (CHIP_HEIGHT + GAP) + GAP;
    chips
}

/// Which key chip is under the pointer, if any — see
/// [`InstrumentView::keys`].
pub fn instrument_key_hit(layout: &InstrumentLayout, x: f32, y: f32) -> Option<usize> {
    chip_hit(&layout.keys, x, y)
}

fn chip_hit(chips: &[(usize, Rect)], x: f32, y: f32) -> Option<usize> {
    chips
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(index, _)| *index)
}

/// Which control is under the pointer.
pub fn instrument_hit(layout: &InstrumentLayout, x: f32, y: f32) -> Option<(usize, usize)> {
    layout
        .cells
        .iter()
        .find(|(_, _, rect)| rect.contains(x, y))
        .map(|(group, param, _)| (*group, *param))
}

/// How many pixels of vertical drag walk a knob from one end to the other.
///
/// A hundred and fifty rather than the height of the panel: a knob whose full
/// throw is a screen is one nobody can set, and one whose throw is thirty
/// pixels is one nobody can set *precisely*. Shift is the answer to the second
/// half — see `FINE`.
const KNOB_THROW_PX: f32 = 150.0;

/// How much slower a fine drag is.
const FINE: f32 = 6.0;

/// The value a vertical drag of `dy` pixels from `start` is asking for.
///
/// Up is more, which is the way every knob in every DAW works and the opposite
/// of the screen's y axis, hence the sign.
pub fn knob_value(start: f32, dy: f32, fine: bool) -> f32 {
    let throw = KNOB_THROW_PX * if fine { FINE } else { 1.0 };
    (start - dy / throw).clamp(0.0, 1.0)
}

/// The next value a *click* asks for — a switch flips, a choice steps and
/// wraps, and a knob is not a thing you click.
pub fn next_value(kind: &ParamKind, value: f32) -> f32 {
    match kind {
        ParamKind::Knob => value,
        ParamKind::Switch => {
            if value >= 0.5 {
                0.0
            } else {
                1.0
            }
        }
        ParamKind::Choice(options) => {
            if options.len() < 2 {
                return value;
            }
            let next = (choice_index(kind, value) + 1) % options.len();
            next as f32 / (options.len() - 1) as f32
        }
    }
}

/// Which option a normalised value names.
///
/// The endpoints are the first and last options, so a four-way choice sits at
/// 0, 1/3, 2/3 and 1 — which is what makes a knob and a choice the same kind of
/// number, and what lets automation address either.
pub fn choice_index(kind: &ParamKind, value: f32) -> usize {
    let ParamKind::Choice(options) = kind else {
        return 0;
    };
    if options.len() < 2 {
        return 0;
    }
    let last = options.len() - 1;
    ((value.clamp(0.0, 1.0) * last as f32).round() as usize).min(last)
}
