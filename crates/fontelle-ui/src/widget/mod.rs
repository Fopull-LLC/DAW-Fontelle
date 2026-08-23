/// Opaque handle into the retained widget tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WidgetId(u32);

/// Retained tree with explicit invalidation (TDD §16.3): redraw only dirty
/// regions, and when the transport is stopped and nothing is animating, issue no
/// frames at all — required for the idle-CPU target, easier to build in now than
/// to retrofit.
pub struct WidgetTree {
    dirty: Vec<WidgetId>,
}

impl WidgetTree {
    pub fn new() -> Self {
        Self { dirty: Vec::new() }
    }

    pub fn mark_dirty(&mut self, id: WidgetId) {
        self.dirty.push(id);
    }

    pub fn has_dirty_regions(&self) -> bool {
        !self.dirty.is_empty()
    }

    pub fn take_dirty(&mut self) -> Vec<WidgetId> {
        std::mem::take(&mut self.dirty)
    }
}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}
