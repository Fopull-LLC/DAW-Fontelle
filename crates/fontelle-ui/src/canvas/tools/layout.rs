//! Where a tool's dialog goes, and what a press on it lands on.
//!
//! Plan §3.4: the dialog used to hang off the chip, over the roll's right
//! half, and on the tour song it covered the very chords it was previewing.
//! It now opens on the side of the grid away from the selection, and its title
//! is a handle: wherever it is dragged to, it opens there again.

use super::{ToolControl, ToolKind, ToolRow, Tools};
use crate::layout::Rect;
use crate::theme::Metrics;

/// A little air inside the frame.
const PAD: f32 = 4.0;

/// Wide enough for "Leave near misses" and "slows down 100%" side by side.
const WIDTH: f32 = 260.0;

/// An arrow's width either side of a value.
const ARROW: f32 = 18.0;

/// The value between them: room for "speeds up 100%".
const VALUE: f32 = 112.0;

/// The space a gap leaves between the settings and the buttons.
const BUTTON_GAP: f32 = 4.0;

/// Where a pattern's cells begin, after its name.
const PATTERN_LABEL: f32 = 88.0;

/// How far from the grid's edge a dialog opens.
const MARGIN: f32 = 8.0;

/// One tool's dialog, laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolsDialog {
    pub kind: ToolKind,
    pub frame: Rect,
    /// The band across the top: the name, what it acts on, and the handle the
    /// dialog is dragged by.
    pub title: Rect,
    /// *"Strum — 3 selected"*, *"Strum — whole clip (12)"*.
    pub title_text: String,
    /// Every row, settings then the three buttons on one line. A row with no
    /// room left (a short window) is an empty rectangle: drawn as nothing,
    /// hit as nothing, and the list keeps its length.
    pub rows: Vec<(ToolRow, Rect)>,
}

/// What a press on a dialog lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPress {
    /// The title: the start of a drag that moves the dialog.
    Title,
    /// A row's middle: a button, or a value's own text.
    Row(ToolRow),
    /// One of a value's arrows: `-1` back, `1` on.
    Step(ToolRow, i32),
    /// A pattern's cell.
    Cell(ToolRow, usize),
}

/// The width and height a dialog with these setting rows takes.
pub fn tools_dialog_size(rows: &[ToolRow], metrics: &Metrics) -> (f32, f32) {
    let row = metrics.row_height.max(1.0);
    // The title, the settings, the buttons.
    (
        WIDTH,
        row * (rows.len() + 2) as f32 + PAD * 2.0 + BUTTON_GAP,
    )
}

/// Where a dialog of `size` opens: where it was dragged to if it was
/// (`chosen`), otherwise at the top of `grid` on the side away from the
/// selection (`selection` is its left and right edge on screen). Always
/// inside `bounds`.
pub fn tools_dialog_place(
    size: (f32, f32),
    grid: Rect,
    selection: Option<(f32, f32)>,
    chosen: Option<(f32, f32)>,
    bounds: Rect,
) -> (f32, f32) {
    let (x, y) = chosen.unwrap_or_else(|| {
        let right = (grid.right() - size.0 - MARGIN, grid.y + MARGIN);
        let left = (grid.x + MARGIN, grid.y + MARGIN);
        match selection {
            Some((from, to)) if (from + to) / 2.0 > grid.x + grid.width / 2.0 => left,
            _ => right,
        }
    });
    (
        x.clamp(bounds.x, (bounds.right() - size.0).max(bounds.x)),
        y.clamp(bounds.y, (bounds.bottom() - size.1).max(bounds.y)),
    )
}

