//! The keymap: what each shortcut is bound to, and the means to change it.
//!
//! > *"could you make these keybinds in the ? tab completely configurable so
//! > users can cleanly click on one they don't like, then input the new
//! > binding they want ... it waits for the release to see your combination
//! > basically."*
//!
//! TDD §16.5 always said every keybind is remappable and the window shipped
//! with every one of them as a literal in a `match`. This is the map those
//! literals become. Three parts, all pure:
//!
//! - A [`Chord`]: modifiers and one key, with a text form (`Ctrl+Shift+Z`)
//!   because the map is kept in the settings file as text and drawn on the
//!   shortcuts page as text — one spelling, both ways.
//! - An [`Action`] for every command a key can mean, and the [`Keymap`] from
//!   chords to actions. The window asks the map what a key means
//!   (`action`) and then does that, instead of asking what the key *is*.
//!   Contexts keep the studio's letters out of an editor window and let one
//!   key mean two things in two places that never listen at once.
//! - A [`Rebind`] listener: the page's "press the new shortcut". It commits
//!   on the **release** of the key, with the modifiers that were down at the
//!   press or the release, so Ctrl-then-X is Ctrl+X whichever finger lifts
//!   first — and a modifier on its own is never a chord.
//!
//! What is deliberately not here: the arrow-key family (four directions by
//! four modifier sets, one meaning — a table, not a list of bindings), Esc
//! (the one key every prompt and page relies on to be itself), Enter in
//! lists, and everything typed into a text field. Those stay fixed and the
//! page says so by listing them without a chip you can press.

/// A key on its own, without its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChordKey {
    /// A character key, as the keyboard reports it. Letters are kept in
    /// lower case; the shift is the chord's.
    Char(char),
    Space,
    Tab,
    Enter,
    Delete,
    Backspace,
    Home,
    End,
    Insert,
    PageUp,
    PageDown,
    /// F1 to F24.
    F(u8),
}

impl ChordKey {
    fn label(self) -> String {
        match self {
            Self::Char(c) => c.to_uppercase().to_string(),
            Self::Space => "Space".to_string(),
            Self::Tab => "Tab".to_string(),
            Self::Enter => "Enter".to_string(),
            Self::Delete => "Delete".to_string(),
            Self::Backspace => "Backspace".to_string(),
            Self::Home => "Home".to_string(),
            Self::End => "End".to_string(),
            Self::Insert => "Insert".to_string(),
            Self::PageUp => "PageUp".to_string(),
            Self::PageDown => "PageDown".to_string(),
            Self::F(n) => format!("F{n}"),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return Some(Self::Char(c.to_lowercase().next().unwrap_or(c)));
        }
        Some(match text.to_ascii_lowercase().as_str() {
            "space" => Self::Space,
            "tab" => Self::Tab,
            "enter" | "return" => Self::Enter,
            "delete" | "del" => Self::Delete,
            "backspace" => Self::Backspace,
            "home" => Self::Home,
            "end" => Self::End,
            "insert" => Self::Insert,
            "pageup" => Self::PageUp,
            "pagedown" => Self::PageDown,
            // The arrows are a fixed family (`WindowApp::arrow`) answered
            // before the map is asked, and Esc is every prompt's own: neither
            // is a chord, so neither reads as one.
            other => {
                let n: u8 = other.strip_prefix('f')?.parse().ok()?;
                if !(1..=24).contains(&n) {
                    return None;
                }
                Self::F(n)
            }
        })
    }

    /// The key a keyboard event carries, or `None` for one that cannot be
    /// part of a chord on its own — a modifier, a dead key, a media key.
    pub fn of(key: &winit::keyboard::Key) -> Option<Self> {
        use winit::keyboard::{Key, NamedKey};
        Some(match key {
            Key::Character(text) => {
                let mut chars = text.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                Self::Char(c.to_lowercase().next().unwrap_or(c))
            }
            Key::Named(named) => match named {
                NamedKey::Space => Self::Space,
                NamedKey::Tab => Self::Tab,
                NamedKey::Enter => Self::Enter,
                NamedKey::Delete => Self::Delete,
                NamedKey::Backspace => Self::Backspace,
                NamedKey::Home => Self::Home,
                NamedKey::End => Self::End,
                NamedKey::Insert => Self::Insert,
                NamedKey::PageUp => Self::PageUp,
                NamedKey::PageDown => Self::PageDown,
                NamedKey::F1 => Self::F(1),
                NamedKey::F2 => Self::F(2),
                NamedKey::F3 => Self::F(3),
                NamedKey::F4 => Self::F(4),
                NamedKey::F5 => Self::F(5),
                NamedKey::F6 => Self::F(6),
                NamedKey::F7 => Self::F(7),
                NamedKey::F8 => Self::F(8),
                NamedKey::F9 => Self::F(9),
                NamedKey::F10 => Self::F(10),
                NamedKey::F11 => Self::F(11),
                NamedKey::F12 => Self::F(12),
                _ => return None,
            },
            _ => return None,
        })
    }
}

