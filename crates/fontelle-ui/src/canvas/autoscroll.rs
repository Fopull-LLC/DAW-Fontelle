//! Middle-click autoscroll.
//!
//! > *"we're lacking a way for people to scroll without their scroll wheel
//! > needing to turn. traditionally people do this by clicking with the middle
//! > mouse and then that puts you in a scroll mode inside the window you middle
//! > clicked in and then from that point if you move your mouse up or down it
//! > will scroll in that direction faster depending on how far you move your
//! > mouse cursor away from the scroll point you middle clicked at."*
//! > (a user, 2026-10-18)
//!
//! The middle button goes down on a point — the **anchor** — and from then on
//! the view under it moves towards wherever the pointer is, at a speed that
//! grows with the distance. Let go at once and it stays on until the next
//! click (a tap, as in a browser); hold it and move, and letting go stops it.
//!
//! This is the arithmetic only: rates per second against a real clock, the
//! same reason [`super::EdgeScroll`] is (a mouse's report rate must not be
//! what decides how fast the view goes). The window decides what the travel
//! scrolls.

/// How far the pointer can stray from the anchor before anything moves, in
/// points. A hand at rest is not perfectly still.
pub const AUTOSCROLL_DEAD_ZONE: f32 = 10.0;

/// A middle press let go within this many seconds, without straying past the
/// dead zone, is a tap: scrolling stays on until the next click.
pub const AUTOSCROLL_TAP_S: f32 = 0.35;

/// Points per second per point past the dead zone — the slow end.
const GAIN: f32 = 4.0;
/// And the square term, which is what makes a long throw fast: 400 points out
/// is several screens a second, 20 points out is a crawl you can read along.
const GAIN_SQUARED: f32 = 0.025;
/// No faster than this, however far the pointer goes.
const MAX_PX_PER_SEC: f32 = 12_000.0;
/// A step longer than this is a frame that came late, not time the view
/// should catch up on in one jump.
const MAX_STEP_S: f32 = 0.1;

/// One autoscroll gesture: where it was anchored, and what is owed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Autoscroll {
    pub anchor: (f32, f32),
    /// Whether the pointer has been past the dead zone since the press —
    /// what makes a release a hold rather than a tap.
    strayed: bool,
    /// Points not yet added up to a whole row, per axis — see
    /// [`whole`](Self::whole).
    carry: (f32, f32),
}

impl Autoscroll {
    pub fn new(anchor: (f32, f32)) -> Self {
        Self {
            anchor,
            strayed: false,
            carry: (0.0, 0.0),
        }
    }

    /// How fast the view moves with the pointer at `cursor`, in points per
    /// second along each axis. Positive is right and down: the pointer below
    /// the anchor scrolls the way a wheel turned towards you does.
    ///
    /// Each axis on its own, so a pointer straight below the anchor scrolls
    /// straight down with no sideways creep.
    pub fn velocity(&self, cursor: (f32, f32)) -> (f32, f32) {
        let axis = |d: f32| {
            if !d.is_finite() {
                return 0.0;
            }
            let past = d.abs() - AUTOSCROLL_DEAD_ZONE;
            if past <= 0.0 {
                return 0.0;
            }
            let speed = (past * GAIN + past * past * GAIN_SQUARED).min(MAX_PX_PER_SEC);
            speed.copysign(d)
        };
        (
            axis(cursor.0 - self.anchor.0),
            axis(cursor.1 - self.anchor.1),
        )
    }

    /// How far the view moves in `dt` seconds with the pointer at `cursor`,
    /// in points. Notes whether the pointer has strayed.
    pub fn step(&mut self, cursor: (f32, f32), dt: f32) -> (f32, f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return (0.0, 0.0);
        }
        let (vx, vy) = self.velocity(cursor);
        if vx != 0.0 || vy != 0.0 {
            self.strayed = true;
        }
        let dt = dt.min(MAX_STEP_S);
        (vx * dt, vy * dt)
    }

    /// `px` points of travel as whole steps of `step` points each, keeping
    /// what does not add up to one for next time. For the lists that scroll
    /// by rows: a frame of a slow autoscroll is a fraction of a row, and a
    /// fraction truncated every frame is a list that never moves.
    pub fn whole(&mut self, px: (f32, f32), step: (f32, f32)) -> (i32, i32) {
        let take = |carry: &mut f32, px: f32, step: f32| -> i32 {
            if !px.is_finite() || step <= 0.0 {
                return 0;
            }
            *carry += px / step;
            let whole = carry.trunc();
            *carry -= whole;
            whole as i32
        };
        (
            take(&mut self.carry.0, px.0, step.0),
            take(&mut self.carry.1, px.1, step.1),
        )
    }

    /// The middle button came up at `cursor`, `held` seconds after it went
    /// down. True when scrolling carries on — a tap, which latches until the
    /// next click — and false when this ends it.
    pub fn release(&mut self, cursor: (f32, f32), held: f32) -> bool {
        let (vx, vy) = self.velocity(cursor);
        if vx != 0.0 || vy != 0.0 {
            self.strayed = true;
        }
        !self.strayed && held < AUTOSCROLL_TAP_S
    }
}
