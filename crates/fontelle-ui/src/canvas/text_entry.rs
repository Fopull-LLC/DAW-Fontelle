//! Being edited: a string, a caret, and the other end of a selection.
//!
//! > *"it doesn't look like a input field it's just text on a background
//! > making it look like it's a label and not somewhere you can type you can't
//! > even see your cursor where you're typing and i can't ctrl a to select all
//! > my text and stuff like that."*
//!
//! Three complaints, one cause: **there was no model of being edited.** Every
//! place you could type in this program held a bare `String` and drew it with
//! a block character stuck on the end — so there was no caret to move, no
//! selection to make, and nothing for Ctrl+A to select. A field that cannot
//! say where its caret is cannot draw one, and a field that cannot draw one
//! looks like a label. This is the missing half.
//!
//! **Where the split is.** The model is here, in the canvas, where it can be
//! tested without a window. The *geometry* — where the caret sits in pixels,
//! how wide the selection is — belongs to the renderer, because placing a
//! caret means measuring the text before it and shaping text is not something
//! this crate may do (INVARIANT 2). So this holds a byte index and the
//! renderer turns it into an x.
//!
//! Byte indices, always on a character boundary: Rust strings are bytes, and a
//! caret that could land inside a multi-byte character is a panic waiting for
//! somebody to type an accent.

/// A string being typed into.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextEntry {
    text: String,
    /// Where the caret is, as a byte index into `text`.
    caret: usize,
    /// The far end of the selection, when there is one. The caret is the
    /// *moving* end — which is what makes shift-arrow extend from where you
    /// last were rather than from wherever the selection happens to start.
    anchor: Option<usize>,
}

impl TextEntry {
    /// A field holding `text`, with the caret **at the end**.
    ///
    /// At the end rather than the start: a field you open on an existing name
    /// is one you are usually about to add to, and starting at the front would
    /// mean pressing End before every rename.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = text.len();
        Self {
            text,
            caret,
            anchor: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The selection as `(from, to)` with `from <= to`, or `None` for a plain
    /// caret. An anchor sitting exactly on the caret is not a selection — it
    /// is a caret, and returning an empty range would make every draw check
    /// for one.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        if anchor == self.caret {
            return None;
        }
        Some((anchor.min(self.caret), anchor.max(self.caret)))
    }

    pub fn selected_text(&self) -> &str {
        match self.selection() {
            Some((from, to)) => &self.text[from..to],
            None => "",
        }
    }

    /// Everything, selected — Ctrl+A.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    /// Empties it and puts the caret back at the start.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Replaces whatever the field held.
    pub fn set(&mut self, text: impl Into<String>) {
        *self = Self::new(text);
    }

    /// Types `what` in, over the selection if there is one.
    pub fn insert(&mut self, what: &str) {
        self.delete_selection();
        self.text.insert_str(self.caret, what);
        self.caret += what.len();
    }

    /// The key to the left of the caret, or the selection if there is one.
    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        let Some(previous) = self.previous_boundary(self.caret) else {
            return;
        };
        self.text.replace_range(previous..self.caret, "");
        self.caret = previous;
    }

    /// The key to the right of it.
    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        let Some(next) = self.next_boundary(self.caret) else {
            return;
        };
        self.text.replace_range(self.caret..next, "");
    }

    /// Takes the selection out. Whether there was one.
    pub fn delete_selection(&mut self) -> bool {
        let Some((from, to)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.text.replace_range(from..to, "");
        self.caret = from;
        self.anchor = None;
        true
    }

    /// Puts the caret at `at`, extending the selection with `select`.
    ///
    /// Clamped into the string and onto a character boundary, because the
    /// two callers that need it are handed a *position* rather than a step: a
    /// click, which arrives as a place on screen, and a page's Up and Down,
    /// which arrive as a row and a column
    /// ([`notepad_step_row`](super::notepad_step_row)). Everything else moves
    /// the caret by asking for a direction.
    pub fn place(&mut self, at: usize, select: bool) {
        self.begin(select);
        let at = at.min(self.text.len());
        // Onto the boundary at or before `at`: a caret inside a multi-byte
        // character is a panic waiting for somebody to type an accent.
        self.caret = if self.text.is_char_boundary(at) {
            at
        } else {
            self.text[..at]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index)
        };
    }

    /// Left by one character, or by one **word** with `word`, extending the
    /// selection with `select`.
    ///
    /// A plain arrow over a selection **collapses to its near edge** rather
    /// than stepping from the caret. That is what every text field does, and
    /// it is what stops a selection being nudged into a different selection
    /// when somebody meant to dismiss it.
    pub fn left(&mut self, word: bool, select: bool) {
        if !select && let Some((from, _)) = self.selection() {
            self.caret = from;
            self.anchor = None;
            return;
        }
        self.begin(select);
        self.caret = if word {
            self.word_left(self.caret)
        } else {
            self.previous_boundary(self.caret).unwrap_or(0)
        };
    }

    pub fn right(&mut self, word: bool, select: bool) {
        if !select && let Some((_, to)) = self.selection() {
            self.caret = to;
            self.anchor = None;
            return;
        }
        self.begin(select);
        self.caret = if word {
            self.word_right(self.caret)
        } else {
            self.next_boundary(self.caret).unwrap_or(self.text.len())
        };
    }

    pub fn home(&mut self, select: bool) {
        self.begin(select);
        self.caret = 0;
    }

    pub fn end(&mut self, select: bool) {
        self.begin(select);
        self.caret = self.text.len();
    }

    /// Starts or clears the selection before a move.
    fn begin(&mut self, select: bool) {
        if select {
            // From where the caret is now, if there was no selection: shift
            // extends from where you were, not from where the last one began.
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
    }

    fn previous_boundary(&self, at: usize) -> Option<usize> {
        if at == 0 {
            return None;
        }
        Some(
            self.text[..at]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index),
        )
    }

    fn next_boundary(&self, at: usize) -> Option<usize> {
        if at >= self.text.len() {
            return None;
        }
        Some(at + self.text[at..].chars().next().map_or(1, char::len_utf8))
    }

    /// The start of the word before `at` — past any spaces first, so
    /// Ctrl+Left from the end of "one two " lands on "two" and not on the
    /// space in front of it.
    fn word_left(&self, at: usize) -> usize {
        let mut index = at;
        while let Some(previous) = self.previous_boundary(index) {
            if !self.text[previous..].starts_with(char::is_whitespace) {
                break;
            }
            index = previous;
        }
        while let Some(previous) = self.previous_boundary(index) {
            if self.text[previous..].starts_with(char::is_whitespace) {
                break;
            }
            index = previous;
        }
        index
    }

    fn word_right(&self, at: usize) -> usize {
        let mut index = at;
        while index < self.text.len() {
            let Some(next) = self.next_boundary(index) else {
                break;
            };
            if self.text[index..].starts_with(char::is_whitespace) {
                break;
            }
            index = next;
        }
        while index < self.text.len() {
            let Some(next) = self.next_boundary(index) else {
                break;
            };
            if !self.text[index..].starts_with(char::is_whitespace) {
                break;
            }
            index = next;
        }
        index
    }
}

