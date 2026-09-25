//! Formats that live outside this tree, through a bridge.
//!
//! > *"if vst3 and vst2 are legally murky we could always try and add it in
//! > a way that keeps it completely separate to the open source stuff and
//! > never gets included with it ... that way i can locally use vsts lv2s or
//! > clap plugins"*
//!
//! §3.4's problem is that the VST3 SDK is GPLv3-or-proprietary and the VST2
//! SDK is withdrawn, so neither can be linked from an MIT/Apache tree. The
//! answer here is the one §8.4 anticipated — *"VST3 through a bridge"* —
//! made concrete: a **bridge** is a shared library implementing
//! [`fontelle_bridge_abi`], found in Fontelle's own data folder at run time
//! ([`bridge_search_paths`]), and it hosts one format on this program's
//! behalf. Fontelle never links a bridge and a bridge never links Fontelle;
//! what the bridge links — the SDK, `yabridge`'s output, anything — is built
//! from a repository of its own under its own licence, and this tree holds
//! nothing but the contract and this loader.
//!
//! A bridged plugin rides the same [`HostedPlugin`] and [`HostedProcessor`]
//! CLAP and LV2 do, as a third arm of the same enums, so nothing above the
//! host can tell which of the three it has. The one honest difference is
//! visible in [`PluginFormat::hosted`]: it says what *this build* loads,
//! and a bridged format is instead answered by [`Bridges::serves`] and
//! [`crate::PluginHost::can_host`], because it depends on what is installed
//! on the machine rather than on what was compiled.
//!
//! `fontelle-testbridge` is a bridge with no SDK in it, and is what the tests
//! load: what they prove is the loader and the seam, not any SDK.
//!
//! [`HostedPlugin`]: crate::HostedPlugin
//! [`HostedProcessor`]: crate::HostedProcessor

use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontelle_bridge_abi::{ABI_VERSION, EntryFn, FontelleBridge, Instance, ParamInfo};
use fontelle_types::{PluginFormat, PluginKey};

use crate::param::{HostedParam, ParamValues};
use crate::plugin::HostError;
use crate::scan::PluginInfo;

/// A bridge library that would not load. Kept the way scan failures are:
/// the bridge somebody just installed is the one they most need told about.
#[derive(Debug, Clone)]
pub struct BridgeFailure {
    pub path: PathBuf,
    pub why: String,
}

/// One loaded bridge.
struct Loaded {
    /// The table, valid for as long as `_library` is.
    table: *const FontelleBridge,
    format: PluginFormat,
    name: String,
    path: PathBuf,
    /// Declared last so it is dropped last: the table points into it.
    _library: libloading::Library,
}

// SAFETY: the table is `Sync` by the ABI's own promise (function pointers
// and `'static` strings), and the library handle is `Send + Sync` already.
unsafe impl Send for Loaded {}
unsafe impl Sync for Loaded {}

impl Loaded {
    fn table(&self) -> &FontelleBridge {
        // SAFETY: `table` came from the library's entry point and the
        // library is alive for as long as `self` is.
        unsafe { &*self.table }
    }
}

/// Every bridge this process has loaded.
///
/// Shared by the host, every plugin it opens and every processor those hand
/// out, so a bridge's library outlives anything still calling into it.
#[derive(Default)]
pub struct Bridges {
    loaded: Vec<Loaded>,
    pub failures: Vec<BridgeFailure>,
}

impl Bridges {
    /// No bridges at all — what a build has until somebody installs one.
    pub fn none() -> Self {
        Self::default()
    }

