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
    /// What each page is **about** — its first line, or `None` for a blank
    /// one (`NotepadPages::caption`). One short string per page, so the
    /// pages menu can say "chorus" instead of "3".
    pub captions: Vec<Option<String>>,
}

impl NotepadView {
    /// What the footer's page counter reads — counted from one, because that
    /// is how a person counts pages.
    pub fn page_label(&self) -> String {
        format!("page {} / {}", self.page + 1, self.pages.max(1))
    }

    /// What one row of the pages menu says: the number, and what the page is
    /// about when it is about anything.
    ///
    /// Here rather than in the window so that the menu and anything else that
    /// lists the pages cannot word them differently.
    pub fn page_row(&self, page: usize) -> String {
        match self.captions.get(page).and_then(Option::as_ref) {
            Some(caption) => format!("{}  \u{b7}  {caption}", page + 1),
            None => format!("{}  \u{b7}  {NOTEPAD_BLANK}", page + 1),
        }
    }
}

/// What the footer says while the pad has the keyboard.
///
/// Short, because it shares a row with the page controls — and it names the
/// way *out*, which is the only part anybody has to be told.
pub const NOTEPAD_HINT: &str = "Esc gives the keyboard back";

/// What the pages menu calls a page with nothing written on it.
///
/// A word rather than an empty row: a list with a gap in it reads as a list
/// that failed to load.
pub const NOTEPAD_BLANK: &str = "blank";

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
    /// Where the footer says how to give the keyboard back, in the room left
    /// between the page controls and the chips. Empty when there is none —
    /// a hint drawn over a control is worse than no hint.
    pub hint: Rect,
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
            hint: Rect::ZERO,
            line_height: 1.0,
            advance: 1.0,
            lines: 1,
            columns: 1,
        }
    }
}

/// The air each side of the footer's hint, and the least room worth drawing
/// it in — under this it says nothing rather than half a word.
const HINT_GAP: f32 = 10.0;
const HINT_MIN: f32 = 120.0;

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
    // Whatever is left in the middle, if it is enough to read a few words in.
    let gap = size_chip.x - HINT_GAP - (remove.right() + HINT_GAP);
    let hint = if gap >= HINT_MIN {
        row(remove.right() + HINT_GAP, gap)
    } else {
        Rect::ZERO
    };

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
        hint,
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
    /// The page counter, which drops down the list of pages by what each one
    /// is about — walking left and right is for the page next door.
    Pages,
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
        (layout.count, NotepadHit::Pages),
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

/// How wide the mark down the sheet's edge is, and how far in it sits.
const SCROLLBAR: f32 = 3.0;
const SCROLLBAR_GAP: f32 = 3.0;
/// The shortest the thumb is ever drawn. A page four hundred screens long
/// would otherwise mark its position with nothing.
const SCROLLBAR_MIN: f32 = 8.0;