/// What a key did to a field, or that it was not a field's key at all.
///
/// Returned rather than acted on, because the three places that own a field —
/// a menu's name prompt, the browser's search, an inline rename — each have
/// something of their own to do afterwards: relay out the menu, refilter the
/// list, write the name into the document. What they must **not** each have is
/// their own idea of what Ctrl+A means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKey {
    /// The field changed. Whoever owns it should do whatever it does next.
    Edited,
    /// A key the field claims but which changed nothing — a caret move.
    Moved,
    /// Not a field's key. The caller should carry on and handle it itself.
    Ignored,
}

/// One key, applied to `entry`.
///
/// **One implementation for every field in the program.** They were three, and
/// the one that went stale was always the one you were not looking at: the
/// menu prompt had backspace and typing, the rename had backspace and typing,
/// and neither had a caret to move or a selection to make.
///
/// `clipboard` is read for paste and written for cut and copy, so a name can
/// be moved from one field to another the way anything else is moved.
pub fn text_key(
    entry: &mut TextEntry,
    key: &winit::keyboard::Key,
    ctrl: bool,
    shift: bool,
    clipboard: &mut String,
) -> TextKey {
    use winit::keyboard::{Key, NamedKey};
    match key {
        Key::Named(NamedKey::Backspace) => {
            entry.backspace();
            TextKey::Edited
        }
        Key::Named(NamedKey::Delete) => {
            entry.delete();
            TextKey::Edited
        }
        Key::Named(NamedKey::ArrowLeft) => {
            entry.left(ctrl, shift);
            TextKey::Moved
        }
        Key::Named(NamedKey::ArrowRight) => {
            entry.right(ctrl, shift);
            TextKey::Moved
        }
        Key::Named(NamedKey::Home) => {
            entry.home(shift);
            TextKey::Moved
        }
        Key::Named(NamedKey::End) => {
            entry.end(shift);
            TextKey::Moved
        }
        Key::Named(NamedKey::Space) if !ctrl => {
            entry.insert(" ");
            TextKey::Edited
        }
        Key::Character(text) if ctrl => match text.as_str() {
            "a" => {
                entry.select_all();
                TextKey::Moved
            }
            "c" => {
                clipboard.clear();
                clipboard.push_str(entry.selected_text());
                TextKey::Moved
            }
            "x" => {
                clipboard.clear();
                clipboard.push_str(entry.selected_text());
                if entry.delete_selection() {
                    TextKey::Edited
                } else {
                    TextKey::Moved
                }
            }
            "v" => {
                if clipboard.is_empty() {
                    TextKey::Moved
                } else {
                    let pasted = clipboard.clone();
                    entry.insert(&pasted);
                    TextKey::Edited
                }
            }
            _ => TextKey::Ignored,
        },
        Key::Character(text) => {
            // Control characters are keys, not text: a field that inserted
            // them would fill up with things nobody can see or delete.
            let typed: String = text.chars().filter(|c| !c.is_control()).collect();
            if typed.is_empty() {
                return TextKey::Ignored;
            }
            entry.insert(&typed);
            TextKey::Edited
        }
        _ => TextKey::Ignored,
    }
}
