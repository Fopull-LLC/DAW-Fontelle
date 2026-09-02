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
}

impl MenuEntry {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            separator: false,
        }
    }

    pub fn disabled(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            enabled: false,
            separator: false,
        }
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
    pub rows: Vec<Rect>,
    pub entries: Vec<MenuEntry>,
}

impl Default for ContextMenu {
    fn default() -> Self {
        Self {
            frame: Rect::ZERO,
            rows: Vec::new(),
            entries: Vec::new(),
        }
    }
}

impl ContextMenu {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// A little air around the rows, and how far the caption is indented.
const MENU_PAD: f32 = 3.0;
pub const MENU_TEXT_INSET: f32 = 8.0;

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
    let width = (longest * font_size * CHAR_WIDTH + MENU_TEXT_INSET * 2.0)
        .max(MIN_WIDTH)
        .min(bounds.width);
    let height = row * entries.len() as f32 + MENU_PAD * 2.0;
    if entries.is_empty() || bounds.is_empty() || width <= 0.0 || height > bounds.height {
        // Nowhere to put it. An empty menu hit-tests as absent and draws as
        // nothing, which is better than a sliver listing two of six things.
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

    let rows = (0..entries.len())
        .map(|index| {
            Rect::new(
                frame.x + MENU_PAD,
                frame.y + MENU_PAD + index as f32 * row,
                (frame.width - MENU_PAD * 2.0).max(0.0),
                row,
            )
            .clamped()
        })
        .collect();

    ContextMenu {
        frame,
        rows,
        entries,
    }
}

/// Which entry `(x, y)` is on, if it is on one that can be chosen.
///
/// A greyed row hit-tests as **nothing**, not as itself: it is there to say why
/// something is missing, and a press on it must neither act nor be mistaken for
/// a press that missed the menu — which is why the caller asks `frame.contains`
/// separately to decide whether the menu closes.
pub fn context_menu_hit(menu: &ContextMenu, x: f32, y: f32) -> Option<usize> {
    menu.rows
        .iter()
        .zip(menu.entries.iter())
        .position(|(row, entry)| entry.enabled && row.contains(x, y))
}
