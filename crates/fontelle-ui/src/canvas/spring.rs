//! Scrolling with give: the wheel glides, a touchpad follows the fingers and
//! coasts when they lift, and every edge gives a little and springs back.
//!
//! > *"could you also encorperate springy scrolling like apple devices have
//! > just to make everything have more polished feeling"* — Ty, 2026-11-06
//!
//! [`SpringScroll`] is one scroll position, in points down a page, between
//! `0` and the page's [`max`](SpringScroll::set_max). It may stand outside
//! that range for a moment — that is the give — and always comes to rest
//! inside it. Pure: time comes in as arguments, so a test can run a second
//! of motion in sixty calls and no sleep.
//!
//! The four motions, and the numbers that shape them:
//!
//! - **Wheel**: the target moves, the position closes on it with a short
//!   time constant ([`GLIDE_TAU_S`]). A target past an edge is allowed only
//!   [`WHEEL_GIVE`] past it; once the position gets there the target is the
//!   edge again, and the glide brings it home — the small knock of a page
//!   hitting its end.
//! - **Touch**: the position moves by exactly what the fingers moved, inside
//!   the content. Past an edge each point of finger moves the page less the
//!   further out it already is, never past [`MAX_OVERSCROLL`] — a rubber
//!   band. The velocity is kept from the last few events.
//! - **Coast**: when the fingers lift — said, or [`TOUCH_IDLE_S`] of quiet —
//!   the page carries on at the fingers' speed and slows by friction
//!   ([`COAST_TAU_S`]); a lift after the fingers had stopped carries nothing.
//!   Coasting past an edge brakes hard and springs back.
//! - **Spring**: anything outside the content returns to its edge on a
//!   critically damped spring ([`SPRING_TAU_S`]): no wobble, no overshoot
//!   back the other way.

/// How far a notch of the wheel moves a full page, in points.
pub const PAGE_SCROLL_STEP: f32 = 48.0;

/// The furthest past an edge a page may be pulled, in points.
pub const MAX_OVERSCROLL: f32 = 120.0;

/// How far past an edge the wheel may send the page before it springs back.
const WHEEL_GIVE: f32 = 28.0;

/// The wheel glide's time constant: a notch is over in a few frames.
const GLIDE_TAU_S: f32 = 0.06;

/// The spring back from past an edge.
const SPRING_TAU_S: f32 = 0.09;

/// The coast's friction: the speed falls to a third in this long — close to
/// the deceleration people know from phones and Macs.
const COAST_TAU_S: f32 = 0.33;

/// The friction past an edge while coasting: a short stop before the spring.
const COAST_EDGE_TAU_S: f32 = 0.035;

/// Below this speed, in points a second, a coast is over.
const COAST_MIN_SPEED: f32 = 15.0;

/// With no touch event for this long, the fingers have lifted — for the
/// desktops that never say so.
const TOUCH_IDLE_S: f64 = 0.05;

/// Fingers still for this long before they lift carry no speed into a coast.
const STILL_BEFORE_LIFT_S: f64 = 0.08;

/// Closer than this, in points, a motion has arrived.
const SETTLE: f32 = 0.5;

/// The most a single touch event's time apart is taken to be, so the first
/// event after a pause does not read as a crawl.
const MAX_EVENT_GAP_S: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Motion {
    Rest,
    /// Closing on a wheel target.
    Glide {
        target: f32,
    },
    /// Fingers on the pad; `last` is when the last event came.
    Touch {
        last: f64,
    },
    /// Carrying on after the fingers lifted.
    Coast {
        velocity: f32,
    },
}

/// One scrolling page's position and how it is moving. See the module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringScroll {
    position: f32,
    max: f32,
    motion: Motion,
    /// Points a second, smoothed over the last few touch events.
    velocity: f32,
}

impl Default for SpringScroll {
    fn default() -> Self {
        Self {
            position: 0.0,
            max: 0.0,
            motion: Motion::Rest,
            velocity: 0.0,
        }
    }
}

