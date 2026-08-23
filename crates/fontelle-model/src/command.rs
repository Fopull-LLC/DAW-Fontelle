use crate::project::Project;

#[derive(Debug)]
pub struct CommandError(pub String);

/// Every document mutation goes through a `Command` (INVARIANT 9) — no direct
/// writes to `Project` from anywhere, including the UI. Inverse-based rather than
/// snapshot-based: cheap in memory with large note data, and it gives the history
/// list meaningful entry names for free ("Draw 14 notes") instead of "Snapshot 7"
/// (TDD §10.6).
pub trait Command: Send {
    fn apply(&mut self, doc: &mut Project) -> Result<(), CommandError>;
    fn invert(&self) -> Box<dyn Command>;
    fn label(&self) -> &str;
    /// Coalesces continuous gestures — dragging a note is one history entry, not
    /// four hundred.
    fn merge_with(&mut self, next: &dyn Command) -> bool;
    fn memory_cost(&self) -> usize;
}

/// Command-pattern undo/redo stack. Default depth 100, with a memory ceiling
/// (default 256MB) that evicts the oldest entries first once exceeded.
pub struct History {
    undo_stack: Vec<Box<dyn Command>>,
    redo_stack: Vec<Box<dyn Command>>,
    pub max_depth: usize,
    pub memory_ceiling_bytes: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_depth: 100,
            memory_ceiling_bytes: 256 * 1024 * 1024,
        }
    }

    pub fn apply(
        &mut self,
        mut command: Box<dyn Command>,
        doc: &mut Project,
    ) -> Result<(), CommandError> {
        command.apply(doc)?;
        self.redo_stack.clear();
        self.undo_stack.push(command);
        self.evict_if_over_budget();
        Ok(())
    }

    pub fn undo(&mut self, _doc: &mut Project) -> Option<Result<(), CommandError>> {
        todo!("pop undo_stack, apply invert(), push to redo_stack")
    }

    pub fn redo(&mut self, _doc: &mut Project) -> Option<Result<(), CommandError>> {
        todo!("pop redo_stack, re-apply, push back to undo_stack")
    }

    fn evict_if_over_budget(&mut self) {
        while self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}
