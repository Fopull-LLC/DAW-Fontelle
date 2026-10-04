//! A plugin's **own** presets: the library it ships with.
//!
//! > *"we need to ensure our presets system works with it kind of like how
//! > flx does so that you can use the presets system for all plugins cleanly
//! > and it just works."*
//!
//! Fontelle's own presets of a plugin are its whole state, saved and loaded
//! by the studio. This is the other half: whatever the plugin brought with
//! it, found the way its format keeps it, so it can sit in the same list.
//!
//! - **CLAP** — a preset-discovery factory beside the plugin factory, whose
//!   providers declare where presets live and describe each one, and the
//!   `preset-load` extension, which loads one by the location and key the
//!   description gave. The plugin does the reading: the host never knows the
//!   file format. Surge XT has one.
//! - **VST 3** — `.vstpreset` files, under `<root>/<vendor>/<plugin>/` in
//!   the folders the SDK names. Each names the class it is for and carries
//!   the component's state, and usually the controller's, as chunks.
//! - **LV2** — `pset:Preset` resources that apply to the plugin, in its
//!   bundle's Turtle or beside it: port values, and sometimes a `state:state`
//!   of properties. Read when listed — the port values by index, and the
//!   rest as lilv's own state object, which is restored into the running
//!   instance — so loading one needs no lilv world.
//! - **`.fxp`** — VST 2's patch file, which JUCE plugins still ship as their
//!   library (OB-Xf's 488 patches, under `/usr/share/Surge Synth Team/OB-Xf/
//!   Patches`). No CLAP interface reaches them, but a JUCE plugin's state is
//!   `getStateInformation` in every format, and so is an opaque `.fxp`
//!   chunk: OB-Xf's CLAP state and its `.fxp` chunks both begin `VC2!` and
//!   carry the same XML. So the chunk is handed over as the plugin's state —
//!   **only** when it begins the way the plugin's own state does, so a
//!   patch of some other program in a folder that happens to share a name
//!   is never forced on it.

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};

use clack_extensions::preset_discovery::prelude::*;
use clack_host::prelude::{HostError as ClapHostError, PluginEntry};
use fontelle_types::PluginFormat;

use crate::plugin::Inner;
use crate::{HostedPlugin, HostedProcessor, PluginHost, PluginInfo};

/// How deep a library folder is walked. OB-Xf's is two (category, file),
/// Surge's three; this is room for a pack inside a category without walking
/// a whole disk that somebody pointed a folder at.
const MAX_DEPTH: usize = 6;

/// One preset from a plugin's own library.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnPreset {
    pub name: String,
    /// The folder, the bank or the tag it was filed under — empty when the
    /// plugin said nothing.
    pub category: String,
    pub source: OwnPresetSource,
}

/// Where a plugin keeps one of its presets, and so how it is loaded.
#[derive(Debug, Clone, PartialEq)]
pub enum OwnPresetSource {
    /// Through the plugin's `preset-load`: `None` is a preset inside the
    /// plugin itself, a path a file (or a container) it declared.
    Clap {
        location: Option<PathBuf>,
        load_key: Option<String>,
    },
    /// A `.vstpreset` file.
    Vst3(PathBuf),
    /// An LV2 preset, already read: the value of each control port it sets,
    /// by port index, and lilv's copy of its whole state.
    Lv2 {
        uri: String,
        ports: Vec<(u32, f32)>,
        state: Option<crate::lv2::Lv2Preset>,
    },
    /// A `.fxp` whose chunk is the plugin's own state.
    Fxp(PathBuf),
}

/// The folders a plugin's library is looked for under.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetRoots {
    /// Where a plugin keeps data of its own: `<root>/<plugin>`,
    /// `<root>/<vendor>/<plugin>`, or `<root>/<anything>/<plugin>` — OB-Xf's
    /// is `/usr/share/Surge Synth Team/OB-Xf`, which names neither its
    /// vendor field nor anything a host could guess.
    pub data: Vec<PathBuf>,
    /// Where VST 3 presets live: `<root>/<vendor>/<plugin>/…`.
    pub vst3: Vec<PathBuf>,
}

impl PresetRoots {
    /// Nowhere — a plugin's library is then only what the plugin itself
    /// lists (CLAP discovery, LV2's own Turtle).
    pub fn none() -> Self {
        Self::default()
    }

