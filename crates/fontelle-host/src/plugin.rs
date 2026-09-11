//! One running plugin, on the main thread.

use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clack_extensions::audio_ports::{AudioPortFlags, AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, HostGui, HostGuiImpl, PluginGui};
use clack_extensions::latency::PluginLatency;
use clack_extensions::note_ports::{NoteDialects, NotePortInfoBuffer, PluginNotePorts};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
#[cfg(unix)]
use clack_extensions::posix_fd::{FdFlags, HostPosixFd, HostPosixFdImpl};
use clack_extensions::state::PluginState as PluginStateExt;
use clack_extensions::timer::{HostTimer, HostTimerImpl, PluginTimer, TimerId};
use clack_host::host::HostError as HostGuiError;
use clack_host::plugin::PluginDescriptor;
use clack_host::prelude::*;
use clack_host::utils::Cookie;
use fontelle_types::{PluginFormat, PluginKey, PluginState, decode_base64, encode_base64};

use crate::bridge::{BridgedPlugin, Bridges};
use crate::lv2::Lv2Plugin;
use crate::param::{HostedParam, ParamValues};
use crate::processor::HostedProcessor;
use crate::scan::PluginInfo;

/// Why a plugin could not be opened or used.
#[derive(Debug)]
pub enum HostError {
    /// The format is named but this build cannot load it — see §8.4's order of
    /// intent and §3.4's licence rule.
    Unsupported(PluginFormat),
    /// The file would not open at all.
    Bundle { path: PathBuf, why: String },
    /// The bundle opened and does not hold that plugin. The usual cause is a
    /// project opened on a machine where a different version is installed.
    NoSuchPlugin { key: PluginKey, path: PathBuf },
    /// The plugin refused to start.
    Instantiate { key: PluginKey, why: String },
    /// The plugin refused to be activated at this sample rate or block size.
    Activate { key: PluginKey, why: String },
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(format) => {
                write!(f, "{} plugins cannot be loaded yet", format.label())
            }
            Self::Bundle { path, why } => {
                write!(f, "{} could not be opened: {why}", path.display())
            }
            Self::NoSuchPlugin { key, path } => {
                write!(f, "{} does not hold {key}", path.display())
            }
            Self::Instantiate { key, why } => write!(f, "{key} would not start: {why}"),
            Self::Activate { key, why } => write!(f, "{key} would not activate: {why}"),
        }
    }
}

impl std::error::Error for HostError {}

/// What this program tells a plugin about itself.
fn host_info() -> HostInfo {
    HostInfo::new(
        "Fontelle",
        "Fopull LLC",
        "https://github.com/Fopull-LLC/DAW-Fontelle",
        env!("CARGO_PKG_VERSION"),
    )
    .expect("the host's own name has no interior nul")
}

/// The callbacks a plugin may make back into the host.
///
/// Deliberately the smallest set that is legal. A plugin asking to be
/// restarted or to have its parameters rescanned is asking for something that
/// costs a graph rebuild, and rebuilding the graph from inside a plugin's
/// callback — which may arrive on the audio thread — is how a host deadlocks.
/// What this build does is *record* the request; the window acts on it between
/// frames, which is the same shape every other off-thread request in this
/// program has.
pub(crate) struct FontelleShared {
    pub(crate) wants_restart: std::sync::atomic::AtomicBool,
    pub(crate) wants_callback: std::sync::atomic::AtomicBool,
    /// What the plugin's editor has asked of its window since the studio last
    /// looked — see [`crate::gui`].
    ///
    /// Atomics rather than a queue: every one of these requests is a *latest
    /// wins* fact ("be this big", "be visible"), and a plugin dragging its own
    /// resize corner sends one per frame. The window reads them between
    /// frames, which is where a resize can safely happen.
    pub(crate) gui_resize: std::sync::atomic::AtomicU64,
    pub(crate) gui_show: std::sync::atomic::AtomicBool,
    pub(crate) gui_hide: std::sync::atomic::AtomicBool,
    pub(crate) gui_closed: std::sync::atomic::AtomicBool,
}

