//! Things that live beside the product: a catalogue Fontelle offers to
//! install, kept in the binary and fetched with the updater's own downloader.
//!
//! > *"if vst3 and vst2 are legally murky we could always try and add it in
//! > a way that keeps it completely separate to the open source stuff and
//! > never gets included with it"*
//!
//! `docs/vst-plan.md` §4. An **extension** is a bridge (or, later, anything
//! else) that Fontelle finds at run time and knows about from a catalogue
//! compiled in here — the id, the repository it is released from, and the
//! name its archive takes. What Fontelle offers to install is therefore
//! exactly what its release was reviewed with: the catalogue is code, not a
//! network resource, and the folder is scanned only for the ids in it (§4.3).
//!
//! The one extension today is `vst2` — the format that has no licence to
//! offer, kept out of the tree so withdrawing it is deleting a download
//! rather than cutting a product. A bridge whose ABI is not this build's is
//! listed as *needs a newer Fontelle* and not loaded, which is the refusal
//! [`crate::PluginRack`] already makes, given a sentence.
//!
//! Installing is the updater's path (`updates.rs`): the extension
//! repository's latest release, its asset verified against `SHA256SUMS`, and
//! the file renamed into place under Fontelle's own data folder. Removing is
//! deleting that file. Both are refused while a plugin is open through the
//! extension, which the rack asserts — the row says *close the project
//! first* rather than being greyed to no explanation.

use std::path::{Path, PathBuf};

use fontelle_bridge_abi::ABI_VERSION;

use crate::updates::{self, Fetcher, Release};

/// What kind of thing an extension is. Only bridges today; the enum is here
/// so the install path can branch when the second kind arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionKind {
    /// A `fontelle-bridge-abi` shared library, installed into the bridges
    /// folder and loaded like any bridge dropped there by hand.
    Bridge {
        /// The ABI it was built against. A bridge whose number is not this
        /// build's is offered as *needs a newer Fontelle* rather than
        /// installed, so a download that could not load is never fetched.
        abi: u32,
    },
}

/// One entry in the catalogue.
#[derive(Debug, Clone)]
pub struct Extension {
    /// The stable id, and the folder it installs into. Also the format tag a
    /// bridge of it serves, for a bridge.
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    /// The GitHub repository it is released from, `owner/name`.
    pub repo: &'static str,
    pub kind: ExtensionKind,
}

impl Extension {
    /// GitHub's `releases/latest` for this extension's repository.
    pub fn latest_url(&self) -> String {
        format!("https://api.github.com/repos/{}/releases/latest", self.repo)
    }

    /// The release page a person is sent to when the updater cannot install.
    pub fn releases_page(&self) -> String {
        format!("https://github.com/{}/releases", self.repo)
    }

    /// The archive the release attaches for this platform.
    ///
    /// `fontelle-<id>-<version>-<target>.tar.gz` (`.zip` on Windows) — the
    /// same shape [`updates::asset_name`] gives Fontelle's own, so the
    /// extension's release workflow and this file share one convention.
    pub fn asset_name(&self, version: updates::Version, target: &str) -> String {
        let extension = if target.contains("windows") {
            "zip"
        } else {
            "tar.gz"
        };
        format!("fontelle-{}-{version}-{target}.{extension}", self.id)
    }

    /// The file the bridge is inside the archive — its shared library, named
    /// the way the platform names one.
    fn library_name(&self) -> String {
        match self.kind {
            ExtensionKind::Bridge { .. } => {
                if cfg!(target_os = "windows") {
                    format!("fontelle-{}.dll", self.id)
                } else if cfg!(target_os = "macos") {
                    format!("libfontelle_{}.dylib", self.id.replace('-', "_"))
                } else {
                    format!("libfontelle_{}.so", self.id.replace('-', "_"))
                }
            }
        }
    }

