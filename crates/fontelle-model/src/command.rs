use crate::project::Project;

#[derive(Debug)]
pub struct CommandError(pub String);

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CommandError {}

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
    /// Lets `merge_with` ask what kind of edit is arriving.
    ///
    /// Coalescing is only ever defined between two commands of the same type —
    /// a fader move absorbs a fader move, not a note drag that happens to
    /// share a label — and downcasting is the honest way to ask. Every
    /// implementation is the same line: `self`.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Command-pattern undo/redo stack. Default depth 100, with a memory ceiling
/// (default 256MB) that evicts the oldest entries first once exceeded.
pub struct History {
    undo_stack: Vec<Box<dyn Command>>,
    redo_stack: Vec<Box<dyn Command>>,
    /// Set by `break_gesture`, and by an undo or a redo — resuming a drag
    /// across an undo is not the same drag.
    gesture_broken: bool,
    pub max_depth: usize,
    pub memory_ceiling_bytes: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            gesture_broken: true,
            max_depth: 100,
            memory_ceiling_bytes: 256 * 1024 * 1024,
        }
    }

    /// Runs `command` and records it, coalescing it into the previous entry
    /// when both are part of the same gesture (see [`Self::break_gesture`]).
    ///
    /// A command that fails leaves the history exactly as it was: a refused
    /// edit is not an undo entry, and offering to undo something that never
    /// happened is worse than offering nothing.
    pub fn apply(
        &mut self,
        mut command: Box<dyn Command>,
        doc: &mut Project,
    ) -> Result<(), CommandError> {
        command.apply(doc)?;
        self.redo_stack.clear();

        if !self.gesture_broken
            && let Some(previous) = self.undo_stack.last_mut()
            && previous.merge_with(command.as_ref())
        {
            return Ok(());
        }
        self.gesture_broken = false;
        self.undo_stack.push(command);
        self.evict_if_over_budget();
        Ok(())
    }

    /// Ends the current gesture, so the next command starts a new history
    /// entry rather than coalescing into the last one.
    ///
    /// TDD §10.6 asks for one entry per drag and `merge_with` is how, but the
    /// trait cannot tell a drag's four hundredth step from a deliberate second
    /// nudge a minute later. Only the caller knows the mouse came up. A time
    /// window would be guesswork that either splits a slow drag or swallows an
    /// edit the user meant to keep.
    pub fn break_gesture(&mut self) {
        self.gesture_broken = true;
    }

    /// How many entries `undo` could walk back through.
    pub fn depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// The label of the entry `undo` would take back, for the Edit menu.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo_stack.last().map(|c| c.label())
    }

    /// The label of the entry `redo` would put back.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|c| c.label())
    }

    /// `None` when there is nothing to undo. An inverse that fails puts its
    /// entry back rather than losing it.
    pub fn undo(&mut self, doc: &mut Project) -> Option<Result<(), CommandError>> {
        let command = self.undo_stack.pop()?;
        match command.invert().apply(doc) {
            Ok(()) => {
                self.redo_stack.push(command);
                self.gesture_broken = true;
                Some(Ok(()))
            }
            Err(e) => {
                self.undo_stack.push(command);
                Some(Err(e))
            }
        }
    }

    /// Re-applies the command itself rather than an inverse of the inverse.
    ///
    /// That is what makes a redone insertion give back the id it minted the
    /// first time: the command remembers what it created, and `Arena` lets it
    /// put the element back under exactly that key. Without both halves, a
    /// redo would mint a fresh id and the *next* redo would be looking for a
    /// note that no longer exists.
    pub fn redo(&mut self, doc: &mut Project) -> Option<Result<(), CommandError>> {
        let mut command = self.redo_stack.pop()?;
        match command.apply(doc) {
            Ok(()) => {
                self.undo_stack.push(command);
                self.gesture_broken = true;
                Some(Ok(()))
            }
            Err(e) => {
                self.redo_stack.push(command);
                Some(Err(e))
            }
        }
    }

    /// Drops the oldest entries once either limit is exceeded (TDD §10.6).
    ///
    /// The memory ceiling matters more than the depth on this document: a
    /// hundred fader moves cost nothing, and a hundred "delete every note in
    /// the piece" commands each hold a copy of what they removed.
    fn evict_if_over_budget(&mut self) {
        while self.undo_stack.len() > self.max_depth {
            self.undo_stack.remove(0);
        }
        // Always keep one, or a single edit larger than the ceiling would be
        // unundoable the moment it happened.
        let mut total: usize = self.undo_stack.iter().map(|c| c.memory_cost()).sum();
        while self.undo_stack.len() > 1 && total > self.memory_ceiling_bytes {
            total -= self.undo_stack.remove(0).memory_cost();
        }
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}
