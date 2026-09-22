//! The notepad: an insert that makes no sound and holds words
//! (`docs/effects-catalogue.md` §2.8).
//!
//! > *"make a new built in mixer track effect called Notepad. its just a basic
//! > text editor with pages you can go left and right between and use it like a
//! > normal text editor to write down lyrics for example as you record to sing
//! > them back."*
//!
//! # Why the words are not in the config
//!
//! Every other effect's whole state is its [`EffectConfig`](crate::EffectConfig),
//! which is `Copy`, fixed-size, and handed to the audio thread every block. A
//! page of lyrics is none of those things. So the pad is split exactly the way
//! a hosted plugin is (`EffectSlot::plugin` says the same thing in the same
//! words): [`NotepadConfig`] is the knobs — how it *looks* — and
//! [`NotepadPages`] is what it *says*, stored beside the slot and never read on
//! the audio thread. Nothing here crosses to the engine at all; the notepad's
//! signal path is a wire.
//!
//! # Why the edits are an algebra
//!
//! [`NotepadEdit::apply`] returns **the inverse of what it just did**, so the
//! command in `fontelle-model` has nothing to work out and no second copy of
//! these rules to keep in step — the same shape `WavetableEdit` takes, and for
//! the same reason.

use crate::effect::{MIX, with_mix};

/// The pad is a wire, so it opens fully wet like every other processor.
const ALL_WET: f32 = 100.0;

fn all_wet() -> f32 {
    1.0
}

/// How a notepad is painted (`docs/effects-catalogue.md` §2.8).
///
/// Seven, and they are the *presets* too: a pad has nothing to say about the
/// sound, so its bank is its looks (see [`NotepadPreset`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NotepadTheme {
    /// Green on near-black: the terminal everybody pictures.
    Phosphor,
    /// The other CRT — amber on brown-black, easier on the eyes at night.
    Amber,
    /// Dark ink on warm paper, for a room with the lights on.
    Paper,
    /// Pale blue on deep navy.
    Ice,
    /// Pink on aubergine.
    Rose,
    /// White on black, no colour at all.
    Slate,
    /// Fontelle's own inks, so the pad matches the studio around it.
    Studio,
}

impl NotepadTheme {
    /// What the window and the preset row call it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Phosphor => "phosphor",
            Self::Amber => "amber",
            Self::Paper => "paper",
            Self::Ice => "ice",
            Self::Rose => "rose",
            Self::Slate => "slate",
            Self::Studio => "studio",
        }
    }

    /// What the **settings file** remembers, so that the last theme chosen is
    /// the next pad's default (`Settings::notepad_theme`).
    ///
    /// A word rather than a position, and that is the whole point of it: an
    /// index would mean that adding a theme in the middle of the list silently
    /// changed somebody's default to a different one.
    pub fn slug(self) -> &'static str {
        // The same word as the label today, and a separate function because
        // the two answer to different rules: a label may be renamed, a slug
        // is in a file on somebody's disk.
        self.label()
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.slug() == slug)
    }

    pub const ALL: [Self; 7] = [
        Self::Phosphor,
        Self::Amber,
        Self::Paper,
        Self::Ice,
        Self::Rose,
        Self::Slate,
        Self::Studio,
    ];

    /// Where this one sits on the control.
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|theme| *theme == self)
            .unwrap_or(0)
    }

    /// The next one round, which is what a click on the theme chip does.
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// The one before, which is what a right-click on it does. Seven is too
    /// many to walk one way round.
    pub fn previous(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// How big the words are drawn.
///
/// Three rather than a continuous size: the pad is read from across a room
/// while somebody sings, and "the big one" is a decision rather than a number
/// to dial in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NotepadSize {
    Small,
    Medium,
    Large,
}

impl NotepadSize {
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|size| *size == self).unwrap_or(0)
    }

    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// What a notepad insert is set to: how it looks, and the mix every effect
