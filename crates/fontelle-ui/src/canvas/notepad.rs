//! The notepad's window: a sheet of monospace text, and a footer that turns
//! the pages (`docs/effects-catalogue.md` §2.8).
//!
//! > *"its just a basic text editor with pages you can go left and right
//! > between and use it like a normal text editor to write down lyrics for
//! > example as you record to sing them back. … should have clean ux and
//! > pretty visual design thats simplistic and looks like a computer terminal
//! > notepad."*
//!
//! # Why monospace, beyond the look
//!
//! A terminal is what was asked for, and it is also what makes the editor
//! honest. Placing a caret in proportional text means measuring the width of
//! every prefix of the line under the pointer, and this crate may not shape
//! text at all (INVARIANT 2) — so a proportional page would have to ask the
//! window for a measurement per click and per keystroke. On a fixed grid the
//! window measures **one** character when the page is laid out and
//! everything here is arithmetic: column times advance, row times line
//! height. Every rule below is checked without a window
//! (`fontelle-ui/tests/notepad.rs`).
//!
//! # What is here and what is not
//!
//! [`TextEntry`](super::TextEntry) is the model of *being edited* — the
//! string, the caret, the selection — and it is the same one the browser's
//! search box and every rename use, keys and all
//! ([`text_key`](super::text_key)). What a page adds is **lines**: where the
//! text wraps, which row the caret is on, and what Up, Down, Home, End and a
//! click mean once there is more than one. That is all this module is.

use fontelle_types::{NotepadSize, NotepadTheme};

use super::TextEntry;
use crate::layout::Rect;
use crate::theme::Metrics;

/// What the window shows of one notepad insert.
///
/// The showing page only: the others are in the document and nothing here can
/// draw them, so carrying them would be a copy to keep in step for nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct NotepadView {
    /// The strip this pad is an insert on — what the window calls itself, so
    /// that two pads open on two tracks are tellable apart on the desktop.
    pub track: String,
    pub theme: NotepadTheme,
    pub size: NotepadSize,
    /// Which page is showing, counted from zero.
    pub page: usize,
    /// How many there are. Never zero.
    pub pages: usize,
    /// What the showing page says.
    pub text: String,
}

impl NotepadView {
    /// What the footer's page counter reads — counted from one, because that
    /// is how a person counts pages.
    pub fn page_label(&self) -> String {
        format!("page {} / {}", self.page + 1, self.pages.max(1))
    }
}

/// How much taller than its text a line is drawn.
///
/// A terminal's leading. Tighter than this and a page of lyrics is a block;
/// looser and there is no page left in the window.
pub const NOTEPAD_LEADING: f32 = 1.45;

/// How big the words are, at the chrome's own text size.
///
/// Three steps rather than a knob: the pad is read from across a room while
/// somebody sings, and *the big one* is a decision rather than a number to
/// dial in.
pub fn notepad_text_px(size: NotepadSize, base_px: f32) -> f32 {
    let base = base_px.max(9.0);
    match size {
        NotepadSize::Small => base * 0.95,
        NotepadSize::Medium => base * 1.2,
        NotepadSize::Large => base * 1.7,
    }
}

/// How many characters fit across `width` at `advance`.
///
/// Never zero: every piece of arithmetic below divides by it or takes a
/// remainder, and a window dragged to nothing must not be a panic.
pub fn notepad_columns(width: f32, advance: f32) -> usize {
    if advance <= 0.0 || width <= 0.0 {
        return 1;
    }
    ((width / advance).floor() as usize).max(1)
}

/// Where everything in the pad's window is.
#[derive(Debug, Clone, PartialEq)]
pub struct NotepadLayout {
    pub body: Rect,
    /// The page itself — the ground the words are written on.
    pub sheet: Rect,
    /// The writing area inside it, which is what the grid is measured from.
    pub text: Rect,
    pub footer: Rect,
    pub previous: Rect,
    pub count: Rect,
    pub next: Rect,
    pub add: Rect,
    pub remove: Rect,
    pub size_chip: Rect,
    pub theme_chip: Rect,
    /// One drawn line, in pixels.
    pub line_height: f32,
    /// How wide one character is — **the window's own measurement**, kept
    /// here so that the glyphs, the caret and the click all count in the
    /// same unit. Derived from the width instead, it would be a fraction
    /// wider than the letters actually are, and a click near the end of a
    /// long line would land one character short.
    pub advance: f32,
    /// How many lines are on screen at once.
    pub lines: usize,
    /// How many characters fit across the sheet.
    pub columns: usize,
}

