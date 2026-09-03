//! Where Fontelle keeps things, and how the user changes it (TDD §17.3, §18).
//!
//! # INVARIANT 10
//!
//! > Fontelle writes nothing outside locations the user has explicitly
//! > configured, except its own config directory.
//!
//! Everything here exists to keep that true and checkable. Two consequences
//! worth stating plainly, because both were decisions:
//!
//! - **Nothing defaults to `~/Documents`, `~/Music`, or anywhere else that
//!   belongs to the user.** The only paths this module invents are under the
//!   XDG config and data directories, which are Fontelle's own.
//! - **The soundfont bank defaults to `$XDG_DATA_HOME/fontelle/soundfonts`**,
//!   and Fontelle creates it — once, on first run, saying so. That is a write
//!   outside the *config* directory, so it is a deliberate reading of the
//!   invariant rather than a strict one: the XDG data directory is as much
//!   Fontelle's own as the config directory is, and "a folder you drop your
//!   soundfonts into" cannot exist without something creating it. Any other
//!   folder is added by the user, with `--soundfonts <dir>`, and is remembered
//!   here. **Flag this to the owner if it is the wrong reading** — it is one
//!   function and one line of `create_dir_all`.
//!
//! The full first-run wizard (§18) is M7. This is its smallest useful half: a
//! file the user can edit, with the paths in it, that Fontelle reads and never
//! silently rewrites.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Re-exported so the settings file's own vocabulary is in one place, while
/// the window — which may not depend on this crate — can still name it.
pub use fontelle_types::FolderKind;

/// The revision of the settings file this build writes. Its own number, like
/// the theme's and the project's — where Fontelle keeps things has nothing to
/// do with either.
///
/// Two since the settings file grew [`MidiInputSettings`], three since it grew
/// the two import folders. Every added field carries `#[serde(default)]`, so
/// an older file still reads — the bump is so that an *older build* handed a
/// newer file says "upgrade Fontelle" rather than "unknown field `midi_dir`".
pub const SETTINGS_FORMAT_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub format_version: u32,
    /// Every folder the browser scans for `.sf2` files, in the order they are
    /// searched. §17.5's "one or more soundfont directories".
    pub soundfont_dirs: Vec<PathBuf>,
    /// Where the file dialogs start. `None` means "ask" — never a guess.
    pub projects_dir: Option<PathBuf>,
    /// A theme file, if the user has pointed at one (§16.6).
    pub theme: Option<PathBuf>,
    /// How live MIDI input is read (§14.3).
    ///
    /// `default` rather than required, because a settings file written before
    /// this existed is not a broken one — and because the default is the
    /// identity, so a person who never opens the tab is in exactly the state
    /// they were in before it did.
    #[serde(default)]
    pub midi_input: MidiInputSettings,
    /// Where `.mid` files are kept, for the roll's *Import MIDI*.
    ///
    /// `None` means "ask", and never a guess: INVARIANT 10 says Fontelle
    /// touches nothing the user has not named, and `~/Music` is as much
    /// somebody's own folder as any other. The window's answer to `None` is
    /// to send you here rather than to browse something you did not choose.
    #[serde(default)]
    pub midi_dir: Option<PathBuf>,
    /// Where FL Studio's `.fsc` score files are kept.
    ///
    /// Usually somewhere inside an FL Studio installation, which is exactly
    /// why it cannot be guessed: on this machine it is under a Wine prefix on
    /// a second disk, and on the next one it will be somewhere else again.
    #[serde(default)]
    pub score_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format_version: SETTINGS_FORMAT_VERSION,
            soundfont_dirs: Vec::new(),
            projects_dir: None,
            theme: None,
            midi_input: MidiInputSettings::default(),
            midi_dir: None,
            score_dir: None,
        }
    }
}

/// What a MIDI keyboard's notes are read as (TDD §14.3).
///
/// Reported from playing one: *"every single input device will register
/// differently, we need to expose options like this for users."* The engine's
/// own shape for this is [`fontelle_midi::InputSettings`], which is built to
/// cross onto a device callback thread in one atomic; this is the shape that
/// goes in the file, which is a different job — it is read by a person with a
/// text editor, so the curve is a name and not a tag byte, and `Fixed`'s value
/// is a field of its own rather than a payload that vanishes when the curve is
/// anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MidiInputSettings {
    pub velocity_curve: VelocityCurveSetting,
    /// The velocity window a note must fall inside to be played at all, both
    /// ends inclusive. `(0, 127)` lets everything through.
    pub velocity_min: u8,
    pub velocity_max: u8,
    /// What [`VelocityCurveSetting::Fixed`] plays at. Kept even while another
    /// curve is selected, so switching away and back does not lose it.
    pub fixed_velocity: u8,
    pub transpose_semitones: i8,
    /// Which MIDI channel to listen to, counted from 1 as every keyboard's
    /// own front panel counts it. `None` is all sixteen.
    pub channel_filter: Option<u8>,
}

