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
//!   the component's state, and usually the controller's, as chunks. And
//!   **programs**, compiled into the plugin: a program list on a unit and a
//!   program-change parameter that selects one (`Vst3Plugin::programs`) —
//!   how a JUCE plugin offers `getNumPrograms()`, Dexed's 32 voices.
//! - **LV2** — `pset:Preset` resources that apply to the plugin, in its
//!   bundle's Turtle or beside it: port values, and sometimes a `state:state`
//!   of properties. Read when listed — the port values by index, and the
//!   rest as lilv's own state object, which is restored into the running
//!   instance — so loading one needs no lilv world. A plugin with none
//!   there may have programs compiled into it, offered through the KXStudio
//!   programs extension (Dexed's LV2, DISTRHO's ports): asked of the running
//!   instance, and selected in it.
//! - **`.fxp`** — VST 2's patch file, which JUCE plugins still ship as their
//!   library (OB-Xf's 488 patches, under `/usr/share/Surge Synth Team/OB-Xf/
//!   Patches`). No CLAP interface reaches them, but a JUCE plugin's state is
//!   `getStateInformation` in every format, and so is an opaque `.fxp`
//!   chunk: OB-Xf's CLAP state and its `.fxp` chunks both begin `VC2!` and
//!   carry the same XML. So the chunk is handed over as the plugin's state —
//!   **only** when it begins the way the plugin's own state does, so a
//!   patch of some other program in a folder that happens to share a name
//!   is never forced on it. OB-Xf's LV2 keeps the same state in JUCE's
//!   base64 under `StateString`, and takes them the same way.
//! - **Other library files a state is made of** — the same rule, each
//!   checked against the plugin's state on this machine: Vital's `.vital`
//!   is its whole state (CLAP, VST 3 inside JUCE's VST 2 bank; Vitalium's
//!   LV2 `stateBinary`), and Cardinal's `.vcv` is the `patch` its DPF state
//!   carries in base64 (CLAP and LV2). See `state_from_file`.
//! - **amsynth's banks** — text files of parameters by name, which its LV2
//!   uses as its ports' symbols, and nothing an LV2 host can list. A preset
//!   naming a parameter the plugin has no port for is not offered.

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
    /// A file that **is** the plugin's state, or the part of it a file of
    /// its library replaces: Vital's `.vital` (its state is that JSON),
    /// Cardinal's `.vcv` (the patch its state carries). Fitted into the state
    /// the plugin keeps when loaded — see `state_from_file`.
    StateFile(PathBuf),
    /// A **program** compiled into the plugin, chosen by setting its
    /// program-change parameter `param` to `value` (in the host's units for
    /// that parameter: the step, for a stepped one). Read off the running
    /// instance — see [`HostedPlugin::programs`].
    Program { param: u32, value: f64 },
    /// A program compiled into an LV2 plugin, offered through the KXStudio
    /// programs extension and selected in its running instance — see
    /// [`HostedPlugin::programs_with`].
    Lv2Program { bank: u32, program: u32 },
}