impl FontelleShared {
    pub(crate) fn new() -> Self {
        Self {
            wants_restart: std::sync::atomic::AtomicBool::new(false),
            wants_callback: std::sync::atomic::AtomicBool::new(false),
            gui_resize: std::sync::atomic::AtomicU64::new(0),
            gui_show: std::sync::atomic::AtomicBool::new(false),
            gui_hide: std::sync::atomic::AtomicBool::new(false),
            gui_closed: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl<'a> SharedHandler<'a> for FontelleShared {
    fn request_restart(&self) {
        self.wants_restart
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn request_process(&self) {}

    fn request_callback(&self) {
        self.wants_callback
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl HostGuiImpl for FontelleShared {
    fn resize_hints_changed(&self) {
        // Nothing yet: the window asks for the hints when it needs them, and
        // this build does not constrain a drag by aspect ratio.
    }

    fn request_resize(&self, new_size: clack_extensions::gui::GuiSize) -> Result<(), HostGuiError> {
        // Packed into one word so the pair cannot be read half-updated: a
        // window one frame wide and the next frame's height is a flicker
        // nobody can explain.
        let packed = (u64::from(new_size.width) << 32) | u64::from(new_size.height);
        self.gui_resize
            .store(packed, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    fn request_show(&self) -> Result<(), HostGuiError> {
        self.gui_show
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    fn request_hide(&self) -> Result<(), HostGuiError> {
        self.gui_hide
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    fn closed(&self, _was_destroyed: bool) {
        self.gui_closed
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

/// The host's main-thread half.
///
/// It exists for the editor. CLAP puts `register_timer` and `register_fd` on
/// the main thread — they are how a plugin's GUI is *driven*, and a plugin
/// that registers one and is never called back opens a window that never
/// paints. See [`crate::gui::GuiPump`].
#[derive(Default)]
pub(crate) struct FontelleMain {
    pub(crate) pump: crate::gui::GuiPump,
}

impl<'a> MainThreadHandler<'a> for FontelleMain {}

impl HostTimerImpl for FontelleMain {
    fn register_timer(&mut self, period_ms: u32) -> Result<TimerId, HostGuiError> {
        Ok(TimerId(self.pump.register_timer(period_ms)))
    }

    fn unregister_timer(&mut self, timer_id: TimerId) -> Result<(), HostGuiError> {
        if self.pump.unregister_timer(timer_id.0) {
            Ok(())
        } else {
            Err(HostGuiError::Message("no such timer"))
        }
    }
}

#[cfg(unix)]
impl HostPosixFdImpl for FontelleMain {
    fn register_fd(&mut self, fd: std::os::fd::RawFd, _flags: FdFlags) -> Result<(), HostGuiError> {
        self.pump.register_fd(fd);
        Ok(())
    }

    fn modify_fd(&mut self, fd: std::os::fd::RawFd, _flags: FdFlags) -> Result<(), HostGuiError> {
        // One set of flags is watched — readable — which is what an X11
        // connection and every GUI toolkit's event pipe want. A plugin asking
        // to be told when a descriptor is *writable* is asking for something
        // this build does not do, and saying so is better than pretending.
        self.pump.register_fd(fd);
        Ok(())
    }

    fn unregister_fd(&mut self, fd: std::os::fd::RawFd) -> Result<(), HostGuiError> {
        self.pump.unregister_fd(fd);
        Ok(())
    }
}

pub(crate) struct FontelleHost;

impl HostHandlers for FontelleHost {
    type Shared<'a> = FontelleShared;
    type MainThread<'a> = FontelleMain;
    type AudioProcessor<'a> = ();

    /// What Fontelle offers a plugin back.
    ///
    /// The three the editor needs and nothing else — see the note on
    /// [`FontelleShared`] about keeping this set the smallest that is legal.
    fn declare_extensions(builder: &mut HostExtensions<Self>, _shared: &Self::Shared<'_>) {
        builder.register::<HostGui>().register::<HostTimer>();
        // A descriptor to watch is a POSIX thing; CLAP offers the extension
        // nowhere else, and neither does this.
        #[cfg(unix)]
        builder.register::<HostPosixFd>();
    }
}

/// Every bundle this session has opened.
///
/// A cache rather than a convenience: opening a CLAP bundle runs its entry
/// point, and a suite of twenty plugins is one file. Loading it once per
/// instrument would run that twenty times and hold twenty copies of whatever
/// it allocated at load. An LV2 bundle is parsed Turtle rather than run
/// code, but parsing it once is still the right number of times.
#[derive(Default)]
pub struct PluginHost {
    /// The bridges this host may reach a third format through — see
    /// [`crate::bridge`]. Shared with everything opened through one.
    bridges: Arc<Bridges>,
    bundles: HashMap<PathBuf, PluginEntry>,
    /// One lilv world per LV2 bundle — see `lv2` for why not one for all.
    worlds: HashMap<PathBuf, crate::lv2::World>,
    /// The feature set every LV2 plugin of this host shares, built on the
    /// first one. It owns a worker thread, which is why there is one.
    lv2_features: Option<Arc<crate::lv2::Features>>,
}

impl PluginHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// A host that can also reach whatever `bridges` serve.
    pub fn with_bridges(bridges: Arc<Bridges>) -> Self {
        Self {
            bridges,
            ..Self::default()
        }
    }

    pub fn bridges(&self) -> &Arc<Bridges> {
        &self.bridges
    }

    /// Whether this host can load a plugin of `format` — natively, or
    /// through a bridge that is installed. [`PluginFormat::hosted`] answers
    /// only the first half, because the second depends on the machine.
    pub fn can_host(&self, format: PluginFormat) -> bool {
        format.hosted() || self.bridges.serves(format)
    }

    /// How many bundles are open, of either format. For tests, and for a
    /// status line.
    pub fn loaded_bundles(&self) -> usize {
        self.bundles.len() + self.worlds.len()
    }

    /// Starts one plugin out of `path`.
    pub fn open(&mut self, path: &Path, key: &PluginKey) -> Result<HostedPlugin, HostError> {
        match key.format {
            PluginFormat::Clap => self.open_clap(path, key),
            PluginFormat::Lv2 => self.open_lv2(path, key),
            other if self.bridges.serves(other) => self.open_bridged(path, key),
            other => Err(HostError::Unsupported(other)),
        }
    }

    fn open_bridged(&mut self, path: &Path, key: &PluginKey) -> Result<HostedPlugin, HostError> {
        let opened = crate::bridge::open(&self.bridges, path, key)?;
        let values = Arc::new(ParamValues::new(&opened.params));
        Ok(HostedPlugin {
            info: opened.info,
            atoms: Arc::new(crate::atom::AtomPipes::new(None, None)),
            inner: Inner::Bridged(opened.plugin),
            params: opened.params,
            values,
            audio_inputs: opened.audio_inputs,
            audio_outputs: opened.audio_outputs,
            input_ports: PortLayout::single(opened.audio_inputs),
            output_ports: PortLayout::single(opened.audio_outputs),
            accepts_notes: opened.accepts_notes,
            note_dialect: opened.accepts_notes.then_some(NoteDialect::Midi),
            keeps_state: opened.keeps_state,
            // LV2 reports latency through an output control port designated
            // `lv2:latency`, and a bridge's table does not carry one at all.
            // Neither is read yet: zero is what a host that cannot ask must
            // assume, and it is the honest answer for this build.
            latency: 0,
            active: false,
            editor_open: false,
            lv2_editor: None,
        })
    }

    fn open_lv2(&mut self, path: &Path, key: &PluginKey) -> Result<HostedPlugin, HostError> {
        if !self.worlds.contains_key(path) {
            let world = crate::lv2::load_world(path).map_err(|why| HostError::Bundle {
                path: path.to_path_buf(),
                why,
            })?;
            self.worlds.insert(path.to_path_buf(), world);
        }
        let world = &self.worlds[path];
        let features = match &self.lv2_features {
            Some(features) => Arc::clone(features),
            None => {
                let features = crate::lv2::build_features(world);
                self.lv2_features = Some(Arc::clone(&features));
                features
            }
        };
        let opened = crate::lv2::open(path, key, world, &features)?;
        let values = Arc::new(ParamValues::new(&opened.params));
        let (atom_in, atom_out) = opened.plugin.atom_ports();
        Ok(HostedPlugin {
            info: opened.info,
            atoms: Arc::new(crate::atom::AtomPipes::new(atom_in, atom_out)),
            inner: Inner::Lv2(opened.plugin),
            params: opened.params,
            values,
            audio_inputs: opened.audio_inputs,
            audio_outputs: opened.audio_outputs,
            input_ports: opened.input_ports,
            output_ports: PortLayout::single(opened.audio_outputs),
            accepts_notes: opened.accepts_notes,
            // An LV2 note port is an atom port that takes MIDI, always.
            note_dialect: opened.accepts_notes.then_some(NoteDialect::Midi),
            keeps_state: opened.keeps_state,
            // LV2 reports latency through an output control port designated
            // `lv2:latency`, which this build does not read. Zero is what a
            // host that cannot ask has to assume — see `latency_samples`.
            latency: 0,
            active: false,
            editor_open: false,
            lv2_editor: None,
        })
    }

    fn open_clap(&mut self, path: &Path, key: &PluginKey) -> Result<HostedPlugin, HostError> {
        let entry = self.entry(path)?;
        let id = std::ffi::CString::new(key.id.as_str()).map_err(|_| HostError::NoSuchPlugin {
            key: key.clone(),
            path: path.to_path_buf(),
        })?;
        let found = entry
            .get_plugin_factory()
            .and_then(|factory| {
                factory
                    .plugin_descriptors()
                    .find(|d| d.id() == Some(id.as_c_str()))
                    .map(|d| describe(path, d))
            })
            .ok_or_else(|| HostError::NoSuchPlugin {
                key: key.clone(),
                path: path.to_path_buf(),
            })?;

        let mut instance = PluginInstance::<FontelleHost>::new(
            |_| FontelleShared::new(),
            |_| FontelleMain::default(),
            entry,
            id.as_c_str(),
            &host_info(),
        )
        .map_err(|e| HostError::Instantiate {
            key: key.clone(),
            why: e.to_string(),
        })?;

        let params = read_params(&mut instance);
        let (input_ports, output_ports) = read_audio_ports(&mut instance);
        let (audio_inputs, audio_outputs) =
            (input_ports.main_channels(), output_ports.main_channels());
        let note_dialect = read_note_ports(&mut instance);
        let accepts_notes = note_dialect.is_some();
        let keeps_state = instance
            .plugin_handle()
            .get_extension::<PluginStateExt>()
            .is_some();
        // What the plugin says it puts between its input and its output, so
        // the graph can line the rest of the mix up with it (TDD §5.5).
        // Read once, here: CLAP lets a plugin change it and tell the host,
        // and following that means rebuilding the graph — which is what
        // reopening the project or touching the rack does. A plugin that
        // does not declare the extension answers zero, which is what a host
        // that cannot ask has to assume.
        let latency = instance
            .plugin_handle()
            .get_extension::<PluginLatency>()
            .map_or(0, |ext| ext.get(&mut instance.plugin_handle()));
        let values = Arc::new(ParamValues::new(&params));

        Ok(HostedPlugin {
            info: found,
            atoms: Arc::new(crate::atom::AtomPipes::new(None, None)),
            inner: Inner::Clap(instance),
            params,
            values,
            audio_inputs,
            audio_outputs,
            input_ports,
            output_ports,
            accepts_notes,
            note_dialect,
            keeps_state,
            latency,
            active: false,
            editor_open: false,
            lv2_editor: None,
        })
    }

    fn entry(&mut self, path: &Path) -> Result<&PluginEntry, HostError> {
        if !self.bundles.contains_key(path) {
            let entry = load_entry(path)?;
            self.bundles.insert(path.to_path_buf(), entry);
        }
        Ok(&self.bundles[path])
    }
}

/// Opens a bundle. **This runs somebody else's code**, which is the whole of
/// why it is `unsafe` in `clack` and why every path into it goes through here.
fn load_entry(path: &Path) -> Result<PluginEntry, HostError> {
    // SAFETY: there is no safe version of this. A CLAP bundle is a shared
    // library whose entry point runs on load; nothing this program can check
    // first makes that safe, and every host in existence takes the same step.
    // What is checked is what can be: the file exists, and its extension says
    // it claims to be a plugin.
    unsafe { PluginEntry::load(path) }.map_err(|e| HostError::Bundle {
        path: path.to_path_buf(),
        why: e.to_string(),
    })
}

/// Everything one CLAP bundle holds. Used by the scanner.
pub(crate) fn read_clap_bundle(path: &Path) -> Result<Vec<PluginInfo>, String> {
    let entry = load_entry(path).map_err(|e| e.to_string())?;
    let factory = entry
        .get_plugin_factory()
        .ok_or_else(|| "the bundle offers no plugins".to_string())?;
    Ok(factory
        .plugin_descriptors()
        .filter(|d| d.id().is_some_and(|id| !id.is_empty()))
        .map(|d| describe(path, d))
        .collect())
}

fn describe(path: &Path, descriptor: &PluginDescriptor) -> PluginInfo {
    PluginInfo {
        key: PluginKey::clap(text(descriptor.id())),
        path: path.to_path_buf(),
        name: text(descriptor.name()),
        vendor: text(descriptor.vendor()),
        version: text(descriptor.version()),
        features: descriptor
            .features()
            .map(|f| f.to_string_lossy().into_owned())
            .collect(),
    }
}

fn text(value: Option<&CStr>) -> String {
    value
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// One plugin, running.
///
/// **Main thread only**, and that is CLAP's rule rather than a preference:
/// the specification names which of a plugin's calls belong to which thread,
/// and `clack` refuses to let this type cross to another one. The half that
/// does cross is [`HostedProcessor`], which [`activate`](Self::activate)
/// hands out.
///
/// The format is inside, in [`Inner`], and every method below reads the
/// same to a caller whichever it is — which is the whole claim of this
/// crate. Where a format has nothing to do for a call (LV2 has no state
/// extension here, and no display strings) the method says so honestly
/// rather than pretending.
pub struct HostedPlugin {
    info: PluginInfo,
    inner: Inner,
    params: Vec<HostedParam>,
    values: Arc<ParamValues>,
    /// The main ports' channel counts — what the bus is copied to and from.
    audio_inputs: u32,
    audio_outputs: u32,
    /// **Every** port, in the order the plugin declared them. See
    /// [`PortLayout`] for why the main one is not enough.
    input_ports: PortLayout,
    output_ports: PortLayout,
    accepts_notes: bool,
    /// How notes and controllers are spoken to it — see [`NoteDialect`].
    note_dialect: Option<NoteDialect>,
    keeps_state: bool,
    /// What the plugin says it delays by, in samples — see
    /// [`HostedPlugin::latency_samples`].
    latency: u32,
    active: bool,
    /// Whether the plugin's own editor has been created. `destroy` is only
    /// legal after a `create`, and calling it twice is undefined.
    editor_open: bool,
    /// The LV2 arm of the same thing. CLAP's editor lives inside the plugin
    /// instance and is addressed through an extension; an LV2 UI is a
    /// **separate shared library** with a life of its own, so there is an
    /// object here to hold it. See [`crate::lv2_ui`].
    lv2_editor: Option<crate::lv2_ui::Lv2Ui>,
    /// The atoms an LV2 editor and its plugin exchange — see [`crate::atom`].
    ///
    /// Made when the plugin is opened and shared with whatever is rendering
    /// it, exactly as [`ParamValues`] is: the editor comes and goes, and the
    /// pipes have to outlive it in both directions.
    atoms: Arc<crate::atom::AtomPipes>,
}

/// Which language a plugin's note port speaks, which decides how a
/// controller reaches it.
///
/// CLAP lets a plugin's note port declare the **dialects** it accepts: its
/// own note events, raw MIDI 1.0, MIDI 2.0. Notes are sent as CLAP's own in
/// every case. A **controller** — a mod wheel, a bend, aftertouch — has no
/// CLAP note event, so it is sent as the three MIDI bytes it was when the
/// port takes MIDI, and as the nearest **note expression** otherwise (the
/// wheel as vibrato, aftertouch as pressure, the bend as tuning); see
/// [`HostedProcessor::controller`]. Nothing is invented onto a parameter.
/// An LV2 note port is an atom port that takes MIDI, so it is always
/// [`Midi`](Self::Midi).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteDialect {
    /// CLAP's own note events, and note expressions for everything else.
    Clap,
    /// Raw MIDI 1.0 beside CLAP's note events.
    Midi,
}

/// What a plugin's editor has asked its window for. See
/// [`HostedPlugin::take_editor_requests`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EditorRequests {
    pub resize: Option<crate::gui::GuiSize>,
    pub show: bool,
    pub hide: bool,
    /// The plugin closed its own editor — a preset browser that opens a window
    /// of its own and takes the first one down.
    pub closed: bool,
}

/// The format-specific half of a [`HostedPlugin`].
enum Inner {
    Clap(PluginInstance<FontelleHost>),
    Lv2(Lv2Plugin),
    Bridged(BridgedPlugin),
}

impl HostedPlugin {
    fn clap(&mut self) -> Option<&mut PluginInstance<FontelleHost>> {
        match &mut self.inner {
            Inner::Clap(instance) => Some(instance),
            Inner::Lv2(_) | Inner::Bridged(_) => None,
        }
    }

    pub fn info(&self) -> &PluginInfo {
        &self.info
    }

    pub fn key(&self) -> &PluginKey {
        &self.info.key
    }

    pub fn name(&self) -> &str {
        &self.info.name
    }

    /// Every parameter, in the order the plugin declared them.
    pub fn params(&self) -> &[HostedParam] {
        &self.params
    }

    pub fn param(&self, id: u32) -> Option<&HostedParam> {
        self.params.iter().find(|param| param.id == id)
    }

    /// The wire a knob writes on. Shared with whatever is rendering.
    pub fn values(&self) -> &Arc<ParamValues> {
        &self.values
    }

    /// How many audio channels its main input port has. Zero for an
    /// instrument.
    pub fn audio_inputs(&self) -> u32 {
        self.audio_inputs
    }

    pub fn audio_outputs(&self) -> u32 {
        self.audio_outputs
    }

    /// The channel count of **every** input port, in the order the plugin
    /// declared them. The bus feeds only the main one (see
    /// [`audio_inputs`](Self::audio_inputs)); the rest are handed silence.
    pub fn input_ports(&self) -> &[u32] {
        &self.input_ports.channels
    }

    /// Likewise every output port. The bus takes only the main one; the rest
    /// are rendered into and dropped.
    pub fn output_ports(&self) -> &[u32] {
        &self.output_ports.channels
    }

    /// Whether it has a note input — whether it can be played.
    pub fn accepts_notes(&self) -> bool {
        self.accepts_notes
    }

    /// How its note port is spoken to, or `None` when it has none.
    pub fn note_dialect(&self) -> Option<NoteDialect> {
        self.note_dialect
    }

    /// Whether it has a **sidechain**: an audio input beside the main one
    /// that another track's bus can be handed to.
    ///
    /// CLAP has no sidechain flag; a sidechain is any input port that is not
    /// the main one, and a compressor's key arrives on it. LV2 has a port
    /// property, `lv2:isSideChain`, and `crate::lv2::open` folds the inputs
    /// that carry it into the same second port. `false` for every bridged
    /// plugin in this build — the ABI carries one input — so a key on one of
    /// those is an edge that orders the graph and feeds nothing.
    pub fn takes_key(&self) -> bool {
        self.input_ports.key().is_some()
    }

    /// How many samples this plugin puts between its input and its output.
    ///
    /// What delay compensation is built on (TDD §5.5): a plugin that looks
    /// ahead — a mastering limiter, a linear-phase EQ — hands back audio
    /// later than it was given, and everything else has to be held back to
    /// meet it. Read from CLAP's `latency` extension when the plugin
    /// declares one, and **zero** otherwise: for a plugin with no extension,
    /// for every LV2 plugin (whose answer is an output control port this
    /// build does not read) and for every bridged one (whose table has no
    /// entry for it).
    ///
    /// Read once, when the plugin is opened. CLAP allows a plugin to change
    /// its latency and tell the host; following that means rebuilding the
    /// graph, which is what reopening the project or changing the rack does.
    pub fn latency_samples(&self) -> u32 {
        self.latency
    }

    /// Whether it has state of its own beyond its parameters.
    pub fn keeps_state(&self) -> bool {
        self.keeps_state
    }

    /// Sets a parameter. `false` if the plugin has no such parameter.
    ///
    /// Writes the wire, and — while the plugin is **not** running — tells the
    /// plugin directly as well. Both halves are needed: the wire is how a
    /// running plugin hears a knob, and the direct call is how a plugin that
    /// has not been activated yet is still in the right state when its own
    /// blob is read back.
    pub fn set_param(&mut self, id: u32, value: f64) -> bool {
        let Some(param) = self.param(id) else {
            return false;
        };
        let value = value.clamp(param.min.min(param.max), param.max.max(param.min));
        if !self.values.set(id, value) {
            return false;
        }
        if !self.active {
            self.flush_param(id, value);
        }
        true
    }

    /// What the document should remember about this plugin right now.
    ///
    /// For an LV2 plugin that keeps state and is running, the blob is out
    /// with the processor and this answers with the last state it was
    /// **given** — see [`snapshot_with`](Self::snapshot_with), and
    /// [`state_needs_processor`](Self::state_needs_processor) for how to
    /// know which to call.
    pub fn snapshot(&mut self) -> PluginState {
        let mut state = PluginState::new(self.info.key.clone(), self.info.name.clone());
        for (id, value) in self.values.all() {
            state.set_param(id, value);
        }
        state.blob = self.save_state().map(|bytes| encode_base64(&bytes));
        state
    }

    /// [`snapshot`](Self::snapshot), with the processor in hand.
    ///
    /// **The LV2 arm's whole reason to exist.** An LV2 plugin is one object,
    /// and its `state:interface` is on the instance that rides in the
    /// processor; the specification forbids calling it while `run` executes.
    /// So a snapshot of a running LV2 plugin borrows the processor — from
    /// the bay directly, or through [`crate::ProcessorBay::recall`] when a
    /// graph is playing it — and this reads the state while the caller holds
    /// it. For the other formats the processor is not needed and not
    /// touched.
    pub fn snapshot_with(&mut self, processor: &mut HostedProcessor) -> PluginState {
        let mut state = PluginState::new(self.info.key.clone(), self.info.name.clone());
        for (id, value) in self.values.all() {
            state.set_param(id, value);
        }
        state.blob = self
            .save_state_with(processor)
            .map(|bytes| encode_base64(&bytes));
        state
    }

    /// Whether a snapshot right now needs the processor to be honest.
    ///
    /// `true` only for an LV2 plugin that keeps state **and** is running:
    /// before activation there is no instance to ask, after deactivation
    /// its last state has already been read off, and a CLAP or bridged
    /// plugin keeps its state on this half.
    pub fn state_needs_processor(&self) -> bool {
        matches!(&self.inner, Inner::Lv2(plugin) if plugin.keeps_state()) && self.active
    }

    /// Puts a plugin back the way the document remembers it.
    ///
    /// `false` — and nothing applied — if the state belongs to a different
    /// plugin. That is not a defensive nicety: a slot whose plugin was
    /// swapped would otherwise have somebody else's numbers written into it,
    /// and parameter ids mean different things in different plugins.
    ///
    /// The blob goes in first and the parameters after, and the order is the
    /// point. The blob is the plugin's own account of itself and carries what
    /// no parameter can — a loaded sample, a drawn curve. The parameters are
    /// what *this* program knows and what its automation lanes address, so
    /// they are the last word. A blob that will not decode is skipped rather
    /// than refused: a plugin at its defaults with the right knob positions is
    /// a great deal closer to the song than one that would not open.
    pub fn restore(&mut self, state: &PluginState) -> bool {
        if state.key != self.info.key {
            return false;
        }
        if let Some(blob) = &state.blob
            && let Some(bytes) = decode_base64(blob)
        {
            self.load_state(&bytes);
        }
        for param in &state.params {
            self.set_param(param.id, param.value);
        }
        true
    }

    /// The plugin's own state. `None` if it keeps none.
    ///
    /// For LV2 this is the state last **given** to the plugin (or read off
    /// it when it stopped), not the state it is in now — that needs the
    /// processor; see [`save_state_with`](Self::save_state_with).
    pub fn save_state(&mut self) -> Option<Vec<u8>> {
        if let Inner::Bridged(plugin) = &self.inner {
            return plugin.save_state();
        }
        if let Inner::Lv2(plugin) = &self.inner {
            return plugin.pending_state().map(<[u8]>::to_vec);
        }
        let instance = self.clap()?;
        let extension = instance.plugin_handle().get_extension::<PluginStateExt>()?;
        let mut bytes = Vec::new();
        extension
            .save(&mut instance.plugin_handle(), &mut bytes)
            .ok()?;
        Some(bytes)
    }

    /// [`save_state`](Self::save_state), with the processor in hand — the
    /// running instance's actual state for LV2, and the same answer as
    /// `save_state` for everything else. The state read is also kept, so a
    /// later `save_state` without the processor answers with it.
    pub fn save_state_with(&mut self, processor: &mut HostedProcessor) -> Option<Vec<u8>> {
        if let Inner::Lv2(plugin) = &mut self.inner {
            if !plugin.keeps_state() {
                return None;
            }
            let read = processor.lv2_save_state();
            if let Some(bytes) = &read {
                plugin.stash_state(bytes);
            }
            return read.or_else(|| plugin.pending_state().map(<[u8]>::to_vec));
        }
        self.save_state()
    }

    /// Hands the plugin back its own state. `false` if it would not take it.
    ///
    /// An LV2 plugin has no instance until it is activated, so the state is
    /// **kept** and applied the moment one exists — `Lv2Plugin::activate` —
    /// which is also the one moment LV2 lets a host restore a plugin
    /// without asking whether it is thread-safe to.
    pub fn load_state(&mut self, bytes: &[u8]) -> bool {
        if let Inner::Lv2(plugin) = &mut self.inner {
            return plugin.stash_state(bytes);
        }
        if let Inner::Bridged(plugin) = &self.inner {
            let loaded = plugin.load_state(bytes);
            if loaded {
                self.reread_params();
            }
            return loaded;
        }
        let Some(instance) = self.clap() else {
            return false;
        };
        let Some(extension) = instance.plugin_handle().get_extension::<PluginStateExt>() else {
            return false;
        };
        let mut reader = bytes;
        let loaded = extension
            .load(&mut instance.plugin_handle(), &mut reader)
            .is_ok();
        if loaded {
            // The plugin now disagrees with the wire about every parameter,
            // and the plugin is right — it is the one that just read the file.
            self.reread_params();
        }
        loaded
    }

    /// Prepares the plugin to run, and hands out the half that renders.
    ///
    /// Everything a block needs is allocated here: the event lists, the port
    /// tables, and the scratch the channel counts do not line up in. See the
    /// crate's note on what INVARIANT 1 can and cannot promise about foreign
    /// code.
    pub fn activate(
        &mut self,
        sample_rate: f64,
        max_block: u32,
    ) -> Result<HostedProcessor, HostError> {
        let max_block = max_block.max(1) as usize;
        // An LV2 editor may be holding the instance this replaces, through
        // `instance-access` — see `Lv2Plugin::instance`. It goes first.
        if self.lv2_editor.is_some() {
            self.close_editor();
        }
        let processor = match &mut self.inner {
            Inner::Clap(instance) => {
                let config = PluginAudioConfiguration {
                    sample_rate,
                    min_frames_count: 1,
                    max_frames_count: max_block as u32,
                };
                let stopped =
                    instance
                        .activate(|_, _| (), config)
                        .map_err(|e| HostError::Activate {
                            key: self.info.key.clone(),
                            why: e.to_string(),
                        })?;
                let started = stopped
                    .start_processing()
                    .map_err(|e| HostError::Activate {
                        key: self.info.key.clone(),
                        why: e.to_string(),
                    })?;
                HostedProcessor::clap(
                    started,
                    Arc::clone(&self.values),
                    self.input_ports.clone(),
                    self.output_ports.clone(),
                    self.note_dialect,
                    max_block,
                )
            }
            Inner::Lv2(plugin) => HostedProcessor::lv2(plugin.activate(
                &self.info.key,
                Arc::clone(&self.values),
                Arc::clone(&self.atoms),
                sample_rate,
                max_block,
            )?),
            Inner::Bridged(plugin) => HostedProcessor::bridged(plugin.activate(
                &self.info.key,
                Arc::clone(&self.values),
                sample_rate,
                max_block,
                self.audio_inputs as usize,
                self.audio_outputs as usize,
            )?),
        };
        self.active = true;
        // A freshly activated plugin is at its own defaults and has never been
        // told what this project wants.
        self.values.mark_all();
        Ok(processor)
    }

    /// Stops the plugin, taking back the half that renders.
    ///
    /// **This is not optional.** A `PluginInstance` dropped while its
    /// processor is still alive is deliberately leaked by `clack` rather than
    /// freed on the wrong thread, so anything that opens a plugin owns getting
    /// its processor back — see `fontelle_app::plugins`, which is what does.
    /// An LV2 processor *is* the instance, and dropping it here on the main
    /// thread is exactly the deactivate-and-cleanup the specification asks
    /// for.
    pub fn deactivate(&mut self, mut processor: HostedProcessor) {
        // The last thing an LV2 instance was, read before it goes: a
        // snapshot after this has no instance to ask and answers with it.
        if let Inner::Lv2(plugin) = &mut self.inner
            && plugin.keeps_state()
            && let Some(bytes) = processor.lv2_save_state()
        {
            plugin.stash_state(&bytes);
        }
        match (&mut self.inner, processor.into_clap_stopped()) {
            (Inner::Clap(instance), Some(stopped)) => instance.deactivate(stopped),
            (Inner::Bridged(plugin), _) => plugin.deactivate(),
            _ => {}
        }
        self.active = false;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// What the plugin says this value reads as — "2.00x", "-6.0 dB".
    ///
    /// From the plugin rather than from a format string here, because units
    /// are the plugin's business and a host that guessed would put decibels
    /// after a ratio.
    ///
    /// `None` for an LV2 plugin, whose Turtle has units but no formatter;
    /// the panel then shows the number, which is what every LV2 host does.
    pub fn display(&mut self, id: u32, value: f64) -> Option<String> {
        if let Inner::Bridged(plugin) = &self.inner {
            return plugin.display(id, value);
        }
        let instance = self.clap()?;
        let extension = instance.plugin_handle().get_extension::<PluginParams>()?;
        let mut buffer = [0u8; 128];
        let text = extension
            .value_to_text(
                &mut instance.plugin_handle(),
                ClapId::new(id),
                value,
                &mut buffer,
            )
            .ok()?;
        Some(String::from_utf8_lossy(text).into_owned())
    }

    // -------------------------------------------- the plugin's own editor ---

    /// Whether this plugin has an editor Fontelle can show.
    ///
    /// Two questions, not one: the plugin has to offer an editor at all,
    /// **and** it has to accept the shape this host can provide — an X11
    /// window it draws into. Surge XT answers yes to *x11, embedded* and no
    /// to floating, to Wayland and to floating-Wayland, which is the ordinary
    /// answer from anything built on JUCE; an LV2 plugin answers yes when it
    /// ships an `ui:X11UI`, which almost all of them do (see
    /// [`crate::lv2_ui`]); a bridged plugin answers through its bridge (ABI
    /// 2). `false` for Calf, which ships no UI at all — that keeps the
    /// generated panel.
    pub fn has_editor(&mut self) -> bool {
        if let Inner::Lv2(plugin) = &self.inner {
            return plugin.has_editor();
        }
        if let Inner::Bridged(plugin) = &self.inner {
            return plugin.has_editor();
        }
        let Some(instance) = self.clap() else {
            return false;
        };
        let Some(gui) = instance.plugin_handle().get_extension::<PluginGui>() else {
            return false;
        };
        gui.is_api_supported(
            &mut instance.plugin_handle(),
            GuiConfiguration {
                api_type: GuiApiType::X11,
                is_floating: false,
            },
        )
    }

    /// Opens the editor into `window` and shows it, answering how big the
    /// plugin wants to be.
    ///
    /// CLAP's opening sequence in order — `create`, `set_scale`, `get_size`,
    /// `set_parent`, `show` — and each step is the plugin's to refuse.
    pub fn open_editor(
        &mut self,
        window: &crate::gui::PluginWindow,
        scale: f64,
    ) -> Result<crate::gui::GuiSize, crate::gui::GuiError> {
        use crate::gui::GuiError;
        if let Inner::Lv2(plugin) = &self.inner {
            let editor = plugin.open_editor(
                &self.info.key.id,
                &self.info.path,
                Arc::clone(&self.values),
                Arc::clone(&self.atoms),
                &self.params,
                window,
            )?;
            self.lv2_editor = Some(editor);
            self.editor_open = true;
            // An LV2 UI says how big it wants to be by *asking*, through
            // `ui:resize`, rather than by being asked — so the first size is
            // whatever it requests on its way up, and until then the window
            // keeps the one it was made with.
            return Ok(window.size());
        }
        if let Inner::Bridged(plugin) = &self.inner {
            // The bridge embeds it and says how big it is — ABI 2.
            let wanted = plugin.open_editor(window)?;
            self.editor_open = true;
            return Ok(wanted);
        }
        let Some(instance) = self.clap() else {
            return Err(GuiError::NoEditor);
        };
        let Some(gui) = instance.plugin_handle().get_extension::<PluginGui>() else {
            return Err(GuiError::NoEditor);
        };
        let configuration = GuiConfiguration {
            api_type: GuiApiType::X11,
            is_floating: false,
        };
        gui.create(&mut instance.plugin_handle(), configuration)
            .map_err(|_| GuiError::Refused("create"))?;
        // From here on the plugin **has** a GUI, so every way out has to free
        // it: `destroy` is only legal after a `create`, and a `create` nobody
        // destroyed is a plugin holding a toolkit open for the rest of the
        // session.
        macro_rules! give_up {
            ($step:expr) => {{
                gui.destroy(&mut instance.plugin_handle());
                return Err(GuiError::Refused($step));
            }};
        }
        // Advisory: a plugin that reads the display's scale itself is entitled
        // to ignore this, and several do.
        let _ = gui.set_scale(&mut instance.plugin_handle(), scale);
        let wanted = gui
            .get_size(&mut instance.plugin_handle())
            .map(|size| crate::gui::GuiSize {
                width: size.width,
                height: size.height,
            })
            .unwrap_or(crate::gui::GuiSize::FALLBACK);
        // The window this process made and owns. It outlives the editor:
        // `close_editor` destroys the plugin's GUI first, and only then is
        // the window dropped.
        // `c_ulong` is 64 bits on Linux, where this runs, and 32 on Windows,
        // where the conversion is the identity and clippy would call it
        // useless — but the code is one code, so the lint is answered here.
        #[allow(clippy::useless_conversion)]
        let parent = clack_extensions::gui::Window::from_x11_handle(window.id().into());
        // SAFETY: `parent` names a live X11 window this process made and owns,
        // and it outlives the editor — `close_editor` destroys the plugin's
        // GUI first, and only then is the window dropped.
        if unsafe { gui.set_parent(&mut instance.plugin_handle(), parent) }.is_err() {
            give_up!("attach")
        }
        // **A `false` from `show` is not a refusal.** clap-helpers' default
        // `guiShow` returns false unless the plugin overrides it, and plugins
        // that put their window up in `set_parent` — SpectMorph, the OneTrick
        // drum synths — never do. Reading that false as failure destroyed the
        // editor this had just embedded, and the window went with it: *"just
        // closing their window as soon as it opens"*. Once `set_parent` has
        // succeeded the editor exists and is drawn; what `show` says after
        // that is advisory.
        let _ = gui.show(&mut instance.plugin_handle());
        self.editor_open = true;
        Ok(wanted)
    }

    /// Tells the plugin the window is now this big.
    ///
    /// Only when the plugin says it can be resized: one that cannot is one
    /// whose editor is a fixed image, and setting a size it refused would
    /// leave the window and the drawing disagreeing.
    pub fn resize_editor(&mut self, size: crate::gui::GuiSize) {
        if matches!(self.inner, Inner::Lv2(_)) {
            // An LV2 UI is told the size by the window it was given being that
            // size; there is no call for it in the extension this build uses.
            let _ = size;
            return;
        }
        if let Inner::Bridged(plugin) = &self.inner {
            if self.editor_open {
                let _ = plugin.resize_editor(size);
            }
            return;
        }
        let Some(instance) = self.clap() else {
            return;
        };
        let Some(gui) = instance.plugin_handle().get_extension::<PluginGui>() else {
            return;
        };
        if !gui.can_resize(&mut instance.plugin_handle()) {
            return;
        }
        let wanted = clack_extensions::gui::GuiSize {
            width: size.width,
            height: size.height,
        };
        // Negotiated, not imposed: the plugin adjusts the size to one it can
        // actually draw, and that is the one it is set to.
        let agreed = gui
            .adjust_size(&mut instance.plugin_handle(), wanted)
            .unwrap_or(wanted);
        let _ = gui.set_size(&mut instance.plugin_handle(), agreed);
    }

    /// Whether the plugin's editor may be dragged to a different size.
    pub fn editor_resizable(&mut self) -> bool {
        if matches!(self.inner, Inner::Lv2(_)) {
            return true;
        }
        // A bridged editor is the size the bridge reported; the ABI has no
        // way to ask whether it can be dragged, and a `resize_editor` that
        // is refused would leave the window and the drawing disagreeing.
        if matches!(self.inner, Inner::Bridged(_)) {
            return false;
        }
        let Some(instance) = self.clap() else {
            return false;
        };
        instance
            .plugin_handle()
            .get_extension::<PluginGui>()
            .is_some_and(|gui| gui.can_resize(&mut instance.plugin_handle()))
    }

    /// Frees the editor's resources. **Before the window is dropped**, so the
    /// plugin unhooks from a window that still exists.
    pub fn close_editor(&mut self) {
        if !self.editor_open {
            return;
        }
        self.editor_open = false;
        // Dropping it is the LV2 `cleanup` — see `Lv2Ui::drop`, which is where
        // the UI's own library is let go of too.
        if self.lv2_editor.take().is_some() {
            return;
        }
        if let Inner::Bridged(plugin) = &self.inner {
            plugin.close_editor();
            return;
        }
        let Some(instance) = self.clap() else {
            return;
        };
        if let Some(gui) = instance.plugin_handle().get_extension::<PluginGui>() {
            let _ = gui.hide(&mut instance.plugin_handle());
            gui.destroy(&mut instance.plugin_handle());
        }
    }

    pub fn editor_is_open(&self) -> bool {
        self.editor_open
    }

    /// How many atoms have gone each way between this plugin and its editor,
    /// as `(to the plugin, to the editor)`.
    ///
    /// The one number that separates *"the editor is open and talking"* from
    /// *"the editor is open and its words are going nowhere"* — which look
    /// exactly alike from outside, and which is the difference between a
    /// sampler you can load a file into and one you cannot. Always `(0, 0)`
    /// for a CLAP plugin, whose editor talks to it directly.
    pub fn editor_traffic(&self) -> (u64, u64) {
        (
            self.atoms.to_plugin.carried(),
            self.atoms.to_editor.carried(),
        )
    }

    /// **Drives the editor.** Call once per frame while it is open.
    ///
    /// A CLAP editor has no thread of its own: it repaints when the host fires
    /// the timer it registered, and it sees a click when the host tells it its
    /// X11 connection is readable. Both are the host's to do, and this is
    /// where. A window that opens grey and stays grey is this call missing.
    pub fn tick_editor(&mut self) {
        if let Some(editor) = &mut self.lv2_editor {
            // `false` is the editor saying it has closed itself, which LV2
            // spells as a non-zero return from `idle`.
            if !editor.tick() {
                self.close_editor();
            }
            return;
        }
        if let Inner::Bridged(plugin) = &self.inner {
            if !self.editor_open {
                return;
            }
            plugin.tick_editor();
            // Whatever the editor moved comes back off the bridge: the ABI
            // has no parameter *events*, so the wire is refreshed from the
            // plugin's own answers after every frame.
            self.reread_params();
            return;
        }
        let Some(instance) = self.clap() else {
            return;
        };
        let due = instance.access_handler_mut(|main: &mut FontelleMain| {
            main.pump.due_timers(std::time::Instant::now())
        });
        #[cfg(unix)]
        let ready = instance.access_handler_mut(|main: &mut FontelleMain| main.pump.ready_fds());
        #[cfg(not(unix))]
        let ready: Vec<()> = Vec::new();
        if due.is_empty() && ready.is_empty() {
            return;
        }
        if let Some(timer) = instance.plugin_handle().get_extension::<PluginTimer>() {
            for id in due {
                timer.on_timer(&mut instance.plugin_handle(), TimerId(id));
            }
        }
        #[cfg(unix)]
        if let Some(fds) = instance
            .plugin_handle()
            .get_extension::<clack_extensions::posix_fd::PluginPosixFd>()
        {
            for fd in ready {
                fds.on_fd(&mut instance.plugin_handle(), fd, FdFlags::READ);
            }
        }
    }

    /// What the plugin's editor has asked its window for since last time.
    ///
    /// `(resize to, show, hide, closed itself)`. Read between frames, where a
    /// window can safely be resized.
    pub fn take_editor_requests(&self) -> EditorRequests {
        use std::sync::atomic::Ordering;
        if let Some(editor) = &self.lv2_editor {
            return EditorRequests {
                resize: editor.take_resize(),
                ..EditorRequests::default()
            };
        }
        let Inner::Clap(instance) = &self.inner else {
            return EditorRequests::default();
        };
        instance.access_shared_handler(|shared: &FontelleShared| {
            let packed = shared.gui_resize.swap(0, Ordering::Acquire);
            EditorRequests {
                resize: (packed != 0).then_some(crate::gui::GuiSize {
                    width: (packed >> 32) as u32,
                    height: (packed & 0xffff_ffff) as u32,
                }),
                show: shared.gui_show.swap(false, Ordering::Acquire),
                hide: shared.gui_hide.swap(false, Ordering::Acquire),
                closed: shared.gui_closed.swap(false, Ordering::Acquire),
            }
        })
    }

    /// Tells an inactive CLAP plugin about a value now. An LV2 plugin has no
    /// instance until it is activated, and the wire is applied then.
    fn flush_param(&mut self, id: u32, value: f64) {
        if let Inner::Bridged(plugin) = &self.inner {
            plugin.set_param(id, value);
            return;
        }
        let Some(instance) = self.clap() else {
            return;
        };
        let Some(extension) = instance.plugin_handle().get_extension::<PluginParams>() else {
            return;
        };
        let Some(mut handle) = instance.inactive_plugin_handle() else {
            return;
        };
        let event = clack_host::events::event_types::ParamValueEvent::new(
            0,
            ClapId::new(id),
            Pckn::match_all(),
            value,
            Cookie::empty(),
        );
        let events = [event];
        let mut out = EventBuffer::with_capacity(8);
        extension.flush(
            &mut handle,
            &InputEvents::from_buffer(&events),
            &mut OutputEvents::from_buffer(&mut out),
        );
    }

    /// Reads every parameter's value back off the plugin onto the wire.
    fn reread_params(&mut self) {
        let ids: Vec<u32> = self.params.iter().map(|param| param.id).collect();
        let values = Arc::clone(&self.values);
        if let Inner::Bridged(plugin) = &self.inner {
            for id in ids {
                values.set(id, plugin.get_param(id));
            }
            return;
        }
        let Some(instance) = self.clap() else {
            return;
        };
        let Some(extension) = instance.plugin_handle().get_extension::<PluginParams>() else {
            return;
        };
        for id in ids {
            if let Some(value) = extension.get_value(&mut instance.plugin_handle(), ClapId::new(id))
            {
                values.set(id, value);
            }
        }
    }
}

fn read_params(instance: &mut PluginInstance<FontelleHost>) -> Vec<HostedParam> {
    let Some(extension) = instance.plugin_handle().get_extension::<PluginParams>() else {
        return Vec::new();
    };
    let count = extension.count(&mut instance.plugin_handle());
    let mut buffer = ParamInfoBuffer::new();
    let mut params = Vec::with_capacity(count as usize);
    for index in 0..count {
        let Some(info) = extension.get_info(&mut instance.plugin_handle(), index, &mut buffer)
        else {
            continue;
        };
        let id: u32 = info.id.into();
        let (min, max) = (info.min_value, info.max_value);
        // **What the plugin says it is, not what its description claims.**
        //
        // A freshly instantiated plugin is at its own defaults, so asking it
        // is asking for the default — and it is the answer to believe when
        // the two disagree. They disagree in the field: Surge XT's CLAP build
        // reports `default_value` as zero for all seven hundred and
        // seventy-five of its parameters while `get_value` returns the real
        // setting (Global Volume at -2.03 dB, Polyphony Limit at 16). Seeding
        // the wire from the description and then sending the lot — which is
        // what `activate` does — turned its global volume down to -48 dB
        // before the first block, and the synth was silent.
        //
        // `default_value` is still the fallback, for a plugin that offers no
        // `get_value` at all. See `fontelle-testplug`'s sine, which tells the
        // same lie on purpose.
        let default = extension
            .get_value(&mut instance.plugin_handle(), info.id)
            .filter(|value| value.is_finite())
            .unwrap_or(info.default_value)
            .clamp(min.min(max), max.max(min));
        params.push(HostedParam {
            id,
            name: String::from_utf8_lossy(info.name).into_owned(),
            module: String::from_utf8_lossy(info.module).into_owned(),
            min,
            max,
            default,
            stepped: info.flags.contains(ParamInfoFlags::IS_STEPPED),
            hidden: info.flags.contains(ParamInfoFlags::IS_HIDDEN),
            readonly: info.flags.contains(ParamInfoFlags::IS_READONLY),
        });
    }
    params
}

/// Every audio port a plugin declares in one direction, and which of them is
/// the main one.
///
/// **All of them, not the main one alone.** CLAP says the host passes as
/// many ports as the plugin declared, and a plugin is entitled to take that
/// at its word: nih-plug reads its auxiliary ports straight off the end of
/// whatever array it was given, so a drum synth with individual outputs
/// (the OneTrick series) handed only its main pair cleared memory at
/// address `0x13` in its first block — *"a lot of drum synth plugin keeps
/// making the daw crash"*. The main port is the one the bus is copied to
/// and from; the others are given buffers of their own, silent in and
/// dropped out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PortLayout {
    /// Channel count per port, in declared order.
    pub channels: Vec<u32>,
    /// Which of them is the main port — the one flagged `IS_MAIN`, or the
    /// first when none is.
    pub main: usize,
}

impl PortLayout {
    /// One port of `channels` channels, or no ports at all for zero — the
    /// shape LV2 and bridged plugins present, which do their own port
    /// bookkeeping.
    pub fn single(channels: u32) -> Self {
        Self {
            channels: if channels == 0 {
                Vec::new()
            } else {
                vec![channels]
            },
            main: 0,
        }
    }

    /// The main port's channel count; zero with no ports.
    pub fn main_channels(&self) -> u32 {
        self.channels.get(self.main).copied().unwrap_or(0)
    }

    /// The **sidechain**: the first port that is not the main one, if any.
    /// Only meaningful for inputs.
    pub fn key(&self) -> Option<usize> {
        (0..self.channels.len()).find(|&index| index != self.main)
    }
}

/// Every audio port the plugin declares, each way.
fn read_audio_ports(instance: &mut PluginInstance<FontelleHost>) -> (PortLayout, PortLayout) {
    let Some(extension) = instance.plugin_handle().get_extension::<PluginAudioPorts>() else {
        return (PortLayout::single(0), PortLayout::single(0));
    };
    let mut buffer = AudioPortInfoBuffer::new();
    let mut layout = |is_input: bool| {
        let ports = extension.count(&mut instance.plugin_handle(), is_input);
        let mut channels = Vec::with_capacity(ports as usize);
        let mut main = None;
        for index in 0..ports {
            // A port the plugin will not describe is still a port it
            // declared, and it will still expect a buffer at that index.
            let (count, is_main) = extension
                .get(&mut instance.plugin_handle(), index, is_input, &mut buffer)
                .map(|info| {
                    (
                        info.channel_count,
                        info.flags.contains(AudioPortFlags::IS_MAIN),
                    )
                })
                .unwrap_or((0, false));
            if is_main && main.is_none() {
                main = Some(index as usize);
            }
            channels.push(count);
        }
        PortLayout {
            channels,
            main: main.unwrap_or(0),
        }
    };
    (layout(true), layout(false))
}

/// The dialect of the plugin's first note input, or `None` when it has no
/// note input at all.
///
/// **MIDI when it is supported, whatever the plugin prefers.** The
/// preference is about notes, which are sent as CLAP's own in every case;
/// what MIDI support decides is whether a controller can be handed over as
/// the bytes it was, which is the honest mapping when the plugin will take
/// it. A port the plugin will not describe still counts as a note input, in
/// CLAP's dialect.
fn read_note_ports(instance: &mut PluginInstance<FontelleHost>) -> Option<NoteDialect> {
    let extension = instance
        .plugin_handle()
        .get_extension::<PluginNotePorts>()?;
    if extension.count(&mut instance.plugin_handle(), true) == 0 {
        return None;
    }
    let mut buffer = NotePortInfoBuffer::new();
    let dialects = extension
        .get(&mut instance.plugin_handle(), 0, true, &mut buffer)
        .map(|info| info.supported_dialects)
        .unwrap_or(NoteDialects::CLAP);
    Some(if dialects.contains(NoteDialects::MIDI) {
        NoteDialect::Midi
    } else {
        NoteDialect::Clap
    })
}
