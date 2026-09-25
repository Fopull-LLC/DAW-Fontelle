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

/// How often the window draws while something on it is moving.
///
/// Sixty a second: the §19 target, and the fastest anything here can usefully
/// change.
pub const FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_micros(16_667);

/// How often an idle window looks at the engine it is showing.
///
/// **This is not a frame rate.** The loop wakes, reads a handful of atomics,
/// finds nothing changed and goes straight back to sleep having drawn nothing;
/// §16.3's promise is about frames issued, and this issues none.
///
/// It exists because the transport is shared state that something other than
/// the window can change — the CLI that opened the project, a record armed by
/// a MIDI event, a second view later on. A window asleep in `Wait` never
/// learns that playback started, and what the user gets is a frozen playhead
/// over audio they can hear. That is not a hypothetical: it is what the first
/// run of the transport bar did.
pub const ENGINE_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// How long the event loop may sleep before it has to look again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sleep {
    /// Indefinitely — until the OS has something to say. Nothing on this side
    /// will change on its own.
    Forever,
    AtMost(std::time::Duration),
}

/// The §16.3 decision, as a value rather than as a shape the event loop
/// happens to have.
///
/// `watching_engine` is whether there is anything behind the window whose
/// state can change without the user touching it.
pub fn sleep_budget(animating: bool, watching_engine: bool) -> Sleep {
    match (animating, watching_engine) {
        (true, _) => Sleep::AtMost(FRAME_INTERVAL),
        (false, true) => Sleep::AtMost(ENGINE_POLL),
        (false, false) => Sleep::Forever,
    }
}

/// Whether anything behind the window can change without the user touching
/// it: the engine it is showing, or a shared song somebody else is editing
/// (`docs/collab-plan.md` §9.2, F42). Either keeps the window looking every
/// [`ENGINE_POLL`]; neither lets it sleep until the OS has something to say.
pub fn watching(engine: bool, session: bool) -> bool {
    engine || session
}

/// Whether an autosave is due, given how long it has been since the last one.
///
/// Here rather than in `fontelle-app` for the reason everything in this module
/// is: it is a decision about **when the window does something**, and this is
/// where "may the window sleep" already lives.
///
/// Pure, so the interval is a test rather than something to notice by leaving
/// a window open for a minute — and the window only asks while it is *awake*.
/// It sleeps at idle (§16.3), so a window nobody is touching takes no backups,
/// which is right: there is nothing new in it to lose.
pub fn autosave_due(since_last: std::time::Duration, every: std::time::Duration) -> bool {
    since_last >= every
}
