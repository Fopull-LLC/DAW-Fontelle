//! Who installed this copy, and so who updates it (hub card 0429,
//! `docs/steam.md`).
//!
//! A copy from the release page updates itself from the start menu
//! (`updates.rs`). A copy Steam installed must not: Steam owns the install
//! folder, checks it file by file, and the updater's download, staging
//! folder, `.old` binary and ASIO helper are all files Steam did not put
//! there.
//!
//! # A marker, not a cargo feature
//!
//! The Steam depots carry [`STEAM_MARKER`] beside the program (on macOS in
//! the bundle's `Contents/Resources`; codesign takes everything in
//! `Contents/MacOS` for code). A `steam` feature was weighed and turned
//! down: it would make the Steam binary a second build of every target —
//! four more builds a release, binaries no CI run had tested, and depots
//! that could no longer be the published, checksummed archives
//! (`tools/steam/make-depots.sh` lays out exactly those bytes). The cost of
//! the marker is that the updater's code is in a Steam copy, unreachable;
//! somebody who deletes the file gets the updater back until Steam's next
//! verification restores it, which is theirs to do.

use std::path::{Path, PathBuf};

/// The file a Steam depot carries to say Steam installed it.
pub const STEAM_MARKER: &str = "fontelle-steam.txt";

/// Who installed this copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    /// The release page, a package, a build from source: it updates itself
    /// (or its packager does) exactly as before.
    Direct,
    /// Steam: Steam updates it.
    Steam,
}

impl Distribution {
    /// Read from the install `exe` was launched from.
    pub fn of_install(exe: &Path) -> Self {
        if marker_places(exe).iter().any(|path| path.is_file()) {
            Self::Steam
        } else {
            Self::Direct
        }
    }

    /// This process's, asked once: the binary it was launched as
    /// ([`crate::updates::launched_exe`]), or [`Direct`](Self::Direct) when
    /// it cannot say.
    pub fn here() -> Self {
        static HERE: std::sync::OnceLock<Distribution> = std::sync::OnceLock::new();
        *HERE.get_or_init(|| {
            crate::updates::launched_exe()
                .map(|exe| Self::of_install(&exe))
                .unwrap_or(Self::Direct)
        })
    }

    /// Whether the start menu may replace the program.
    pub fn updates_itself(self) -> bool {
        self == Self::Direct
    }

    /// Who does, when it is not this: the name the start menu shows.
    pub fn store(self) -> Option<&'static str> {
        match self {
            Self::Direct => None,
            Self::Steam => Some("Steam"),
        }
    }
}

/// The folder `exe` is installed in: its own folder, or — for a program
/// inside `Something.app/Contents/MacOS` — the folder holding the app, which
/// is what a store installs.
pub fn install_dir(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    Some(
        app_bundle(dir)
            .and_then(Path::parent)
            .unwrap_or(dir)
            .to_path_buf(),
    )
}

/// The `.app` folder `dir` is the `Contents/MacOS` of, if it is one.
fn app_bundle(dir: &Path) -> Option<&Path> {
    if dir.file_name()? != "MacOS" {
        return None;
    }
    let contents = dir.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    (app.extension()? == "app").then_some(app)
}

/// Where [`STEAM_MARKER`] may be for the program at `exe`.
fn marker_places(exe: &Path) -> Vec<PathBuf> {
    let Some(dir) = exe.parent() else {
        return Vec::new();
    };
    let mut places = vec![dir.join(STEAM_MARKER)];
    if let Some(app) = app_bundle(dir) {
        places.push(app.join("Contents").join("Resources").join(STEAM_MARKER));
    }
    places
}