    /// Where plugins put their libraries on this machine: the VST 3 SDK's
    /// preset folders, and the usual data folders for everything else.
    pub fn standard() -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        let mut data = Vec::new();
        let mut vst3 = Vec::new();
        if cfg!(target_os = "windows") {
            for var in ["APPDATA", "PROGRAMDATA", "LOCALAPPDATA"] {
                if let Some(dir) = std::env::var_os(var).map(PathBuf::from) {
                    data.push(dir.clone());
                    vst3.push(dir.join("VST3 Presets"));
                }
            }
            if let Some(home) = &home {
                data.push(home.join("Documents"));
                vst3.push(home.join("Documents").join("VST3 Presets"));
            }
        } else if cfg!(target_os = "macos") {
            if let Some(home) = &home {
                data.push(home.join("Documents"));
                data.push(home.join("Library").join("Application Support"));
                vst3.push(home.join("Library").join("Audio").join("Presets"));
            }
            data.push(PathBuf::from("/Library/Application Support"));
            vst3.push(PathBuf::from("/Library/Audio/Presets"));
        } else {
            if let Some(home) = &home {
                data.push(home.join(".local").join("share"));
                data.push(home.join("Documents"));
                vst3.push(home.join(".vst3").join("presets"));
            }
            data.push(PathBuf::from("/usr/share"));
            data.push(PathBuf::from("/usr/local/share"));
            vst3.push(PathBuf::from("/usr/share/vst3/presets"));
            vst3.push(PathBuf::from("/usr/local/share/vst3/presets"));
        }
        Self { data, vst3 }
    }
}

impl PluginHost {
    /// Everything in `info`'s own library, by category and then name.
    ///
    /// Opens the plugin to learn what its state looks like, when there are
    /// `.fxp` files to compare with it. The rack does not use this: it lists
    /// a big library off the main thread with [`list_own_presets`], handing
    /// over the state of the instance it already has — Surge XT describes
    /// each of its three thousand files through its own provider, which
    /// takes most of a second.
    pub fn own_presets(&mut self, info: &PluginInfo, roots: &PresetRoots) -> Vec<OwnPreset> {
        if info.key.format == PluginFormat::Lv2 {
            return sorted(self.lv2_presets(info));
        }
        let own = if library_folders(info, &roots.data).is_empty() {
            None
        } else {
            self.open(&info.path, &info.key)
                .ok()
                .and_then(|mut plugin| plugin.save_state())
        };
        list_own_presets(info, roots, own.as_deref())
    }

    /// An LV2 plugin's own library — see [`list_own_presets`] for why this
    /// one is not listed off the main thread.
    pub fn own_lv2_presets(&mut self, info: &PluginInfo) -> Vec<OwnPreset> {
        sorted(self.lv2_presets(info))
    }

    /// The `pset:Preset`s that apply to an LV2 plugin, read whole — through
    /// this host's one feature set, the one its instances are made with, so
    /// the URIDs in a preset's state are the plugin's own.
    fn lv2_presets(&mut self, info: &PluginInfo) -> Vec<OwnPreset> {
        if !self.worlds.contains_key(&info.path) {
            let Ok(world) = crate::lv2::load_world(&info.path) else {
                return Vec::new();
            };
            self.worlds.insert(info.path.clone(), world);
        }
        let world = &self.worlds[&info.path];
        let features = match &self.lv2_features {
            Some(features) => std::sync::Arc::clone(features),
            None => {
                let features = crate::lv2::build_features(world);
                self.lv2_features = Some(std::sync::Arc::clone(&features));
                features
            }
        };
        crate::lv2::presets(world, &features, &info.key.id)
    }
}

impl HostedPlugin {
    /// Whether [`load_own_preset`](Self::load_own_preset) cannot do it alone:
    /// an LV2 plugin that is running takes a state only with its processor in
    /// hand — see [`load_own_preset_with`](Self::load_own_preset_with).
    pub fn own_preset_needs_processor(&self) -> bool {
        matches!(self.inner, Inner::Lv2(_)) && self.active
    }

