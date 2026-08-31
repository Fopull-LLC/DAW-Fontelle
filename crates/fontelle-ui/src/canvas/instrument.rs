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
    pub groups: Vec<InstrumentGroup>,
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
    /// One heading strip per group, in order.
    pub headings: Vec<(usize, Rect)>,
    /// Every control, as `(group, param, cell)`.
    pub cells: Vec<(usize, usize, Rect)>,
    /// How tall the whole thing came out, so a scrollbar can be drawn and a
    /// scroll offset clamped once there are enough parameters to need one.
    pub content_height: f32,
}

/// How wide one control's cell is: a dial, its name, and its read-out.
pub const CELL_WIDTH: f32 = 92.0;

/// And how tall. Enough for the dial, the caption over it and the value under.
pub const CELL_HEIGHT: f32 = 76.0;

/// Between cells, and around them.
const GAP: f32 = 6.0;

pub fn instrument_layout(body: Rect, metrics: &Metrics, view: &InstrumentView) -> InstrumentLayout {
    let mut headings = Vec::new();
    let mut cells = Vec::new();
    if body.is_empty() {
        return InstrumentLayout {
            body,
            headings,
            cells,
            content_height: 0.0,
        };
    }

    // How many cells fit across, at least one — a panel too narrow for a cell
    // gets a clipped column rather than a division by zero.
    let per_row = (((body.width + GAP) / (CELL_WIDTH + GAP)).floor() as usize).max(1);
    let mut y = body.y;

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
        headings,
        cells,
        content_height: (y - body.y).max(0.0),
    }
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
