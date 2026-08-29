//! Where things go, as arithmetic.
//!
//! `docs/first-usable-plan.md` §2.5: everything that can be a pure function is
//! one, and the tests live there. Window geometry is the easiest thing in a GUI
//! to get subtly wrong and the easiest to test, so none of it happens inside an
//! event loop.
//!
//! Everything here is in **logical** pixels. The surface is in physical ones;
//! [`Rect::scale`] is the single place that conversion happens.

use crate::theme::Metrics;

/// An axis-aligned rectangle.
///
/// Half-open: a point on the right or bottom edge is *outside*. That is what
/// lets two abutting rectangles tile a surface without both claiming the seam,
/// which matters because dirty regions are unioned and redrawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// A rectangle with nothing in it. The identity of [`union`](Rect::union).
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// True when there is nothing to draw in it. A rectangle with a negative
    /// dimension counts as empty rather than as an error — see
    /// [`Rect::clamped`].
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        !self.is_empty() && x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// The smallest rectangle covering both. An empty operand is ignored, so
    /// unioning into `ZERO` accumulates rather than dragging the result back to
    /// the origin.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    /// The overlap, or an empty rectangle when there is none.
    pub fn intersection(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Rect::new(
            x,
            y,
            (self.right().min(other.right()) - x).max(0.0),
            (self.bottom().min(other.bottom()) - y).max(0.0),
        )
    }

    /// Shrunk by `by` on every side, never past nothing.
    pub fn inset(&self, by: f32) -> Rect {
        Rect::new(
            self.x + by,
            self.y + by,
            self.width - 2.0 * by,
            self.height - 2.0 * by,
        )
        .clamped()
    }

    /// Logical pixels to physical ones.
    pub fn scale(&self, factor: f32) -> Rect {
        Rect::new(
            self.x * factor,
            self.y * factor,
            self.width * factor,
            self.height * factor,
        )
    }

    /// Replaces a negative dimension with zero.
    ///
    /// A user dragging a window edge past the chrome is ordinary; a negative
    /// width reaching the GPU is a panic or a garbage draw. Every constructor
    /// here that can subtract goes through this.
    pub fn clamped(&self) -> Rect {
        Rect::new(self.x, self.y, self.width.max(0.0), self.height.max(0.0))
    }

    /// The top `height` of this rectangle, and what is left below it.
    pub fn split_top(&self, height: f32) -> (Rect, Rect) {
        let height = height.clamp(0.0, self.height.max(0.0));
        (
            Rect::new(self.x, self.y, self.width, height).clamped(),
            Rect::new(self.x, self.y + height, self.width, self.height - height).clamped(),
        )
    }
}

/// One docked panel: its frame, the strip carrying its name, and the area its
/// contents get.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelLayout {
    /// The whole panel, header included — what gets the background and border.
    pub frame: Rect,
    pub header: Rect,
    /// Where the panel's contents draw, already inset by the padding.
    pub body: Rect,
}

/// The window, and what is in it.
///
/// A transport bar across the top and one panel under it (items 6 and 7 of
/// `docs/first-usable-plan.md`). The docked splits are item 9; this is the
/// shape they grow out of, not a placeholder for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowLayout {
    pub window: Rect,
    /// The strip carrying play/stop, the playhead and the master meter. See
    /// [`crate::transport::transport_bar_layout`] for what is inside it.
    pub transport: Rect,
    pub panel: PanelLayout,
}

/// Lays out a window of `width` x `height` logical pixels.
pub fn window_layout(width: f32, height: f32, metrics: &Metrics) -> WindowLayout {
    let window = Rect::new(0.0, 0.0, width, height).clamped();
    let content = window.inset(metrics.panel_margin);

    let (transport, below_bar) = content.split_top(metrics.transport_bar_height);
    // The same gap between the bar and the panel as between the panel and the
    // window edge, so the chrome reads as evenly spaced rather than as a bar
    // with a panel stuck to it.
    let (_gap, rest) = below_bar.split_top(metrics.panel_margin);

    let frame = rest;
    let (header, below) = frame.split_top(metrics.panel_header_height);

    WindowLayout {
        window,
        transport,
        panel: PanelLayout {
            frame,
            header,
            body: below.inset(metrics.panel_padding),
        },
    }
}