impl SpringScroll {
    /// Where the page is: may be a little below `0` or above the max while
    /// it gives.
    pub fn position(&self) -> f32 {
        self.position
    }

    /// How far the content can scroll. A position now past the new end
    /// springs back to it on the next frames.
    pub fn set_max(&mut self, max: f32) {
        self.max = max.max(0.0);
    }

    /// Puts the page at `position` at once, still: a new section, a page
    /// opened where it was left.
    pub fn jump_to(&mut self, position: f32) {
        self.position = position.clamp(0.0, self.max);
        self.motion = Motion::Rest;
        self.velocity = 0.0;
    }

    /// Glides to `position` within the content, with no give — what a key
    /// asks for: Page Down, End.
    pub fn glide_to(&mut self, position: f32) {
        self.velocity = 0.0;
        self.motion = Motion::Glide {
            target: position.clamp(0.0, self.max),
        };
    }

    /// Where the page is going: a glide's target, or where it is.
    pub fn destination(&self) -> f32 {
        match self.motion {
            Motion::Glide { target } => target.clamp(0.0, self.max),
            _ => self.position.clamp(0.0, self.max),
        }
    }

    /// A wheel moved the page `points` (down is positive).
    pub fn wheel(&mut self, points: f32) {
        let from = match self.motion {
            Motion::Glide { target } => target,
            _ => self.position,
        };
        let target = (from + points).clamp(-WHEEL_GIVE, self.max + WHEEL_GIVE);
        self.velocity = 0.0;
        self.motion = Motion::Glide { target };
    }

    /// Fingers on a touchpad moved the page `points` at time `at`, in
    /// seconds on the caller's clock.
    pub fn touch(&mut self, points: f32, at: f64) {
        let last = match self.motion {
            Motion::Touch { last } => Some(last),
            _ => {
                self.velocity = 0.0;
                None
            }
        };
        let before = self.position;
        self.position = self.pulled(self.position, points);
        if let Some(last) = last {
            let gap = (at - last).clamp(1.0 / 480.0, MAX_EVENT_GAP_S) as f32;
            let speed = (self.position - before) / gap;
            // The last few events count most: a swipe's end is its speed.
            self.velocity = self.velocity * 0.4 + speed * 0.6;
        } else {
            self.velocity = 0.0;
        }
        self.motion = Motion::Touch { last: at };
    }

    /// The fingers lifted at `at`.
    pub fn release(&mut self, at: f64) {
        if let Motion::Touch { last } = self.motion {
            let velocity = if at - last > STILL_BEFORE_LIFT_S {
                0.0
            } else {
                self.velocity
            };
            self.motion = Motion::Coast { velocity };
        }
    }

