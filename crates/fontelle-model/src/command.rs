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
    /// This command as a value, ids and all, so another machine can apply it
    /// the way redo does (`docs/collab-plan.md` §5.3). Taken **after** apply,
    /// when the command knows what it minted. Every command has one —
    /// `tests/wire.rs` sends each of them across and back.
    fn to_edit(&self) -> crate::wire::Edit;
}

/// One edit on its way to whoever is sharing the song, and the history entry
/// it came from.
///
/// The entry is what a joiner's session needs if the host refuses the edit:
/// the entry goes with it ([`History::forget`]). An undo or a redo names the
/// entry it undid or redid.
#[derive(Debug, Clone)]
pub struct Outgoing {
    pub entry: u64,
    pub edit: crate::wire::Edit,
}

/// One undo entry: the command, and a number that names it for as long as it
/// lives — across undo and redo, and past entries evicted below it.
struct Entry {
    key: u64,
    command: Box<dyn Command>,
}

/// Command-pattern undo/redo stack. Default depth 100, with a memory ceiling
/// (default 256MB) that evicts the oldest entries first once exceeded.
pub struct History {
    undo_stack: Vec<Entry>,
    redo_stack: Vec<Entry>,
    next_key: u64,
    /// Set by `break_gesture`, and by an undo or a redo — resuming a drag
    /// across an undo is not the same drag.
    gesture_broken: bool,
    /// Every edit that has landed, in order, for whoever is sharing the song
    /// — `None` while nobody is, so a studio with no session open keeps no
    /// copies and behaves exactly as it did (`docs/collab-plan.md` §1, 9).
    outbox: Option<Vec<Outgoing>>,
    /// Whether the entry on top of the undo stack has gone to the outbox. A
    /// gesture still in the hand has not: it goes once, merged, when it
    /// breaks — a drag is one message when the button comes up, not four
    /// hundred.
    head_sent: bool,
    /// Where what this history applies mints its ids — see
    /// [`crate::arena::minting_in`]. `None` everywhere but a joiner's studio.
    mint_space: Option<u16>,
    /// Moves on every apply, merge, undo and redo — see
    /// [`generation`](Self::generation).
    generation: u64,
    pub max_depth: usize,
    pub memory_ceiling_bytes: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            next_key: 0,
            gesture_broken: true,
            outbox: None,
            head_sent: true,
            mint_space: None,
            generation: 0,
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
        crate::arena::minting_in(self.mint_space, || command.apply(doc))?;
        self.generation += 1;
        self.redo_stack.clear();