impl Default for NotepadLayout {
    /// A window with nothing in it: every rectangle empty, which the renderer
    /// skips and the hit test declines. What the window holds before it has
    /// been given a pad.
    fn default() -> Self {
        Self {
            body: Rect::ZERO,
            sheet: Rect::ZERO,
            text: Rect::ZERO,
            footer: Rect::ZERO,
            previous: Rect::ZERO,
            count: Rect::ZERO,
            next: Rect::ZERO,
            add: Rect::ZERO,
            remove: Rect::ZERO,
            size_chip: Rect::ZERO,
            theme_chip: Rect::ZERO,
            line_height: 1.0,
            advance: 1.0,
            lines: 1,
            columns: 1,
        }
    }
}

/// How wide a footer button is. Square, so ‹ and › are the same target as +
/// and −.
const BUTTON: f32 = 24.0;
/// How wide the page counter is. Room for "page 10 / 20" and no more.
const COUNT: f32 = 92.0;
/// The chips on the right: the size stepper is one letter, the theme's name
/// is a word.
const SIZE_CHIP: f32 = 34.0;
const THEME_CHIP: f32 = 88.0;

/// The pad's window, laid out from the room it has.
///
/// `advance` and `line_height` come from the window, because they are
/// measurements of shaped text and this crate may not shape (INVARIANT 2).
pub fn notepad_layout(
    body: Rect,
    metrics: &Metrics,
    _view: &NotepadView,
    advance: f32,
    line_height: f32,
) -> NotepadLayout {
    let pad = metrics.panel_padding;
    let footer_height = (metrics.row_height + 6.0).min(body.height);
    let (above, footer) = body.split_top((body.height - footer_height).max(0.0));
    let sheet = above.inset(pad);
    // The sheet has a rule drawn round it, and the words sit a character in
    // from it — a terminal's own margin, and what stops a descender touching
    // the border.
    let text = sheet.inset(pad.max(6.0));

    let line_height = line_height.max(1.0);
    let lines = ((text.height / line_height).floor() as usize).max(1);
    let columns = notepad_columns(text.width, advance);

    // The footer reads left to right: where you are in the pad, then what
    // changes how many pages there are, then how it looks.
    let row = |x: f32, width: f32| {
        Rect::new(
            x,
            footer.y + (footer.height - BUTTON).max(0.0) / 2.0,
            width,
            BUTTON.min(footer.height),
        )
        .clamped()
    };
    let left = footer.x + pad;
    let previous = row(left, BUTTON);
    let count = row(previous.right() + 2.0, COUNT);
    let next = row(count.right() + 2.0, BUTTON);
    let add = row(next.right() + 10.0, BUTTON);
    let remove = row(add.right() + 2.0, BUTTON);
    // The two chips are pinned to the right-hand edge, so they stay together
    // as the window is resized and never collide with the page controls.
    let theme_x = (footer.right() - pad - THEME_CHIP).max(remove.right() + 6.0);
    let size_x = (theme_x - 6.0 - SIZE_CHIP).max(remove.right() + 6.0);
    let size_chip = row(size_x, SIZE_CHIP);
    let theme_chip = row(theme_x, THEME_CHIP);

    NotepadLayout {
        body,
        sheet,
        text,
        footer,
        previous,
        count,
        next,
        add,
        remove,
        size_chip,
        theme_chip,
        line_height,
        advance: advance.max(1.0),
        lines,
        columns,
    }
}

/// What is under the pointer in the pad's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotepadHit {
    /// The sheet: a press puts the caret where it landed and starts typing.
    Page,
    Previous,
    Next,
    AddPage,
    RemovePage,
    /// The size stepper.
    Size,
    /// The theme chip, which steps to the next look.
    Theme,
    Nothing,
}

