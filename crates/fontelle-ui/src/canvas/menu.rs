//! A right-click menu: a list of things you can do to whatever was clicked.
//!
//! The window had three menus already — the roll's lane property chip, the
//! rack's route chip, the mixer's effect list — and each was written against
//! the one enum it lists. That was fine while a menu belonged to a chip you
//! could see; it stops being fine the moment the *right button* has to open one
//! anywhere, which is what these were asked for:
//!
//! - *"theres no way to actually edit arrangement rows right now like i made
//!   one i dont want but i cant right click and delete it."*
//! - *"stuff like being able to right click and duplicate too, for instruments
//!   in the channel rack for example."*
//! - *"right click on a knob and select create automation clip with value."*
//!
//! So this one lists **strings**, and what each entry does is the caller's to
//! remember. It is geometry and hit-testing and nothing else — pure, like every
//! other piece of layout in this crate, which is what lets "it does not hang off
//! the bottom of the window" be a test rather than something to notice by
//! looking at it once.

use crate::layout::Rect;
use crate::theme::Metrics;

/// One line of a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub label: String,
    /// A greyed row: it says what would be there, and does nothing. *"Delete
    /// lane"* on the last lane left is the case this is for — a menu that
    /// hides the entry teaches nothing about why it is not there.
    pub enabled: bool,
    /// Draw a rule above this entry. What keeps *"Delete"* from sitting flush
    /// against the thing you meant to click.
    pub separator: bool,
    /// A star at the row's right end, and whether it is lit.
    ///
    /// `None` for a row that is an action (*Delete*, *Rename*): there is
    /// nothing to star. `Some(false)` for a thing that could be a favourite
    /// and is not; `Some(true)` for one that is, which is also what draws the
    /// row lit — see [`MenuEntry::is_favorite`]. A press on the star is not a
    /// press on the row: [`context_menu_star_hit`] and [`context_menu_hit`]
    /// divide the row between them.
    pub star: Option<bool>,
}

impl MenuEntry {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            separator: false,
            star: None,
        }
    }

    pub fn disabled(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: false,
            separator: false,
            star: None,
        }
    }

    /// Gives this row a star, lit when `favorite`.
    pub fn starred(mut self, favorite: bool) -> Self {
        self.star = Some(favorite);
        self
    }

    /// Whether this row is a favourite — a lit star, and a highlighted row.
    pub fn is_favorite(&self) -> bool {
        self.star == Some(true)
    }

    /// Puts a rule above this entry.
    pub fn after_rule(mut self) -> Self {
        self.separator = true;
        self
    }
}

/// A laid-out menu, waiting for the click that dismisses or chooses.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextMenu {
    /// The whole panel — for the background, and for "did the click miss".
    pub frame: Rect,
    /// One rectangle per entry, in the order they were given.
    ///
    /// An entry scrolled out of sight has an **empty** rectangle. That is the
    /// one rule for which rows are showing, and both the renderer (which skips
    /// an empty row) and [`context_menu_hit`] (which cannot land on one) read
    /// it — so what is drawn and what can be clicked cannot disagree.
    pub rows: Vec<Rect>,
    pub entries: Vec<MenuEntry>,
    /// How far down the list has been scrolled, in pixels.
    scroll: f32,
    /// One row's height, and the height of all of them together. Kept so that
    /// scrolling is a function of the menu alone rather than a re-layout
    /// needing the caller to have kept the anchor and the bounds.
    row: f32,
    content: f32,
    /// How the rows are cut into columns: how many rows each column holds,
    /// and how wide one column is. One column is the ordinary menu; several
    /// is a list too long for the window laid out side by side, which is
    /// what the preset drop-down is (see [`context_menu_layout`]).
    per_column: usize,
    column_width: f32,
}

impl Default for ContextMenu {
    fn default() -> Self {
        Self {
            frame: Rect::ZERO,
            rows: Vec::new(),
            entries: Vec::new(),
            scroll: 0.0,
            row: 0.0,
            content: 0.0,
            per_column: 1,
            column_width: 0.0,
        }
    }
}

impl ContextMenu {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// How tall the rows are allowed to be, inside the frame.
    fn visible(&self) -> f32 {
        (self.frame.height - MENU_PAD * 2.0).max(0.0)
    }

    /// Whether there is more list than there is room for it.
    pub fn scrolls(&self) -> bool {
        self.max_scroll() > 0.0
    }