/// One control input of an LV2 plugin, as a library that names parameters
/// by symbol (amsynth's banks) is mapped onto it.
#[derive(Debug, Clone, PartialEq)]
pub struct Lv2ControlPort {
    pub symbol: String,
    pub index: u32,
    pub default: f32,
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
            let turtle = sorted(self.lv2_presets(info));
            let ports = self.lv2_control_ports(info);
            // Its state, and what is compiled into it, asked of an instance
            // — see `HostedPlugin::programs_with`. Programs only when the
            // Turtle lists none: a DPF plugin offers the same set both ways.
            let (own, programs) = match self.open(&info.path, &info.key) {
                Ok(mut plugin) => match plugin.activate(48_000.0, 256) {
                    Ok(mut processor) => {
                        let programs = match &plugin.inner {
                            Inner::Lv2(lv2) if turtle.is_empty() && lv2.declares_programs() => {
                                plugin.programs_with(&mut processor)
                            }
                            _ => Vec::new(),
                        };
                        let own = plugin.save_state_with(&mut processor);
                        plugin.deactivate(processor);
                        (own, programs)
                    }
                    Err(_) => (None, Vec::new()),
                },
                Err(_) => (None, Vec::new()),
            };
            let mut found = turtle;
            found.extend(list_own_presets(info, roots, own.as_deref(), &ports));
            let mut found = sorted(found);
            found.extend(programs);
            return found;
        }
        // A VST 3 plugin's programs are known only by asking an instance.
        let opened = if info.key.format == PluginFormat::Vst3
            || !library_folders(info, &roots.data).is_empty()
        {
            self.open(&info.path, &info.key).ok()
        } else {
            None
        };
        let (own, programs) = match opened {
            Some(mut plugin) => (plugin.save_state(), plugin.programs()),
            None => (None, Vec::new()),
        };
        let mut found = list_own_presets(info, roots, own.as_deref(), &[]);
        found.extend(programs);
        found
    }

    /// An LV2 plugin's control inputs, by symbol — what a library that names
    /// parameters (amsynth's banks) is mapped onto. Empty for any other
    /// format.
    pub fn lv2_control_ports(&mut self, info: &PluginInfo) -> Vec<Lv2ControlPort> {
        if info.key.format != PluginFormat::Lv2 {
            return Vec::new();
        }
        match self.lv2_world(info) {
            Some(world) => crate::lv2::control_ports(world, &info.key.id),
            None => Vec::new(),
        }
    }

    /// The lilv world of `info`'s bundle, loaded the first time it is asked.
    fn lv2_world(&mut self, info: &PluginInfo) -> Option<&crate::lv2::World> {
        if !self.worlds.contains_key(&info.path) {
            let world = crate::lv2::load_world(&info.path).ok()?;
            self.worlds.insert(info.path.clone(), world);
        }
        self.worlds.get(&info.path)
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
        if self.lv2_world(info).is_none() {
            return Vec::new();
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
        let _inside = self.inside();
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
            OwnPresetSource::Fxp(path) | OwnPresetSource::StateFile(path) => {
                let file = std::fs::read(path).map_err(|e| e.to_string())?;
                // For an LV2 plugin that is not running, the state it was
                // last given — what it will start on.
                let own = self.save_state().unwrap_or_default();
                let blob =
                    state_from_file(self.info.key.format, &own, &file).ok_or_else(refused)?;
                self.load_state(&blob).then_some(()).ok_or_else(refused)
            }
            // Through the parameter, as a knob is: the controller now, and
            // the processor on its next block (or when it starts).
            OwnPresetSource::Program { param, value } => self
                .set_param(*param, *value)
                .then_some(())
                .ok_or_else(refused),
            OwnPresetSource::Lv2Program { .. } => Err(format!(
                "{} has to be running, and paused, to take {}",
                self.info.name, preset.name
            )),
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
        let _inside = self.inside();
        // > *"other daws manage to do this like in fl"*
        //
        // **By way of another program.** A JUCE plugin changes program only
        // when told one other than its current one, and its current one is
        // not in its state: chosen again after a knob was turned — or after
        // an undo put back the patch from before, which Dexed still filed
        // under the program — the program did not come back. So the
        // parameter is first sent somewhere else for a block, then where it
        // is to go. Only with the processor in hand, which is the only way
        // to make the two arrive in separate blocks.
        if let OwnPresetSource::Program { param, value } = &preset.source
            && let Some(other) = self
                .param(*param)
                .map(|held| {
                    if *value > held.min {
                        held.min
                    } else {
                        held.max
                    }
                })
                .filter(|other| other != value)
        {
            self.set_param(*param, other);
            self.run_quiet_block(processor);
            return self.load_own_preset(preset);
        }
        if let OwnPresetSource::Lv2Program { bank, program } = &preset.source {
            return processor
                .lv2_select_program(*bank, *program)
                .then_some(())
                .ok_or_else(|| format!("{} would not load {}", self.info.name, preset.name));
        }
        // A library file goes into a running LV2 instance the way a whole
        // state of its own does (`restore_blob_with`), fitted into the state
        // it is in now.
        if let OwnPresetSource::Fxp(path) | OwnPresetSource::StateFile(path) = &preset.source
            && self.active
            && let Inner::Lv2(plugin) = &mut self.inner
        {
            let refused = || format!("{} would not load {}", self.info.name, preset.name);
            let file = std::fs::read(path).map_err(|e| e.to_string())?;
            let own = processor
                .lv2_save_state()
                .or_else(|| plugin.pending_state().map(<[u8]>::to_vec))
                .unwrap_or_default();
            let bytes = state_from_file(PluginFormat::Lv2, &own, &file).ok_or_else(refused)?;
            if !plugin.stash_state(&bytes) {
                return Err(refused());
            }
            self.values()
                .forget_unsent(|id| id >= crate::lv2::PATCH_PARAM_BASE);
            let taken = processor.lv2_restore_state(&bytes);
            processor.finish_work();
            return taken.then_some(()).ok_or_else(refused);
        }
        let OwnPresetSource::Lv2 { ports, state, .. } = &preset.source else {
            return self.load_own_preset(preset);
        };
        if let Some(state) = state {
            // What the studio had not sent its `patch:` parameters yet is
            // older than this preset: sent after it, it put the patch from
            // before back over it.
            self.values()
                .forget_unsent(|id| id >= crate::lv2::PATCH_PARAM_BASE);
            processor.lv2_restore_preset(state);
        }
        for (index, value) in ports {
            self.set_param(*index, f64::from(*value));
        }
        Ok(())
    }
}