/// `kind`'s dialog with its top-left at `at`, kept inside `bounds` — and
/// clipped, if `bounds` is shorter than it: a short window must still be one
/// you can transpose in.
pub fn tools_dialog_layout(
    kind: ToolKind,
    rows: &[ToolRow],
    title_text: String,
    at: (f32, f32),
    bounds: Rect,
    metrics: &Metrics,
) -> ToolsDialog {
    let (width, height) = tools_dialog_size(rows, metrics);
    let width = width.min(bounds.width);
    let row = metrics.row_height.max(1.0);
    let buttons = [ToolRow::Reset, ToolRow::Hear, ToolRow::Apply];
    if bounds.is_empty() || width <= 0.0 {
        return ToolsDialog {
            kind,
            frame: Rect::ZERO,
            title: Rect::ZERO,
            title_text,
            rows: rows
                .iter()
                .chain(&buttons)
                .map(|r| (*r, Rect::ZERO))
                .collect(),
        };
    }
    let x = at.0.clamp(bounds.x, (bounds.right() - width).max(bounds.x));
    let y =
        at.1.clamp(bounds.y, (bounds.bottom() - height).max(bounds.y));
    let frame = Rect::new(x, y, width, height).intersection(&bounds);
    let inner = (width - PAD * 2.0).max(0.0);
    let clip = |r: Rect| r.intersection(&frame).clamped();
    let title = clip(Rect::new(x + PAD, y + PAD, inner, row));
    let mut laid: Vec<(ToolRow, Rect)> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            (
                *r,
                clip(Rect::new(
                    x + PAD,
                    y + PAD + row * (i + 1) as f32,
                    inner,
                    row,
                )),
            )
        })
        .collect();
    let button_y = y + PAD + row * (rows.len() + 1) as f32 + BUTTON_GAP;
    let button_width = (inner - PAD * 2.0) / 3.0;
    for (i, b) in buttons.into_iter().enumerate() {
        laid.push((
            b,
            clip(Rect::new(
                x + PAD + (button_width + PAD) * i as f32,
                button_y,
                button_width,
                row,
            )),
        ));
    }
    ToolsDialog {
        kind,
        frame,
        title,
        title_text,
        rows: laid,
    }
}

/// A value row's back arrow, value and forward arrow, left to right at its
/// right-hand end.
pub fn step_arrows(row: Rect) -> (Rect, Rect, Rect) {
    let forward = Rect::new(row.right() - ARROW, row.y, ARROW, row.height);
    let value = Rect::new(forward.x - VALUE, row.y, VALUE, row.height);
    let back = Rect::new(value.x - ARROW, row.y, ARROW, row.height);
    (back, value, forward)
}

/// A pattern row's cells, after its name.
pub fn pattern_cells(row: Rect, len: u8) -> Vec<Rect> {
    let len = usize::from(len.max(1));
    let from = row.x + PATTERN_LABEL.min(row.width);
    let span = (row.right() - from - 2.0).max(0.0);
    let cell = span / len as f32;
    (0..len)
        .map(|i| {
            Rect::new(
                from + cell * i as f32,
                row.y + 2.0,
                cell,
                (row.height - 4.0).max(1.0),
            )
            .inset(1.0)
        })
        .collect()
}

/// What `(x, y)` lands on, if anything.
pub fn tools_dialog_press(
    dialog: &ToolsDialog,
    tools: &Tools,
    x: f32,
    y: f32,
) -> Option<ToolPress> {
    if !dialog.title.is_empty() && dialog.title.contains(x, y) {
        return Some(ToolPress::Title);
    }
    let row = tools_dialog_hit(dialog, x, y)?;
    let rect = dialog.rows.iter().find(|(r, _)| *r == row)?.1;
    Some(match tools.control(row) {
        ToolControl::Button => ToolPress::Row(row),
        ToolControl::Pattern => {
            match pattern_cells(rect, tools.chop.pattern_len)
                .iter()
                .position(|c| c.inset(-1.0).contains(x, y))
            {
                Some(i) => ToolPress::Cell(row, i),
                None => ToolPress::Row(row),
            }
        }
        ToolControl::Steps => {
            let (back, _, forward) = step_arrows(rect);
            if back.contains(x, y) {
                ToolPress::Step(row, -1)
            } else if forward.contains(x, y) {
                ToolPress::Step(row, 1)
            } else {
                ToolPress::Row(row)
            }
        }
    })
}

/// Which row `(x, y)` is on. The title is not a row.
pub fn tools_dialog_hit(dialog: &ToolsDialog, x: f32, y: f32) -> Option<ToolRow> {
    dialog
        .rows
        .iter()
        .find(|(_, rect)| !rect.is_empty() && rect.contains(x, y))
        .map(|(row, _)| *row)
}