/// One shortcut: a key and the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: ChordKey,
}

impl Chord {
    /// A chord: the key that was pressed — **without** its modifiers, so
    /// Shift+1 is `1` with shift and not `!` — and the modifiers held.
    pub fn new(ctrl: bool, shift: bool, alt: bool, key: ChordKey) -> Self {
        Self {
            ctrl,
            shift,
            alt,
            key,
        }
    }

    /// `Ctrl+Shift+Z` — modifiers in a fixed order, then the key.
    pub fn label(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.shift {
            out.push_str("Shift+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        out.push_str(&self.key.label());
        out
    }

    /// The chord `text` spells, in any case and with any spacing, or `None`
    /// for text that does not spell one. The key is the last `+`-separated
    /// part; a `+` on its own at the end is the `+` key.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        // Split on `+`, but a trailing `+` is the key itself ("Shift++", "+").
        let (mods, key) = match text.rsplit_once('+') {
            Some((mods, "")) => match mods.strip_suffix('+') {
                // "Shift++" -> mods "Shift", key "+"
                Some(mods) => (mods, "+"),
                // "+" alone
                None if mods.is_empty() => ("", "+"),
                None => return None,
            },
            Some((mods, key)) => (mods, key),
            None => ("", text),
        };
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        for part in mods.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "" => {}
                "ctrl" | "control" => ctrl = true,
                "shift" => shift = true,
                "alt" => alt = true,
                _ => return None,
            }
        }
        let key = ChordKey::parse(key.trim())?;
        Some(Self::new(ctrl, shift, alt, key))
    }
}

/// Where a key is pressed, which decides which bindings can hear it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// Everywhere: the studio and every editor window.
    Global,
    /// The studio window's own panels.
    Studio,
    /// A floating editor window.
    Editor,
}

impl Context {
    /// Whether a binding in `self` and one in `other` could both hear one
    /// key press — which is what makes sharing a chord a conflict.
    pub fn overlaps(self, other: Self) -> bool {
        self == other || self == Self::Global || other == Self::Global
    }
}

/// Every command a key can be bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    // --- transport ---
    Play,
    Stop,
    Metronome,
    /// Legato with notes selected, song/clip mode without — one key, and
    /// the selection decides. See `WindowApp::legato_or_play_mode`.
    LegatoOrPlayMode,
    // --- project ---
    Save,
    Undo,
    Redo,
    ExportWav,
    ExportMidi,
    Help,
    // --- panels and views ---
    ShowRoll,
    ShowMixer,
    RackTab,
    SwapSplit,
    ToggleTimeline,
    ToolsPanel,
    Search,
    ZoomIn,
    ZoomOut,
    // --- tools ---
    DrawTool,
    PaintTool,
    SelectTool,
    DeleteTool,
    SliceTool,
    SnapOrStretch,
    Slide,
    LaneProperty,
    Ghosts,
    // --- editing ---
    SelectAll,
    Copy,
    Cut,
    Paste,
    Duplicate,
    /// Cut every selected clip at the marker — the blue mark play returns
    /// to — the blade's cut, from the keyboard. *"make ctrl b split my
    /// selection at ... where the blue marker where the play marker returns
    /// to is."*
    SplitAtMarker,
    DeleteSelection,
    MuteClips,
    // --- mixer ---
    MuteTrack,
    SoloTrack,
    // --- editor windows ---
    RemoveBand,
}

impl Action {
    pub const ALL: [Self; 39] = [
        Self::Play,
        Self::Stop,
        Self::Metronome,
        Self::LegatoOrPlayMode,
        Self::Save,
        Self::Undo,
        Self::Redo,
        Self::ExportWav,
        Self::ExportMidi,
        Self::Help,
        Self::ShowRoll,
        Self::ShowMixer,
        Self::RackTab,
        Self::SwapSplit,
        Self::ToggleTimeline,
        Self::ToolsPanel,
        Self::Search,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::DrawTool,
        Self::PaintTool,
        Self::SelectTool,
        Self::DeleteTool,
        Self::SliceTool,
        Self::SnapOrStretch,
        Self::Slide,
        Self::LaneProperty,
        Self::Ghosts,
        Self::SelectAll,
        Self::Copy,
        Self::Cut,
        Self::Paste,
        Self::Duplicate,
        Self::SplitAtMarker,
        Self::DeleteSelection,
        Self::MuteClips,
        Self::MuteTrack,
        Self::SoloTrack,
        Self::RemoveBand,
    ];

