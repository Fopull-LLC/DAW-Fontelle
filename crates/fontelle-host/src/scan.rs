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
        for (path, format) in bundles_in(folders, bridges) {
            let outcome = scan_bundle_as(&path, format, bridges);
            scan.take(path, outcome);
        }
        scan.sort();
        scan
    }

    /// [`of_with`](Self::of_with), with every bundle that has to be loaded
    /// to be read read by `prober`'s child processes — see [`crate::probe`].
    /// The same list in the same order; a bundle that crashed or hung is a
    /// failure in it rather than the end of the studio.
    pub fn of_probed(
        folders: &[PathBuf],
        bridges: &crate::Bridges,
        prober: &crate::BundleProber,
    ) -> Self {
        let bundles = bundles_in(folders, bridges);
        let away: Vec<PathBuf> = bundles
            .iter()
            .filter(|(_, format)| crate::probe::needs_loading_with(format, bridges))
            .map(|(path, _)| path.clone())
            .filter(|path| {
                bundles
                    .iter()
                    .find(|(p, _)| p == path)
                    .is_some_and(|(p, f)| crate::probe::needs_loading(p, *f))
            })
            .collect();
        let mut read_away: std::collections::HashMap<PathBuf, Result<Vec<PluginInfo>, String>> =
            away.iter()
                .cloned()
                .zip(prober.read_all(&away, bridges.folders()))
                .collect();
        let mut scan = Self::default();
        for (path, format) in bundles {
            let outcome = match read_away.remove(&path) {
                Some(outcome) => outcome,
                None => scan_bundle_as(&path, format, bridges),
            };
            scan.take(path, outcome);
        }
        scan.sort();
        scan
    }

    fn take(&mut self, path: PathBuf, outcome: Result<Vec<PluginInfo>, String>) {
        match outcome {
            Ok(found) => {
                for plugin in found {
                    // The same plugin found twice — a folder symlinked into
                    // another, a copy left beside the original — is one
                    // plugin, and the first place it was found wins.
                    if !self.plugins.iter().any(|seen| seen.key == plugin.key) {
                        self.plugins.push(plugin);
                    }
                }
            }
            Err(why) => self.failures.push(ScanFailure { path, why }),
        }
    }

    fn sort(&mut self) {
        self.plugins.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.key.cmp(&b.key))
        });
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
}

/// Every bundle under `folders`, in order, each once, with its format.
///
/// The rules a distribution's folders need, each found on a real machine:
///
/// - **An LV2 bundle is a folder with a `manifest.ttl`**, whatever it is
///   called — `.lv2` on the end is a habit. setBfree's `b_synth` has none.
/// - **A CLAP plugin is a file**, except on macOS: a folder called
///   `Cardinal.clap` holding `Cardinal.clap`, `CardinalFX.clap` and
///   `CardinalSynth.clap` is a folder of them.
/// - **A folder reached twice is walked once**: `/usr/lib64` is a link to
///   `/usr/lib` on Arch, and both are searched because on Fedora it is not.
fn bundles_in(folders: &[PathBuf], bridges: &crate::Bridges) -> Vec<(PathBuf, PluginFormat)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for folder in folders {
        collect(folder, 0, bridges, &mut seen, &mut out);
    }
    out
}

fn collect(
    folder: &Path,
    depth: usize,
    bridges: &crate::Bridges,
    seen: &mut std::collections::HashSet<PathBuf>,
    out: &mut Vec<(PathBuf, PluginFormat)>,
) {
    // Vendors nest one folder deep and a few nest two. Stopping somewhere
    // matters: a plugin folder that somebody pointed at their home directory
    // would otherwise walk the whole disk opening every file.
    const MAX_DEPTH: usize = 4;
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(canonical) = std::fs::canonicalize(folder) else {
        return;
    };
    if !seen.insert(canonical) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        let is_dir = path.is_dir();
        if is_dir && path.join("manifest.ttl").is_file() {
            if let Ok(canonical) = std::fs::canonicalize(&path)
                && seen.insert(canonical)
            {
                out.push((path, PluginFormat::Lv2));
            }
            continue;
        }
        // A file is a bundle when something here can read it: VST 2's
        // "extension" is the platform's shared library, and a folder of `.so`
        // files is not a folder of failures until a bridge is installed that
        // says which of them are plugins.
        let format = path.extension().and_then(|ext| {
            PluginFormat::ALL
                .into_iter()
                .find(|f| ext == f.extension() && (f.hosted() || bridges.serves(*f)))
        });
        let folder_of_claps =
            format == Some(PluginFormat::Clap) && is_dir && !cfg!(target_os = "macos");
        match format {
            Some(format) if !folder_of_claps => {
                if let Ok(canonical) = std::fs::canonicalize(&path)
                    && seen.insert(canonical)
                {
                    out.push((path, format));
                }
            }
            _ if is_dir => collect(&path, depth + 1, bridges, seen, out),
            _ => {}
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
    scan_bundle_as(path, format, bridges)
}

/// [`scan_bundle_with`], for a bundle whose format is already known — an
/// LV2 bundle is known by its `manifest.ttl`, not by its name.
pub(crate) fn scan_bundle_as(
    path: &Path,
    format: PluginFormat,
    bridges: &crate::Bridges,
) -> Result<Vec<PluginInfo>, String> {
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
        // Fedora, openSUSE and RHEL package 64-bit plugins under `lib64`.
        paths.push(PathBuf::from("/usr/lib64/clap"));
        paths.push(PathBuf::from("/usr/local/lib64/clap"));
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