/// A plugin's own library as files and the plugin's discovery, by category
/// and then name: a CLAP's discovery factory, a VST 3's `.vstpreset`s, and
/// in any format the library files that fit `own_state` — the plugin's own
/// state, from an instance somebody already has (`.fxp`, `.vital`, `.vcv`;
/// see `state_from_file`) — and, for an LV2 plugin whose control inputs
/// are `ports`, the amsynth banks that name them. `None` and no ports list
/// no files at all.
///
/// **Never instantiates the plugin**, so it can run on a thread of its own:
/// a JUCE plugin made on one thread while the studio's instance lives on
/// another is asking for trouble. An LV2 plugin's Turtle presets are not
/// listed here — they carry URIDs from the one map the host's instances
/// share ([`PluginHost::own_lv2_presets`]).
pub fn list_own_presets(
    info: &PluginInfo,
    roots: &PresetRoots,
    own_state: Option<&[u8]>,
    ports: &[Lv2ControlPort],
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
        PluginFormat::Lv2 => Vec::new(),
        // Through a bridge, whose parameters are read back a moment after a
        // state goes in: Vital's VST 2 handed a `.vital` reported one value
        // the document then kept wrong. Not offered until that is looked at.
        PluginFormat::Vst2 => return Vec::new(),
    };
    // Library files beside a plugin that lists its own library are often
    // that library again, as files: Surge XT's CLAP describes its three
    // thousand patches through its provider, and the same patches are what
    // is in `/usr/share/surge-xt`. A file whose name the plugin already
    // listed is left out.
    let listed: std::collections::HashSet<String> =
        found.iter().map(|preset| preset.name.clone()).collect();
    let mut files = Vec::new();
    if let Some(own) = own_state {
        files.extend(state_file_presets(info, roots, own));
    }
    if info.key.format == PluginFormat::Lv2 && !ports.is_empty() {
        files.extend(bank_presets(info, roots, ports));
    }
    found.extend(
        files
            .into_iter()
            .filter(|preset| !listed.contains(&preset.name)),
    );
    sorted(found)
}

/// Presets in the order a library is shown in: by category and then name,
/// ignoring case, and one of each name in a category.
pub fn sorted_presets(found: Vec<OwnPreset>) -> Vec<OwnPreset> {
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

// ------------------------------------------------------- library files
//
// > *"i tried a bunch of different instruments including obxf, amsynth, and
// > cardinal synth. all of these have built in presets but our daws preset
// > system did not detect them"*
//
// None of the three says so through its format. Their libraries are files a
// state can be made of, which is only known by looking at the state: so a
// file is offered when the plugin's own state has the shape that file fits
// into, and is fitted into that state when chosen. A file nothing fits is
// never handed to a plugin.

/// What a library file can be, judged by the plugin's own state.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FileKind {
    /// A `.fxp` whose chunk begins the way the state does — JUCE's `VC2!`
    /// for OB-Xf. The state is anything else that is not one of the below.
    Fxp,
    /// A JSON object opening with the state's own first two keys: Vital's
    /// state is the JSON of a `.vital`, `{"author":…,"comments":…`.
    Json,
    /// A VCV Rack patch (`.vcv`), for a state carrying one under `patch`:
    /// Cardinal, DPF's port of Rack.
    RackPatch,
}