    /// Whether this build can load the extension, given its ABI.
    pub fn loadable(&self) -> bool {
        match self.kind {
            ExtensionKind::Bridge { abi } => abi == ABI_VERSION,
        }
    }
}

/// The catalogue, compiled in. What Fontelle offers is what its release was
/// reviewed with.
pub const CATALOGUE: &[Extension] = &[Extension {
    id: "vst2",
    name: "VST 2 plugins",
    summary: "Loads plugins in the VST 2.4 format, including ones yabridge makes of Windows plugins.",
    repo: "Fopull-LLC/fontelle-vst2",
    kind: ExtensionKind::Bridge { abi: ABI_VERSION },
}];

/// The extension with this id, if it is in the catalogue.
pub fn find(id: &str) -> Option<&'static Extension> {
    CATALOGUE.iter().find(|extension| extension.id == id)
}

/// Where a bridge extension is installed: Fontelle's own bridges folder, the
/// same one [`fontelle_host::bridge_search_paths`] loads from and a
/// developer drops a `.so` into by hand.
pub fn bridges_dir() -> Option<PathBuf> {
    fontelle_host::bridge_search_paths().into_iter().next()
}

/// Where an installed extension's library is, if it is installed. The file
/// is named by its id, so two extensions cannot collide and removing one is
/// removing a known file rather than guessing.
pub fn installed_path(extension: &Extension) -> Option<PathBuf> {
    let file = match extension.kind {
        ExtensionKind::Bridge { .. } => installed_bridge_name(extension),
    };
    bridges_dir().map(|dir| dir.join(file))
}

/// The name the installed bridge takes on disk — its id, so the catalogue's
/// scan-by-id (§4.3) and the file are the same fact.
fn installed_bridge_name(extension: &Extension) -> String {
    if cfg!(target_os = "windows") {
        format!("fontelle-{}.dll", extension.id)
    } else if cfg!(target_os = "macos") {
        format!("libfontelle-{}.dylib", extension.id)
    } else {
        format!("libfontelle-{}.so", extension.id)
    }
}

/// Whether the extension's library is present.
pub fn is_installed(extension: &Extension) -> bool {
    installed_path(extension).is_some_and(|path| path.exists())
}

/// What state an extension is in, for the row that shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionState {
    /// Not installed, and this build could load it if it were.
    NotInstalled,
    /// Installed; the newest release matches or could not be checked.
    Installed,
    /// Installed, and a newer release is out.
    UpdateAvailable { to: updates::Version },
    /// In the catalogue, but this build's ABI is too old to load it — a
    /// bridge from a later Fontelle. Never fetched.
    NeedsNewerFontelle,
}

impl ExtensionState {
    /// The state, given whether it is installed and what the newest release
    /// is (from a check; `None` when none was made or it failed).
    pub fn of(
        extension: &Extension,
        installed: bool,
        latest: Option<updates::Version>,
    ) -> ExtensionState {
        if !extension.loadable() {
            return ExtensionState::NeedsNewerFontelle;
        }
        if !installed {
            return ExtensionState::NotInstalled;
        }
        // An installed bridge carries no version stamp on disk — the ABI is
        // the only compatibility fact — so this build cannot tell a current
        // install from an old one, and does not pretend to: an installed
        // extension reads as installed. The `latest` a check found is kept
        // for the day a stamp exists.
        let _ = latest;
        ExtensionState::Installed
    }
}

/// The action a row's button performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionAction {
    /// Fetch and install it.
    Install,
    /// Delete it.
    Remove,
    /// Nothing to do — the button is inert (a bridge this build cannot load).
    None,
}

/// What the button on an extension's row does, given its state.
pub fn action_for(state: &ExtensionState) -> ExtensionAction {
    match state {
        ExtensionState::NotInstalled => ExtensionAction::Install,
        ExtensionState::Installed | ExtensionState::UpdateAvailable { .. } => {
            ExtensionAction::Remove
        }
        ExtensionState::NeedsNewerFontelle => ExtensionAction::None,
    }
}