/// has.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NotepadConfig {
    pub theme: NotepadTheme,
    pub size: NotepadSize,
    /// The one control every effect carries (`crate::MIX`), and the one on
    /// this effect that does nothing at all: the pad passes the signal
    /// through untouched, so a blend of it with itself is itself at any
    /// setting. It is here because an automation lane naming `mix` must find
    /// one whatever the slot holds (`tests/effect_mix.rs`), and the pad's own
    /// window does not draw it.
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl NotepadConfig {
    pub fn new() -> Self {
        Self {
            theme: NotepadTheme::Phosphor,
            size: NotepadSize::Medium,
            mix: 1.0,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "theme" => self.theme.index() as f32,
            "size" => self.size.index() as f32,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "theme" => {
                if let Some(theme) = NotepadTheme::ALL.get(value.round().max(0.0) as usize) {
                    self.theme = *theme;
                }
            }
            "size" => {
                if let Some(size) = NotepadSize::ALL.get(value.round().max(0.0) as usize) {
                    self.size = *size;
                }
            }
            _ => {}
        }
    }
}

impl Default for NotepadConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) static NOTEPAD_PARAMS: [crate::ParamSpec; 3] = with_mix(&NOTEPAD_OWN_PARAMS, ALL_WET);

pub(crate) static NOTEPAD_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Notepad",
    count: NOTEPAD_PARAMS.len(),
}];

static NOTEPAD_THEMES: [&str; 7] = [
    "phosphor", "amber", "paper", "ice", "rose", "slate", "studio",
];

static NOTEPAD_SIZES: [&str; 3] = ["small", "medium", "large"];

static NOTEPAD_OWN_PARAMS: [crate::ParamSpec; 2] = [
    crate::ParamSpec {
        id: "theme",
        name: "Theme",
        min: 0.0,
        max: 6.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(7),
        positions: &NOTEPAD_THEMES,
    },
    crate::ParamSpec {
        id: "size",
        name: "Size",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(3),
        positions: &NOTEPAD_SIZES,
    },
];

/// The pad's bank: one per theme.
///
/// Every built-in ships a bank worth opening (`fontelle-app/tests/
/// effect_editor.rs` holds that promise), and this is the honest one for a
/// device whose only settings are how it is painted — a preset row here is a
/// look, and picking one is how somebody finds out the pad has seven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NotepadPreset {
    Phosphor,
    Amber,
    Paper,
    Ice,
    Rose,
    Slate,
    Studio,
}

impl NotepadPreset {
    pub fn label(self) -> &'static str {
        self.theme().label()
    }

    /// The look this preset is.
    pub fn theme(self) -> NotepadTheme {
        match self {
            Self::Phosphor => NotepadTheme::Phosphor,
            Self::Amber => NotepadTheme::Amber,
            Self::Paper => NotepadTheme::Paper,
            Self::Ice => NotepadTheme::Ice,
            Self::Rose => NotepadTheme::Rose,
            Self::Slate => NotepadTheme::Slate,
            Self::Studio => NotepadTheme::Studio,
        }
    }

    pub const ALL: [Self; 7] = [
        Self::Phosphor,
        Self::Amber,
        Self::Paper,
        Self::Ice,
        Self::Rose,
        Self::Slate,
        Self::Studio,
    ];
}

impl NotepadConfig {
    pub fn from_preset(preset: NotepadPreset) -> Self {
        Self {
            theme: preset.theme(),
            ..Self::new()
        }
    }
}

// ------------------------------------------------------------- the words

/// What a notepad says: its pages, and which one is showing.
///
/// Stored on the insert's slot rather than in [`NotepadConfig`] — the module's
/// own note says why — and serialised into `project.json` as plain strings, so
/// somebody's lyrics are legible in the file that holds them.
///
/// **There is always at least one page.** A pad with none is a window with
/// nothing to type in, and every constructor, edit and deserialisation here
/// keeps that true.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "PagesFile")]
pub struct NotepadPages {
    pages: Vec<String>,
    /// Which page the window opens on — the one you were last writing in.
    /// Part of the document rather than the window's own state, so a project
    /// reopens on the verse you were working on.
    showing: usize,
}

/// What the file may hold, before the invariants are put back.
///
/// A hand-edited project with no pages, or a `showing` past the end, opens
/// rather than refusing: neither is worth losing a song over.
#[derive(serde::Deserialize)]
struct PagesFile {
    #[serde(default)]
    pages: Vec<String>,
    #[serde(default)]
    showing: usize,
}