impl Default for MidiInputSettings {
    /// The identity, per §14.3: per-device config is an "optional refinement,
    /// never required setup".
    fn default() -> Self {
        Self {
            velocity_curve: VelocityCurveSetting::Linear,
            velocity_min: 0,
            velocity_max: 127,
            fixed_velocity: 100,
            transpose_semitones: 0,
            channel_filter: None,
        }
    }
}

/// Which shape a velocity is read through — the file's spelling of
/// [`fontelle_midi::VelocityCurve`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VelocityCurveSetting {
    /// What the keyboard sent, unchanged.
    #[default]
    Linear,
    /// Quiet playing reads louder — the same effort reaches further up the
    /// range, which is what a light action wants.
    Soft,
    /// Quiet playing reads quieter, for a keyboard that is too eager.
    Hard,
    /// Every note at one velocity, whatever was played. What an organ patch
    /// wants, and what a keyboard with a broken sensor needs.
    Fixed,
}

impl VelocityCurveSetting {
    /// What the settings tab writes in the row's value column.
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Soft => "Soft",
            Self::Hard => "Hard",
            Self::Fixed => "Fixed",
        }
    }

    /// The four in the order the tab steps through them.
    pub const ALL: [Self; 4] = [Self::Linear, Self::Soft, Self::Hard, Self::Fixed];
}

/// One row of the settings tab (TDD §18).
///
/// The rows are an enum rather than an index into a table because what a step
/// *means* differs per row — a choice wraps and a number stops — and because
/// the window addresses them by position: `nudge_setting(3, +1)` has to reach
/// the same setting the third row drew, and an enum in a `const` array is the
/// one shape where those cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRow {
    /// A section title. Nothing to set, and a click does nothing — it is what
    /// makes "Transpose" read as *the MIDI keyboard's* transpose rather than
    /// as some property of the song.
    Heading(&'static str),
    VelocityCurve,
    FixedVelocity,
    VelocityMin,
    VelocityMax,
    Transpose,
    ChannelFilter,
    /// A folder to import from. Clicking it opens a picker rather than
    /// stepping a value — see [`SettingRow::folder`].
    Folder(FolderKind),
}

/// Every row the settings tab shows, in the order it shows them.
///
/// One section today. More belong here — the theme, the autosave interval —
/// and adding one is a variant, a `label`, a `value` and a `nudge`, with
/// nothing in `fontelle-ui` to change: the window draws names and values and
/// knows what none of them mean.
pub const SETTING_ROWS: [SettingRow; 10] = [
    SettingRow::Heading("MIDI input"),
    SettingRow::VelocityCurve,
    SettingRow::FixedVelocity,
    SettingRow::VelocityMin,
    SettingRow::VelocityMax,
    SettingRow::Transpose,
    SettingRow::ChannelFilter,
    // Their own heading, and not "MIDI input"'s: a row called "MIDI files"
    // under a heading about the keyboard reads as a property of the keyboard
    // rather than as a place on disk.
    SettingRow::Heading("Import from"),
    SettingRow::Folder(FolderKind::Midi),
    SettingRow::Folder(FolderKind::Scores),
];

/// How far transpose goes either way. Two octaves is as far as anybody moves a
/// keyboard to reach a part; past it you have chosen the wrong octave.
const MAX_TRANSPOSE: i8 = 24;