pub fn notepad_hit(layout: &NotepadLayout, x: f32, y: f32) -> NotepadHit {
    // The footer's controls first: they are small, and the sheet is
    // everything else.
    for (rect, hit) in [
        (layout.previous, NotepadHit::Previous),
        (layout.next, NotepadHit::Next),
        (layout.add, NotepadHit::AddPage),
        (layout.remove, NotepadHit::RemovePage),
        (layout.size_chip, NotepadHit::Size),
        (layout.theme_chip, NotepadHit::Theme),
    ] {
        if rect.contains(x, y) {
            return hit;
        }
    }
    // The whole sheet, not just the text inside it: a click in the margin is
    // a click on the page, and a person aiming at the end of a line should
    // not have to hit it exactly.
    if layout.sheet.contains(x, y) {
        return NotepadHit::Page;
    }
    NotepadHit::Nothing
}

// ------------------------------------------------------------- the rows

/// One drawn line of a page: the bytes it covers.
///
/// `to` is exclusive and never includes the newline that ended the row — a
/// row is what is *drawn*, and a newline draws as nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotepadRow {
    pub from: usize,
    pub to: usize,
}

impl NotepadRow {
    pub fn contains(&self, at: usize) -> bool {
        at >= self.from && at <= self.to
    }
}

/// How `text` is laid out on a page `columns` wide.
///
/// Word wrap, with a hard break for a word that is longer than the page: a
/// lyric is written to be read, and a sheet you have to scroll sideways is not
/// one. There is **always at least one row**, so an empty page still has a
/// line for the caret to sit on, and a page ending in a newline has an empty
/// row after it for the same reason.
pub fn notepad_rows(text: &str, columns: usize) -> Vec<NotepadRow> {
    let columns = columns.max(1);
    let mut rows = Vec::new();
    // One line of the *text* at a time — what the newlines divide it into —
    // each of which wraps into one or more rows.
    let mut line_start = 0;
    loop {
        let line_end = text[line_start..]
            .find('\n')
            .map_or(text.len(), |at| line_start + at);
        wrap_line(text, line_start, line_end, columns, &mut rows);
        if line_end >= text.len() {
            break;
        }
        line_start = line_end + 1;
    }
    if rows.is_empty() {
        rows.push(NotepadRow { from: 0, to: 0 });
    }
    rows
}

/// One line of text, cut into rows of at most `columns` characters.
fn wrap_line(text: &str, from: usize, to: usize, columns: usize, rows: &mut Vec<NotepadRow>) {
    let mut at = from;
    loop {
        // Where `columns` characters from `at` ends, in bytes.
        let end = step(text, at, to, columns);
        if end >= to {
            rows.push(NotepadRow { from: at, to });
            return;
        }
        // The row is full and the line goes on, so it breaks — after the last
        // space that fits, if there is one, and through the middle of the
        // word if there is not.
        let cut = text[at..end]
            .rfind(' ')
            .map(|space| at + space + 1)
            .filter(|cut| *cut > at)
            .unwrap_or(end);
        rows.push(NotepadRow { from: at, to: cut });
        at = cut;
    }
}

/// `columns` characters past `at`, stopping at `to`.
fn step(text: &str, at: usize, to: usize, columns: usize) -> usize {
    text[at..to]
        .char_indices()
        .nth(columns)
        .map_or(to, |(offset, _)| at + offset)
}

/// Which row `caret` is on.
///
/// A caret sitting exactly on a row's end is on **that** row rather than on
/// the next one's start, so typing at the end of a wrapped line stays where
/// the eye is — except where the row ends at a newline, which is a line
/// somebody deliberately ended.
pub fn notepad_row_of(rows: &[NotepadRow], caret: usize) -> usize {
    for (index, row) in rows.iter().enumerate() {
        if caret < row.to {
            return index;
        }
        if caret == row.to {
            // At a row's end. It belongs to *this* row unless the next one
            // begins in the same place — a wrap, where the eye has already
            // moved on. A newline leaves a gap between the rows, so the end
            // of a line somebody ended is their own row's.
            let wrapped = rows.get(index + 1).is_some_and(|next| next.from == caret);
            if !wrapped {
                return index;
            }
        }
    }
    rows.len().saturating_sub(1)
}

