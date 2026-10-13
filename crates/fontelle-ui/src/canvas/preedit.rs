//! An input method's composition, shown while it is composed.
//!
//! Japanese and Chinese are typed through an input method: the kana or
//! pinyin being composed is the IME's until it is committed, and winit
//! hands it over as `Ime::Preedit(text, cursor)` on every keystroke. A
//! field that ignores those shows nothing at all until Enter — you type
//! blind. This is the composition as the field draws it: spliced in at the
//! caret (over the selection, which the commit will replace), underlined,
//! with the IME's own cursor. The field's text is untouched until the
//! commit arrives (`Ime::Commit`), which goes through the field's `insert`
//! like typing.

use super::text_entry::TextEntry;

/// What the input method is composing, and its cursor (byte offsets into
/// the composition; `None` is a hidden cursor).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preedit {
    text: String,
    cursor: Option<(usize, usize)>,
}

fn floor_boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

impl Preedit {
    /// The composition as the IME last said; an empty one is none.
    pub fn set(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        if text.is_empty() {
            self.clear();
            return;
        }
        self.text = text.chars().filter(|c| !c.is_control()).collect();
        self.cursor =
            cursor.map(|(a, b)| (floor_boundary(&self.text, a), floor_boundary(&self.text, b)));
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = None;
    }

    pub fn is_active(&self) -> bool {
        !self.text.is_empty()
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// A field as drawn: its text with any composition in it, the caret, the
/// selection, and the composition's span to underline (byte offsets into
/// `text`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    pub text: String,
    pub caret: usize,
    pub selection: Option<(usize, usize)>,
    pub underline: Option<(usize, usize)>,
}

/// `entry` as drawn while `preedit` is being composed into it.
pub fn compose(entry: &TextEntry, preedit: &Preedit) -> Composed {
    if !preedit.is_active() {
        return Composed {
            text: entry.text().to_string(),
            caret: entry.caret(),
            selection: entry.selection(),
            underline: None,
        };
    }
    let field = entry.text();
    let (a, b) = entry.selection().unwrap_or((entry.caret(), entry.caret()));
    let (a, b) = (floor_boundary(field, a), floor_boundary(field, b));
    let pre = preedit.text();
    let mut text = String::with_capacity(field.len() + pre.len());
    text.push_str(&field[..a]);
    text.push_str(pre);
    text.push_str(&field[b..]);
    let caret = a + preedit.cursor.map_or(pre.len(), |(_, end)| end);
    Composed {
        text,
        caret,
        selection: None,
        underline: Some((a, a + pre.len())),
    }
}