/// Where, in a plugin's own state, the part a library file replaces is.
#[derive(Debug, Clone, PartialEq)]
enum Carrier {
    /// All of it: CLAP, VST 2.
    Whole,
    /// A VST 3's component half. The controller's is left out, as a
    /// `.vstpreset` with no controller chunk leaves it: the plugin builds it
    /// from the component.
    Vst3,
    /// DPF's own CLAP state — `key\0value\0` pairs between
    /// `__dpf_state_begin__` and `__dpf_state_end__` — under `patch`, in
    /// base64 (Cardinal).
    DpfClap,
    /// One property of an LV2 state, held as `encoding` says.
    Lv2 { key: String, encoding: Lv2Encoding },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Lv2Encoding {
    /// The bytes themselves, an `atom:Chunk` (Vitalium's `urn:juce:stateBinary`).
    Chunk,
    /// JUCE's base64 as an `atom:String` (OB-Xf's `StateString`).
    JuceBase64,
    /// Standard base64 as an `atom:String` (DPF's `urn:distrho:patch`).
    Base64,
}

const DPF_STATE_BEGIN: &[u8] = b"__dpf_state_begin__";
const DPF_STATE_END: &[u8] = b"__dpf_state_end__";
const DPF_PATCH_KEY: &[u8] = b"patch";
const LV2_DPF_PATCH: &str = "urn:distrho:patch";
const ATOM_CHUNK: &str = "http://lv2plug.in/ns/ext/atom#Chunk";
const ATOM_STRING: &str = "http://lv2plug.in/ns/ext/atom#String";
const ZSTD_MAGIC: &[u8] = b"\x28\xb5\x2f\xfd";

/// What kind of file `inner` — the part of a state a file would replace —
/// takes. `strict` (inside an LV2 property, which holds anything) asks for
/// the evidence of a JUCE state for a `.fxp`, rather than taking whatever
/// is not JSON.
fn kind_of(inner: &[u8], strict: bool) -> Option<FileKind> {
    if json_first_key(inner).is_some() {
        Some(FileKind::Json)
    } else if !strict || inner.starts_with(b"VC2!") {
        Some(FileKind::Fxp)
    } else {
        None
    }
}

/// `{"key":` — the opening of a JSON object up to its first key's colon.
fn json_first_key(bytes: &[u8]) -> Option<&[u8]> {
    let rest = bytes.strip_prefix(b"{\"")?;
    let close = rest.iter().take(64).position(|&b| b == b'"')?;
    (rest.get(close + 1) == Some(&b':')).then(|| &bytes[..close + 4])
}

/// The first two keys of a JSON object whose first value is a string —
/// what tells a Vital preset (`author`, `comments`) from the wavetables
/// beside it by the same author (`author`, `full_normalize`), read off the
/// head of a file of megabytes. `None` for anything else.
fn json_opening_keys(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let first = json_first_key(bytes)?;
    let key = &first[2..first.len() - 2];
    // The first value, a string: to its closing quote, past escapes.
    let rest = bytes.get(first.len()..)?.strip_prefix(b"\"")?;
    let mut at = 0;
    loop {
        match *rest.get(at)? {
            b'\\' => at += 2,
            b'"' => break,
            _ => at += 1,
        }
    }
    let after = rest.get(at + 1..)?.strip_prefix(b",\"")?;
    let close = after.iter().take(64).position(|&b| b == b'"')?;
    (after.get(close + 1) == Some(&b':')).then(|| (key, &after[..close]))
}

/// The fields of DPF's CLAP state, split at its NULs.
fn dpf_fields(own: &[u8]) -> Option<Vec<&[u8]>> {
    if !own.starts_with(DPF_STATE_BEGIN) || own.get(DPF_STATE_BEGIN.len()) != Some(&0) {
        return None;
    }
    Some(own.split(|&b| b == 0).collect())
}

/// Where the value of DPF's `key` is among `fields`: keys and values in
/// pairs after the opening marker, up to the closing one.
fn dpf_value_at(fields: &[&[u8]], key: &[u8]) -> Option<usize> {
    let mut at = 1;
    while at + 1 < fields.len() && fields[at] != DPF_STATE_END {
        if fields[at] == key {
            return Some(at + 1);
        }
        at += 2;
    }
    None
}

/// The part of `own` (the plugin's state as this host keeps it for
/// `format`) a library file replaces, where it is held, and the kind of file
/// it takes. `None`: no file fits this state.
fn carried(format: PluginFormat, own: &[u8]) -> Option<(Carrier, Vec<u8>, FileKind)> {
    match format {
        PluginFormat::Lv2 => {
            let state = crate::Lv2State::decode(own)?;
            if let Some(property) = state.properties.iter().find(|p| p.key == LV2_DPF_PATCH) {
                let carrier = Carrier::Lv2 {
                    key: property.key.clone(),
                    encoding: Lv2Encoding::Base64,
                };
                return Some((carrier, Vec::new(), FileKind::RackPatch));
            }
            state.properties.iter().find_map(|property| {
                let (encoding, inner) = match property.type_uri.as_str() {
                    ATOM_CHUNK => (Lv2Encoding::Chunk, property.value.clone()),
                    ATOM_STRING => {
                        let text = property.value.strip_suffix(&[0]).unwrap_or(&property.value);
                        (Lv2Encoding::JuceBase64, juce_base64_decode(text)?)
                    }
                    _ => return None,
                };
                let kind = kind_of(&inner, true)?;
                let carrier = Carrier::Lv2 {
                    key: property.key.clone(),
                    encoding,
                };
                Some((carrier, inner, kind))
            })
        }
        PluginFormat::Clap if own.starts_with(DPF_STATE_BEGIN) => {
            dpf_value_at(&dpf_fields(own)?, DPF_PATCH_KEY)?;
            Some((Carrier::DpfClap, Vec::new(), FileKind::RackPatch))
        }
        PluginFormat::Vst3 => {
            let component = crate::vst3::split_state(own)
                .map(|(component, _)| component)
                .unwrap_or_default();
            // JUCE's VST 3 keeps its state as a VST 2 bank, for a host that
            // puts it in place of the plugin's VST 2; the plugin's own state
            // is that bank's chunk, and it takes the chunk alone back.
            let component = juce_vst2_bank_chunk(component)
                .unwrap_or(component)
                .to_vec();
            let kind = kind_of(&component, false)?;
            Some((Carrier::Vst3, component, kind))
        }
        PluginFormat::Clap | PluginFormat::Vst2 => {
            Some((Carrier::Whole, own.to_vec(), kind_of(own, false)?))
        }
    }
}

/// The chunk of the VST 2 bank JUCE's VST 3 wraps its state in
/// (`VstW`, then an `FBCh` bank). `None` for any other component.
fn juce_vst2_bank_chunk(component: &[u8]) -> Option<&[u8]> {
    let rest = component.strip_prefix(b"VstW")?;
    let header = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?) as usize;
    let bank = rest.get(4 + header..)?;
    if bank.get(..4)? != b"CcnK" || bank.get(8..12)? != b"FBCh" {
        return None;
    }
    let size = u32::from_be_bytes(bank.get(156..160)?.try_into().ok()?) as usize;
    bank.get(160..160usize.checked_add(size)?)
}