    /// How many columns the rows are laid out in. One for nearly every menu.
    pub fn columns(&self) -> usize {
        if self.entries.is_empty() {
            return 0;
        }
        self.entries.len().div_ceil(self.per_column.max(1))
    }

    /// Where the divide between column `index` and the one after it is — the
    /// rule the renderer draws. Empty for the last column and for a one-column
    /// menu.
    pub fn column_rule(&self, index: usize) -> Rect {
        if index + 1 >= self.columns() {
            return Rect::ZERO;
        }
        let x = self.frame.x + MENU_PAD + (index as f32 + 1.0) * (self.column_width + COLUMN_GAP)
            - COLUMN_GAP / 2.0;
        Rect::new(x - 0.5, self.frame.y + MENU_PAD, 1.0, self.visible()).intersection(&self.frame)
    }

    /// The scrollbar's thumb: where in the list the visible page is, drawn at
    /// the frame's right-hand edge. Empty for a menu that fits, because a
    /// thumb the length of its track says nothing.
    pub fn scrollbar(&self) -> Rect {
        let track = self.scrollbar_track();
        if track.is_empty() {
            return Rect::ZERO;
        }
        let height = self.thumb_height(&track);
        let travel = (track.height - height).max(0.0);
        let y = track.y + travel * (self.scroll / self.max_scroll()).clamp(0.0, 1.0);
        Rect::new(track.x, y, track.width, height).intersection(&self.frame)
    }

    /// The whole bar the thumb slides in — the thumb's travel, and what a
    /// press has to land in for a drag to start.
    ///
    /// Empty for a menu that fits, like the thumb.
    pub fn scrollbar_track(&self) -> Rect {
        if self.max_scroll() <= 0.0 || self.content <= 0.0 {
            return Rect::ZERO;
        }
        Rect::new(
            self.frame.right() - MENU_PAD - SCROLLBAR_WIDTH,
            self.frame.y + MENU_PAD,
            SCROLLBAR_WIDTH,
            self.visible(),
        )
    }

    /// How tall the thumb is: its share of the list, floored so that a very
    /// long list still leaves something to take hold of.
    fn thumb_height(&self, track: &Rect) -> f32 {
        let share = (track.height / self.content).clamp(0.0, 1.0);
        (track.height * share).max(THUMB_MIN).min(track.height)
    }

    /// Takes hold of the thumb at `(x, y)`, answering **how far down the
    /// thumb** the pointer took it — what [`drag_thumb`](Self::drag_thumb)
    /// needs to keep that point under the pointer for the rest of the drag.
    ///
    /// A press anywhere on the bar starts a drag, not only one on the thumb:
    /// the bar is four pixels wide and a control you have to aim at is a
    /// control that reads as broken. Off the bar — or on a menu that does not
    /// scroll — the answer is `None`, and the press means whatever it would
    /// have meant.
    pub fn thumb_grab(&self, x: f32, y: f32) -> Option<f32> {
        let track = self.scrollbar_track();
        if track.is_empty() || !track.contains(x, y) {
            return None;
        }
        let thumb = self.scrollbar();
        if thumb.contains(x, y) {
            Some(y - thumb.y)
        } else {
            // Beside the thumb: it comes to the pointer, taken by its middle,
            // and the drag carries on from there.
            Some(thumb.height / 2.0)
        }
    }

    /// Drags the thumb so that the point `grab` pixels down it sits at `y` —
    /// the number [`thumb_grab`](Self::thumb_grab) gave when the drag started.
    ///
    /// Clamped at both ends by [`scroll_by`](Self::scroll_by), so a pointer
    /// dragged past the frame parks the thumb against the end it is heading
    /// for rather than losing it.
    pub fn drag_thumb(&mut self, y: f32, grab: f32) {
        let track = self.scrollbar_track();
        if track.is_empty() {
            return;
        }
        let travel = (track.height - self.thumb_height(&track)).max(0.0);
        if travel <= 0.0 {
            return;
        }
        let wanted = ((y - grab) - track.y) / travel;
        let scroll = (wanted.clamp(0.0, 1.0) * self.max_scroll()) - self.scroll;
        self.scroll_by(scroll);
    }

    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// The furthest down it goes: the last entry resting on the bottom.
    ///
    /// Never past that, and never above the first: a menu scrolled past its
    /// end shows a page of nothing and reads as broken.
    pub fn max_scroll(&self) -> f32 {
        (self.content - self.visible()).max(0.0)
    }