    /// Loads every library in `folders`. A folder that is not there is not
    /// a failure; a library that is there and is not a bridge is.
    ///
    /// **This runs somebody else's code**, exactly as opening a plugin
    /// bundle does, and for the same reason there is no safe version of it.
    pub fn load(folders: &[PathBuf]) -> Self {
        let mut bridges = Self::none();
        for folder in folders {
            let Ok(entries) = std::fs::read_dir(folder) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file() && is_library(path))
                .collect();
            paths.sort();
            for path in paths {
                match load_one(&path) {
                    Ok(loaded) => {
                        // One bridge per format: the first found wins, and
                        // a second saying the same thing is reported rather
                        // than silently shadowed.
                        if bridges.loaded.iter().any(|b| b.format == loaded.format) {
                            bridges.failures.push(BridgeFailure {
                                path,
                                why: format!(
                                    "another bridge already serves {}",
                                    loaded.format.label()
                                ),
                            });
                        } else {
                            bridges.loaded.push(loaded);
                        }
                    }
                    Err(why) => bridges.failures.push(BridgeFailure { path, why }),
                }
            }
        }
        bridges
    }

    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }

    /// The formats a bridge serves, in the order they were found.
    pub fn formats(&self) -> Vec<PluginFormat> {
        self.loaded.iter().map(|b| b.format).collect()
    }

    /// What each bridge calls itself, for a settings page.
    pub fn names(&self) -> Vec<String> {
        self.loaded.iter().map(|b| b.name.clone()).collect()
    }

    /// Where each bridge was loaded from.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.loaded.iter().map(|b| b.path.clone()).collect()
    }

    pub fn serves(&self, format: PluginFormat) -> bool {
        self.loaded.iter().any(|b| b.format == format)
    }

    fn bridge(&self, format: PluginFormat) -> Option<&Loaded> {
        self.loaded.iter().find(|b| b.format == format)
    }

    /// The folders every bridge says its format is installed in.
    pub(crate) fn search_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for bridge in &self.loaded {
            let mut out: [*const c_char; 16] = [std::ptr::null(); 16];
            // SAFETY: the table promises to write at most `capacity`
            // pointers to strings that live as long as the bridge.
            let count = unsafe { (bridge.table().search_paths)(out.as_mut_ptr(), 16) };
            for pointer in out.iter().take(count.min(16) as usize) {
                if pointer.is_null() {
                    continue;
                }
                let text = unsafe { CStr::from_ptr(*pointer) }.to_string_lossy();
                let path = PathBuf::from(text.as_ref());
                if path.is_absolute() {
                    paths.push(path);
                }
            }
        }
        paths
    }

    /// Everything one bundle of a bridged format holds.
    pub(crate) fn scan_bundle(
        &self,
        format: PluginFormat,
        path: &Path,
    ) -> Result<Vec<PluginInfo>, String> {
        let bridge = self
            .bridge(format)
            .ok_or_else(|| format!("no bridge for {}", format.label()))?;
        let table = bridge.table();
        let c_path = c_string(path)?;
        let mut infos: *mut fontelle_bridge_abi::PluginInfo = std::ptr::null_mut();
        let mut count = 0u32;
        // SAFETY: the table's contract; `infos` and `count` are ours to be
        // written, and what comes back is freed by the same bridge below.
        let result = unsafe { (table.scan_bundle)(c_path.as_ptr(), &mut infos, &mut count) };
        if result < 0 {
            return Err(format!("{} would not read it", bridge.name));
        }
        let mut found = Vec::new();
        if !infos.is_null() {
            for index in 0..count as usize {
                let info = unsafe { &*infos.add(index) };
                let id = text(info.id);
                if id.is_empty() {
                    continue;
                }
                found.push(PluginInfo {
                    key: PluginKey::new(format, id),
                    path: path.to_path_buf(),
                    name: text(info.name),
                    vendor: text(info.vendor),
                    version: text(info.version),
                    features: vec![if info.is_instrument != 0 {
                        "instrument".to_string()
                    } else {
                        "audio-effect".to_string()
                    }],
                });
            }
            unsafe { (table.free_infos)(infos, count) };
        }
        Ok(found)
    }
}

fn is_library(path: &Path) -> bool {
    let expected = if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    path.extension().is_some_and(|ext| ext == expected)
}

fn load_one(path: &Path) -> Result<Loaded, String> {
    // SAFETY: loading a library runs its initialisers; there is no safe
    // version of this and every host takes the same step. The folder it
    // came from is Fontelle's own, which is the whole of what can be checked.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
    let entry: libloading::Symbol<EntryFn> =
        unsafe { library.get(fontelle_bridge_abi::ENTRY_SYMBOL.as_bytes()) }
            .map_err(|_| "not a Fontelle bridge (no entry point)".to_string())?;
    let table = unsafe { entry() };
    if table.is_null() {
        return Err("the bridge's entry point returned nothing".to_string());
    }
    let bridge = unsafe { &*table };
    if bridge.abi_version != ABI_VERSION {
        return Err(format!(
            "built for bridge ABI {} and this is {ABI_VERSION}",
            bridge.abi_version
        ));
    }
    let tag = text(bridge.format);
    let format = PluginKey::parse(&format!("{tag}:x"))
        .map(|key| key.format)
        .ok_or_else(|| format!("serves a format this build cannot name: {tag:?}"))?;
    if format.hosted() {
        return Err(format!(
            "{} is hosted natively; a bridge for it is refused",
            format.label()
        ));
    }
    let name = text(bridge.name);
    Ok(Loaded {
        table,
        format,
        name,
        path: path.to_path_buf(),
        _library: library,
    })
}

fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    // SAFETY: the ABI promises NUL-terminated strings for the life of
    // whatever handed them out.
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn c_string(path: &Path) -> Result<CString, String> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| "a path with a NUL in it".to_string())
}

/// Where Fontelle looks for bridges: its own data folder, and
/// `FONTELLE_BRIDGES` for anywhere else.
///
/// Under the data directory beside the soundfont bank rather than in a
/// plugin folder, because a bridge is something installed *for this
/// program*, and INVARIANT 10 puts what this program owns in one place.
pub fn bridge_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(from_env) = std::env::var_os("FONTELLE_BRIDGES") {
        paths.extend(std::env::split_paths(&from_env).filter(|path| path.is_absolute()));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data = if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        Some(PathBuf::from(xdg))
    } else if cfg!(target_os = "macos") {
        home.as_ref().map(|h| h.join("Library/Application Support"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        home.as_ref().map(|h| h.join(".local/share"))
    };
    if let Some(data) = data {
        paths.push(data.join("fontelle").join("bridges"));
    }
    paths.retain(|path| path.is_absolute());
    paths.dedup();
    paths
}

/// What both halves of a bridged plugin hold: the instance and the table.
///
/// Closed when the last half is dropped, whichever order that happens in —
/// a processor still parked in a graph keeps the instance open.
pub(crate) struct BridgedShared {
    bridges: Arc<Bridges>,
    format: PluginFormat,
    instance: Instance,
}

// SAFETY: the instance is used from the main thread through `BridgedPlugin`
// and the audio thread through `BridgedProcessor`, which is the ABI's
// contract and the bridge's job to synchronise — the same shape CLAP draws.
unsafe impl Send for BridgedShared {}
unsafe impl Sync for BridgedShared {}

impl BridgedShared {
    fn table(&self) -> &FontelleBridge {
        self.bridges
            .bridge(self.format)
            .expect("a bridged plugin was opened by a bridge that is still loaded")
            .table()
    }
}

impl Drop for BridgedShared {
    fn drop(&mut self) {
        unsafe { (self.table().close)(self.instance) };
    }
}

/// The main-thread half.
pub(crate) struct BridgedPlugin {
    shared: Arc<BridgedShared>,
    keeps_state: bool,
}

pub(crate) struct Opened {
    pub info: PluginInfo,
    pub params: Vec<HostedParam>,
    pub audio_inputs: u32,
    pub audio_outputs: u32,
    pub accepts_notes: bool,
    pub keeps_state: bool,
    pub plugin: BridgedPlugin,
}

pub(crate) fn open(
    bridges: &Arc<Bridges>,
    path: &Path,
    key: &PluginKey,
) -> Result<Opened, HostError> {
    let Some(bridge) = bridges.bridge(key.format) else {
        return Err(HostError::Unsupported(key.format));
    };
    let found = bridges
        .scan_bundle(key.format, path)
        .map_err(|why| HostError::Bundle {
            path: path.to_path_buf(),
            why,
        })?
        .into_iter()
        .find(|info| &info.key == key)
        .ok_or_else(|| HostError::NoSuchPlugin {
            key: key.clone(),
            path: path.to_path_buf(),
        })?;
    let table = bridge.table();
    let c_path = c_string(path).map_err(|why| HostError::Bundle {
        path: path.to_path_buf(),
        why,
    })?;
    let c_id = CString::new(key.id.as_bytes()).map_err(|_| HostError::NoSuchPlugin {
        key: key.clone(),
        path: path.to_path_buf(),
    })?;
    // SAFETY: the table's contract, with strings that live for the call.
    let instance = unsafe { (table.open)(c_path.as_ptr(), c_id.as_ptr()) };
    if instance.is_null() {
        return Err(HostError::Instantiate {
            key: key.clone(),
            why: format!("{} would not open it", bridge.name),
        });
    }
    let shared = Arc::new(BridgedShared {
        bridges: Arc::clone(bridges),
        format: key.format,
        instance,
    });
    let count = unsafe { (table.param_count)(instance) };
    let mut params = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut info = ParamInfo {
            id: 0,
            name: std::ptr::null(),
            module: std::ptr::null(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            stepped: 0,
            hidden: 0,
            readonly: 0,
        };
        if unsafe { (table.param_info)(instance, index, &mut info) } < 0 {
            continue;
        }
        params.push(HostedParam {
            id: info.id,
            name: text(info.name),
            module: text(info.module),
            min: info.min,
            max: info.max,
            default: info.default,
            stepped: info.stepped != 0,
            hidden: info.hidden != 0,
            readonly: info.readonly != 0,
        });
    }
    let audio_inputs = unsafe { (table.audio_inputs)(instance) };
    let audio_outputs = unsafe { (table.audio_outputs)(instance) };
    let accepts_notes = unsafe { (table.accepts_notes)(instance) } != 0;
    // Whether it keeps state is learnt by asking once: a bridge has no
    // other way to say, and the ABI makes a refusal cheap.
    let keeps_state = {
        let mut bytes: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        let ok = unsafe { (table.save_state)(instance, &mut bytes, &mut len) } >= 0;
        if ok && !bytes.is_null() {
            unsafe { (table.free_state)(bytes, len) };
        }
        ok
    };
    Ok(Opened {
        info: found,
        params,
        audio_inputs,
        audio_outputs,
        accepts_notes,
        keeps_state,
        plugin: BridgedPlugin {
            shared,
            keeps_state,
        },
    })
}

impl BridgedPlugin {
    fn table(&self) -> &FontelleBridge {
        self.shared.table()
    }

    /// Tells the plugin a value now, on the main thread.
    pub(crate) fn set_param(&self, id: u32, value: f64) {
        unsafe { (self.table().set_param)(self.shared.instance, id, value) };
    }

    pub(crate) fn get_param(&self, id: u32) -> f64 {
        unsafe { (self.table().get_param)(self.shared.instance, id) }
    }

    pub(crate) fn display(&self, id: u32, value: f64) -> Option<String> {
        let mut buffer = [0 as c_char; 128];
        let written = unsafe {
            (self.table().display)(self.shared.instance, id, value, buffer.as_mut_ptr(), 128)
        };
        if written < 0 {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(buffer.as_ptr()) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    pub(crate) fn save_state(&self) -> Option<Vec<u8>> {
        if !self.keeps_state {
            return None;
        }
        let mut bytes: *mut u8 = std::ptr::null_mut();
        let mut len = 0u32;
        if unsafe { (self.table().save_state)(self.shared.instance, &mut bytes, &mut len) } < 0
            || bytes.is_null()
        {
            return None;
        }
        let out = unsafe { std::slice::from_raw_parts(bytes, len as usize) }.to_vec();
        unsafe { (self.table().free_state)(bytes, len) };
        Some(out)
    }

    pub(crate) fn load_state(&self, bytes: &[u8]) -> bool {
        let result = unsafe {
            (self.table().load_state)(self.shared.instance, bytes.as_ptr(), bytes.len() as u32)
        };
        result >= 0
    }

    pub(crate) fn activate(
        &self,
        key: &PluginKey,
        values: Arc<ParamValues>,
        sample_rate: f64,
        max_block: usize,
        inputs: usize,
        outputs: usize,
    ) -> Result<BridgedProcessor, HostError> {
        let result =
            unsafe { (self.table().activate)(self.shared.instance, sample_rate, max_block as u32) };
        if result < 0 {
            return Err(HostError::Activate {
                key: key.clone(),
                why: "the bridge refused".to_string(),
            });
        }
        Ok(BridgedProcessor {
            shared: Arc::clone(&self.shared),
            values,
            input: vec![vec![0.0; max_block]; inputs],
            output: vec![vec![0.0; max_block]; outputs],
            in_ptrs: vec![std::ptr::null(); inputs],
            out_ptrs: vec![std::ptr::null_mut(); outputs],
            max_block,
        })
    }

    pub(crate) fn deactivate(&self) {
        unsafe { (self.table().deactivate)(self.shared.instance) };
    }

    // ---- ABI 2: the plugin's own editor, through the bridge.

    pub(crate) fn has_editor(&self) -> bool {
        (unsafe { (self.table().has_editor)(self.shared.instance) }) != 0
    }

    /// Opens the editor into `window`, answering the size the bridge says
    /// it wants. The window's id is what the platform embeds into — an X11
    /// window, or on Windows an `HWND`, which is what `effEditOpen` takes —
    /// and what a headless window reports as zero.
    pub(crate) fn open_editor(
        &self,
        window: &crate::gui::PluginWindow,
    ) -> Result<crate::gui::GuiSize, crate::gui::GuiError> {
        if !self.has_editor() {
            return Err(crate::gui::GuiError::NoEditor);
        }
        let (mut width, mut height) = (0u32, 0u32);
        let result = unsafe {
            (self.table().open_editor)(self.shared.instance, window.id(), &mut width, &mut height)
        };
        if result < 0 {
            return Err(crate::gui::GuiError::Refused("create"));
        }
        Ok(if width == 0 || height == 0 {
            crate::gui::GuiSize::FALLBACK
        } else {
            crate::gui::GuiSize { width, height }
        })
    }

    pub(crate) fn close_editor(&self) {
        unsafe { (self.table().close_editor)(self.shared.instance) };
    }

    pub(crate) fn tick_editor(&self) {
        unsafe { (self.table().tick_editor)(self.shared.instance) };
    }

    pub(crate) fn resize_editor(&self, size: crate::gui::GuiSize) -> bool {
        (unsafe { (self.table().resize_editor)(self.shared.instance, size.width, size.height) })
            >= 0
    }
}

/// The audio half.
pub(crate) struct BridgedProcessor {
    shared: Arc<BridgedShared>,
    values: Arc<ParamValues>,
    input: Vec<Vec<f32>>,
    output: Vec<Vec<f32>>,
    /// The pointer tables handed across, sized once.
    in_ptrs: Vec<*const f32>,
    out_ptrs: Vec<*mut f32>,
    max_block: usize,
}

// SAFETY: the pointer tables point into `input`/`output`, which move with
// the struct; they are rewritten before every use. See `BridgedShared`.
unsafe impl Send for BridgedProcessor {}

impl BridgedProcessor {
    fn table(&self) -> &FontelleBridge {
        self.shared.table()
    }

    pub(crate) fn max_block(&self) -> usize {
        self.max_block
    }

    pub(crate) fn input(&mut self) -> &mut [Vec<f32>] {
        &mut self.input
    }

    pub(crate) fn output(&self) -> &[Vec<f32>] {
        &self.output
    }

    pub(crate) fn note_on(&mut self, frame: usize, key: u8, velocity: f64) {
        unsafe { (self.table().note_on)(self.shared.instance, frame as u32, key, velocity) };
    }

    pub(crate) fn note_off(&mut self, frame: usize, key: u8) {
        unsafe { (self.table().note_off)(self.shared.instance, frame as u32, key) };
    }

    pub(crate) fn reset(&mut self) {
        unsafe { (self.table().reset)(self.shared.instance) };
    }

    /// **RT.** A controller, a bend and channel pressure — ABI 3's half of
    /// a performance. See [`fontelle_bridge_abi::FontelleBridge::controller`].
    pub(crate) fn controller(&mut self, frame: usize, controller: u8, value: u8) {
        unsafe { (self.table().controller)(self.shared.instance, frame as u32, controller, value) };
    }

    pub(crate) fn pitch_bend(&mut self, frame: usize, value: i16) {
        unsafe { (self.table().pitch_bend)(self.shared.instance, frame as u32, value) };
    }

    pub(crate) fn channel_pressure(&mut self, frame: usize, value: u8) {
        unsafe { (self.table().channel_pressure)(self.shared.instance, frame as u32, value) };
    }

    /// **RT.** One block. Whatever moved on the wire is written first, the
    /// way the other two arms do it.
    pub(crate) fn run(&mut self, frames: usize) {
        let frames = frames.min(self.max_block);
        let instance = self.shared.instance;
        // A raw copy of the table pointer rather than a borrow of `self`, so
        // the pointer tables below can be written; the table lives in the
        // bridge's library, which `shared` keeps loaded.
        let table: *const FontelleBridge = self.table();
        let table = unsafe { &*table };
        self.values
            .drain(|id, value| unsafe { (table.set_param)(instance, id, value) });
        for (pointer, buffer) in self.in_ptrs.iter_mut().zip(&self.input) {
            *pointer = buffer.as_ptr();
        }
        for (pointer, buffer) in self.out_ptrs.iter_mut().zip(&mut self.output) {
            *pointer = buffer.as_mut_ptr();
        }
        // SAFETY: exactly the channel counts the plugin declared, each
        // `max_block` long, which is at least `frames`.
        unsafe {
            (table.process)(
                instance,
                self.in_ptrs.as_ptr(),
                self.in_ptrs.len() as u32,
                self.out_ptrs.as_ptr(),
                self.out_ptrs.len() as u32,
                frames as u32,
            )
        };
    }
}