    /// The name the settings file knows this by. **Never renamed**: a file
    /// written under one name must read back under it (INVARIANT 7's rule
    /// for parameter addresses, applied to a key).
    pub fn id(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Stop => "stop",
            Self::Metronome => "metronome",
            Self::LegatoOrPlayMode => "legato-or-play-mode",
            Self::Save => "save",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::ExportWav => "export-wav",
            Self::ExportMidi => "export-midi",
            Self::Help => "help",
            Self::ShowRoll => "show-roll",
            Self::ShowMixer => "show-mixer",
            Self::RackTab => "rack-tab",
            Self::SwapSplit => "swap-split",
            Self::ToggleTimeline => "toggle-timeline",
            Self::ToolsPanel => "tools-panel",
            Self::Search => "search",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::DrawTool => "draw-tool",
            Self::PaintTool => "paint-tool",
            Self::SelectTool => "select-tool",
            Self::DeleteTool => "delete-tool",
            Self::SliceTool => "slice-tool",
            Self::SnapOrStretch => "snap-or-stretch",
            Self::Slide => "slide",
            Self::LaneProperty => "lane-property",
            Self::Ghosts => "ghosts",
            Self::SelectAll => "select-all",
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
            Self::Duplicate => "duplicate",
            Self::SplitAtMarker => "split-at-marker",
            Self::DeleteSelection => "delete-selection",
            Self::MuteClips => "mute-clips",
            Self::MuteTrack => "mute-track",
            Self::SoloTrack => "solo-track",
            Self::RemoveBand => "remove-band",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.id() == id)
    }

    /// What it does, in a person's words — the shortcuts page's right-hand
    /// column.
    pub fn does(self) -> &'static str {
        match self {
            Self::Play => "Play from the marker; again to stop and come back to it",
            Self::Stop => "Stop, and go to the start of the song",
            Self::Metronome => "Metronome on or off",
            Self::LegatoOrPlayMode => "Legato on the selected notes; Song or Clip mode with none",
            Self::Save => "Save",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::ExportWav => "Export as a WAV — asks which stretch, and about the tail",
            Self::ExportMidi => "Export the song as a MIDI file",
            Self::Help => "This page",
            Self::ShowRoll => "Show the piano roll",
            Self::ShowMixer => "Show the mixer",
            Self::RackTab => "Rack: Instruments or Prefabs",
            Self::SwapSplit => "Swap which is taller, the arrangement or the editor",
            Self::ToggleTimeline => "Show or hide the arrangement",
            Self::ToolsPanel => "The Tools panel",
            Self::Search => "Search the browser",
            Self::ZoomIn => "Zoom in, about the pointer",
            Self::ZoomOut => "Zoom out, about the pointer",
            Self::DrawTool => "Draw",
            Self::PaintTool => "Paint (piano roll)",
            Self::SelectTool => "Select",
            Self::DeleteTool => "Delete (piano roll)",
            Self::SliceTool => "Slice",
            Self::SnapOrStretch => "Snap grid (piano roll) / Stretch (arrangement)",
            Self::Slide => "Slide notes on or off",
            Self::LaneProperty => "Next property lane: velocity, pan, pitch\u{2026}",
            Self::Ghosts => "Ghost notes from other clips",
            Self::SelectAll => "Select every note",
            Self::Copy => "Copy",
            Self::Cut => "Cut",
            Self::Paste => "Paste",
            Self::Duplicate => "Duplicate the selection after itself",
            Self::SplitAtMarker => "Cut the selected clips at the marker",
            Self::DeleteSelection => "Delete the selection",
            Self::MuteClips => "Mute the selected clips",
            Self::MuteTrack => "Mute the selected track (mixer showing)",
            Self::SoloTrack => "Solo the selected track (mixer showing)",
            Self::RemoveBand => "Remove the selected EQ band",
        }
    }

    /// Where this action listens.
    pub fn context(self) -> Context {
        match self {
            Self::Play
            | Self::Stop
            | Self::Metronome
            | Self::Save
            | Self::Undo
            | Self::Redo
            | Self::ExportWav
            | Self::ExportMidi
            | Self::Help => Context::Global,
            Self::RemoveBand => Context::Editor,
            _ => Context::Studio,
        }
    }

    /// FL's bindings, as the window has always had them.
    fn defaults(self) -> &'static [&'static str] {
        match self {
            Self::Play => &["Space"],
            Self::Stop => &["Home"],
            Self::Metronome => &["Ctrl+M"],
            Self::LegatoOrPlayMode => &["Ctrl+L"],
            Self::Save => &["Ctrl+S"],
            Self::Undo => &["Ctrl+Z"],
            Self::Redo => &["Ctrl+Shift+Z", "Ctrl+Y"],
            Self::ExportWav => &["Ctrl+E"],
            Self::ExportMidi => &["Ctrl+Shift+E"],
            Self::Help => &["F1"],
            Self::ShowRoll => &["1"],
            Self::ShowMixer => &["2"],
            Self::RackTab => &["F"],
            Self::SwapSplit => &["Tab"],
            Self::ToggleTimeline => &["Ctrl+T"],
            Self::ToolsPanel => &["T"],
            Self::Search => &["Ctrl+F"],
            Self::ZoomIn => &["+", "="],
            Self::ZoomOut => &["-", "_"],
            Self::DrawTool => &["P"],
            Self::PaintTool => &["B"],
            Self::SelectTool => &["E"],
            Self::DeleteTool => &["D"],
            Self::SliceTool => &["C"],
            Self::SnapOrStretch => &["S"],
            Self::Slide => &["A"],
            Self::LaneProperty => &["L"],
            Self::Ghosts => &["G"],
            Self::SelectAll => &["Ctrl+A"],
            Self::Copy => &["Ctrl+C"],
            Self::Cut => &["Ctrl+X"],
            Self::Paste => &["Ctrl+V"],
            // Ctrl+B was a second Duplicate until the cut asked for it;
            // Duplicate keeps Ctrl+D.
            Self::Duplicate => &["Ctrl+D"],
            Self::SplitAtMarker => &["Ctrl+B"],
            Self::DeleteSelection => &["Delete", "Backspace"],
            Self::MuteClips => &["Ctrl+Shift+M"],
            Self::MuteTrack => &["M"],
            Self::SoloTrack => &["N"],
            Self::RemoveBand => &["Delete", "Backspace"],
        }
    }

    fn default_chords(self) -> Vec<Chord> {
        self.defaults()
            .iter()
            .filter_map(|text| Chord::parse(text))
            .collect()
    }
}