/// Downloads the extension's newest release and puts its library in place.
///
/// The updater's path exactly: the repository's `releases/latest`, the asset
/// for this target, its checksum out of `SHA256SUMS`, and — only if that
/// matches — the archive unpacked and the library renamed into the bridges
/// folder. A download that will not verify is not installed, the same rule
/// the binary updater follows.
pub fn install(
    extension: &Extension,
    target: &str,
    fetch: &Fetcher,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), String> {
    if !extension.loadable() {
        return Err(format!("{} needs a newer Fontelle to load", extension.name));
    }
    let listing = fetch(&extension.latest_url(), &mut |_, _| {})?;
    let listing =
        String::from_utf8(listing).map_err(|_| "the release listing was not text".to_string())?;
    let release = Release::from_github_json(&listing)?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == extension.asset_name(release.version, target))
        .ok_or_else(|| format!("{} has no build for {target}", extension.name))?;
    let sums = release.checksums().ok_or_else(|| {
        format!(
            "{} was published without {}",
            extension.name,
            updates::CHECKSUMS
        )
    })?;
    let sums = fetch(&sums.url, &mut |_, _| {})?;
    let sums = String::from_utf8_lossy(&sums);
    let expected = updates::expected_sha256(&sums, &asset.name)
        .ok_or_else(|| format!("{} does not list {}", updates::CHECKSUMS, asset.name))?;
    let bytes = fetch(&asset.url, progress)?;
    if updates::sha256_hex(&bytes) != expected {
        return Err(format!(
            "the checksum of {} did not match — the download was not installed",
            asset.name
        ));
    }
    let dir = bridges_dir().ok_or_else(|| "no data folder to install into".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    install_archive(&bytes, extension, &dir)
}

/// Unpacks the archive in a staging folder beside the bridges folder and
/// renames the extension's library into place — the same atomic-rename swap
/// the binary updater uses, so a half-written library is never loaded.
fn install_archive(archive: &[u8], extension: &Extension, dir: &Path) -> Result<(), String> {
    let staging = dir.join(format!(".fontelle-ext-{}", extension.id));
    std::fs::remove_dir_all(&staging).ok();
    std::fs::create_dir_all(&staging)
        .map_err(|e| format!("cannot write to {}: {e}", dir.display()))?;
    let result = unpack_and_place(archive, extension, dir, &staging);
    std::fs::remove_dir_all(&staging).ok();
    result
}

fn unpack_and_place(
    archive: &[u8],
    extension: &Extension,
    dir: &Path,
    staging: &Path,
) -> Result<(), String> {
    let archive_path = staging.join("archive");
    std::fs::write(&archive_path, archive)
        .map_err(|e| format!("cannot stage the download: {e}"))?;
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&archive_path)
        .arg("-C")
        .arg(staging)
        .status()
        .map_err(|e| format!("could not run tar: {e}"))?;
    if !status.success() {
        return Err("could not unpack the extension".to_string());
    }
    let wanted = extension.library_name();
    let library =
        find_file(staging, &wanted).ok_or_else(|| format!("the archive holds no {wanted}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&library, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("cannot mark the library executable: {e}"))?;
    }
    let destination = dir.join(installed_bridge_name(extension));
    // Renamed into place from the same filesystem, so it is atomic and the
    // rack never loads a half-written bridge.
    std::fs::rename(&library, &destination)
        .map_err(|e| format!("cannot put the extension in place: {e}"))?;
    Ok(())
}

/// The first file named `name` at or under `root`.
fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    let direct = root.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Some(found) = find_file(&path, name)
        {
            return Some(found);
        }
    }
    None
}

/// Deletes an installed extension. `Ok` when it was not there to begin with.
pub fn remove(extension: &Extension) -> Result<(), String> {
    let Some(path) = installed_path(extension) else {
        return Ok(());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("could not remove {}: {e}", path.display())),
    }
}