    /// Where entry `index`'s star is: the right end of its row.
    ///
    /// Empty for a row with no star, for one scrolled out of sight (its row
    /// is empty too), and for an index that is not a row.
    pub fn star_rect(&self, index: usize) -> Rect {
        let (Some(row), Some(entry)) = (self.rows.get(index), self.entries.get(index)) else {
            return Rect::ZERO;
        };
        if entry.star.is_none() || row.is_empty() {
            return Rect::ZERO;
        }
        let width = STAR_WIDTH.min(row.width);
        Rect::new(row.right() - width, row.y, width, row.height)
    }

    /// Scrolls by `pixels`, positive downward, and puts the rows where that
    /// leaves them. A menu that fits does not move.
    pub fn scroll_by(&mut self, pixels: f32) {
        let wanted = if pixels.is_finite() {
            self.scroll + pixels
        } else {
            self.scroll
        };
        let scroll = wanted.clamp(0.0, self.max_scroll());
        if scroll == self.scroll {
            return;
        }
        self.scroll = scroll;
        self.rows = place_rows(
            self.frame,
            self.row,
            self.scroll,
            self.entries.len(),
            self.per_column,
            self.column_width,
            self.scrolls(),
        );
    }
}

/// Where each row sits at a given scroll, and which are showing at all.
///
/// A row is placed only when it fits **whole**: half a caption sliding under
/// the frame's edge is a row you cannot read and should not be able to click.
///
/// Row `index` is in column `index / per_column`, `index % per_column` down
/// it. A menu that scrolls keeps its right-hand edge for the thumb, so a
/// star at the end of a row is not under it.
fn place_rows(
    frame: Rect,
    row: f32,
    scroll: f32,
    count: usize,
    per_column: usize,
    column_width: f32,
    scrolls: bool,
) -> Vec<Rect> {
    let top = frame.y + MENU_PAD;
    let room = (frame.height - MENU_PAD * 2.0).max(0.0);
    let per_column = per_column.max(1);
    let reserved = if scrolls { SCROLLBAR_WIDTH + 2.0 } else { 0.0 };
    let width = (column_width - reserved).max(0.0);
    (0..count)
        .map(|index| {
            let (column, slot) = (index / per_column, index % per_column);
            let x = frame.x + MENU_PAD + column as f32 * (column_width + COLUMN_GAP);
            let y = top + slot as f32 * row - scroll;
            if y < top - 0.01 || y + row > top + room + 0.01 {
                Rect::ZERO
            } else {
                Rect::new(x, y, width, row).clamped()
            }
        })
        .collect()
}

/// Whether a menu entry survives what has been typed into the picker.
///
/// > *"its still not seeing my plugins"* — with three hundred and fifty-seven
/// > effects installed, a menu you can only scroll is sixteen screens deep.
///
/// A plain case-insensitive substring, not a fuzzy match: at this many
/// entries, a hit nobody can explain is worse than a miss they can fix by
/// typing something else. An empty query matches everything, which is what an
/// unfiltered menu is.
pub fn menu_matches(label: &str, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    label.to_lowercase().contains(&query.to_lowercase())
}

/// A little air around the rows, and how far the caption is indented.
const MENU_PAD: f32 = 3.0;
pub const MENU_TEXT_INSET: f32 = 8.0;

/// Between two columns of a menu laid out in several — room for the rule.
const COLUMN_GAP: f32 = 8.0;

/// The scrollbar's thumb, and the least it shrinks to.
const SCROLLBAR_WIDTH: f32 = 4.0;
const THUMB_MIN: f32 = 18.0;

/// How much of a row's right end is its star, when it has one.
///
/// A little wider than the glyph, because it is a target: a star the width
/// of its own strokes is a star you miss and a row you chose by accident.
pub const STAR_WIDTH: f32 = 20.0;

/// How wide a character is taken to be, as a fraction of the font size.
///
/// The canvas may not shape text — that needs a `TextContext`, which lives on
/// the window — so a menu wide enough for its longest caption has to be
/// *estimated*. Six tenths is a little wider than a proportional sans at this
/// size, which errs the right way: a menu with room to spare looks like a menu,
/// and one two pixels short of its longest entry looks broken.
const CHAR_WIDTH: f32 = 0.6;

/// The narrowest a menu may be, whatever is in it.
const MIN_WIDTH: f32 = 120.0;