    /// Loads one of this plugin's own presets into it, on the main thread.
    ///
    /// Its parameters are read back afterwards, so the wire and the knobs say
    /// what the preset set. An LV2 plugin that is not running is handed the
    /// preset to take when it starts.
    pub fn load_own_preset(&mut self, preset: &OwnPreset) -> Result<(), String> {
        let message = format!("{} would not load {}", self.info.name, preset.name);
        let refused = || message.clone();
        match &preset.source {
            OwnPresetSource::Clap { location, load_key } => {
                let path = location
                    .as_ref()
                    .map(|path| CString::new(path.to_string_lossy().as_bytes()))
                    .transpose()
                    .map_err(|_| refused())?;
                let key = load_key
                    .as_ref()
                    .map(|key| CString::new(key.as_str()))
                    .transpose()
                    .map_err(|_| refused())?;
                let location = match &path {
                    Some(path) => Location::File { path },
                    None => Location::Plugin,
                };
                let instance = self.clap().ok_or_else(refused)?;
                let loader = instance
                    .plugin_handle()
                    .get_extension::<PluginPresetLoad>()
                    .ok_or_else(refused)?;
                loader
                    .load_from_location(&mut instance.plugin_handle(), location, key.as_deref())
                    .map_err(|_| refused())?;
                self.reread_params();
                Ok(())
            }
            OwnPresetSource::Vst3(path) => {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let (component, controller) = vstpreset_chunks(&bytes).ok_or_else(refused)?;
                let blob = crate::vst3::join_state(component, controller);
                self.load_state(&blob).then_some(()).ok_or_else(refused)
            }
            OwnPresetSource::Fxp(path) => {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let chunk = fxp_chunk(&bytes).ok_or_else(refused)?;
                // A program chunk, fitted into the state the plugin keeps
                // when that is its shape — see `program_into_state`.
                let format = self.info.key.format;
                let own = self.save_state().map(|state| component_of(format, &state));
                let chunk = own
                    .and_then(|own| program_into_state(chunk, &own))
                    .unwrap_or_else(|| chunk.to_vec());
                let blob = match format {
                    PluginFormat::Vst3 => crate::vst3::join_state(&chunk, &[]),
                    _ => chunk,
                };
                self.load_state(&blob).then_some(()).ok_or_else(refused)
            }
            OwnPresetSource::Lv2 { ports, state, .. } => {
                // A state goes into an instance, and there is none to put it
                // in until the plugin runs — nor may one that is running be
                // handed it without its processor.
                if state.is_some() {
                    return Err(format!(
                        "{} has to be running, and paused, to take {}",
                        self.info.name, preset.name
                    ));
                }
                for (index, value) in ports {
                    self.set_param(*index, f64::from(*value));
                }
                Ok(())
            }
        }
    }

    /// [`load_own_preset`](Self::load_own_preset) with the processor in hand
    /// — the running instance's own state for LV2, recalled out of the graph
    /// the way a snapshot recalls it; the same as `load_own_preset` for every
    /// other format.
    pub fn load_own_preset_with(
        &mut self,
        processor: &mut HostedProcessor,
        preset: &OwnPreset,
    ) -> Result<(), String> {
        let OwnPresetSource::Lv2 { ports, state, .. } = &preset.source else {
            return self.load_own_preset(preset);
        };
        if let Some(state) = state {
            processor.lv2_restore_preset(state);
        }
        for (index, value) in ports {
            self.set_param(*index, f64::from(*value));
        }
        Ok(())
    }
}

/// A CLAP or VST 3 plugin's own library, by category and then name — the
/// discovery factory's list, `.vstpreset` files, and the `.fxp` files whose
/// chunk begins the way `own_state` (the plugin's own state, from an
/// instance somebody already has) does. `None` lists no `.fxp` at all.
///
/// **Never instantiates the plugin**, so it can run on a thread of its own:
/// a JUCE plugin made on one thread while the studio's instance lives on
/// another is asking for trouble. An LV2 plugin's library is not listed
/// here — its presets carry URIDs from the one map the host's instances
/// share, and it is fast anyway ([`PluginHost::own_lv2_presets`]).
pub fn list_own_presets(
    info: &PluginInfo,
    roots: &PresetRoots,
    own_state: Option<&[u8]>,
) -> Vec<OwnPreset> {
    let mut found = match info.key.format {
        PluginFormat::Clap => {
            let mut host = PluginHost::new();
            match host.entry(&info.path) {
                Ok(entry) => clap_presets(entry, &info.key.id),
                Err(_) => Vec::new(),
            }
        }
        PluginFormat::Vst3 => vst3_presets(info, roots),
        _ => return Vec::new(),
    };
    // `.fxp` files beside a plugin that lists its own library are often that
    // library again, as files: Surge XT's CLAP describes its three thousand
    // patches through its provider, and the same patches are what is in
    // `/usr/share/surge-xt`. A file whose name the plugin already listed is
    // left out.
    if let Some(own) = own_state {
        let listed: std::collections::HashSet<String> =
            found.iter().map(|preset| preset.name.clone()).collect();
        found.extend(
            fxp_presets(info, roots, &component_of(info.key.format, own))
                .into_iter()
                .filter(|preset| !listed.contains(&preset.name)),
        );
    }
    sorted(found)
}