impl SettingRow {
    /// The name in the row's left-hand column.
    ///
    /// **Short enough for a 248-pixel panel with a value beside it.** The
    /// section heading is what carries "MIDI", so the rows under it do not
    /// have to repeat it and can afford to be words rather than abbreviations.
    pub fn label(self) -> &'static str {
        match self {
            Self::Heading(title) => title,
            Self::VelocityCurve => "Velocity curve",
            Self::FixedVelocity => "Fixed velocity",
            Self::VelocityMin => "Velocity min",
            Self::VelocityMax => "Velocity max",
            Self::Transpose => "Transpose",
            Self::ChannelFilter => "Channel",
            Self::Folder(kind) => kind.label(),
        }
    }

    /// Which folder this row is about, for the rows that are about one.
    ///
    /// What tells the host that a click here opens a **picker** rather than
    /// stepping a number. It is asked of the row rather than matched at the
    /// call site so that adding a third folder is a variant and nothing else.
    pub fn folder(self) -> Option<FolderKind> {
        match self {
            Self::Folder(kind) => Some(kind),
            _ => None,
        }
    }

    /// What it is set to, in the row's right-hand column.
    ///
    /// Takes the whole of [`Settings`] rather than just the MIDI half,
    /// because the rows now span two sections of it — and a row that could
    /// only see one of them is a row that would have to be told about the
    /// other by its caller.
    pub fn value(self, settings: &Settings) -> String {
        let midi = &settings.midi_input;
        match self {
            Self::Heading(_) => String::new(),
            Self::VelocityCurve => midi.velocity_curve.label().to_string(),
            Self::FixedVelocity => midi.fixed_velocity.to_string(),
            Self::VelocityMin => midi.velocity_min.to_string(),
            Self::VelocityMax => midi.velocity_max.to_string(),
            // With its sign, always: "+0" against "0" is the difference
            // between a number that can go either way and one that might not.
            Self::Transpose => format!("{:+} st", midi.transpose_semitones),
            Self::ChannelFilter => match midi.channel_filter {
                Some(channel) => channel.to_string(),
                None => "All".to_string(),
            },
            // The **end** of the path: `…/FL Studio/Scores` says where you
            // are and `/home/someone/Docum…` says nothing at all. The same
            // rule the bank's own status line follows.
            Self::Folder(kind) => match settings.folder(kind) {
                Some(path) => crate::desktop::elide_path(path, 2),
                // Never blank: an empty right-hand column reads as a bug, and
                // "there is nothing here" is the state this whole feature
                // starts in.
                None => "Not set \u{2014} click".to_string(),
            },
        }
    }

    /// Steps this row's value one place in the direction `delta` says.
    ///
    /// **A choice wraps and a number stops**, and the difference is not a
    /// detail: four curves are a set with no ends, so stopping at one would
    /// mean knowing to go back through; 0 and 127 are the ends of a range, and
    /// running off one and arriving at the other is a control that cannot be
    /// trusted to a held press.
    pub fn nudge(self, settings: &mut MidiInputSettings, delta: i32) {
        if delta == 0 {
            return;
        }
        let step = delta.signum();
        match self {
            // Neither of these is a value to step. A folder row is a button,
            // and its press is the host's — this module may not open a
            // dialog. What matters here is that it does not quietly step the
            // row above it instead.
            Self::Heading(_) | Self::Folder(_) => {}
            Self::VelocityCurve => {
                let all = VelocityCurveSetting::ALL;
                let at = all
                    .iter()
                    .position(|c| *c == settings.velocity_curve)
                    .unwrap_or(0) as i32;
                let next = (at + step).rem_euclid(all.len() as i32) as usize;
                settings.velocity_curve = all[next];
            }
            // Never zero: a velocity of zero is a note-off by convention
            // everywhere in MIDI, so "every note at velocity 0" is "no notes".
            Self::FixedVelocity => {
                settings.fixed_velocity = clamped(settings.fixed_velocity, step, 1, 127);
            }
            Self::VelocityMin => {
                settings.velocity_min = clamped(settings.velocity_min, step, 0, 127);
                // The two ends push each other rather than crossing: a window
                // whose bottom is above its top lets nothing through at all,
                // which looks exactly like a broken keyboard.
                settings.velocity_max = settings.velocity_max.max(settings.velocity_min);
            }
            Self::VelocityMax => {
                settings.velocity_max = clamped(settings.velocity_max, step, 0, 127);
                settings.velocity_min = settings.velocity_min.min(settings.velocity_max);
            }
            Self::Transpose => {
                settings.transpose_semitones = (settings.transpose_semitones as i32 + step)
                    .clamp(-(MAX_TRANSPOSE as i32), MAX_TRANSPOSE as i32)
                    as i8;
            }
            // Seventeen states in a ring: every channel, then the sixteen a
            // keyboard prints on its own front panel.
            Self::ChannelFilter => {
                let at = settings.channel_filter.unwrap_or(0) as i32;
                let next = (at + step).rem_euclid(17);
                settings.channel_filter = (next > 0).then_some(next as u8);
            }
        }
    }
}