        if !self.gesture_broken
            && let Some(previous) = self.undo_stack.last_mut()
            && previous.command.merge_with(command.as_ref())
        {
            return Ok(());
        }
        // A new entry closes the one below it, as far as anybody else is
        // concerned: it can never be merged into again.
        self.send_head();
        self.gesture_broken = false;
        let key = self.next_key;
        self.next_key += 1;
        self.undo_stack.push(Entry { key, command });
        self.head_sent = false;
        self.evict_if_over_budget();
        Ok(())
    }

    /// Makes every command this history applies from now on mint its ids in
    /// `space` — a joiner's own, so nothing it makes can collide with what
    /// the host or another joiner makes (`docs/collab-plan.md` §18, F55).
    pub fn set_mint_space(&mut self, space: Option<u16>) {
        self.mint_space = space;
    }

    /// Starts keeping every edit that lands, for a shared session to send.
    pub fn open_outbox(&mut self) {
        self.outbox.get_or_insert_with(Vec::new);
        // Whatever was on top before the session began is in the snapshot
        // the others were given, not in the stream.
        self.head_sent = true;
    }

    /// Stops keeping them, and drops any that were not taken.
    pub fn close_outbox(&mut self) {
        self.outbox = None;
    }

    /// Every edit that has landed since the last call, in order: merged
    /// gestures once, when they break; an undo as the inverse it applied; a
    /// redo as the command again. Empty while no outbox is open.
    pub fn take_outbox(&mut self) -> Vec<crate::wire::Edit> {
        self.take_outgoing()
            .into_iter()
            .map(|outgoing| outgoing.edit)
            .collect()
    }

    /// [`take_outbox`](Self::take_outbox), with the entry each edit came from.
    pub fn take_outgoing(&mut self) -> Vec<Outgoing> {
        self.outbox.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// Whether the entry on top is a gesture still in the hand — applied
    /// here, not yet sent, and still able to take the next step of a drag.
    ///
    /// A shared session holds everything from outside while this is true: an
    /// edit from somebody else rebased under a drag in progress would move
    /// the ground the drag measured its limits on (§5.5, F17).
    pub fn gesture_in_hand(&self) -> bool {
        !self.head_sent
    }

    /// A number that moves every time the history applies anything — a new
    /// entry, a step merged into the last, an undo, a redo.
    ///
    /// How a shared session tells a drag still moving from an edit left in
    /// the hand with nothing more coming (a key press never has a mouse-up to
    /// end it): the same number twice, some time apart, is a gesture to let go
    /// of.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Takes the entry `key` out of the history, wherever it is, as though it
    /// had never been made.
    ///
    /// What a shared session does when the host refuses an edit that has
    /// already been applied here: the document is put back without it, and
    /// an undo that tried to take it back again would be undoing nothing
    /// (§5.5). Anything above it on the undo stack stays.
    pub fn forget(&mut self, key: u64) {
        let was_head = self.undo_stack.last().is_some_and(|entry| entry.key == key);
        self.undo_stack.retain(|entry| entry.key != key);
        self.redo_stack.retain(|entry| entry.key != key);
        if was_head {
            // Whatever is on top now went out when the forgotten one was
            // made above it.
            self.head_sent = true;
            self.gesture_broken = true;
        }
    }

    /// Applies an edit that came from somebody else.
    ///
    /// It changes the document and nothing else: it does not enter the undo
    /// stack, because undo is each person's own (§5.7), and it does not go to
    /// the outbox, because it came from the stream rather than into it. The
    /// gesture in progress, if any, is left alone.
    pub fn apply_foreign(
        &mut self,
        edit: crate::wire::Edit,
        doc: &mut Project,
    ) -> Result<(), CommandError> {
        edit.into_command().apply(doc)
    }

    /// Offers the entry on top of the undo stack to the outbox, once.
    fn send_head(&mut self) {
        if self.head_sent {
            return;
        }
        self.head_sent = true;
        if let (Some(outbox), Some(head)) = (self.outbox.as_mut(), self.undo_stack.last()) {
            outbox.push(Outgoing {
                entry: head.key,
                edit: head.command.to_edit(),
            });
        }
    }

    fn send(&mut self, entry: u64, edit: impl FnOnce() -> crate::wire::Edit) {
        if let Some(outbox) = self.outbox.as_mut() {
            outbox.push(Outgoing {
                entry,
                edit: edit(),
            });
        }
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
        self.send_head();
    }

    /// The entry on top of the undo stack — the command just applied, unless
    /// it merged into the one before it.
    ///
    /// The piano roll needs this and there is no other way to get it: drawing
    /// a note and dragging it to length is one gesture, and its second half
    /// needs the `NoteId` the first half minted. `AddNotes` keeps that id, and
    /// the history has owned the command since it was applied. Downcast
    /// through `Command::as_any`.
    pub fn last_applied(&self) -> Option<&dyn Command> {
        self.undo_stack.last().map(|entry| entry.command.as_ref())
    }

    /// How many entries `undo` could walk back through.
    pub fn depth(&self) -> usize {
        self.undo_stack.len()
    }

    /// The label of the entry `undo` would take back, for the Edit menu.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo_stack.last().map(|entry| entry.command.label())
    }

    /// The label of the entry `redo` would put back.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|entry| entry.command.label())
    }

    /// `None` when there is nothing to undo. An inverse that fails puts its
    /// entry back rather than losing it.
    pub fn undo(&mut self, doc: &mut Project) -> Option<Result<(), CommandError>> {
        // A gesture nobody has been sent yet goes first, so the other side
        // never inverts a thing it was never given.
        self.send_head();
        let entry = self.undo_stack.pop()?;
        let mut inverse = entry.command.invert();
        match crate::arena::minting_in(self.mint_space, || inverse.apply(doc)) {
            Ok(()) => {
                self.generation += 1;
                self.send(entry.key, || inverse.to_edit());
                self.redo_stack.push(entry);
                self.gesture_broken = true;
                Some(Ok(()))
            }
            Err(e) => {
                self.undo_stack.push(entry);
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
        self.send_head();
        let mut entry = self.redo_stack.pop()?;
        match crate::arena::minting_in(self.mint_space, || entry.command.apply(doc)) {
            Ok(()) => {
                self.generation += 1;
                let edit = entry.command.to_edit();
                self.send(entry.key, || edit);
                self.undo_stack.push(entry);
                self.gesture_broken = true;
                Some(Ok(()))
            }
            Err(e) => {
                self.redo_stack.push(entry);
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
        let mut total: usize = self
            .undo_stack
            .iter()
            .map(|entry| entry.command.memory_cost())
            .sum();
        while self.undo_stack.len() > 1 && total > self.memory_ceiling_bytes {
            total -= self.undo_stack.remove(0).command.memory_cost();
        }
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}