fn sorted(mut found: Vec<OwnPreset>) -> Vec<OwnPreset> {
    found.sort_by(|a, b| {
        (a.category.to_lowercase(), a.name.to_lowercase())
            .cmp(&(b.category.to_lowercase(), b.name.to_lowercase()))
    });
    found.dedup_by(|a, b| a.name == b.name && a.category == b.category);
    found
}

/// The `.fxp` files in `info`'s data folders whose chunk begins the way
/// `own` — the plugin's own state — does.
fn fxp_presets(info: &PluginInfo, roots: &PresetRoots, own: &[u8]) -> Vec<OwnPreset> {
    if own.len() < 4 {
        return Vec::new();
    }
    let mut found = Vec::new();
    for folder in library_folders(info, &roots.data) {
        walk(&folder, 0, &mut |path| {
            if !has_extension(path, "fxp") {
                return;
            }
            let Ok(bytes) = std::fs::read(path) else {
                return;
            };
            let Some(chunk) = fxp_chunk(&bytes) else {
                return;
            };
            if chunk.len() < 4 || chunk[..4] != own[..4] {
                return;
            }
            found.push(OwnPreset {
                name: stem(path),
                category: category_of(&folder, path),
                source: OwnPresetSource::Fxp(path.to_path_buf()),
            });
        });
    }
    found
}

// ------------------------------------------------------------------- CLAP

/// Everything a CLAP plugin's preset-discovery providers list for it.
fn clap_presets(entry: &PluginEntry, plugin_id: &str) -> Vec<OwnPreset> {
    let Some(factory) = entry.get_factory::<PresetDiscoveryFactory>() else {
        return Vec::new();
    };
    // **An entry that answers every factory id with its plugin factory.**
    // SpectMorph's does: asked for the discovery factory, it returned the
    // plugin factory — the same three functions in the same places — so a
    // "provider" was a SpectMorph plugin, handed the indexer as its host,
    // and its first `get_extension("clap.log")` landed in the indexer's
    // `declare_filetype` and read a string as a struct (a crash in every
    // listing, 2026-09-29). A discovery factory that is the plugin factory
    // is not one.
    if entry.get_plugin_factory().is_some_and(|plugins| {
        plugins.raw().as_ptr().cast::<()>() == factory.raw().as_ptr().cast::<()>()
    }) {
        return Vec::new();
    }
    let host = crate::plugin::host_info();
    let mut found = Vec::new();
    for descriptor in factory.provider_descriptors() {
        let Some(id) = descriptor.id() else {
            continue;
        };
        let Ok(mut provider) = Provider::instantiate(Declared::default(), entry, id, &host) else {
            continue;
        };
        let declared = provider.indexer().clone();
        for (root, name) in &declared.locations {
            let mut receiver = Described::default();
            match root {
                None => {
                    provider.get_metadata(Location::Plugin, &mut receiver);
                    found.extend(receiver.presets(plugin_id, None, "", name));
                }
                Some(root) if root.is_file() => {
                    describe_file(&mut provider, root, &mut receiver);
                    found.extend(receiver.presets(plugin_id, Some(root), "", name));
                }
                Some(root) => {
                    let mut files = Vec::new();
                    walk(root, 0, &mut |path| {
                        if declared.extensions.is_empty()
                            || declared
                                .extensions
                                .iter()
                                .any(|ext| has_extension(path, ext))
                        {
                            files.push(path.to_path_buf());
                        }
                    });
                    for file in files {
                        let mut receiver = Described::default();
                        describe_file(&mut provider, &file, &mut receiver);
                        found.extend(receiver.presets(
                            plugin_id,
                            Some(&file),
                            &category_of(root, &file),
                            name,
                        ));
                    }
                }
            }
        }
    }
    found
}