/// `inner` put back where `carrier` says, in `own`.
fn put_back(own: &[u8], carrier: &Carrier, inner: Vec<u8>) -> Option<Vec<u8>> {
    match carrier {
        Carrier::Whole => Some(inner),
        Carrier::Vst3 => Some(crate::vst3::join_state(&inner, &[])),
        Carrier::DpfClap => {
            let mut fields = dpf_fields(own)?;
            let at = dpf_value_at(&fields, DPF_PATCH_KEY)?;
            let encoded = fontelle_types::encode_base64(&inner);
            fields[at] = encoded.as_bytes();
            Some(fields.join(&0))
        }
        Carrier::Lv2 { key, encoding } => {
            let mut state = crate::Lv2State::decode(own)?;
            let property = state.properties.iter_mut().find(|p| p.key == *key)?;
            property.value = match encoding {
                Lv2Encoding::Chunk => inner,
                Lv2Encoding::JuceBase64 => {
                    let mut text = juce_base64_encode(&inner);
                    text.push(0);
                    text
                }
                Lv2Encoding::Base64 => {
                    let mut text = fontelle_types::encode_base64(&inner).into_bytes();
                    text.push(0);
                    text
                }
            };
            Some(state.encode())
        }
    }
}

/// A file of a plugin's library made into the state the plugin keeps, given
/// `own` — its state now, as this host holds it for `format`. `None` when
/// the file is not something that state could be made of.
fn state_from_file(format: PluginFormat, own: &[u8], file: &[u8]) -> Option<Vec<u8>> {
    let (carrier, inner, kind) = carried(format, own)?;
    let made = match kind {
        FileKind::Fxp => {
            let chunk = fxp_chunk(file)?;
            if inner.len() >= 4 && (chunk.len() < 4 || chunk[..4] != inner[..4]) {
                return None;
            }
            // A program chunk, fitted into the state the plugin keeps when
            // that is its shape — see `program_into_state`.
            program_into_state(chunk, &inner).unwrap_or_else(|| chunk.to_vec())
        }
        // Parsed whole before a plugin is handed it: Vital's own reader is
        // not one to trust with half a file.
        FileKind::Json => serde_json::from_slice::<serde_json::Value>(file)
            .ok()
            .filter(serde_json::Value::is_object)
            .map(|_| file.to_vec())?,
        FileKind::RackPatch => is_rack_patch(file).then(|| file.to_vec())?,
    };
    put_back(own, &carrier, made)
}

/// A VCV Rack patch: Rack 1's JSON, or Rack 2's zstd archive.
fn is_rack_patch(bytes: &[u8]) -> bool {
    bytes.starts_with(b"{") || bytes.starts_with(ZSTD_MAGIC)
}

/// The library files in `info`'s folders that fit `own` — see
/// [`state_from_file`].
fn state_file_presets(info: &PluginInfo, roots: &PresetRoots, own: &[u8]) -> Vec<OwnPreset> {
    let Some((_, inner, kind)) = carried(info.key.format, own) else {
        return Vec::new();
    };
    match kind {
        FileKind::Fxp => fxp_presets(info, roots, &inner),
        FileKind::Json => {
            let Some(opening) = json_opening_keys(&inner) else {
                return Vec::new();
            };
            files_in(&library_folders(info, &roots.data), |path| {
                let mut head = Vec::new();
                let read = std::fs::File::open(path).and_then(|file| {
                    std::io::Read::read_to_end(&mut std::io::Read::take(file, 64 << 10), &mut head)
                });
                read.is_ok() && json_opening_keys(&head) == Some(opening)
            })
        }
        FileKind::RackPatch => {
            // Cardinal Synth's patches are Cardinal's: a folder named for
            // the family. Only here — a patch fits no plugin but one that
            // carries a patch, where a `.fxp` of "OneTrick KEYS" would fit
            // OneTrick CHONK's state as well as its own.
            let mut folders = library_folders(info, &roots.data);
            if let Some(family) = info.name.split_whitespace().next()
                && family != info.name
            {
                let family = PluginInfo {
                    name: family.to_string(),
                    ..info.clone()
                };
                for folder in library_folders(&family, &roots.data) {
                    if !folders.contains(&folder) {
                        folders.push(folder);
                    }
                }
            }
            // A data folder that keeps its patches apart from everything
            // else (`/usr/share/cardinal`, seven thousand files of modules'
            // resources) is looked in there alone.
            let folders: Vec<PathBuf> = folders
                .into_iter()
                .map(|folder| {
                    let patches = folder.join("patches");
                    if patches.is_dir() { patches } else { folder }
                })
                .collect();
            files_in(&folders, |path| {
                if !has_extension(path, "vcv") {
                    return false;
                }
                let mut head = [0u8; 4];
                std::fs::File::open(path)
                    .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut head))
                    .is_ok_and(|()| is_rack_patch(&head))
            })
        }
    }
}

/// Every file under `folders` that `wanted` takes, as a preset loaded from
/// that file.
fn files_in(folders: &[PathBuf], wanted: impl Fn(&Path) -> bool) -> Vec<OwnPreset> {
    let mut found = Vec::new();
    for folder in folders {
        walk(folder, 0, &mut |path| {
            if wanted(path) {
                found.push(OwnPreset {
                    name: stem(path),
                    category: category_of(folder, path),
                    source: OwnPresetSource::StateFile(path.to_path_buf()),
                });
            }
        });
    }
    found
}