impl From<PagesFile> for NotepadPages {
    fn from(file: PagesFile) -> Self {
        let pages = if file.pages.is_empty() {
            vec![String::new()]
        } else {
            file.pages
        };
        let showing = file.showing.min(pages.len() - 1);
        Self { pages, showing }
    }
}

impl NotepadPages {
    /// One empty page, showing.
    pub fn new() -> Self {
        Self {
            pages: vec![String::new()],
            showing: 0,
        }
    }

    pub fn pages(&self) -> &[String] {
        &self.pages
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// Never true — a pad always has a page. Here because `len` without it is
    /// a clippy warning, and because a caller asking is better off told.
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn showing(&self) -> usize {
        self.showing
    }

    /// One page's words, or `""` for a page that is not there.
    pub fn text(&self, page: usize) -> &str {
        self.pages.get(page).map_or("", String::as_str)
    }

    pub fn showing_text(&self) -> &str {
        self.text(self.showing)
    }

    /// Whether this pad has anything written in it at all — what a strip's
    /// insert row would ask before drawing a mark.
    pub fn is_blank(&self) -> bool {
        self.pages.iter().all(|page| page.trim().is_empty())
    }

    /// Does `edit`, and gives back **the edit that undoes it** — or `None`
    /// when there was nothing to do, which is a command that never reaches
    /// the history rather than one that does nothing.
    pub fn apply(&mut self, edit: &NotepadEdit) -> Option<NotepadEdit> {
        match edit {
            NotepadEdit::Write { page, text } => {
                let held = self.pages.get_mut(*page)?;
                if held == text {
                    return None;
                }
                let before = std::mem::replace(held, text.clone());
                // The page that changed is the page you are looking at, so
                // that an undo taken from somewhere else shows you what it
                // put back.
                self.showing = *page;
                Some(NotepadEdit::Write {
                    page: *page,
                    text: before,
                })
            }
            NotepadEdit::InsertPage { at, text } => {
                if *at > self.pages.len() {
                    return None;
                }
                self.pages.insert(*at, text.clone());
                // A page you just made is a page you are about to write on.
                self.showing = *at;
                Some(NotepadEdit::RemovePage { page: *at })
            }
            NotepadEdit::RemovePage { page } => {
                if *page >= self.pages.len() || self.pages.len() == 1 {
                    return None;
                }
                let text = self.pages.remove(*page);
                self.showing = self.showing.min(self.pages.len() - 1);
                Some(NotepadEdit::InsertPage { at: *page, text })
            }
            NotepadEdit::Show { page } => {
                if *page >= self.pages.len() || *page == self.showing {
                    return None;
                }
                let was = self.showing;
                self.showing = *page;
                Some(NotepadEdit::Show { page: was })
            }
        }
    }
}

impl Default for NotepadPages {
    fn default() -> Self {
        Self::new()
    }
}

/// One thing done to a notepad.
///
/// Coarse on purpose: a page is rewritten whole rather than as an insertion at
/// a byte index. A page of lyrics is a few hundred bytes, the window already
/// holds the edited string ([`TextEntry`](../../fontelle_ui/canvas/struct.TextEntry.html)),
/// and an edit algebra fine enough to describe a paste over a selection is a
/// second text model to keep in step with the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotepadEdit {
    /// Page `page` now says `text`.
    Write { page: usize, text: String },
    /// A page holding `text` inserted at `at` (`at == len` appends).
    InsertPage { at: usize, text: String },
    /// Page `page` taken out. Refused on the last one.
    RemovePage { page: usize },
    /// Turn to page `page`.
    Show { page: usize },
}

impl NotepadEdit {
    /// What the history entry is called.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Write { .. } => "Type",
            Self::InsertPage { .. } => "Add page",
            Self::RemovePage { .. } => "Remove page",
            Self::Show { .. } => "Turn page",
        }
    }

    /// Roughly what it costs to keep, for the history's memory ceiling.
    pub fn memory_cost(&self) -> usize {
        match self {
            Self::Write { text, .. } | Self::InsertPage { text, .. } => {
                std::mem::size_of::<Self>() + text.len()
            }
            _ => std::mem::size_of::<Self>(),
        }
    }
}