/// What each action is bound to. One entry per action, in [`Action::ALL`]'s
/// order; an action may have several chords (Redo has two by default) or
/// none, once a rebind has taken its only one away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bound: Vec<(Action, Vec<Chord>)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bound: Action::ALL
                .into_iter()
                .map(|action| (action, action.default_chords()))
                .collect(),
        }
    }
}

/// Between two chords of one action, on the page and in the file.
const CHORD_SEPARATOR: &str = " / ";

impl Keymap {
    /// What the page says for an action nothing is bound to.
    pub const UNBOUND: &'static str = "\u{2014}";

    /// The chords bound to `action`.
    pub fn chords(&self, action: Action) -> &[Chord] {
        self.bound
            .iter()
            .find(|(a, _)| *a == action)
            .map_or(&[], |(_, chords)| chords.as_slice())
    }

    /// The chords as the page writes them: `Ctrl+Shift+Z / Ctrl+Y`, or
    /// [`Keymap::UNBOUND`].
    pub fn label(&self, action: Action) -> String {
        let chords = self.chords(action);
        if chords.is_empty() {
            return Self::UNBOUND.to_string();
        }
        chords
            .iter()
            .map(Chord::label)
            .collect::<Vec<_>>()
            .join(CHORD_SEPARATOR)
    }

    /// What `chord` means where `context` is listening: an action in that
    /// context or a global one.
    pub fn action(&self, chord: &Chord, context: Context) -> Option<Action> {
        self.bound
            .iter()
            .find(|(action, chords)| {
                chords.contains(chord)
                    && (action.context() == Context::Global || action.context() == context)
            })
            .map(|(action, _)| *action)
    }

    /// What a key press means, given both the chord it is and the symbol it
    /// **typed**.
    ///
    /// The chord first. Then, only when shift was held on a key that is not
    /// a letter and the chord itself is bound to nothing, the typed symbol
    /// on its own: `+` is Shift+= on a US keyboard and its own key on
    /// another, and the window has always taken both for zoom. A letter has
    /// no such fallback — Shift+M is not M — and a binding to the shifted
    /// chord itself always wins over what it happens to type.
    pub fn action_of_press(
        &self,
        chord: &Chord,
        typed: Option<ChordKey>,
        context: Context,
    ) -> Option<Action> {
        if let Some(action) = self.action(chord, context) {
            return Some(action);
        }
        let fallback = match (chord.shift, chord.key, typed) {
            (true, ChordKey::Char(pressed), Some(ChordKey::Char(typed)))
                if !pressed.is_alphabetic() && typed != pressed =>
            {
                Chord::new(chord.ctrl, false, chord.alt, ChordKey::Char(typed))
            }
            _ => return None,
        };
        self.action(&fallback, context)
    }

