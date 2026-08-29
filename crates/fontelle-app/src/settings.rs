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

/// The revision of the settings file this build writes. Its own number, like
/// the theme's and the project's — where Fontelle keeps things has nothing to
/// do with either.
pub const SETTINGS_FORMAT_VERSION: u32 = 1;

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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            format_version: SETTINGS_FORMAT_VERSION,
            soundfont_dirs: Vec::new(),
            projects_dir: None,
            theme: None,
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
