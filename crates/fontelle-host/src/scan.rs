//! What is installed on this machine.
//!
//! A scan is a **walk of folders the format nominates**, opening each bundle
//! and asking it what it holds. It is slow — every bundle is a `dlopen` of
//! somebody else's code — which is why it produces a list that can be kept
//! (see `fontelle_app::plugins`) rather than being run whenever a menu opens.

use std::path::{Path, PathBuf};

use fontelle_types::{PluginFormat, PluginKey};

/// One plugin, as the machine has it.
///
/// The path is here and the [`PluginKey`] is not built from it, which is the
/// distinction INVARIANT 8 draws for audio: what a project stores is the
/// plugin's own id, and a scan is what turns that back into a file on *this*
/// computer.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInfo {
    pub key: PluginKey,
    pub path: PathBuf,
    pub name: String,
    pub vendor: String,
    pub version: String,
    /// What the plugin says it is — CLAP's feature strings, verbatim.
    ///
    /// How many channels it takes and gives is **not** here, and that is
    /// deliberate: reading it costs an instantiation per plugin where a scan
    /// already costs a `dlopen` per bundle. A slot finds out when it opens
    /// one — see [`crate::HostedPlugin::audio_inputs`].
    pub features: Vec<String>,
}

impl PluginInfo {
    /// Whether this can go on an instrument channel.
    pub fn is_instrument(&self) -> bool {
        self.has("instrument") || self.has("synthesizer") || self.has("drum-machine")
    }

    /// Whether this can go in an insert slot.
    ///
    /// A plugin that says nothing about itself is treated as an effect, which
    /// is the safe way round: an effect in a chain that turns out to make no
    /// sound is a slot somebody can remove, where an instrument that turns out
    /// to need audio input is a channel that never plays.
    pub fn is_effect(&self) -> bool {
        if self.is_instrument() {
            return false;
        }
        self.has("audio-effect") || self.has("analyzer") || !self.has("note-effect")
    }

    fn has(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

/// A bundle that could not be read.
#[derive(Debug, Clone)]
pub struct ScanFailure {
    pub path: PathBuf,
    pub why: String,
}

/// The result of walking some folders.
///
/// Failures are kept rather than dropped for the reason
/// `fontelle_app::bundle::MissingAsset` keeps its list: a plugin that will not
/// load is the thing somebody most needs to be told about, and a scan that
/// silently produced a shorter list is a scan that looks like it worked.
#[derive(Debug, Clone, Default)]
pub struct PluginScan {
    /// Sorted by name, then by id — see the note on ordering below.
    pub plugins: Vec<PluginInfo>,
    pub failures: Vec<ScanFailure>,
}

impl PluginScan {
    /// Walks `folders`, in order.
    ///
    /// A folder that is not there is not a failure: the standard install
    /// locations name several folders per platform and a machine has some of
    /// them. Only a *bundle* that will not open is.
    ///
    /// The result is sorted, and that is not cosmetic: a directory listing is
    /// in whatever order the filesystem hands it back, so an unsorted scan
    /// would reshuffle the plugin menu every time it ran.
    pub fn of(folders: &[PathBuf]) -> Self {
        Self::of_with(folders, &crate::Bridges::none())
    }

    /// [`of`](Self::of), with the formats `bridges` serve included.
    pub fn of_with(folders: &[PathBuf], bridges: &crate::Bridges) -> Self {
        let mut scan = Self::default();
        for folder in folders {
            scan.walk(folder, 0, bridges);
        }
        scan.plugins.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.key.cmp(&b.key))
        });
        scan
    }

    /// Everything that can go on an instrument channel.
    pub fn instruments(&self) -> impl Iterator<Item = &PluginInfo> {
        self.plugins.iter().filter(|p| p.is_instrument())
    }

    /// Everything that can go in an insert slot.
    pub fn effects(&self) -> impl Iterator<Item = &PluginInfo> {
        self.plugins.iter().filter(|p| p.is_effect())
    }

    /// Where a plugin with this key was found, if it was.
    pub fn find(&self, key: &PluginKey) -> Option<&PluginInfo> {
        self.plugins.iter().find(|plugin| &plugin.key == key)
    }

    fn walk(&mut self, folder: &Path, depth: usize, bridges: &crate::Bridges) {
        // Vendors nest one folder deep and a few nest two. Stopping somewhere
        // matters: a plugin folder that somebody pointed at their home
        // directory would otherwise walk the whole disk opening every file.
        const MAX_DEPTH: usize = 4;
        if depth > MAX_DEPTH {
            return;
        }
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // A file is a bundle when something here can read it: VST 2's
            // "extension" is the platform's shared library, and a folder
            // of `.so` files is not a folder of failures until a bridge is
            // installed that says which of them are plugins.
            let is_bundle = path.extension().is_some_and(|ext| {
                PluginFormat::ALL
                    .iter()
                    .any(|f| ext == f.extension() && (f.hosted() || bridges.serves(*f)))
            });
            if is_bundle {
                match scan_bundle_with(&path, bridges) {
                    Ok(found) => {
                        for plugin in found {
                            // The same plugin found twice — a folder symlinked
                            // into another, a copy left beside the original —
                            // is one plugin, and the first place it was found
                            // wins. Two identical rows is a menu that looks
                            // broken.
                            if !self.plugins.iter().any(|seen| seen.key == plugin.key) {
                                self.plugins.push(plugin);
                            }
                        }
                    }
                    Err(why) => self.failures.push(ScanFailure { path, why }),
                }
            } else if path.is_dir() {
                self.walk(&path, depth + 1, bridges);
            }
        }
    }
}