/// Where the mark down the right-hand edge of the sheet goes, or `None` when
/// the whole page fits and there is nothing to say.
///
/// A **mark, not a control**: it says where in the page you are, and the
/// wheel and the caret are what move it. The one thing in this window that
/// cannot be clicked.
pub fn notepad_scrollbar(layout: &NotepadLayout, rows: usize, scroll: usize) -> Option<Rect> {
    if rows <= layout.lines || layout.text.is_empty() {
        return None;
    }
    let track = layout.text.height;
    let shown = layout.lines as f32 / rows as f32;
    let height = (track * shown).max(SCROLLBAR_MIN).min(track);
    // Against the rows that can be *scrolled past*, so the thumb reaches the
    // bottom exactly when the last line is on screen.
    let most = rows.saturating_sub(layout.lines).max(1) as f32;
    let along = (scroll as f32 / most).clamp(0.0, 1.0);
    let y = layout.text.y + along * (track - height);
    Some(Rect::new(
        layout.sheet.right() - SCROLLBAR_GAP - SCROLLBAR,
        y,
        SCROLLBAR,
        height,
    ))
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

// -------------------------------------------------------------- editing ---
//
// > *"add ability to select characters copy paste things and entire lines
// > all the things youd expect for a full text editor thats fast and quick
// > and easy to use"* (2026-10-18)
//
// What a pad does that a one-line field does not: words and lines selected
// by a click, whole lines copied, cut, duplicated, moved and indented, and a
// paste that keeps its line breaks. Lines here are the text's own — split on
// `\n` — not the rows word wrap draws: a line moved by Alt+Up is the line you
// wrote, however wide the window is.

/// Where the line holding byte `at` begins.
fn line_start(text: &str, at: usize) -> usize {
    text[..at.min(text.len())].rfind('\n').map_or(0, |i| i + 1)
}

/// Where the line holding byte `at` ends, before its newline.
fn line_end(text: &str, at: usize) -> usize {
    let at = at.min(text.len());
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

/// The lines the selection touches — or the caret's line — as the start of
/// the first and the end of the last, before its newline. A selection that
/// ends at the very start of a line does not take that line in.
fn lines_span(entry: &TextEntry) -> (usize, usize) {
    let text = entry.text();
    let (from, to) = entry.selection().unwrap_or((entry.caret(), entry.caret()));
    let last = if to > from && text[..to].ends_with('\n') {
        to - 1
    } else {
        to
    };
    (line_start(text, from), line_end(text, last))
}

/// The anchor and caret, so text can be swapped and the selection put back
/// facing the same way.
fn ends(entry: &TextEntry) -> (usize, usize) {
    match entry.selection() {
        Some((from, to)) if entry.caret() == from => (to, from),
        Some((from, to)) => (from, to),
        None => (entry.caret(), entry.caret()),
    }
}

/// `entry` holding `text`, its selection put back at `anchor`..`caret`.
fn replace(entry: &mut TextEntry, text: String, anchor: usize, caret: usize) {
    entry.set(text);
    entry.place(anchor, false);
    if caret != anchor {
        entry.place(caret, true);
    }
}

/// A double-click: the word under byte `at` — or the run of spaces, if that
/// is what is there. At the end of a line, the word before it.
pub fn notepad_select_word(entry: &mut TextEntry, at: usize) {
    let text = entry.text();
    let at = at.min(text.len());
    let probe = if at == text.len() || text[at..].starts_with('\n') {
        text[..at]
            .chars()
            .next_back()
            .map_or(at, |c| at - c.len_utf8())
    } else {
        at
    };
    let Some(here) = text[probe..].chars().next() else {
        return;
    };
    if here == '\n' {
        entry.place(at, false);
        return;
    }
    let space = here.is_whitespace();
    let same = |c: char| c != '\n' && c.is_whitespace() == space;
    let mut from = probe;
    while let Some(c) = text[..from].chars().next_back() {
        if !same(c) {
            break;
        }
        from -= c.len_utf8();
    }
    let mut to = probe;
    while let Some(c) = text[to..].chars().next() {
        if !same(c) {
            break;
        }
        to += c.len_utf8();
    }
    entry.place(from, false);
    entry.place(to, true);
}

/// A triple-click, or Ctrl+L: the whole line under byte `at`, with its
/// newline. With `more` and a selection already made, the next line as well
/// — Ctrl+L again.
pub fn notepad_select_line(entry: &mut TextEntry, at: usize, more: bool) {
    let text = entry.text();
    let take_newline = |end: usize| if end < text.len() { end + 1 } else { end };
    if more && let Some((from, to)) = entry.selection() {
        let end = take_newline(line_end(text, to));
        entry.place(line_start(text, from), false);
        entry.place(end, true);
        return;
    }
    let start = line_start(text, at);
    let end = take_newline(line_end(text, at));
    entry.place(start, false);
    entry.place(end, true);
}

/// What Ctrl+C takes: the selection, or with nothing selected the whole line
/// the caret is on — always ending in a newline, so a paste puts it on a line
/// of its own.
pub fn notepad_copy_text(entry: &TextEntry) -> String {
    if entry.selection().is_some_and(|(from, to)| to > from) {
        return entry.selected_text().to_string();
    }
    let text = entry.text();
    let (start, end) = (
        line_start(text, entry.caret()),
        line_end(text, entry.caret()),
    );
    format!("{}\n", &text[start..end])
}

/// Ctrl+X: [`notepad_copy_text`]'s text, taken out.
pub fn notepad_cut(entry: &mut TextEntry) -> String {
    let taken = notepad_copy_text(entry);
    if !entry.delete_selection() {
        notepad_delete_line(entry);
    }
    taken
}

/// What a paste into the pad puts down: every line of it. Line endings of
/// any system become `\n`; a tab is four spaces on the fixed grid, and any
/// other control character goes.
pub fn notepad_paste_text(raw: &str) -> String {
    raw.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ")
        .chars()
        .filter(|c| *c == '\n' || !c.is_control())
        .collect()
}

/// Ctrl+Shift+K: the lines the caret or selection are on, gone. The caret
/// lands where they were.
pub fn notepad_delete_line(entry: &mut TextEntry) {
    let text = entry.text().to_string();
    let (start, end) = lines_span(entry);
    let (from, to) = if end < text.len() {
        (start, end + 1)
    } else if start > 0 {
        // The last line: the newline before it goes with it.
        (start - 1, end)
    } else {
        (start, end)
    };
    let mut kept = text[..from].to_string();
    kept.push_str(&text[to..]);
    let caret = from.min(kept.len());
    replace(entry, kept, caret, caret);
}

/// Ctrl+D: a copy of the lines under the caret or selection, underneath
/// them, with the caret — and selection — moved onto the copy.
pub fn notepad_duplicate_line(entry: &mut TextEntry) {
    let text = entry.text().to_string();
    let (start, end) = lines_span(entry);
    let block = &text[start..end];
    let mut out = text[..end].to_string();
    out.push('\n');
    out.push_str(block);
    out.push_str(&text[end..]);
    let by = block.len() + 1;
    let (anchor, caret) = ends(entry);
    replace(entry, out, anchor + by, caret + by);
}

/// Alt+Up and Alt+Down: the lines under the caret or selection swapped with
/// the line above (`by` < 0) or below. False at the top or bottom.
pub fn notepad_move_lines(entry: &mut TextEntry, by: i32) -> bool {
    let text = entry.text().to_string();
    let (start, end) = lines_span(entry);
    let block = &text[start..end];
    let (anchor, caret) = ends(entry);
    if by < 0 {
        if start == 0 {
            return false;
        }
        let above = line_start(&text, start - 1);
        let line = &text[above..start - 1];
        let out = format!("{}{block}\n{line}{}", &text[..above], &text[end..]);
        let shift = line.len() + 1;
        replace(entry, out, anchor - shift, caret - shift);
    } else {
        if end >= text.len() {
            return false;
        }
        let below_end = line_end(&text, end + 1);
        let line = &text[end + 1..below_end];
        let out = format!("{}{line}\n{block}{}", &text[..start], &text[below_end..]);
        let shift = line.len() + 1;
        replace(entry, out, anchor + shift, caret + shift);
    }
    true
}

/// How many spaces Tab puts in, and Shift+Tab takes out.
const INDENT: &str = "    ";

/// Tab: four spaces at the caret — or, with a selection over more than one
/// line, four at the head of each of them, the selection kept.
pub fn notepad_indent(entry: &mut TextEntry) {
    let multi = entry
        .selection()
        .is_some_and(|_| entry.selected_text().contains('\n'));
    if !multi {
        entry.insert(INDENT);
        return;
    }
    let text = entry.text().to_string();
    let (start, end) = lines_span(entry);
    let (anchor, caret) = ends(entry);
    let mut out = text[..start].to_string();
    let mut added = 0;
    let (mut new_anchor, mut new_caret) = (anchor, caret);
    let mut line_head = start;
    for line in text[start..end].split('\n') {
        out.push_str(INDENT);
        added += INDENT.len();
        if anchor >= line_head {
            new_anchor = anchor + added;
        }
        if caret >= line_head {
            new_caret = caret + added;
        }
        out.push_str(line);
        out.push('\n');
        line_head += line.len() + 1;
    }
    out.pop();
    out.push_str(&text[end..]);
    replace(entry, out, new_anchor, new_caret);
}

/// Shift+Tab: up to four spaces taken from the head of each line under the
/// caret or selection.
pub fn notepad_outdent(entry: &mut TextEntry) {
    let text = entry.text().to_string();
    let (start, end) = lines_span(entry);
    let (anchor, caret) = ends(entry);
    let mut out = text[..start].to_string();
    let (mut new_anchor, mut new_caret) = (anchor, caret);
    let mut line_head = start;
    let mut removed_before = 0usize;
    for line in text[start..end].split('\n') {
        let cut = line.len() - line.trim_start_matches(' ').len();
        let cut = cut.min(INDENT.len());
        let shift = |at: usize| -> usize {
            if at >= line_head + cut {
                at - removed_before - cut
            } else if at >= line_head {
                line_head - removed_before
            } else {
                at
            }
        };
        if anchor >= line_head {
            new_anchor = shift(anchor);
        }
        if caret >= line_head {
            new_caret = shift(caret);
        }
        out.push_str(&line[cut..]);
        out.push('\n');
        removed_before += cut;
        line_head += line.len() + 1;
    }
    out.pop();
    out.push_str(&text[end..]);
    replace(entry, out, new_anchor, new_caret);
}

/// Ctrl+Backspace and Ctrl+Delete: back to where Ctrl+Left lands, or on to
/// where Ctrl+Right does. A selection is deleted as it is.
pub fn notepad_delete_word(entry: &mut TextEntry, forward: bool) {
    if entry.delete_selection() {
        return;
    }
    if forward {
        entry.right(true, true);
    } else {
        entry.left(true, true);
    }
    entry.delete_selection();
}