fn describe_file(provider: &mut Provider<Declared>, file: &Path, receiver: &mut Described) {
    let Ok(path) = CString::new(file.to_string_lossy().as_bytes()) else {
        return;
    };
    provider.get_metadata(Location::File { path: &path }, receiver);
}

/// What a provider declared while it was set up: where its presets are, and
/// which files are presets.
#[derive(Default, Clone)]
struct Declared {
    /// `None` is the plugin itself; each with the name it was given.
    locations: Vec<(Option<PathBuf>, String)>,
    /// Lowercase, without the dot. Empty: every file.
    extensions: Vec<String>,
}

impl IndexerImpl for Declared {
    fn declare_filetype(&mut self, file_type: FileType) -> Result<(), ClapHostError> {
        if let Some(ext) = file_type.file_extension.and_then(|ext| ext.to_str().ok()) {
            self.extensions
                .push(ext.trim_start_matches('.').to_lowercase());
        }
        Ok(())
    }

    fn declare_location(&mut self, location: LocationInfo) -> Result<(), ClapHostError> {
        let name = location.name.to_string_lossy().into_owned();
        let root = location
            .location
            .file_path()
            .map(|path| PathBuf::from(path.to_string_lossy().into_owned()));
        self.locations.push((root, name));
        Ok(())
    }

    fn declare_soundpack(&mut self, _soundpack: Soundpack) -> Result<(), ClapHostError> {
        Ok(())
    }
}

/// What a provider described, one preset at a time.
#[derive(Default)]
struct Described {
    presets: Vec<DescribedPreset>,
}

struct DescribedPreset {
    name: Option<String>,
    load_key: Option<String>,
    /// The CLAP ids it says it is for. Empty: the provider's plugin.
    for_plugins: Vec<String>,
    features: Vec<String>,
}

impl Described {
    /// The ones for `plugin_id`, filed under `category` (the folders a file
    /// was found in), or else the first feature the plugin gave them — CLAP
    /// has no word for a category, and a feature (`bass`, `pad`) is what a
    /// plugin uses for one — or else the name of the location it declared.
    fn presets(
        self,
        plugin_id: &str,
        file: Option<&Path>,
        category: &str,
        location: &str,
    ) -> Vec<OwnPreset> {
        self.presets
            .into_iter()
            .filter(|preset| {
                preset.for_plugins.is_empty() || preset.for_plugins.iter().any(|id| id == plugin_id)
            })
            .map(|preset| OwnPreset {
                name: preset
                    .name
                    .or_else(|| file.map(stem))
                    .unwrap_or_else(|| "Preset".to_string()),
                category: match category {
                    "" => preset
                        .features
                        .first()
                        .cloned()
                        .unwrap_or_else(|| location.to_string()),
                    category => category.to_string(),
                },
                source: OwnPresetSource::Clap {
                    location: file.map(Path::to_path_buf),
                    load_key: preset.load_key,
                },
            })
            .collect()
    }

    fn current(&mut self) -> Option<&mut DescribedPreset> {
        self.presets.last_mut()
    }
}

fn text(value: &CStr) -> String {
    value.to_string_lossy().into_owned()
}

impl MetadataReceiverImpl for Described {
    fn on_error(&mut self, _error_code: i32, _error_message: Option<&CStr>) {}

    fn begin_preset(
        &mut self,
        name: Option<&CStr>,
        load_key: Option<&CStr>,
    ) -> Result<(), ClapHostError> {
        self.presets.push(DescribedPreset {
            name: name.map(text),
            load_key: load_key.map(text),
            for_plugins: Vec::new(),
            features: Vec::new(),
        });
        Ok(())
    }

    fn add_plugin_id(&mut self, plugin_id: UniversalPluginId) {
        if plugin_id.abi.to_bytes() == b"clap"
            && let Some(preset) = self.current()
        {
            preset.for_plugins.push(text(plugin_id.id));
        }
    }

    fn set_soundpack_id(&mut self, _soundpack_id: &CStr) {}

    fn set_flags(&mut self, _flags: Flags) {}

    fn add_creator(&mut self, _creator: &CStr) {}

    fn set_description(&mut self, _description: &CStr) {}