/// The presets of the amsynth banks in `info`'s folders, as the values of
/// the ports they name — every port, the ones a preset leaves out at their
/// defaults (what amsynth does with a preset older than a parameter). A
/// preset naming a parameter with no port of that symbol is some other
/// plugin's, and is left out.
fn bank_presets(
    info: &PluginInfo,
    roots: &PresetRoots,
    ports: &[Lv2ControlPort],
) -> Vec<OwnPreset> {
    let mut found = Vec::new();
    for folder in library_folders(info, &roots.data) {
        walk(&folder, 0, &mut |path| {
            // A bank is small; anything big is not one.
            if std::fs::metadata(path).map_or(true, |m| m.len() > 4 << 20) {
                return;
            }
            let Ok(text) = std::fs::read_to_string(path) else {
                return;
            };
            let Some(bank) = amsynth_bank(&text) else {
                return;
            };
            let name = stem(path);
            let category = name.strip_suffix(".amSynth").unwrap_or(&name).to_string();
            // amsynth's banks repeat a name now and then, and each is a
            // preset of its own: the second of a name is "Name (2)".
            let mut seen: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for (preset, values) in bank {
                let count = seen.entry(preset.clone()).or_default();
                *count += 1;
                let preset = match *count {
                    1 => preset,
                    n => format!("{preset} ({n})"),
                };
                let mut set: Vec<(u32, f32)> = ports
                    .iter()
                    .map(|port| (port.index, port.default))
                    .collect();
                let fits = values.iter().all(|(symbol, value)| {
                    ports
                        .iter()
                        .position(|port| port.symbol == *symbol)
                        .map(|at| set[at].1 = *value)
                        .is_some()
                });
                if !fits || values.is_empty() {
                    continue;
                }
                found.push(OwnPreset {
                    name: preset,
                    category: category.clone(),
                    source: OwnPresetSource::Lv2 {
                        uri: format!("{}#{}", path.display(), found.len()),
                        ports: set,
                        state: None,
                    },
                });
            }
        });
    }
    found
}

/// JUCE's base64 alphabet: `.` first, `+` last, and no padding.
const JUCE_BASE64: &[u8; 64] = b".ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+";

/// JUCE's `MemoryBlock::fromBase64Encoding`: the length, a dot, and six
/// bits a character, least significant first, in JUCE's own alphabet.
fn juce_base64_decode(text: &[u8]) -> Option<Vec<u8>> {
    let dot = text.iter().take(12).position(|&b| b == b'.')?;
    let size: usize = std::str::from_utf8(&text[..dot]).ok()?.parse().ok()?;
    let digits = &text[dot + 1..];
    if digits.len() * 6 < size * 8 {
        return None;
    }
    let mut out = vec![0u8; size];
    for (index, &digit) in digits.iter().enumerate() {
        let value = JUCE_BASE64.iter().position(|&c| c == digit)? as u32;
        for bit in 0..6 {
            let at = index * 6 + bit;
            if at / 8 >= size {
                break;
            }
            if value & (1 << bit) != 0 {
                out[at / 8] |= 1 << (at % 8);
            }
        }
    }
    Some(out)
}

/// JUCE's `MemoryBlock::toBase64Encoding` — see [`juce_base64_decode`].
fn juce_base64_encode(bytes: &[u8]) -> Vec<u8> {
    let mut out = format!("{}.", bytes.len()).into_bytes();
    let chars = (bytes.len() * 8).div_ceil(6);
    for index in 0..chars {
        let mut value = 0usize;
        for bit in 0..6 {
            let at = index * 6 + bit;
            if at / 8 < bytes.len() && bytes[at / 8] & (1 << (at % 8)) != 0 {
                value |= 1 << bit;
            }
        }
        out.push(JUCE_BASE64[value]);
    }
    out
}

/// One preset of an amsynth bank: its name, and each parameter it sets by
/// the name amsynth gives it (which its LV2 uses as the port's symbol).
type BankPreset = (String, Vec<(String, f32)>);