    /// One frame, `dt` seconds on, at `now` on the caller's clock. Whether
    /// the page is still moving — and wants another frame.
    pub fn step(&mut self, dt: f32, now: f64) -> bool {
        let dt = dt.max(0.0);
        if let Motion::Touch { last } = self.motion
            && now - last > TOUCH_IDLE_S
        {
            self.release(now);
        }
        match self.motion {
            Motion::Touch { .. } => true,
            Motion::Glide { target } => {
                let target_now = if (target - self.position).abs() < SETTLE
                    && (target < 0.0 || target > self.max)
                {
                    // Reached its give: the edge is where it goes now.
                    target.clamp(0.0, self.max)
                } else {
                    target.clamp(-WHEEL_GIVE, self.max + WHEEL_GIVE)
                };
                // Coming home from past an edge is the spring; anything else
                // is the wheel's glide.
                let outside = self.position < 0.0 || self.position > self.max;
                let homeward = (0.0..=self.max).contains(&target_now);
                let tau = if outside && homeward {
                    SPRING_TAU_S
                } else {
                    GLIDE_TAU_S
                };
                self.position = approach(self.position, target_now, dt, tau);
                if (target_now - self.position).abs() < SETTLE {
                    self.position = target_now;
                    if (0.0..=self.max).contains(&target_now) {
                        self.motion = Motion::Rest;
                        return false;
                    }
                }
                self.motion = Motion::Glide { target: target_now };
                true
            }
            Motion::Coast { velocity } => {
                let outside = self.position < 0.0 || self.position > self.max;
                let tau = if outside {
                    COAST_EDGE_TAU_S
                } else {
                    COAST_TAU_S
                };
                let velocity = velocity * (-dt / tau).exp();
                self.position += velocity * dt;
                // Never further out than the band, coasting or not.
                self.position = self
                    .position
                    .clamp(-MAX_OVERSCROLL * 0.9, self.max + MAX_OVERSCROLL * 0.9);
                if velocity.abs() < COAST_MIN_SPEED {
                    self.velocity = 0.0;
                    if self.position < 0.0 || self.position > self.max {
                        self.motion = Motion::Glide {
                            target: self.position.clamp(0.0, self.max),
                        };
                        return true;
                    }
                    // Where it stopped, unrounded: the layouts round each
                    // row as they draw it, and a rounding here would step
                    // back against the coast.
                    self.motion = Motion::Rest;
                    return false;
                }
                self.motion = Motion::Coast { velocity };
                true
            }
            Motion::Rest => {
                if self.position < 0.0 || self.position > self.max {
                    // The content shrank under the page, or a jump landed
                    // outside: spring to the edge.
                    self.motion = Motion::Glide {
                        target: self.position.clamp(0.0, self.max),
                    };
                    return self.step(dt, now);
                }
                false
            }
        }
    }

    /// `from` moved `points` by a finger, with the rubber band past the
    /// edges: inside the content one for one, outside less and less, never
    /// past [`MAX_OVERSCROLL`].
    fn pulled(&self, from: f32, points: f32) -> f32 {
        // How far out, past which edge.
        let out = |p: f32| {
            if p < 0.0 {
                -p
            } else if p > self.max {
                p - self.max
            } else {
                0.0
            }
        };
        let raw = from + points;
        let heading_out = out(raw) > out(from);
        if !heading_out {
            return raw;
        }
        // The part of the move still inside the content goes one for one;
        // what is past the edge is shrunk by how far out the page already
        // is — the band tightens as it stretches.
        let edge = if raw < 0.0 { 0.0 } else { self.max };
        let start = if out(from) > 0.0 { from } else { edge };
        let beyond = (raw - start).abs();
        let already = out(start);
        let room = (MAX_OVERSCROLL - already).max(0.0);
        let give = room * (1.0 - (-beyond / (MAX_OVERSCROLL * 1.6)).exp());
        let stretched = already + give.min(room * 0.999);
        if raw < 0.0 {
            -stretched
        } else {
            self.max + stretched
        }
    }
}

/// `from` an exponential step toward `to`.
fn approach(from: f32, to: f32, dt: f32, tau: f32) -> f32 {
    to + (from - to) * (-dt / tau).exp()
}

/// The keys that move a long page: a screen at a time, or to an end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageKey {
    PageUp,
    PageDown,
    Home,
    End,
}

/// How far a list may be from where it is going before the glide starts
/// there instead, in rows: a jump to the far end still glides, briefly,
/// rather than scrolling past a hundred rows.
const LIST_SKIP_ROWS: f32 = 4.0;

/// A list's glide's time constant.
const LIST_TAU_S: f32 = 0.05;

/// Closer than this, in rows, a list has arrived.
const LIST_SETTLE_ROWS: f32 = 0.02;

/// Where a list of rows is drawn, in rows from the top — easing toward the
/// whole row its scroll names (Ty, 2026-11-06: lists that glide). The row
/// the list is scrolled to stays the truth; this is only where it is seen.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ListGlide {
    position: f32,
}

impl ListGlide {
    pub fn position(&self) -> f32 {
        self.position
    }

    /// At `rows`, at once.
    pub fn jump(&mut self, rows: f32) {
        self.position = rows;
    }