    fn set_timestamps(
        &mut self,
        _creation_time: Option<Timestamp>,
        _modification_time: Option<Timestamp>,
    ) {
    }

    fn add_feature(&mut self, feature: &CStr) {
        if let Some(preset) = self.current() {
            preset.features.push(text(feature));
        }
    }

    fn add_extra_info(&mut self, _key: &CStr, _value: &CStr) {}
}

// ------------------------------------------------------------------ VST 3

/// The `.vstpreset` files for `info` in the SDK's preset folders: under
/// `<root>/<vendor>/<plugin>`, and made for its class.
fn vst3_presets(info: &PluginInfo, roots: &PresetRoots) -> Vec<OwnPreset> {
    let mut found = Vec::new();
    for root in &roots.vst3 {
        let folder = root.join(&info.vendor).join(&info.name);
        walk(&folder, 0, &mut |path| {
            if !has_extension(path, "vstpreset") {
                return;
            }
            let Ok(bytes) = std::fs::read(path) else {
                return;
            };
            if bytes.len() < 48 || &bytes[..4] != b"VST3" {
                return;
            }
            if !bytes[8..40].eq_ignore_ascii_case(info.key.id.as_bytes()) {
                return;
            }
            found.push(OwnPreset {
                name: stem(path),
                category: category_of(&folder, path),
                source: OwnPresetSource::Vst3(path.to_path_buf()),
            });
        });
    }
    found
}

/// A `.vstpreset`'s component chunk and controller chunk (empty when it
/// has none). `None` for a file that is not one.
fn vstpreset_chunks(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    if bytes.len() < 48 || &bytes[..4] != b"VST3" {
        return None;
    }
    let list = usize::try_from(i64::from_le_bytes(bytes[40..48].try_into().ok()?)).ok()?;
    let head = bytes.get(list..list + 8)?;
    if &head[..4] != b"List" {
        return None;
    }
    let count = i32::from_le_bytes(head[4..8].try_into().ok()?).max(0) as usize;
    let (mut component, mut controller) = (None, &[][..]);
    for index in 0..count {
        let at = list + 8 + index * 20;
        let entry = bytes.get(at..at + 20)?;
        let offset = usize::try_from(i64::from_le_bytes(entry[4..12].try_into().ok()?)).ok()?;
        let size = usize::try_from(i64::from_le_bytes(entry[12..20].try_into().ok()?)).ok()?;
        let chunk = bytes.get(offset..offset.checked_add(size)?)?;
        match &entry[..4] {
            b"Comp" => component = Some(chunk),
            b"Cont" => controller = chunk,
            _ => {}
        }
    }
    Some((component?, controller))
}

// -------------------------------------------------------------------- .fxp

/// The opaque chunk of a `.fxp` program file (`FPCh`) — `None` for one that
/// is a list of parameter values (`FxCk`), which no state could be made of.
fn fxp_chunk(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < 60 || &bytes[..4] != b"CcnK" || &bytes[8..12] != b"FPCh" {
        return None;
    }
    let size = u32::from_be_bytes(bytes[56..60].try_into().ok()?) as usize;
    bytes.get(60..60 + size)
}

/// The part of a plugin's own state an `.fxp` chunk is compared with: the
/// whole of it, or a VST 3's component half.
fn component_of(format: PluginFormat, state: &[u8]) -> Vec<u8> {
    match format {
        PluginFormat::Vst3 => crate::vst3::split_state(state)
            .map(|(component, _)| component.to_vec())
            .unwrap_or_default(),
        _ => state.to_vec(),
    }
}

// ------------------------------------------------------------------ folders

/// The folders under `roots` that are `info`'s own: named after it, directly
/// or one folder down (its vendor, or whatever its maker called itself).
fn library_folders(info: &PluginInfo, roots: &[PathBuf]) -> Vec<PathBuf> {
    let wanted = folder_key(&info.name);
    let named = |path: &Path| {
        path.is_dir()
            && path
                .file_name()
                .is_some_and(|name| folder_key(&name.to_string_lossy()) == wanted)
    };
    let mut folders = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut children: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        children.sort();
        for child in children {
            if named(&child) && !folders.contains(&child) {
                folders.push(child.clone());
            }
            // One level down, for a vendor's folder: `Surge Synth Team/OB-Xf`.
            let Ok(nested) = std::fs::read_dir(&child) else {
                continue;
            };
            let mut nested: Vec<PathBuf> = nested.flatten().map(|entry| entry.path()).collect();
            nested.sort();
            for path in nested {
                if named(&path) && !folders.contains(&path) {
                    folders.push(path);
                }
            }
        }
    }
    folders
}