/// The presets of an amsynth bank file — `amSynth` on the first line, then
/// `<preset> <name> …` and `<parameter> <name> <value>` lines. `None` for a
/// file that is not one.
fn amsynth_bank(text: &str) -> Option<Vec<BankPreset>> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "amSynth" {
        return None;
    }
    let mut presets: Vec<BankPreset> = Vec::new();
    for line in lines {
        if let Some(name) = line.strip_prefix("<preset> <name> ") {
            presets.push((name.trim().to_string(), Vec::new()));
        } else if let Some(rest) = line.strip_prefix("<parameter> ") {
            let mut words = rest.split_whitespace();
            let (Some(symbol), Some(value), Some(preset)) =
                (words.next(), words.next(), presets.last_mut())
            else {
                continue;
            };
            if let Ok(value) = value.parse::<f32>() {
                preset.1.push((symbol.to_string(), value));
            }
        }
    }
    Some(presets)
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
/// `Leads / Mono` a level deeper. A folder a library is kept *in* —
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
    // Anywhere along the way: Vital keeps each pack as `<pack>/Presets/…`.
    parts
        .into_iter()
        .filter(|part| {
            !matches!(
                part.to_lowercase().as_str(),
                "patches" | "presets" | "programs"
            )
        })
        .collect::<Vec<_>>()
        .join(" / ")
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

    // ----------------------------------------------------- library files
    //
    // > *"i tried a bunch of different instruments including obxf, amsynth,
    // > and cardinal synth. all of these have built in presets but our daws
    // > preset system did not detect them"*

    /// OB-Xf's LV2 keeps the same bytes as its CLAP state, in JUCE's own
    /// base64 under `StateString`: `2308.VMjLgrOB…` on this machine, where
    /// the CLAP state of 2308 bytes begins `VC2!` and `fb 08`.
    #[test]
    fn juces_base64_is_read_the_way_juce_writes_it() {
        assert_eq!(
            juce_base64_decode(b"6.VMjLgrOB").as_deref(),
            Some(&b"VC2!\xfb\x08"[..])
        );
        let bytes: Vec<u8> = (0..=255u8).chain([0, 1, 2]).collect();
        let text = juce_base64_encode(&bytes);
        assert!(
            text.starts_with(b"259."),
            "{:?}",
            String::from_utf8_lossy(&text)
        );
        assert_eq!(juce_base64_decode(&text), Some(bytes));
        assert_eq!(juce_base64_decode(b"not juce"), None);
        assert_eq!(juce_base64_decode(b"9.VMjL"), None, "shorter than it says");
    }

    const VITAL_STATE: &[u8] = br#"{"author":"","settings":{"volume":0.5},"tuning":{}}"#;
    const VITAL_FILE: &[u8] = br#"{"author":"Mr. Bill","settings":{"volume":0.25}}"#;

    /// Vital's state is its preset's JSON, and its `.vital` files are that
    /// JSON: one is the state, as CLAP and VST 3 (and VST 2) keep it.
    #[test]
    fn a_json_preset_is_the_state_of_a_plugin_whose_state_is_that_json() {
        assert_eq!(
            state_from_file(PluginFormat::Clap, VITAL_STATE, VITAL_FILE).as_deref(),
            Some(VITAL_FILE)
        );
        let own = crate::vst3::join_state(VITAL_STATE, b"controller");
        assert_eq!(
            state_from_file(PluginFormat::Vst3, &own, VITAL_FILE),
            Some(crate::vst3::join_state(VITAL_FILE, &[]))
        );
        // JUCE's VST 3 keeps its state in a VST 2 bank, for hosts that
        // replace a plugin's VST 2 with it (Vital's): the JSON is that bank's
        // chunk. Vital takes the JSON alone as its component, as it was.
        let mut bank = b"VstW".to_vec();
        bank.extend_from_slice(&8u32.to_be_bytes());
        bank.extend_from_slice(&1u32.to_be_bytes());
        bank.extend_from_slice(&0u32.to_be_bytes());
        let mut fxb = b"CcnK".to_vec();
        fxb.extend_from_slice(&0u32.to_be_bytes());
        fxb.extend_from_slice(b"FBCh");
        fxb.extend_from_slice(&2u32.to_be_bytes());
        fxb.extend_from_slice(b"Vita");
        fxb.extend_from_slice(&67076u32.to_be_bytes());
        fxb.extend_from_slice(&0u32.to_be_bytes());
        fxb.extend_from_slice(&[0; 128]);
        fxb.extend_from_slice(&(VITAL_STATE.len() as u32).to_be_bytes());
        fxb.extend_from_slice(VITAL_STATE);
        let size = (fxb.len() - 8) as u32;
        fxb[4..8].copy_from_slice(&size.to_be_bytes());
        bank.extend_from_slice(&fxb);
        bank.extend_from_slice(b"\0\0\0\0JUCEPrivateData");
        let own = crate::vst3::join_state(&bank, &[]);
        assert_eq!(
            state_from_file(PluginFormat::Vst3, &own, VITAL_FILE),
            Some(crate::vst3::join_state(VITAL_FILE, &[]))
        );
        // A file that does not parse is not handed over.
        assert_eq!(
            state_from_file(PluginFormat::Clap, VITAL_STATE, br#"{"author":"#),
            None
        );
    }

    fn lv2_state(properties: &[(&str, &str, &[u8])]) -> Vec<u8> {
        crate::Lv2State {
            properties: properties
                .iter()
                .map(|(key, type_uri, value)| crate::Lv2Property {
                    key: key.to_string(),
                    type_uri: type_uri.to_string(),
                    flags: 1,
                    value: value.to_vec(),
                })
                .collect(),
        }
        .encode()
    }

    fn lv2_value(state: &[u8], key: &str) -> Vec<u8> {
        crate::Lv2State::decode(state)
            .expect("an LV2 state")
            .properties
            .into_iter()
            .find(|property| property.key == key)
            .expect("the property is kept")
            .value
    }

    const CHUNK: &str = "http://lv2plug.in/ns/ext/atom#Chunk";
    const STRING: &str = "http://lv2plug.in/ns/ext/atom#String";

    /// Vitalium (DISTRHO's port of Vital) keeps the JSON as a chunk under
    /// `urn:juce:stateBinary`; the rest of what it stored stays.
    #[test]
    fn a_json_preset_goes_into_the_lv2_property_that_holds_the_json() {
        let own = lv2_state(&[
            ("urn:juce:stateBinary", CHUNK, VITAL_STATE),
            ("urn:other", STRING, b"kept\0"),
        ]);
        let made = state_from_file(PluginFormat::Lv2, &own, VITAL_FILE).expect("fitted");
        assert_eq!(lv2_value(&made, "urn:juce:stateBinary"), VITAL_FILE);
        assert_eq!(lv2_value(&made, "urn:other"), b"kept\0");
    }

    /// OB-Xf's LV2: the `.fxp`'s program goes into the state the CLAP would
    /// have had, and back into `StateString` in JUCE's base64.
    #[test]
    fn an_fxp_goes_into_a_juce_lv2_plugins_state_string() {
        let mut text = juce_base64_encode(&juce(STATE));
        text.push(0);
        let own = lv2_state(&[("urn:org.surge-synth-team.OB-Xf:StateString", STRING, &text)]);
        let program = juce(
            r#"<?xml version="1.0" encoding="UTF-8"?> <OB-Xf ob-xf_version="2025-12-13" Volume="0.2" programName="Pulled PWM" category="Keys"/>"#,
        );
        let mut fxp = b"CcnK".to_vec();
        fxp.extend_from_slice(&0u32.to_be_bytes());
        fxp.extend_from_slice(b"FPCh");
        fxp.resize(56, 0);
        fxp.extend_from_slice(&(program.len() as u32).to_be_bytes());
        fxp.extend_from_slice(&program);
        let made = state_from_file(PluginFormat::Lv2, &own, &fxp).expect("fitted");
        let value = lv2_value(&made, "urn:org.surge-synth-team.OB-Xf:StateString");
        let value = value.strip_suffix(&[0]).expect("still a string");
        assert_eq!(
            juce_base64_decode(value),
            program_into_state(&program, &juce(STATE))
        );
    }

    /// Cardinal (DPF) keeps its Rack patch base64'd under a `patch` key: in
    /// DPF's own CLAP state, and as `urn:distrho:patch` in LV2. A `.vcv` —
    /// JSON, or Rack 2's zstd archive — goes there, and nothing else moves.
    #[test]
    fn a_rack_patch_goes_where_cardinal_keeps_its_patch() {
        let clap_own = b"__dpf_state_begin__\0comment\0\0patch\0KLUv\0windowSize\0\0__dpf_state_end__\0__dpf_parameters_begin__\0param_1\x000\0__dpf_parameters_end__\0\xfe\0";
        let vcv = br#"{"version":"2.0","modules":[]}"#;
        let made = state_from_file(PluginFormat::Clap, clap_own, vcv).expect("fitted");
        let mut wanted = b"__dpf_state_begin__\0comment\0\0patch\0".to_vec();
        wanted.extend_from_slice(b"eyJ2ZXJzaW9uIjoiMi4wIiwibW9kdWxlcyI6W119");
        wanted.extend_from_slice(b"\0windowSize\0\0__dpf_state_end__\0__dpf_parameters_begin__\0param_1\x000\0__dpf_parameters_end__\0\xfe\0");
        assert_eq!(made, wanted);

        let own = lv2_state(&[
            ("urn:distrho:patch", STRING, b"KLUv\0"),
            ("urn:distrho:windowSize", STRING, b"\0"),
        ]);
        let zstd = b"\x28\xb5\x2f\xfdrest";
        let made = state_from_file(PluginFormat::Lv2, &own, zstd).expect("fitted");
        assert_eq!(lv2_value(&made, "urn:distrho:patch"), b"KLUv/XJlc3Q=\0");

        // Something that is neither is not a patch.
        assert_eq!(
            state_from_file(PluginFormat::Clap, clap_own, b"MThd...."),
            None
        );
    }

    /// A state of no shape a library file fits is left alone.
    #[test]
    fn a_state_nothing_fits_takes_no_file() {
        let own = lv2_state(&[(
            "urn:runs",
            "http://lv2plug.in/ns/ext/atom#Int",
            &[1, 0, 0, 0],
        )]);
        assert_eq!(state_from_file(PluginFormat::Lv2, &own, VITAL_FILE), None);
        assert_eq!(
            state_from_file(PluginFormat::Clap, &[1, 2, 3, 4], VITAL_FILE),
            None
        );
    }

    #[test]
    fn an_amsynth_bank_is_its_presets_and_their_parameters() {
        let bank = "amSynth\n<preset> <name> Derren 1\n<parameter> amp_attack 0.15\n<parameter> filter_cutoff -0.358078\n<preset> <name> Two Words\n<parameter> amp_attack 1\n";
        assert_eq!(
            amsynth_bank(bank),
            Some(vec![
                (
                    "Derren 1".to_string(),
                    vec![
                        ("amp_attack".to_string(), 0.15),
                        ("filter_cutoff".to_string(), -0.358078)
                    ]
                ),
                (
                    "Two Words".to_string(),
                    vec![("amp_attack".to_string(), 1.0)]
                ),
            ])
        );
        assert_eq!(amsynth_bank("something else\n<preset> <name> X\n"), None);
    }
}