/// `value` moved one step and kept inside `low..=high`.
fn clamped(value: u8, step: i32, low: u8, high: u8) -> u8 {
    (value as i32 + step).clamp(low as i32, high as i32) as u8
}

impl From<MidiInputSettings> for fontelle_midi::InputSettings {
    /// **The one place the file's shape becomes the engine's**, and where
    /// everything the file could hold that the engine cannot is squared off:
    /// a velocity window given the wrong way round is put back in order, and
    /// a channel is converted from the 1..=16 a keyboard prints on its own
    /// front panel to the 0..=15 the wire carries.
    fn from(settings: MidiInputSettings) -> Self {
        let low = settings.velocity_min.min(127);
        let high = settings.velocity_max.min(127);
        Self {
            velocity_curve: match settings.velocity_curve {
                VelocityCurveSetting::Linear => fontelle_midi::VelocityCurve::Linear,
                VelocityCurveSetting::Soft => fontelle_midi::VelocityCurve::Soft,
                VelocityCurveSetting::Hard => fontelle_midi::VelocityCurve::Hard,
                VelocityCurveSetting::Fixed => {
                    fontelle_midi::VelocityCurve::Fixed(settings.fixed_velocity.clamp(1, 127))
                }
            },
            velocity_range: (low.min(high), low.max(high)),
            transpose_semitones: settings.transpose_semitones,
            channel_filter: settings
                .channel_filter
                .filter(|c| (1..=16).contains(c))
                .map(|c| c - 1),
        }
    }
}

impl Settings {
    /// `$XDG_CONFIG_HOME/fontelle`, or `$HOME/.config/fontelle`.
    ///
    /// The environment is injected so the answer is testable without one —
    /// this is the one function in the workspace that decides where Fontelle is
    /// allowed to write, and it should not need a particular machine to check.
    pub fn config_dir_from(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
        Self::xdg_from(env, "XDG_CONFIG_HOME", ".config").map(|base| base.join("fontelle"))
    }

    /// `$XDG_DATA_HOME/fontelle/soundfonts`, or `$HOME/.local/share/...`.
    pub fn default_soundfont_dir_from(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
        Self::xdg_from(env, "XDG_DATA_HOME", ".local/share")
            .map(|base| base.join("fontelle").join("soundfonts"))
    }

    fn xdg_from(
        env: &dyn Fn(&str) -> Option<String>,
        variable: &str,
        fallback: &str,
    ) -> Option<PathBuf> {
        // An XDG variable that is set but empty or relative is, per the spec,
        // to be ignored rather than honoured — and honouring a relative one
        // would put Fontelle's config wherever it happened to be launched from.
        match env(variable) {
            Some(value) if Path::new(&value).is_absolute() => Some(PathBuf::from(value)),
            _ => env("HOME").map(|home| PathBuf::from(home).join(fallback)),
        }
    }

    pub fn config_dir() -> Option<PathBuf> {
        Self::config_dir_from(&|key| std::env::var(key).ok())
    }

    pub fn config_path() -> Option<PathBuf> {
        Self::config_dir().map(|dir| dir.join("settings.json"))
    }

    pub fn default_soundfont_dir() -> Option<PathBuf> {
        Self::default_soundfont_dir_from(&|key| std::env::var(key).ok())
    }

    /// Which folder is set for `kind`, if one is.
    ///
    /// One accessor pair rather than a match at every call site — which is
    /// how a *"change my projects folder"* button in this very program came
    /// to replace somebody's soundfont bank.
    pub fn folder(&self, kind: FolderKind) -> Option<&Path> {
        match kind {
            FolderKind::Midi => self.midi_dir.as_deref(),
            FolderKind::Scores => self.score_dir.as_deref(),
        }
    }

    pub fn set_folder(&mut self, kind: FolderKind, dir: Option<PathBuf>) {
        match kind {
            FolderKind::Midi => self.midi_dir = dir,
            FolderKind::Scores => self.score_dir = dir,
        }
    }

    pub fn to_json(&self) -> String {
        let mut settings = self.clone();
        settings.format_version = SETTINGS_FORMAT_VERSION;
        serde_json::to_string_pretty(&settings).expect("Settings is always serialisable")
    }

