//! The channel rack (TDD §16.1, item 9 of `docs/first-usable-plan.md`).
//!
//! What instruments the project has, which one the piano roll is looking at,
//! and the two switches — mute and solo — that make a part balanceable without
//! opening a mixer. Everything else about a channel lives in the mixer panel or
//! the sampler editor; this is the list you work from.
//!
//! Geometry and hit-testing only, and both pure, for the reason §2.5 gives.
//! **Virtualised like the roll** (§16.4): a project with two hundred channels
//! builds a screenful of rectangles, not two hundred.

use crate::layout::Rect;
use crate::theme::Metrics;

/// One channel's row, and the controls inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RackRow {
    /// Which channel this is, counted from the top of the project's list — not
    /// from the top of the panel, which is what makes scrolling work.
    pub index: usize,
    pub frame: Rect,
    /// Where the name is written, and the part you click to select the channel.
    pub name: Rect,
    pub mute: Rect,
    pub solo: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RackLayout {
    pub body: Rect,
    pub rows: Vec<RackRow>,
    /// "Add an instrument". Pinned to the bottom of the panel rather than
    /// placed after the last row: with no channels at all it is the only thing
    /// worth clicking, and with two hundred it must not have scrolled away.
    pub add: Rect,
    /// How many channels there are in total, so a scrollbar can be drawn and a
    /// scroll offset clamped.
    pub total: usize,
    pub scroll: usize,
}

/// How wide the mute and solo squares are.
const SWITCH: f32 = 18.0;

/// Lays out `count` channels in `body`, starting from `scroll`.
pub fn rack_layout(body: Rect, metrics: &Metrics, count: usize, scroll: usize) -> RackLayout {
    let add_height = metrics.row_height.min(body.height.max(0.0));
    let add = Rect::new(
        body.x,
        (body.bottom() - add_height).max(body.y),
        body.width,
        add_height,
    )
    .clamped();

    // What is left above the button is what the list gets.
    let list = Rect::new(body.x, body.y, body.width, (add.y - body.y - 2.0).max(0.0)).clamped();

    let mut rows = Vec::new();
    if metrics.row_height > 0.0 && !list.is_empty() {
        let visible = (list.height / metrics.row_height).ceil() as usize + 1;
        let scroll = scroll.min(count.saturating_sub(1));
        for (slot, index) in (scroll..count).take(visible).enumerate() {
            let frame = Rect::new(
                list.x,
                list.y + slot as f32 * metrics.row_height,
                list.width,
                metrics.row_height,
            );
            if frame.y >= list.bottom() {
                break;
            }
            // The switches sit at the right-hand end, solo then mute, so the
            // name has the whole of the rest — soundfont names are long.
            let mute = Rect::new(
                frame.right() - SWITCH - 2.0,
                frame.y + 2.0,
                SWITCH,
                (frame.height - 4.0).max(0.0),
            )
            .clamped();
            let solo = Rect::new(mute.x - SWITCH - 2.0, mute.y, SWITCH, mute.height).clamped();
            let name = Rect::new(
                frame.x,
                frame.y,
                (solo.x - frame.x - 2.0).max(0.0),
                frame.height,
            )
            .clamped();
            rows.push(RackRow {
                index,
                frame,
                name,
                mute,
                solo,
            });
        }
    }

    RackLayout {
        body,
        rows,
        add,
        total: count,
        scroll,
    }
}

/// What is under the pointer in the rack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RackHit {
    /// Select this channel — the roll follows it.
    Row(usize),
    Mute(usize),
    Solo(usize),
    Add,
    Nothing,
}

pub fn rack_hit(layout: &RackLayout, x: f32, y: f32) -> RackHit {
    if layout.add.contains(x, y) {
        return RackHit::Add;
    }
    for row in &layout.rows {
        if !row.frame.contains(x, y) {
            continue;
        }
        // The switches first: they are inside the row, and a row that claimed
        // the whole width would swallow them.
        if row.mute.contains(x, y) {
            return RackHit::Mute(row.index);
        }
        if row.solo.contains(x, y) {
            return RackHit::Solo(row.index);
        }
        return RackHit::Row(row.index);
    }
    RackHit::Nothing
}

/// The scroll offset that keeps `index` on screen, given where the list is now.
///
/// Called when the selected channel changes by something other than a click —
/// a channel added, or one removed from under the selection — so the panel
/// follows the selection rather than the user having to go and find it.
pub fn scroll_to_show(layout: &RackLayout, index: usize) -> usize {
    if layout.rows.is_empty() {
        return index;
    }
    let first = layout.rows[0].index;
    // The last row may be half off the bottom; the one before it is the last
    // that is really readable.
    let last = layout.rows[layout.rows.len().saturating_sub(2)].index;
    if index < first {
        index
    } else if index > last {
        layout.scroll + (index - last)
    } else {
        layout.scroll
    }
}