/// Drops a menu from `at`, kept inside `bounds`.
///
/// Down and to the right of the pointer by preference — that is where every
/// desktop puts one, and it keeps the thing you right-clicked visible. Against
/// the other edge when there is no room, and never outside `bounds`: a menu
/// half off the window is a menu with entries nobody can reach.
pub fn context_menu_layout(
    at: (f32, f32),
    bounds: Rect,
    metrics: &Metrics,
    font_size: f32,
    entries: Vec<MenuEntry>,
) -> ContextMenu {
    let row = metrics.row_height.max(1.0);
    let longest = entries
        .iter()
        .map(|entry| entry.label.chars().count())
        .max()
        .unwrap_or(0) as f32;
    // Room for the star after the longest caption, when any row has one —
    // or a long name runs under its own star.
    let stars = if entries.iter().any(|entry| entry.star.is_some()) {
        STAR_WIDTH
    } else {
        0.0
    };
    let width = (longest * font_size * CHAR_WIDTH + MENU_TEXT_INSET * 2.0 + stars)
        .max(MIN_WIDTH)
        .min(bounds.width);
    // As tall as its rows, or as tall as there is room for — whichever is
    // less. **A menu longer than the window scrolls; it does not vanish.**
    // It used to vanish, and with 357 plugin effects installed that made
    // *"Plugin…"* a row that did nothing at all, which is indistinguishable
    // from a program that cannot see the plugins. Ty's report.
    //
    // And between fitting and scrolling there is a third shape: **columns**.
    // A hundred and twenty-eight presets under fourteen headings is a column
    // three thousand pixels tall, of which a window showed the first thirty
    // and hinted at nothing. When the rows will not fit in one column and
    // will in several side by side, they go side by side — the way every big
    // menu in every DAW is drawn — and only a list too long even for that
    // scrolls.
    let count = entries.len();
    if count == 0 || bounds.is_empty() || width <= 0.0 {
        return ContextMenu::default();
    }
    let column_width = (width - MENU_PAD * 2.0).max(0.0);
    let fit_rows = (((bounds.height - MENU_PAD * 2.0) / row).floor() as usize).max(1);
    let (per_column, columns) = if count <= fit_rows {
        (count, 1)
    } else {
        let wanted = count.div_ceil(fit_rows);
        let total =
            wanted as f32 * column_width + (wanted - 1) as f32 * COLUMN_GAP + MENU_PAD * 2.0;
        if total <= bounds.width {
            // Balanced, so the last column is not three rows under a column
            // of thirty.
            (count.div_ceil(wanted), wanted)
        } else {
            (count, 1)
        }
    };
    let content = row * per_column as f32;
    let height = (content + MENU_PAD * 2.0).min(bounds.height);
    let width = if columns > 1 {
        columns as f32 * column_width + (columns - 1) as f32 * COLUMN_GAP + MENU_PAD * 2.0
    } else {
        width
    };
    if height < row + MENU_PAD * 2.0 {
        // Nowhere to put even one row. An empty menu hit-tests as absent and
        // draws as nothing, which is better than a sliver with no whole
        // caption in it.
        return ContextMenu::default();
    }

    let x = if at.0 + width <= bounds.right() {
        at.0
    } else {
        (at.0 - width).max(bounds.x)
    };
    let y = if at.1 + height <= bounds.bottom() {
        at.1
    } else {
        (at.1 - height).max(bounds.y)
    };
    let x = x.clamp(bounds.x, (bounds.right() - width).max(bounds.x));
    let y = y.clamp(bounds.y, (bounds.bottom() - height).max(bounds.y));
    let frame = Rect::new(x, y, width, height).intersection(&bounds);

    let scrolls = content > (frame.height - MENU_PAD * 2.0) + 0.01;
    let rows = place_rows(frame, row, 0.0, count, per_column, column_width, scrolls);

    ContextMenu {
        frame,
        rows,
        entries,
        scroll: 0.0,
        row,
        content,
        per_column,
        column_width,
    }
}

/// Which entry `(x, y)` is on, if it is on one that can be chosen.
///
/// A greyed row hit-tests as **nothing**, not as itself: it is there to say why
/// something is missing, and a press on it must neither act nor be mistaken for
/// a press that missed the menu — which is why the caller asks `frame.contains`
/// separately to decide whether the menu closes.
///
/// And a press on a row's **star** is not a press on the row: that is
/// [`context_menu_star_hit`]'s, and a menu that added the reverb when you
/// meant to star it would be a menu that closed on the wrong answer.
pub fn context_menu_hit(menu: &ContextMenu, x: f32, y: f32) -> Option<usize> {
    menu.rows
        .iter()
        .zip(menu.entries.iter())
        .enumerate()
        .position(|(index, (row, entry))| {
            entry.enabled && row.contains(x, y) && !menu.star_rect(index).contains(x, y)
        })
}

