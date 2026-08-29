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

/// The window, and what is in it (TDD §16.1).
///
/// A transport bar across the top, a sidebar down the left carrying the channel
/// rack over the soundfont browser, and the piano roll taking everything that
/// is left. Item 9 of `docs/first-usable-plan.md`; §16.1's resizable, saveable
/// dock layouts are what this grows into, and the shape is already the shape
/// they will need — every panel is a `PanelLayout` computed from one rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowLayout {
    pub window: Rect,
    /// The strip carrying play/stop, the playhead and the master meter. See
    /// [`crate::transport::transport_bar_layout`] for what is inside it.
    pub transport: Rect,
    /// The channel rack: what is in the project and what it is playing.
    pub rack: PanelLayout,
    /// The soundfont bank (TDD §17.5).
    pub browser: PanelLayout,
    /// The piano roll.
    pub panel: PanelLayout,
}

/// How much of the sidebar's height the channel rack gets.
///
/// The rack is a list of names; the browser is a search box over two lists, and
/// wants the room. Below this the browser stops being usable before the rack
/// does, which is why the split is not even.
const RACK_SHARE: f32 = 0.42;

/// The narrowest the sidebar is allowed to squeeze the roll to before it gives
/// up its own width instead.
///
/// The roll is what the window is *for*. A sidebar that keeps its width on a
/// small screen leaves a piano roll two bars wide, which is worse for everyone
/// than a browser that has to be scrolled sideways.
const MIN_ROLL_WIDTH: f32 = 320.0;

/// Lays out a window of `width` x `height` logical pixels.
pub fn window_layout(width: f32, height: f32, metrics: &Metrics) -> WindowLayout {
    let window = Rect::new(0.0, 0.0, width, height).clamped();
    let content = window.inset(metrics.panel_margin);

    let (transport, below_bar) = content.split_top(metrics.transport_bar_height);
    // The same gap between the bar and the panels as between them and the
    // window edge, so the chrome reads as evenly spaced rather than as a bar
    // with panels stuck to it.
    let (_gap, rest) = below_bar.split_top(metrics.panel_margin);

    // The sidebar yields before the roll does, and disappears entirely rather
    // than becoming a column too narrow to read.
    let room_for_sidebar = (rest.width - MIN_ROLL_WIDTH - metrics.panel_margin).max(0.0);
    let sidebar_width = if rest.width <= metrics.panel_margin {
        0.0
    } else {
        metrics.sidebar_width.min(room_for_sidebar).max(0.0)
    };

    let sidebar = Rect::new(rest.x, rest.y, sidebar_width, rest.height).clamped();
    let roll_x = if sidebar.is_empty() {
        rest.x
    } else {
        sidebar.right() + metrics.panel_margin
    };
    let roll = Rect::new(roll_x, rest.y, rest.right() - roll_x, rest.height).clamped();

    // The rack over the browser, with the same margin between them.
    let rack_height = ((sidebar.height - metrics.panel_margin).max(0.0) * RACK_SHARE).max(0.0);
    let (rack_frame, under) = sidebar.split_top(rack_height);
    let (_gap, browser_frame) = under.split_top(metrics.panel_margin);

    WindowLayout {
        window,
        transport,
        rack: panel(rack_frame, metrics),
        browser: panel(browser_frame, metrics),
        panel: panel(roll, metrics),
    }
}

/// A frame split into its header and the body under it.
fn panel(frame: Rect, metrics: &Metrics) -> PanelLayout {
    let (header, below) = frame.split_top(metrics.panel_header_height);
    PanelLayout {
        frame,
        header,
        body: below.inset(metrics.panel_padding),
    }
}