    /// Binds `action` to `chord` and nothing else, and takes the chord away
    /// from any action that could have heard it too. Returns those actions,
    /// so the page can say what was taken from whom — a chord that silently
    /// meant two things would be the worst outcome of a rebind.
    pub fn rebind(&mut self, action: Action, chord: Chord) -> Vec<Action> {
        let mut taken = Vec::new();
        for (other, chords) in &mut self.bound {
            if *other == action {
                continue;
            }
            if other.context().overlaps(action.context()) && chords.contains(&chord) {
                chords.retain(|c| *c != chord);
                taken.push(*other);
            }
        }
        if let Some((_, chords)) = self.bound.iter_mut().find(|(a, _)| *a == action) {
            *chords = vec![chord];
        }
        taken
    }

    /// Every default back.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether anything differs from the defaults.
    pub fn is_changed(&self) -> bool {
        *self != Self::default()
    }

    /// What differs from the defaults, as `(action id, chords)` pairs — the
    /// form the settings file keeps. An unbound action is an empty string.
    /// Only the differences, so the file holds a person's few changes rather
    /// than a copy of every default that would go stale the day one moved.
    pub fn overrides(&self) -> Vec<(String, String)> {
        self.bound
            .iter()
            .filter(|(action, chords)| *chords != action.default_chords())
            .map(|(action, chords)| {
                let text = chords
                    .iter()
                    .map(Chord::label)
                    .collect::<Vec<_>>()
                    .join(CHORD_SEPARATOR);
                (action.id().to_string(), text)
            })
            .collect()
    }

    /// The defaults with `overrides` applied. An id this build has no action
    /// for, or a chord it cannot read, leaves the default in place: a
    /// settings file from a newer build or a hand edit must not cost every
    /// other binding.
    pub fn with_overrides(overrides: &[(String, String)]) -> Self {
        let mut map = Self::default();
        for (id, text) in overrides {
            let Some(action) = Action::from_id(id) else {
                continue;
            };
            let chords: Vec<Chord> = if text.trim().is_empty() {
                Vec::new()
            } else {
                let parsed: Option<Vec<Chord>> = text
                    .split(CHORD_SEPARATOR.trim())
                    .map(Chord::parse)
                    .collect();
                match parsed {
                    Some(chords) => chords,
                    None => continue,
                }
            };
            if let Some((_, bound)) = map.bound.iter_mut().find(|(a, _)| *a == action) {
                *bound = chords;
            }
        }
        map
    }
}

/// The page listening for a new shortcut.
///
/// A chord is made of a key and the modifiers held with it, and a person
/// presses those in some order and lets go of them in some other. So the
/// listener remembers the key that went down and the modifiers that were
/// down with it, and commits when **that key comes up** — with the modifiers
/// held at the press *or* at the release, so Ctrl let go a moment early is
/// still Ctrl. Modifier keys are not chord keys (`ChordKey::of` is `None`
/// for them), so pressing and releasing Ctrl alone is not a binding, and a
/// second key pressed before the first comes up is the one listened for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rebind {
    action: Action,
    /// The key that is down and the modifiers that were down when it went.
    pending: Option<(ChordKey, (bool, bool, bool))>,
}

impl Rebind {
    pub fn new(action: Action) -> Self {
        Self {
            action,
            pending: None,
        }
    }

    pub fn action(&self) -> Action {
        self.action
    }

    /// Whether a key is down and the chord is waiting on its release.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// A key went down. Never commits: a chord is read on the way up.
    pub fn press(
        &mut self,
        key: Option<ChordKey>,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<Chord> {
        if let Some(key) = key {
            self.pending = Some((key, (ctrl, shift, alt)));
        }
        None
    }

    /// A key came up. The chord, when it was the key being listened for.
    pub fn release(
        &mut self,
        key: Option<ChordKey>,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<Chord> {
        let (held, (c, s, a)) = self.pending?;
        if key != Some(held) {
            return None;
        }
        self.pending = None;
        Some(Chord::new(c || ctrl, s || shift, a || alt, held))
    }
}
