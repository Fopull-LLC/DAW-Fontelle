//! The scroll bar a list of whole rows wears: the browser's lists, the Import
//! tab's above all (*"add scroll bar to the import windows"*).
//!
//! It is the menus' bar ([`super::ContextMenu::scrollbar`]) for a list that
//! scrolls a row at a time: the same width, the same place against the right
//! edge, a thumb as long as the share of the list that shows and floored so a
//! very long list still leaves something to hold, and a press anywhere on its
//! strip takes hold of the thumb.

use crate::layout::Rect;

/// How wide the thumb is drawn. Shared with the menus, so the two bars are
/// one bar.
pub(crate) const SCROLLBAR_WIDTH: f32 = 4.0;

/// The shortest the thumb gets, however long the list.
pub(crate) const THUMB_MIN: f32 = 18.0;

/// Between the thumb and the list's edges.
const PAD: f32 = 3.0;

/// The strip along the right edge a scrolling list gives up to its bar: the
/// thumb, the pad either side of it and a pixel more. The rows end where it
/// starts, so a name never runs under the thumb, and a press anywhere in it is
/// the bar's.
pub(crate) const STRIP: f32 = SCROLLBAR_WIDTH + PAD * 2.0 + 1.0;

/// A whole-row list's scroll bar, for one layout of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowScrollbar {
    /// What the thumb slides in.
    pub track: Rect,
    /// The thumb: where the visible page is in the list.
    pub thumb: Rect,
    /// What a press lands in to take hold — the whole strip the rows gave up,
    /// not just the four pixels of the thumb.
    pub strip: Rect,
    /// The furthest down the list goes: the last row on the bottom of the
    /// list rather than alone at its top.
    pub max_scroll: usize,
}

impl RowScrollbar {
    /// The bar for `count` rows of `row_height` in `area`, scrolled to
    /// `scroll`. `None` when they all fit, because a thumb the length of its
    /// track says nothing.
    pub fn of(area: Rect, row_height: f32, count: usize, scroll: usize) -> Option<Self> {
        if area.is_empty() || row_height <= 0.0 {
            return None;
        }
        let visible = (area.height / row_height).floor() as usize;
        if visible == 0 || count <= visible {
            return None;
        }
        let max_scroll = count - visible;
        let strip = Rect::new(
            (area.right() - STRIP).max(area.x),
            area.y,
            STRIP.min(area.width),
            area.height,
        );
        let track = Rect::new(
            strip.right() - PAD - SCROLLBAR_WIDTH,
            area.y + PAD,
            SCROLLBAR_WIDTH,
            (area.height - PAD * 2.0).max(0.0),
        )
        .clamped();
        let share = (visible as f32 / count as f32).clamp(0.0, 1.0);
        let height = (track.height * share).max(THUMB_MIN).min(track.height);
        let travel = (track.height - height).max(0.0);
        let at = scroll.min(max_scroll) as f32 / max_scroll as f32;
        let thumb = Rect::new(track.x, track.y + travel * at, track.width, height);
        Some(Self {
            track,
            thumb,
            strip,
            max_scroll,
        })
    }

    /// Takes hold of the thumb at `(x, y)`, answering how far down the thumb
    /// the pointer has it — what [`scroll_at`](Self::scroll_at) needs to keep
    /// that point under the pointer for the rest of the drag. Beside the
    /// thumb, it comes to the pointer by its middle. Off the strip, `None`.
    pub fn grab(&self, x: f32, y: f32) -> Option<f32> {
        if !self.strip.contains(x, y) {
            return None;
        }
        if y >= self.thumb.y && y < self.thumb.bottom() {
            Some(y - self.thumb.y)
        } else {
            Some(self.thumb.height / 2.0)
        }
    }

    /// The scroll that puts the point `grab` pixels down the thumb at `y`,
    /// in whole rows and inside the list at both ends.
    pub fn scroll_at(&self, y: f32, grab: f32) -> usize {
        let travel = (self.track.height - self.thumb.height).max(0.0);
        if travel <= 0.0 {
            return 0;
        }
        let wanted = ((y - grab - self.track.y) / travel).clamp(0.0, 1.0);
        (wanted * self.max_scroll as f32).round() as usize
    }
}
