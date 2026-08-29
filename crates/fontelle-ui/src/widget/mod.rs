//! The retained tree and, more importantly, **when a frame happens at all**.
//!
//! TDD §16.3 states the rule this module exists to keep: redraw only dirty
//! regions, and when the transport is stopped and nothing is animating, *issue
//! no frames at all*. That is the §19 idle-CPU target (under 0.5% of one core),
//! and the plan is explicit that it is far easier to build in now than to
//! retrofit — so the decision lives here, as a value, rather than inside the
//! event loop where nothing could test it.
//!
//! Don't let this grow into a general-purpose GUI framework (§16.2). It only
//! has to be good enough for chrome; the timeline and piano roll are canvases.

use std::collections::HashMap;

use crate::layout::Rect;

/// Opaque handle into the retained widget tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WidgetId(u32);

impl WidgetId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// What needs redrawing, and whether the event loop is allowed to sleep.
///
/// Two separate questions, deliberately:
///
/// - **Dirty region** — the union of everything that changed since the last
///   frame. Consumed by drawing it. If it is empty, there is no frame.
/// - **Animating** — whether something will *want* a frame soon (a moving
///   playhead, a falling meter), which is what decides whether the loop waits
///   for an OS event or keeps running.
///
/// Keeping them apart is what makes §16.4's "the playhead moving must not
/// redirty the note geometry" true by construction: an animator keeps the loop
/// awake but claims no pixels, so whatever moves still has to say *where*.
#[derive(Debug, Default, Clone)]
pub struct Redraw {
    /// `None` means clean. Not an empty `Rect`, because "nothing to draw" and
    /// "a zero-sized thing to draw" are the same picture and different frames.
    dirty: Option<Rect>,
    animators: u32,
}

impl Redraw {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `rect` to what will be redrawn on the next frame.
    ///
    /// Empty rectangles are dropped: invalidating nothing must not buy a frame.
    pub fn invalidate(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        self.dirty = Some(match self.dirty {
            Some(current) => current.union(&rect),
            None => rect,
        });
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.is_some()
    }

    /// The region to draw, clearing it.
    ///
    /// `None` means **issue no frame**. Calling this twice without an
    /// invalidation in between returns `None` the second time, which is the
    /// whole of §16.3 in one sentence.
    pub fn take_dirty(&mut self) -> Option<Rect> {
        self.dirty.take()
    }

    /// Registers something that will keep asking for frames.
    ///
    /// Counted rather than a flag: a moving playhead and a falling meter are
    /// two animators, and the first one to finish must not put the window to
    /// sleep under the second.
    pub fn begin_animating(&mut self) {
        self.animators = self.animators.saturating_add(1);
    }

    pub fn end_animating(&mut self) {
        self.animators = self.animators.saturating_sub(1);
    }

    pub fn is_animating(&self) -> bool {
        self.animators > 0
    }
}

/// The retained tree: what exists, where it is, and what of it changed.
///
/// Bounds live here rather than on the widgets so that invalidating one can
/// dirty exactly its rectangle — including, when it moves, the rectangle it
/// just left, which is the dirty-region bug everybody writes once.
#[derive(Debug, Default)]
pub struct WidgetTree {
    bounds: HashMap<WidgetId, Rect>,
    redraw: Redraw,
}

impl WidgetTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a widget, or replaces one. Either way it needs drawing.
    pub fn insert(&mut self, id: WidgetId, bounds: Rect) {
        if let Some(old) = self.bounds.insert(id, bounds) {
            self.redraw.invalidate(old);
        }
        self.redraw.invalidate(bounds);
    }

    /// Moves or resizes a widget, dirtying **both** the rectangle it left and
    /// the one it arrived at — redrawing only the new one leaves the old
    /// smeared across the screen.
    pub fn set_bounds(&mut self, id: WidgetId, bounds: Rect) {
        self.insert(id, bounds);
    }

    pub fn bounds(&self, id: WidgetId) -> Option<Rect> {
        self.bounds.get(&id).copied()
    }

    pub fn remove(&mut self, id: WidgetId) {
        if let Some(gone) = self.bounds.remove(&id) {
            self.redraw.invalidate(gone);
        }
    }

    /// Marks a widget's own rectangle dirty. An unknown id is a no-op, not an
    /// error: a widget can be removed while something still holds its handle.
    pub fn invalidate(&mut self, id: WidgetId) {
        if let Some(rect) = self.bounds(id) {
            self.redraw.invalidate(rect);
        }
    }

    /// Marks an arbitrary region dirty — a resize, or a surface the compositor
    /// asked us to repaint.
    pub fn invalidate_rect(&mut self, rect: Rect) {
        self.redraw.invalidate(rect);
    }

    pub fn has_dirty_regions(&self) -> bool {
        self.redraw.is_dirty()
    }

    /// See [`Redraw::take_dirty`]. `None` means issue no frame.
    pub fn take_dirty(&mut self) -> Option<Rect> {
        self.redraw.take_dirty()
    }

    pub fn redraw(&self) -> &Redraw {
        &self.redraw
    }

    pub fn redraw_mut(&mut self) -> &mut Redraw {
        &mut self.redraw
    }
}
