//! Scrolling that glides, and scrolls by the screen rather than by the row.
//!
//! > *"scrolling feels so rigid it should be smoother animated and it should
//! > also scroll slower when really zoomed in right now its still scrolling
//! > super fast when zoomed far in"*
//!
//! The wheel used to write the view directly: a fixed stretch of pixels
//! sideways, two whole rows down, on the frame the notch arrived. That is a
//! jump per notch, and rows scrolled by the row are a big jump when the rows
//! are tall. Two changes, both here and both pure:
//!
//! - [`wheel_travel`]: a notch moves a **fraction of what is on screen**,
//!   and a burst of notches at most half of it. Sideways that is a fraction
//!   of the bars in view — fewer ticks the further in you are, which is the
//!   "slower when zoomed in" asked for. Down it is a fraction of the rows,
//!   and the view keeps a fraction of a row (`TimelineView::lane_offset`,
//!   `RollView::key_offset`) so a notch on tall rows is not a whole row.
//! - [`Glide`]: the wheel moves a *target*; the position follows it, most
//!   of the way each frame, and settles exactly. A burst of notches is one
//!   motion. Anything else that moves the view — a zoom about the pointer,
//!   the playhead pulling the view along, a scroll-to-show — is **adopted**:
//!   the glide takes the new place as its own rather than dragging the view
//!   back to where it was going.

/// How much of the view one notch travels.
///
/// A tenth: a desktop that reports three lines a notch then moves a bit
/// under a third of the screen, which is about what a wheel notch moves in
/// every editor people are used to.
const WHEEL_FRACTION: f32 = 0.1;

/// The most one wheel event may travel, as a fraction of the view: a flick
/// or a high-resolution wheel must not throw the view a screen at a time.
const WHEEL_CAP: f32 = 0.5;

/// How far `notches` of the wheel move a view `extent` pixels across, in
/// pixels — signed the way the notches are.
pub fn wheel_travel(extent: f32, notches: f32) -> f32 {
    let cap = extent * WHEEL_CAP;
    (extent * WHEEL_FRACTION * notches).clamp(-cap, cap)
}

/// How quickly a glide closes on its target: the time constant, in seconds.
/// Short enough that a single notch is over in a few frames, long enough
/// that it is seen to move.
const GLIDE_TAU_S: f32 = 0.05;

/// A scroll position that glides to where it was sent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glide {
    position: f64,
    target: f64,
    /// What this glide last wrote to the view, so a view that reads back
    /// differently was moved by something else — see [`Glide::adopt`].
    written: f64,
    low: f64,
    high: f64,
}

impl Glide {
    /// At rest at `position`, bounded below by zero.
    pub fn at(position: f64) -> Self {
        Self {
            position,
            target: position,
            written: position,
            low: 0.0,
            high: f64::INFINITY,
        }
    }

    /// The same, held inside `low..=high` — the roll's keys, which have a
    /// top as well as a bottom.
    pub fn with_bounds(mut self, low: f64, high: f64) -> Self {
        self.low = low;
        self.high = high;
        self.target = self.target.clamp(low, high);
        self.position = self.position.clamp(low, high);
        self
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    /// Whether it has somewhere still to go.
    pub fn moving(&self) -> bool {
        self.position != self.target
    }

    /// Sends the target `delta` further on. The position does not move
    /// until [`step`](Self::step) is asked.
    pub fn push(&mut self, delta: f64) {
        self.target = (self.target + delta).clamp(self.low, self.high);
    }

    /// Takes `actual` — what the view holds now — as the place to be, if it
    /// is not what this glide last wrote there. Something else moved the
    /// view, and that something wins: the target is dropped.
    pub fn adopt(&mut self, actual: f64) {
        if (actual - self.written).abs() > 1e-3 {
            self.position = actual;
            self.target = actual;
            self.written = actual;
        }
    }

    /// One frame's worth of motion, `dt` seconds after the last.
    ///
    /// Closes on the target exponentially, and **snaps** once the remainder
    /// is under `settle` — the smallest step worth drawing, half a pixel in
    /// the view's own units — so it arrives exactly and stops asking for
    /// frames. `Some(position)` when it moved, for the caller to write into
    /// the view; `None` when there was nowhere to go.
    pub fn step(&mut self, dt: f32, settle: f64) -> Option<f64> {
        if !self.moving() {
            return None;
        }
        let remaining = self.target - self.position;
        let fraction = 1.0 - (-dt.max(0.0) / GLIDE_TAU_S).exp();
        let moved = remaining * f64::from(fraction);
        self.position = if (remaining - moved).abs() < settle.max(f64::EPSILON) {
            self.target
        } else {
            self.position + moved
        };
        self.written = self.position;
        Some(self.position)
    }

    /// Records what the caller actually wrote into the view — a rounded
    /// tick, a row and its fraction — so the next [`adopt`](Self::adopt)
    /// compares against that rather than against the unrounded position.
    pub fn wrote(&mut self, actual: f64) {
        self.written = actual;
    }
}
