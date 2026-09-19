//! Motion tells you what changed (`docs/flopsynth-next.md` §3.1, principle
//! 13): a knob's arc eases to a value set by anything but the pointer, a
//! page fades in, a hover's bubble rises.
//!
//! Pure: every ease is arithmetic on a clock the window hands in, so the
//! window only asks "where is this now" while it draws and "is anything
//! still moving" when it decides whether to sleep — the animator count
//! (`widget::Redraw`) is the one thing that keeps the loop awake, and a
//! registry with nothing in flight lets it sleep (§16.3).

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// How long an arc takes to reach a value it was not dragged to.
pub const ARC: Duration = Duration::from_millis(80);
/// How long a page takes to fade in.
pub const PAGE: Duration = Duration::from_millis(120);
/// How long a hover's bubble takes to rise.
pub const BUBBLE: Duration = Duration::from_millis(100);

/// One value on its way from `from` to `to`, smoothstepped — slow to
/// leave, slow to arrive — over `duration` from `since`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ease {
    pub from: f32,
    pub to: f32,
    pub since: Instant,
    pub duration: Duration,
}

impl Ease {
    pub fn new(from: f32, to: f32, since: Instant, duration: Duration) -> Self {
        Self {
            from,
            to,
            since,
            duration,
        }
    }

    /// Where the value is at `now`: `from` before it starts, `to` after it
    /// ends, and between them a smoothstep of the time.
    pub fn at(&self, now: Instant) -> f32 {
        let elapsed = now.saturating_duration_since(self.since);
        if self.duration.is_zero() || elapsed >= self.duration {
            return self.to;
        }
        let t = elapsed.as_secs_f32() / self.duration.as_secs_f32();
        let t = t.clamp(0.0, 1.0);
        let s = t * t * (3.0 - 2.0 * t);
        self.from + (self.to - self.from) * s
    }

    pub fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since) >= self.duration
    }
}

/// The eases in flight, one per key — a control's address, the page, the
/// bubble.
#[derive(Debug, Clone)]
pub struct Motions<K: Hash + Eq> {
    eases: HashMap<K, Ease>,
}

impl<K: Hash + Eq> Default for Motions<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Hash + Eq> Motions<K> {
    pub fn new() -> Self {
        Self {
            eases: HashMap::new(),
        }
    }

    /// Starts `key` moving to `to`. From where it **is** if it is already
    /// moving — a value that jumped back to its start would be the flicker
    /// motion is meant to remove — else from `from`.
    pub fn begin(&mut self, key: K, from: f32, to: f32, now: Instant, duration: Duration) {
        let from = self.eases.get(&key).map_or(from, |ease| ease.at(now));
        self.eases.insert(key, Ease::new(from, to, now, duration));
    }

    /// Where `key` is at `now`, or `None` for one that is not moving.
    pub fn value(&self, key: K, now: Instant) -> Option<f32> {
        self.eases.get(&key).map(|ease| ease.at(now))
    }

    /// Drops what has arrived.
    pub fn prune(&mut self, now: Instant) {
        self.eases.retain(|_, ease| !ease.done(now));
    }

    /// Whether anything is still on its way at `now`.
    pub fn is_moving(&self, now: Instant) -> bool {
        self.eases.values().any(|ease| !ease.done(now))
    }

    pub fn is_empty(&self) -> bool {
        self.eases.is_empty()
    }
}