/// A plugin's name as a folder for it might be spelt: case, spaces, dashes
/// and underscores aside.
///
/// Linux packages name a plugin's data folder in lower case with dashes —
/// Surge XT's patches are in `/usr/share/surge-xt` — and looked for by the
/// plugin's name exactly, Surge's VST 3 had no library at all. Only those
/// differences: "Surge XT 2" is still another plugin's folder.
fn folder_key(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Every file under `folder`, to [`MAX_DEPTH`], in name order.
fn walk(folder: &Path, depth: usize, found: &mut dyn FnMut(&Path)) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(&path, depth + 1, found);
        } else {
            found(&path);
        }
    }
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The folders between `root` and `file`, as a category: `Leads`, or
/// `Leads / Mono` a level deeper. A top folder a library is kept *in* —
/// `Patches`, `Presets` — is not a category and is dropped.
fn category_of(root: &Path, file: &Path) -> String {
    let Some(parent) = file
        .parent()
        .and_then(|parent| parent.strip_prefix(root).ok())
    else {
        return String::new();
    };
    let parts: Vec<String> = parent
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let parts = match parts.first().map(|first| first.to_lowercase()) {
        Some(first) if matches!(first.as_str(), "patches" | "presets" | "programs") => &parts[1..],
        _ => &parts[..],
    };
    parts.join(" / ")
}

/// A JUCE **program** chunk made into the plugin's **state**, when that is
/// all that separates them.
///
/// A `.fxp` holds one program — JUCE's `getCurrentProgramStateInformation`,
/// the patch as the root element's own attributes — while a CLAP (or VST 3)
/// state is `getStateInformation`, the whole plugin. OB-Xf's state wraps one
/// program: `<OB-Xf … single-program-format="1"><program …/>` and whatever
/// else the plugin keeps (its editor's `<dawExtraState>`). Handed a program
/// chunk as its state, OB-Xf kept the patch it had, and said nothing.
///
/// So when the state has that shape and the chunk is the same root with no
/// children, the chunk's attributes become the state's `<program>` — less
/// the ones the state's root carries itself (the version) — and everything
/// else in the state stays. `None` for anything else, which is handed over
/// whole.
fn program_into_state(chunk: &[u8], state: &[u8]) -> Option<Vec<u8>> {
    let program_xml = juce_xml(chunk)?;
    let state_xml = juce_xml(state)?;
    let program = root_tag(program_xml)?;
    let root = root_tag(state_xml)?;
    if !program.closed || root.closed || program.name != root.name {
        return None;
    }
    let root_attributes = attributes(root.attributes);
    if !root_attributes.contains(&("single-program-format", "\"1\"")) {
        return None;
    }
    // The `<program>` element among the root's children, empty.
    let after_root = &state_xml[root.end..];
    let at = after_root.find("<program")?;
    let start = root.end + at;
    let tag = tag_at(state_xml, start)?;
    if tag.name != "program" || !tag.closed {
        return None;
    }
    let kept: Vec<String> = attributes(program.attributes)
        .into_iter()
        .filter(|(name, _)| !root_attributes.iter().any(|(held, _)| held == name))
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    let mut xml = String::with_capacity(state_xml.len() + program_xml.len());
    xml.push_str(&state_xml[..start]);
    xml.push_str("<program ");
    xml.push_str(&kept.join(" "));
    xml.push_str("/>");
    xml.push_str(&state_xml[tag.end..]);
    Some(juce_binary(&xml))
}

/// The text of JUCE's `copyXmlToBinary`: `VC2!`, the length of the text
/// with its NUL, the text.
fn juce_xml(bytes: &[u8]) -> Option<&str> {
    if bytes.len() < 8 || &bytes[..4] != b"VC2!" {
        return None;
    }
    let length = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    let text = bytes.get(8..8 + length)?;
    let text = text.strip_suffix(&[0]).unwrap_or(text);
    std::str::from_utf8(text).ok()
}