    /// Reads settings, checking the version before the body — the same rule the
    /// project document and the theme follow, for the same reason: a file from
    /// a newer build should be refused by version rather than by whichever
    /// field happened to change shape first.
    pub fn from_json(text: &str) -> Result<Self, SettingsError> {
        let json: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| SettingsError::Format(format!("this is not readable JSON: {e}")))?;
        let found = json
            .get("format_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                SettingsError::Format(
                    "this file has no format_version — it is not Fontelle's settings file"
                        .to_string(),
                )
            })? as u32;
        if found > SETTINGS_FORMAT_VERSION {
            return Err(SettingsError::FromTheFuture {
                found,
                newest: SETTINGS_FORMAT_VERSION,
            });
        }
        serde_json::from_value(json)
            .map_err(|e| SettingsError::Format(format!("these settings could not be read: {e}")))
    }

    /// Reads `path`, **always producing usable settings**.
    ///
    /// A missing file is a first run, which is not an error. A damaged one is:
    /// the error comes back alongside the defaults so the caller can say so and
    /// carry on, and — this is the part that matters — the broken file is left
    /// exactly where it is. Overwriting it with defaults would silently lose
    /// the list of folders somebody spent an afternoon assembling.
    pub fn load_from(path: &Path) -> (Self, Option<SettingsError>) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            Err(source) => {
                return (
                    Self::default(),
                    Some(SettingsError::Io {
                        path: path.to_path_buf(),
                        source,
                    }),
                );
            }
        };
        match Self::from_json(&text) {
            Ok(settings) => (settings, None),
            Err(SettingsError::Format(why)) => (
                Self::default(),
                Some(SettingsError::Format(format!("{}: {why}", path.display()))),
            ),
            Err(other) => (Self::default(), Some(other)),
        }
    }

    /// Writes settings out, creating the directory that holds them.
    ///
    /// Temp-file-then-rename, like the project bundle (§17.1): a settings file
    /// truncated by a crash mid-write is one the next launch cannot read.
    ///
    /// **The temp file's name is unique per writer**, which is not decoration.
    /// One fixed `settings.json.tmp` is a shared mutable file: two writers
    /// interleave into it and both rename, and what lands is a document with an
    /// extra brace on the end — or nothing at all, because the second rename
    /// found the file already moved. That is not a hypothetical either; it is
    /// what this project's own test suite did to a real config directory the
    /// first time this function existed.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&temp, self.to_json())?;
        // A rename over an existing file is atomic on every filesystem this
        // targets, so a reader sees the old settings or the new ones and never
        // half of either.
        let result = std::fs::rename(&temp, path);
        if result.is_err() {
            std::fs::remove_file(&temp).ok();
        }
        result
    }

    /// Settings from the usual place, with whatever went wrong reading them.
    pub fn load() -> (Self, Option<SettingsError>) {
        match Self::config_path() {
            Some(path) => Self::load_from(&path),
            None => (Self::default(), None),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        match Self::config_path() {
            Some(path) => self.save_to(&path),
            None => Err(std::io::Error::other(
                "there is no home directory to keep settings in",
            )),
        }
    }

    /// The folders to scan, filling in the default bank on a first run.
    ///
    /// Creating it is the one write outside the config directory — see this
    /// module's own documentation for why, and `created` is `true` exactly when
    /// it happened, so the caller can tell the user where their soundfonts go.
    pub fn soundfont_dirs_or_default(&mut self) -> BankDirs {
        if !self.soundfont_dirs.is_empty() {
            return BankDirs {
                dirs: self.soundfont_dirs.clone(),
                created: None,
            };
        }
        let Some(default) = Self::default_soundfont_dir() else {
            return BankDirs {
                dirs: Vec::new(),
                created: None,
            };
        };
        let created = !default.exists() && std::fs::create_dir_all(&default).is_ok();
        self.soundfont_dirs.push(default.clone());
        BankDirs {
            dirs: self.soundfont_dirs.clone(),
            created: created.then_some(default),
        }
    }
}

/// What [`Settings::soundfont_dirs_or_default`] settled on.
pub struct BankDirs {
    pub dirs: Vec<PathBuf>,
    /// The folder that had to be created, if one did — worth telling the user
    /// about exactly once.
    pub created: Option<PathBuf>,
}

#[derive(Debug)]
pub enum SettingsError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    FromTheFuture {
        found: u32,
        newest: u32,
    },
    Format(String),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::FromTheFuture { found, newest } => write!(
                f,
                "these settings are in format version {found}, and this build of \
                 Fontelle understands up to version {newest} — upgrade Fontelle, \
                 or move the file aside to start again"
            ),
            Self::Format(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