    /// One frame toward `target`, `dt` seconds on. Whether it is still
    /// moving.
    pub fn step(&mut self, target: f32, dt: f32) -> bool {
        let gap = target - self.position;
        if gap.abs() < LIST_SETTLE_ROWS {
            let moved = self.position != target;
            self.position = target;
            return moved;
        }
        if gap.abs() > LIST_SKIP_ROWS {
            self.position = target - LIST_SKIP_ROWS * gap.signum();
        }
        self.position = approach(self.position, target, dt.max(0.0), LIST_TAU_S);
        if (target - self.position).abs() < LIST_SETTLE_ROWS {
            self.position = target;
        }
        true
    }
}

/// A touchpad's pan of a view that scrolls both ways — the arrangement, the
/// roll — and its coast once the fingers lift. What it hands back each frame
/// is how far to move the view, in points, signed as the fingers moved.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PanCoast {
    velocity: (f32, f32),
    last: Option<f64>,
    coasting: bool,
}

impl PanCoast {
    /// Fingers moved `(dx, dy)` points at `at`. The caller moves the view by
    /// exactly that itself; this keeps the speed for the coast.
    pub fn touch(&mut self, dx: f32, dy: f32, at: f64) {
        match self.last {
            Some(last) if !self.coasting => {
                let gap = (at - last).clamp(1.0 / 480.0, MAX_EVENT_GAP_S) as f32;
                let speed = (dx / gap, dy / gap);
                self.velocity = (
                    self.velocity.0 * 0.4 + speed.0 * 0.6,
                    self.velocity.1 * 0.4 + speed.1 * 0.6,
                );
            }
            _ => self.velocity = (0.0, 0.0),
        }
        self.coasting = false;
        self.last = Some(at);
    }

    /// The fingers lifted at `at`.
    pub fn release(&mut self, at: f64) {
        let Some(last) = self.last.take() else {
            return;
        };
        if at - last > STILL_BEFORE_LIFT_S {
            self.velocity = (0.0, 0.0);
        }
        self.coasting = self.velocity.0.hypot(self.velocity.1) >= COAST_MIN_SPEED;
    }

    /// Whether it is still carrying the view on.
    pub fn coasting(&self) -> bool {
        self.coasting
    }

    /// Whether a frame is wanted: coasting, or fingers down that may lift
    /// without saying so.
    pub fn active(&self) -> bool {
        self.coasting || self.last.is_some()
    }

    /// One frame, `dt` seconds on at `now`: how far to move the view.
    pub fn step(&mut self, dt: f32, now: f64) -> (f32, f32) {
        if let Some(last) = self.last
            && now - last > TOUCH_IDLE_S
        {
            self.release(now);
        }
        if !self.coasting {
            return (0.0, 0.0);
        }
        let decay = (-dt.max(0.0) / COAST_TAU_S).exp();
        self.velocity = (self.velocity.0 * decay, self.velocity.1 * decay);
        if self.velocity.0.hypot(self.velocity.1) < COAST_MIN_SPEED {
            self.coasting = false;
            self.velocity = (0.0, 0.0);
            return (0.0, 0.0);
        }
        (self.velocity.0 * dt, self.velocity.1 * dt)
    }
}

/// How long a page or a menu takes to fade in.
pub const APPEAR_SECONDS: f32 = 0.14;

/// How opaque a page or menu is `age` seconds after it opened: most of the
/// way in the first frames, then easing in — so it is never in the way.
pub fn appear_alpha(age: f32) -> f32 {
    let t = (age / APPEAR_SECONDS).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// How long a toast takes to fade out at the end of its time.
const TOAST_FADE_OUT_S: f32 = 0.3;

/// How opaque a toast is `age` seconds into its `total`: faded in, held,
/// faded out by the end — never a blink either way.
pub fn toast_alpha(age: f32, total: f32) -> f32 {
    if age >= total {
        return 0.0;
    }
    let out = ((total - age) / TOAST_FADE_OUT_S).clamp(0.0, 1.0);
    appear_alpha(age).min(out)
}