/// Which column `caret` sits in, on its own row.
pub fn notepad_column_of(text: &str, rows: &[NotepadRow], caret: usize) -> usize {
    let row = rows.get(notepad_row_of(rows, caret)).copied();
    let Some(row) = row else { return 0 };
    let from = row.from.min(text.len());
    let caret = caret.clamp(from, row.to.min(text.len()));
    text[from..caret].chars().count()
}

/// The byte index of `column` on `row`.
///
/// A column past the end of the row lands at its end, which is what makes a
/// long line and a short one walk up and down together.
pub fn notepad_index_of(text: &str, rows: &[NotepadRow], row: usize, column: usize) -> usize {
    let Some(row) = rows.get(row.min(rows.len().saturating_sub(1))) else {
        return text.len();
    };
    step(text, row.from, row.to, column)
}

// ---------------------------------------------------------- moving about

/// Up (`by` = −1) or down (`by` = 1) a row, aiming for `goal`.
///
/// The goal is the caller's, not the caret's: a run of Down presses through a
/// short line and out the other side should come back to the column it
/// started in, and a caret that forgot would drift left one line at a time.
/// The window keeps it and clears it the moment anything else moves the
/// caret.
pub fn notepad_step_row(
    entry: &mut TextEntry,
    rows: &[NotepadRow],
    by: isize,
    goal: usize,
    select: bool,
) {
    let row = notepad_row_of(rows, entry.caret());
    let wanted = row as isize + by;
    if wanted < 0 || wanted as usize >= rows.len() {
        // Nothing rather than wrapping round: a caret that jumped from the
        // top of the page to the bottom would be a caret nobody could follow.
        return;
    }
    let at = notepad_index_of(entry.text(), rows, wanted as usize, goal);
    entry.place(at, select);
}

/// The start of the caret's own row.
pub fn notepad_line_home(entry: &mut TextEntry, rows: &[NotepadRow], select: bool) {
    let row = notepad_row_of(rows, entry.caret());
    let at = rows.get(row).map_or(0, |row| row.from);
    entry.place(at, select);
}

/// The end of it.
pub fn notepad_line_end(entry: &mut TextEntry, rows: &[NotepadRow], select: bool) {
    let row = notepad_row_of(rows, entry.caret());
    let at = rows
        .get(row)
        .map_or_else(|| entry.text().len(), |row| row.to);
    entry.place(at, select);
}

/// A screenful up or down, which is what Page Up and Page Down mean inside
/// one page of the pad. Turning to the *next page* is Ctrl and these, and is
/// the window's business rather than the caret's.
pub fn notepad_step_page(
    entry: &mut TextEntry,
    rows: &[NotepadRow],
    lines: usize,
    down: bool,
    goal: usize,
    select: bool,
) {
    let by = lines.max(1) as isize;
    notepad_step_row(entry, rows, if down { by } else { -by }, goal, select);
}

// ------------------------------------------------- the pointer and the view

/// Where a click at `x`, `y` puts the caret, with `scroll` rows hidden above.
pub fn notepad_index_at(
    layout: &NotepadLayout,
    text: &str,
    rows: &[NotepadRow],
    scroll: usize,
    x: f32,
    y: f32,
) -> usize {
    if rows.is_empty() {
        return 0;
    }
    let down = ((y - layout.text.y) / layout.line_height).floor();
    let row = (scroll as f32 + down.max(0.0)) as usize;
    let row = row.min(rows.len() - 1);
    // Half a character's grace: a click on the right-hand half of a letter
    // means after it, which is what every text field does and what makes
    // clicking at the end of a word land after the word.
    let across = ((x - layout.text.x) / layout.advance + 0.5)
        .floor()
        .max(0.0) as usize;
    notepad_index_of(text, rows, row, across)
}

/// The first row on screen, so that `caret_row` is on it — moving by the
/// least that gets it there.
///
/// The same rule the preset list follows: a caret already in sight scrolls
/// nothing, and one that has gone off the bottom comes back onto the last
/// line rather than into the middle.
pub fn notepad_scroll_to(caret_row: usize, lines: usize, scroll: usize) -> usize {
    if lines == 0 {
        return 0;
    }
    if caret_row < scroll {
        return caret_row;
    }
    if caret_row >= scroll + lines {
        return caret_row + 1 - lines;
    }
    scroll
}
