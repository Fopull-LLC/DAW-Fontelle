//! The vocal track's pieces of the window that are not pictures: the inline
//! lyric field's keys, and the singer picker's rows.
//!
//! The field is how a whole song's words get written, so it is built for
//! speed the way a vocal producer types: select a note, type, **Tab** to the
//! next one, type — a line without touching the mouse — and with several
//! notes selected, one field holds the whole line and spreads it a word (or
//! a hyphenated syllable) per note on Enter. Escape leaves everything as it
//! was. A pure state machine, like the roll: it hands back what to do and
//! the window does it, through the host, as one undo step per commit.

use fontelle_model::{Arena, Note, VocalVoice};
use fontelle_types::NoteId;
use winit::keyboard::{Key, NamedKey};

use super::menu::{CHOSEN_MARK, MenuEntry};
use super::text_entry::{TextClipboard, TextEntry, TextKey, text_key};
use crate::document::SingerChoice;

/// What a key did to the lyric field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LyricKey {
    /// The text changed or the caret moved; the field stays open.
    Edited,
    /// Not the field's key.
    Ignored,
    /// Keep `text` on `ids` and close. `line` is a line to spread over
    /// several notes rather than one note's own word.
    Commit {
        ids: Vec<NoteId>,
        text: String,
        line: bool,
    },
    /// Keep it and open the field on `to` — Tab, Shift+Tab.
    CommitAndMove {
        ids: Vec<NoteId>,
        text: String,
        line: bool,
        to: NoteId,
    },
    /// Leave everything as it was and close.
    Cancel,
}

/// The inline lyric field: the notes it is for, in time order, and what is
/// typed.
#[derive(Debug, Clone)]
pub struct LyricEditor {
    ids: Vec<NoteId>,
    entry: TextEntry,
}

/// `ids`, sorted the way a line is sung: by start, the higher of two
/// together first. Notes not in `notes` are dropped.
fn in_time_order(notes: &Arena<NoteId, Note>, ids: &[NoteId]) -> Vec<NoteId> {
    let mut out: Vec<NoteId> = ids
        .iter()
        .copied()
        .filter(|id| notes.get(*id).is_some())
        .collect();
    out.sort_by_key(|id| (notes[*id].start, std::cmp::Reverse(notes[*id].key)));
    out.dedup();
    out
}

/// Several notes' lyrics as they would be typed: words apart, a syllable
/// ending in a hyphen joined to the next (`"hel-" "lo"` is `hel-lo`).
pub fn line_text(notes: &Arena<NoteId, Note>, ids: &[NoteId]) -> String {
    let mut out = String::new();
    for id in in_time_order(notes, ids) {
        let lyric = notes[id].lyric.trim();
        if lyric.is_empty() {
            continue;
        }
        if !out.is_empty() && !out.ends_with('-') {
            out.push(' ');
        }
        // A lone "-" after a hyphenated syllable is a slur, not a join.
        if lyric == "-" && out.ends_with('-') {
            out.push(' ');
        }
        out.push_str(lyric);
    }
    out
}

impl LyricEditor {
    /// The field on `ids`: one note's lyric, or several notes' as a line,
    /// all of it selected so typing replaces it. `None` with no notes.
    pub fn open(notes: &Arena<NoteId, Note>, ids: &[NoteId]) -> Option<Self> {
        let ids = in_time_order(notes, ids);
        let text = match ids.as_slice() {
            [] => return None,
            [one] => notes[*one].lyric.clone(),
            many => line_text(notes, many),
        };
        let mut entry = TextEntry::new(text);
        entry.select_all();
        Some(Self { ids, entry })
    }

    /// The notes it is for, in time order. The first is where it is drawn.
    pub fn ids(&self) -> &[NoteId] {
        &self.ids
    }

    pub fn entry(&self) -> &TextEntry {
        &self.entry
    }

    /// Whether this is a line over several notes.
    pub fn is_line(&self) -> bool {
        self.ids.len() > 1
    }

    /// Text typed through an input method (a finished composition), as if
    /// typed.
    pub fn commit_text(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        self.entry.insert(&clean);
    }