/// Which entry's star `(x, y)` is on, if it is on one.
///
/// A greyed row's star still answers — the kind a channel already is can be
/// a favourite, and un-starring it is not choosing it.
pub fn context_menu_star_hit(menu: &ContextMenu, x: f32, y: f32) -> Option<usize> {
    (0..menu.entries.len()).find(|index| menu.star_rect(*index).contains(x, y))
}

/// What is written in front of the thing you already have.
///
/// The same mark the record button's mode menu uses, because it means the same
/// thing in both places.
pub const CHOSEN_MARK: &str = "\u{2713} ";

/// What is written after a row that asks a second question.
pub(crate) const ASKS_MORE: &str = "\u{2026}";

/// The instrument kinds, as a menu, with nothing starred and no plugins
/// installed — [`super::instrument_menu_rows`] with empty lists, and the
/// one place the rules of that menu are written down.
pub fn instrument_menu_entries(current: Option<fontelle_types::InstrumentKind>) -> Vec<MenuEntry> {
    super::instrument_menu_rows(current, &[], &[])
        .into_iter()
        .map(|(entry, _)| entry)
        .collect()
}

/// A mixer strip's input menu (TDD §15.4): *No input*, then every input
/// the machine has, with the one the strip already records from greyed.
///
/// `inputs` is the list **as it was when the menu opened**, and
/// [`input_menu_choice`] reads the chosen row against the same list — the
/// list used to be asked for again when the row was chosen, and a
/// microphone plugged in or unplugged between the two made the row mean a
/// different device than the one written on it.
pub fn input_menu_entries(current: Option<&str>, inputs: &[String]) -> Vec<MenuEntry> {
    let mut entries = vec![if current.is_none() {
        MenuEntry::disabled("No input")
    } else {
        MenuEntry::new("No input")
    }];
    if inputs.is_empty() {
        entries.push(MenuEntry::disabled("nothing to record from").after_rule());
    }
    for name in inputs {
        entries.push(if current == Some(name.as_str()) {
            MenuEntry::disabled(name)
        } else {
            MenuEntry::new(name)
        });
    }
    entries
}

/// What row `index` of [`input_menu_entries`] means: `Some(None)` clears the
/// input, `Some(Some(name))` chooses one, and `None` is a row that chooses
/// nothing — the *nothing to record from* line, or a row past the end.
pub fn input_menu_choice(inputs: &[String], index: usize) -> Option<Option<String>> {
    match index.checked_sub(1) {
        None => Some(None),
        Some(n) => inputs.get(n).cloned().map(Some),
    }
}

/// The caret drawn after a name being typed.
///
/// A block rather than a bar, because the menu's text is laid out once per
/// keystroke and a blinking caret would want a frame clock the menu does not
/// have. It is there to say *this is listening*, which a static one says.
pub const NAME_CARET: &str = "\u{2588}";

/// The most of a long name the prompt shows.
///
/// Names are not usually long. One that is would otherwise lay a menu out
/// wider than the window, and the end is the half worth seeing — it is where
/// the next letter lands.
const NAME_SHOWN: usize = 40;

/// A menu that asks for a name: a heading carrying what has been typed, and
/// one row to press.
///
/// A menu rather than a dialog of its own, because everything a prompt needs
/// already exists here — it lays out, it draws, it hit-tests, it takes the
/// keyboard while it is open (see the plugin picker, which filters as you
/// type). A second modal shape would be a second set of all of that.
///
/// `title` is what the name is *for*; `typed` is what has been typed so far.
pub fn name_prompt_entries(title: &str, typed: &str) -> Vec<MenuEntry> {
    let shown: String = if typed.chars().count() > NAME_SHOWN {
        let skipped = typed.chars().count() - NAME_SHOWN;
        format!(
            "\u{2026}{}",
            typed.chars().skip(skipped).collect::<String>()
        )
    } else {
        typed.to_string()
    };
    vec![
        MenuEntry::disabled(if typed.is_empty() {
            format!("{title} \u{2014} type a name{NAME_CARET}")
        } else {
            format!("{title} \u{2014} {shown}{NAME_CARET}")
        }),
        MenuEntry::new(if typed.trim().is_empty() {
            "Untitled".to_string()
        } else {
            shown
        }),
    ]
}
