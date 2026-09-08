//! The prefab list (TDD §10.5) — the panel's other tab.
//!
//! > *"the prefab tab should be where the channel rack is can be tabbed
//! > between instruments and prefabs which just shows a list of all of the ones
//! > you have and you can scroll down them all and a plus icon to make a new
//! > one and name it and stuff."* — Ty
//!
//! What prefabs the project has, how many places each is drawn in, and which
//! one the piano roll is editing. It is the channel rack's shape on purpose:
//! the same tab strip (shared, from [`crate::canvas::tab_strip`]), the same
//! whole-rows rule, the same pinned add button, the same virtualisation. Two
//! lists in one panel that behaved differently would be two lists you have to
//! learn twice.
//!
//! Geometry and hit-testing only, and both pure, for the reason §2.5 gives.

use crate::canvas::rack::{tab_at, tab_strip};
use crate::document::RackTab;
use crate::layout::Rect;
use crate::theme::Metrics;

/// One prefab's row, and what is inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrefabRow {
    /// Which prefab this is, counted from the top of the project's list — not
    /// from the top of the panel, which is what makes scrolling work.
    pub index: usize,
    pub frame: Rect,
    /// Where the name is written, and the part you click to open it in the
    /// roll.
    pub name: Rect,
    /// The count of places it is drawn in, at the right-hand end.
    ///
    /// A read-out and not a control. It is what makes "delete this" a decision
    /// somebody can make rather than a guess: a prefab used nowhere is a
    /// scratch idea, and one used eleven times is the song.
    pub uses: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrefabLayout {
    pub body: Rect,
    /// The strip that switches back to the instruments.
    pub tabs: Vec<(RackTab, Rect)>,
    /// Where the rows go — the panel above the add button. Its own rectangle
    /// because hit-testing asks "is the pointer in the list" before it asks
    /// "which row", and because the renderer clips to it.
    pub list: Rect,
    pub rows: Vec<PrefabRow>,
    /// "Make a prefab" — the plus icon. Pinned to the bottom rather than
    /// placed after the last row: with no prefabs at all it is the only thing
    /// worth pressing, and with two hundred it must not have scrolled away.
    pub add: Rect,
    pub total: usize,
    pub scroll: usize,
    /// How many rows the list has **room** for, whatever it is showing — the
    /// number a scroll offset is clamped against. See `RackLayout::capacity`
    /// for why this is not `rows.len()`.
    pub capacity: usize,
}

/// How wide the uses count is. Three digits and a little air: a prefab drawn
/// in more than a few hundred places is a prefab, not a read-out problem.
const USES: f32 = 26.0;

/// Lays `count` prefabs out in `body`, starting from `scroll`.
pub fn prefab_layout(body: Rect, metrics: &Metrics, count: usize, scroll: usize) -> PrefabLayout {
    let (tabs, body) = tab_strip(body, metrics);
    let add_height = metrics.row_height.min(body.height.max(0.0));
    let add = Rect::new(
        body.x,
        (body.bottom() - add_height).max(body.y),
        body.width,
        add_height,
    )
    .clamped();

    // What is left above the button is the list's, rounded down to a whole
    // number of rows for the reason `rack::rack_layout` gives.
    let room = (add.y - body.y - 2.0).max(0.0);
    let height = if metrics.row_height > 0.0 {
        (room / metrics.row_height).floor() * metrics.row_height
    } else {
        room
    };
    let list = Rect::new(body.x, body.y, body.width, height).clamped();

    let mut rows = Vec::new();
    let mut capacity = 0;
    if metrics.row_height > 0.0 && !list.is_empty() {
        let visible = (list.height / metrics.row_height).floor() as usize;
        capacity = visible;
        // Clamped the way the rack clamps it: a list scrolled off its own end
        // shows the last row rather than going blank.
        let scroll = scroll.min(count.saturating_sub(1));
        for (slot, index) in (scroll..count).take(visible).enumerate() {
            let frame = Rect::new(
                list.x,
                list.y + slot as f32 * metrics.row_height,
                list.width,
                metrics.row_height,
            );
            let uses = Rect::new(
                frame.right() - USES - 2.0,
                frame.y + 2.0,
                USES,
                (frame.height - 4.0).max(0.0),
            )
            .clamped();
            let name = Rect::new(
                frame.x,
                frame.y,
                (uses.x - frame.x - 2.0).max(0.0),
                frame.height,
            )
            .clamped();
            rows.push(PrefabRow {
                index,
                frame,
                name,
                uses,
            });
        }
    }

    PrefabLayout {
        body,
        tabs,
        list,
        rows,
        add,
        total: count,
        scroll,
        capacity,
    }
}

/// What is under the pointer in the prefab list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefabHit {
    /// Open this prefab in the roll — *"selecting the prefab in the prefab
    /// menu"*.
    Row(usize),
    /// Make one.
    Add,
    /// Show the panel's other list.
    Tab(RackTab),
    Nothing,
}

impl PrefabHit {
    /// What a hover tip says (see [`crate::tooltip`]).
    ///
    /// `None` for a row, which carries the prefab's own name and needs no
    /// gloss.
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Add => "Make a prefab",
            Self::Tab(tab) => match tab {
                RackTab::Instruments => "The instruments in this project",
                RackTab::Prefabs => "Content you can draw in more than one place",
            },
            Self::Row(_) | Self::Nothing => return None,
        })
    }
}

pub fn prefab_hit(layout: &PrefabLayout, x: f32, y: f32) -> PrefabHit {
    // The strip first, for the reason `rack_hit` takes it first: it is inside
    // the panel, and a list that claimed the whole of it would swallow the
    // only way out.
    if let Some(tab) = tab_at(&layout.tabs, x, y) {
        return PrefabHit::Tab(tab);
    }
    if layout.add.contains(x, y) {
        return PrefabHit::Add;
    }
    if !layout.list.contains(x, y) {
        return PrefabHit::Nothing;
    }
    for row in &layout.rows {
        if row.frame.contains(x, y) {
            return PrefabHit::Row(row.index);
        }
    }
    PrefabHit::Nothing
}

/// The scroll offset that keeps `index` on screen, given where the list is now.
///
/// The rack's [`scroll_to_show`](crate::canvas::scroll_to_show), for this list:
/// called when the selection moves by something other than a click — a prefab
/// made, or one removed from under the selection — so the panel follows the
/// selection rather than the user having to go and find it.
pub fn scroll_to_show(layout: &PrefabLayout, index: usize) -> usize {
    if layout.rows.is_empty() {
        return index;
    }
    let first = layout.rows[0].index;
    let last = layout.rows[layout.rows.len().saturating_sub(2)].index;
    if index < first {
        index
    } else if index > last {
        layout.scroll + (index - last)
    } else {
        layout.scroll
    }
}