/// Everything one bundle holds.
///
/// A bundle is not one plugin: every plugin suite ships a single file
/// containing all of them, so this returns a list and the scanner flattens it.
pub fn scan_bundle(path: &Path) -> Result<Vec<PluginInfo>, String> {
    scan_bundle_with(path, &crate::Bridges::none())
}

/// [`scan_bundle`], with a bridged format read through its bridge.
pub fn scan_bundle_with(path: &Path, bridges: &crate::Bridges) -> Result<Vec<PluginInfo>, String> {
    let format = path
        .extension()
        .and_then(|ext| {
            PluginFormat::ALL
                .into_iter()
                .find(|format| ext == format.extension())
        })
        .ok_or_else(|| "not a plugin bundle".to_string())?;
    match format {
        PluginFormat::Clap => crate::plugin::read_clap_bundle(path),
        PluginFormat::Lv2 => crate::lv2::read_lv2_bundle(path),
        PluginFormat::Vst3 => crate::vst3::read_vst3_bundle(path),
        other if bridges.serves(other) => bridges.scan_bundle(other, path),
        other => Err(format!(
            "{} plugins cannot be loaded without a bridge",
            other.label()
        )),
    }
}

/// The folders a plugin of a hosted format is installed in on this platform.
///
/// Read off each format's own specification rather than invented — CLAP,
/// LV2 and the VST 3 SDK all name these, and a host that looked somewhere
/// else would not find what an installer put where it was told to.
/// `CLAP_PATH`, `LV2_PATH` and `VST3_PATH` are honoured because the
/// specifications say to; they are how somebody keeps a plugin folder on
/// another disk. CLAP's folders first, then LV2's, then VST 3's, in the
/// order each specification lists them.
pub fn search_paths() -> Vec<PathBuf> {
    search_paths_with(&crate::Bridges::none())
}

/// [`search_paths`], plus the folders every loaded bridge nominates for
/// its own format.
pub fn search_paths_with(bridges: &crate::Bridges) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(from_env) = std::env::var_os("CLAP_PATH") {
        paths.extend(std::env::split_paths(&from_env).filter(|path| path.is_absolute()));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = &home {
            paths.push(home.join(".clap"));
        }
        paths.push(PathBuf::from("/usr/lib/clap"));
        paths.push(PathBuf::from("/usr/local/lib/clap"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = &home {
            paths.push(home.join("Library/Audio/Plug-Ins/CLAP"));
        }
        paths.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    }
    #[cfg(target_os = "windows")]
    {
        let _ = &home;
        if let Some(common) = std::env::var_os("COMMONPROGRAMFILES") {
            paths.push(PathBuf::from(common).join("CLAP"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            paths.push(PathBuf::from(local).join("Programs/Common/CLAP"));
        }
    }

    paths.extend(crate::lv2::search_paths(home.as_ref()));
    paths.extend(crate::vst3::search_paths(home.as_ref()));
    paths.extend(bridges.search_paths());

    paths.retain(|path| path.is_absolute());
    paths.dedup();
    paths
}