fn juce_binary(xml: &str) -> Vec<u8> {
    let mut bytes = b"VC2!".to_vec();
    bytes.extend_from_slice(&((xml.len() + 1) as u32).to_le_bytes());
    bytes.extend_from_slice(xml.as_bytes());
    bytes.push(0);
    bytes
}

/// One start tag: its name, its attribute text, whether it closes itself,
/// and where it ends.
struct Tag<'a> {
    name: &'a str,
    attributes: &'a str,
    closed: bool,
    end: usize,
}

/// The document's root element's start tag, past the declaration.
fn root_tag(xml: &str) -> Option<Tag<'_>> {
    let mut at = 0;
    loop {
        at += xml[at..].find('<')?;
        match xml[at + 1..].chars().next()? {
            '?' | '!' => at += xml[at..].find('>')? + 1,
            _ => return tag_at(xml, at),
        }
    }
}

/// The start tag that begins at `start` (on its `<`). Quote-aware, so a `>`
/// or `/` inside a value does not end it.
fn tag_at(xml: &str, start: usize) -> Option<Tag<'_>> {
    let body = xml.get(start + 1..)?;
    let name_end = body.find(|c: char| c.is_whitespace() || c == '/' || c == '>')?;
    let mut quote = None;
    for (offset, c) in body.char_indices() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => {
                let inner = &body[..offset];
                let closed = inner.ends_with('/');
                let inner = inner.strip_suffix('/').unwrap_or(inner);
                return Some(Tag {
                    name: &body[..name_end],
                    attributes: inner[name_end..].trim(),
                    closed,
                    end: start + 1 + offset + 1,
                });
            }
            (None, _) => {}
        }
    }
    None
}

/// `name="value"` pairs, the value with its quotes, in order.
fn attributes(text: &str) -> Vec<(&str, &str)> {
    let mut found = Vec::new();
    let mut rest = text.trim_start();
    while let Some(equals) = rest.find('=') {
        let name = rest[..equals].trim();
        let after = rest[equals + 1..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(close) = after[1..].find(quote) else {
            break;
        };
        found.push((name, &after[..close + 2]));
        rest = after[close + 2..].trim_start();
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// JUCE's `copyXmlToBinary`: a magic, the text's length with its NUL,
    /// the text and the NUL.
    fn juce(xml: &str) -> Vec<u8> {
        let mut bytes = b"VC2!".to_vec();
        bytes.extend_from_slice(&((xml.len() + 1) as u32).to_le_bytes());
        bytes.extend_from_slice(xml.as_bytes());
        bytes.push(0);
        bytes
    }

    const STATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?> <OB-Xf ob-xf_version="2025-12-13" single-program-format="1"><program Volume="0.5" programName="Init"/><dawExtraState><DAWExtraState version="1"/></dawExtraState></OB-Xf>"#;

    /// OB-Xf's library is `.fxp` *program* chunks — the patch as the root's
    /// own attributes — and its CLAP state is the whole plugin, one
    /// `<program>` inside a root that says so. Handed the chunk as it is,
    /// OB-Xf kept the patch it had.
    #[test]
    fn a_program_chunk_becomes_the_program_in_the_plugins_state() {
        let program = juce(
            r#"<?xml version="1.0" encoding="UTF-8"?> <OB-Xf ob-xf_version="2025-12-13" Volume="0.2" programName="Pulled PWM" category="Keys"/>"#,
        );
        let made = program_into_state(&program, &juce(STATE)).expect("the shapes match");
        assert_eq!(
            made,
            juce(
                r#"<?xml version="1.0" encoding="UTF-8"?> <OB-Xf ob-xf_version="2025-12-13" single-program-format="1"><program Volume="0.2" programName="Pulled PWM" category="Keys"/><dawExtraState><DAWExtraState version="1"/></dawExtraState></OB-Xf>"#
            )
        );
    }

    /// Anything else is left to be handed over whole.
    #[test]
    fn a_chunk_of_another_shape_is_not_rewritten() {
        let other = juce(r#"<?xml version="1.0"?> <Other a="1"/>"#);
        assert!(program_into_state(&other, &juce(STATE)).is_none());
        let whole = juce(STATE);
        assert!(program_into_state(&whole, &juce(STATE)).is_none());
        assert!(program_into_state(b"not xml at all", &juce(STATE)).is_none());
    }
}