    /// A key while the field is open. The field takes every key there is:
    /// a "d" is a letter in a word, not the delete tool.
    pub fn key(
        &mut self,
        key: &Key,
        ctrl: bool,
        shift: bool,
        clipboard: &mut TextClipboard,
        notes: &Arena<NoteId, Note>,
    ) -> LyricKey {
        let line = self.is_line();
        let text = || self.entry.text().trim().to_string();
        match key {
            Key::Named(NamedKey::Enter) => LyricKey::Commit {
                ids: self.ids.clone(),
                text: text(),
                line,
            },
            Key::Named(NamedKey::Escape) => LyricKey::Cancel,
            Key::Named(NamedKey::Tab) => {
                let anchor = if shift {
                    self.ids.first()
                } else {
                    self.ids.last()
                };
                let to = anchor.and_then(|at| neighbour(notes, *at, !shift));
                match to {
                    Some(to) => LyricKey::CommitAndMove {
                        ids: self.ids.clone(),
                        text: text(),
                        line,
                        to,
                    },
                    None => LyricKey::Commit {
                        ids: self.ids.clone(),
                        text: text(),
                        line,
                    },
                }
            }
            key => match text_key(&mut self.entry, key, ctrl, shift, clipboard) {
                TextKey::Ignored => LyricKey::Ignored,
                _ => LyricKey::Edited,
            },
        }
    }
}

/// The note after (or before) `at` in time order, among `notes`.
pub fn neighbour(notes: &Arena<NoteId, Note>, at: NoteId, forward: bool) -> Option<NoteId> {
    let all: Vec<NoteId> = in_time_order(notes, &notes.keys().collect::<Vec<_>>());
    let here = all.iter().position(|id| *id == at)?;
    if forward {
        all.get(here + 1).copied()
    } else {
        here.checked_sub(1).and_then(|i| all.get(i).copied())
    }
}

/// What a row of the singer picker does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SingerRow {
    /// A heading or a line of information.
    Info,
    /// Sing with `singer` (an index into the list the rows were made from)
    /// in `language`.
    Choose { singer: usize, language: String },
    /// Nobody: the track keeps its notes and words and is silent.
    Nobody,
}

/// The colour of the research-only badge beside a singer's name.
pub const RESEARCH_BADGE: [u8; 4] = [0xe0, 0x8a, 0x2e, 0xff];

/// A language's name for the picker, from its tag.
pub fn language_name(tag: &str) -> String {
    let primary = tag.split(['-', '_']).next().unwrap_or(tag);
    let name = match primary {
        "en" => "English",
        "ja" => "Japanese",
        "zh" => "Chinese",
        "ko" => "Korean",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        "it" => "Italian",
        "pt" => "Portuguese",
        _ => return tag.to_string(),
    };
    if primary == tag {
        name.to_string()
    } else {
        format!("{name} ({tag})")
    }
}

/// The singer picker: every installed singer in each of its languages,
/// engine by engine, the chosen one marked; research-only voices badged and
/// said so; engines that did not load named; and with none installed, where
/// one goes.
pub fn singer_menu_rows(
    singers: &[SingerChoice],
    current: Option<&VocalVoice>,
    problems: &[String],
    folder_hint: &str,
) -> Vec<(MenuEntry, SingerRow)> {
    let mut rows = vec![(MenuEntry::disabled("Singer"), SingerRow::Info)];
    if singers.is_empty() {
        rows.push((
            MenuEntry::disabled("No singer engine is installed"),
            SingerRow::Info,
        ));
        if !folder_hint.is_empty() {
            rows.push((
                MenuEntry::disabled(format!("Put one in {folder_hint}")),
                SingerRow::Info,
            ));
        }
    }
    let mut engine: Option<&str> = None;
    for (index, singer) in singers.iter().enumerate() {
        if engine != Some(singer.engine.as_str()) {
            engine = Some(singer.engine.as_str());
            rows.push((
                MenuEntry::disabled(singer.engine.clone()).after_rule(),
                SingerRow::Info,
            ));
        }
        for language in &singer.languages {
            let chosen = current.is_some_and(|c| {
                c.engine == singer.engine && c.singer == singer.id && &c.language == language
            });
            let label = format!(
                "{}{} \u{2014} {}{}",
                if chosen { CHOSEN_MARK } else { "" },
                singer.name,
                language_name(language),
                if singer.research_only {
                    "  \u{26a0} research only \u{2014} don't share"
                } else {
                    ""
                }
            );
            let mut entry = if chosen {
                MenuEntry::disabled(label)
            } else {
                MenuEntry::new(label)
            };
            if singer.research_only {
                entry = entry.with_swatch(RESEARCH_BADGE);
            }
            rows.push((
                entry,
                SingerRow::Choose {
                    singer: index,
                    language: language.clone(),
                },
            ));
        }
    }
    if !singers.is_empty() {
        rows.push((
            if current.is_none() {
                MenuEntry::disabled(format!("{CHOSEN_MARK}No singer"))
            } else {
                MenuEntry::new("No singer")
            }
            .after_rule(),
            SingerRow::Nobody,
        ));
    }
    for problem in problems {
        rows.push((
            MenuEntry::disabled(format!("Not loaded: {problem}")),
            SingerRow::Info,
        ));
    }
    rows
}
