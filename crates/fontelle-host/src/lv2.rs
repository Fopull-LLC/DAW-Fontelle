//! LV2, through lilv (ISC).
//!
//! > *"i agree with implementing LV2 for sure"*
//!
//! §8.4's order of intent was CLAP first and LV2 second, and this is the
//! second arm of the `match`. What is different about LV2, and what this
//! module absorbs so that nothing above it has to know:
//!
//! - **A bundle is a folder.** The shared library sits beside Turtle files
//!   that describe it, and everything a menu shows — name, author, ports,
//!   ranges — is read from the Turtle by lilv, *not* from the binary. A scan
//!   does not `dlopen` anything.
//! - **A parameter is a control port**, one `f32` the plugin reads on every
//!   `run`. Its id here is the port's **index**, which the specification
//!   makes stable for the life of a plugin — INVARIANT 7 from the other side
//!   of the boundary again. There are no parameter events: a knob's value
//!   is written into the port before the block.
//! - **Notes are MIDI bytes in an atom sequence**, stamped with the frame
//!   they land on, and the plugin learns which atoms are MIDI through the
//!   `urid:map` feature the host provides.
//! - **There is one object, not two.** CLAP splits a plugin into a
//!   main-thread half and an audio half; an LV2 instance is one handle whose
//!   `run` belongs to the audio thread and whose ports may be written from
//!   anywhere. So the whole instance rides in the [`HostedProcessor`] and
//!   the main-thread [`HostedPlugin`] keeps only the description and the
//!   wire — which is why an LV2 plugin is instantiated at `activate` and
//!   freed at `deactivate`, rather than at open and close.
//! - **State is on the instance**, which is in the processor. `state:interface`
//!   is `extension_data` of the running instance, and LV2 forbids calling it
//!   while `run` executes — so an LV2 plugin's own state is read *with* its
//!   processor in hand ([`crate::HostedPlugin::snapshot_with`]), fetched from
//!   a playing graph through [`crate::ProcessorBay::recall`]; and a state the
//!   document hands a plugin before it runs is kept and applied the moment
//!   the instance exists, in [`Lv2Plugin::activate`]. See [`crate::lv2_state`]
//!   for the blob and the path rule.
//!
//! # One world per bundle
//!
//! lilv's `World` is the parsed Turtle. The obvious host makes one and asks
//! it to load everything on `LV2_PATH`; this host makes one **per bundle**
//! and loads only that bundle into it, because the folders Fontelle searches
//! are the user's (`Settings::plugin_dirs`) and not an environment variable
//! — and an environment variable is process-wide, which a test suite running
//! on sixteen threads cannot safely set. The price is that the LV2
//! specification bundles are not loaded alongside, so anything read here is
//! read off the plugin's *own* data (its `rdf:type`s, its ports' classes and
//! properties) rather than resolved through the class tree. That is enough
//! for a scanner and a host, and it holds for a bundle sitting anywhere.
//!
//! # Where the worker thread comes from
//!
//! livi's feature set carries the `work:schedule` feature — the thread a
//! sampler loads files on — and starts one thread per feature set. One set
//! is built per [`PluginHost`] and shared by every LV2 plugin it opens, so
//! there is one such thread, not one per instance per graph rebuild.
//!
//! [`HostedPlugin`]: crate::HostedPlugin
//! [`HostedProcessor`]: crate::HostedProcessor
//! [`PluginHost`]: crate::PluginHost

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};

use fontelle_types::{PluginFormat, PluginKey};
use livi::event::LV2AtomSequence;
use livi::{Features, FeaturesBuilder, PortIndex, PortType};

use crate::param::{HostedParam, ParamValues};
use crate::plugin::HostError;
use crate::scan::PluginInfo;

/// The largest block an LV2 plugin is told to expect.
///
/// The feature set is built once per host and tells every plugin the
/// bounds; an `activate` asking for more than this is refused rather than
/// lied to, because a plugin that sized its buffers from the option would
/// overrun them.
pub const LV2_MAX_BLOCK: usize = 8192;

/// How many bytes of events one block may carry into a plugin.
///
/// A MIDI note is thirty-two bytes with its atom header and padding, so this
/// is about a hundred and twenty of them — the same order as
/// `processor::MAX_EVENTS`, and for the same reason: sized once, never
/// grown on the audio thread.
const ATOM_CAPACITY: usize = 4096;

const LV2_CORE: &str = "http://lv2plug.in/ns/lv2core#";
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const MIDI_EVENT: &str = "http://lv2plug.in/ns/ext/midi#MidiEvent";
const NOT_ON_GUI: &str = "http://lv2plug.in/ns/ext/port-props#notOnGUI";
/// The port property that makes an audio input a **sidechain**.
const IS_SIDE_CHAIN: &str = "http://lv2plug.in/ns/lv2core#isSideChain";

/// The `file:` URI lilv wants for a bundle folder — absolute, with the
/// trailing slash the specification insists on, and percent-encoded.
///
/// By hand rather than through `lilv_new_file_uri`, which would need a
/// world to make the node in before there is a world to load the bundle
/// into. RFC 3986's unreserved set is kept and everything else is escaped,
/// which is what lilv does for the same path.
pub(crate) fn bundle_uri(path: &Path) -> Result<String, String> {
    let absolute = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut uri = String::from("file://");
    for byte in absolute.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(byte as char);
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    if !uri.ends_with('/') {
        uri.push('/');
    }
    Ok(uri)
}

/// Parses one bundle into a world of its own. See the module note on why
/// not one world for everything.
pub(crate) fn load_world(path: &Path) -> Result<livi::World, String> {
    if !path.is_dir() {
        return Err("an LV2 bundle is a folder".to_string());
    }
    Ok(livi::World::with_load_bundle(&bundle_uri(path)?))
}

/// Everything one LV2 bundle holds. Used by the scanner.
///
/// A folder with the extension and no `manifest.ttl` holds no plugins and
/// is answered without a world being made — lilv would answer the same and
/// print an error on the way, and a plugin folder somebody is still
/// assembling is not an error.
pub(crate) fn read_lv2_bundle(path: &Path) -> Result<Vec<PluginInfo>, String> {
    if !path.is_dir() {
        return Err("an LV2 bundle is a folder".to_string());
    }
    if !path.join("manifest.ttl").is_file() {
        return Ok(Vec::new());
    }
    let world = load_world(path)?;
    Ok(world
        .iter_plugins()
        .map(|plugin| describe(path, &world, &plugin))
        .collect())
}

/// What a menu needs, read off the plugin's own Turtle.
pub(crate) fn describe(path: &Path, world: &livi::World, plugin: &livi::Plugin) -> PluginInfo {
    let raw = plugin.raw();
    let lilv = world.raw();
    let vendor = raw
        .author_name()
        .and_then(|node| node.as_str().map(str::to_string))
        .unwrap_or_default();
    let int_of = |predicate: &str| {
        raw.value(&lilv.new_uri(&format!("{LV2_CORE}{predicate}")))
            .iter()
            .next()
            .and_then(|node| node.as_int())
    };
    let version = match (int_of("minorVersion"), int_of("microVersion")) {
        (Some(minor), Some(micro)) => format!("{minor}.{micro}"),
        (Some(minor), None) => minor.to_string(),
        _ => String::new(),
    };
    // The plugin's `rdf:type`s, by their fragment: "Plugin",
    // "InstrumentPlugin", "AmplifierPlugin". Read off the data rather than
    // the class tree — see the module note.
    let classes: Vec<String> = raw
        .value(&lilv.new_uri(RDF_TYPE))
        .iter()
        .filter_map(|node| {
            node.as_uri()
                .map(|uri| uri.rsplit(['#', '/']).next().unwrap_or(uri).to_string())
        })
        .collect();
    let counts = plugin.port_counts();
    // What it is, in the words the CLAP scanner uses so that
    // `PluginInfo::is_instrument` is one question. A plugin that does not
    // call itself an instrument but takes notes and no audio is one anyway:
    // plenty of synths declare only `lv2:Plugin`.
    let plays_notes = counts.atom_sequence_inputs > 0 && counts.audio_outputs > 0;
    let mut features = vec![if classes.iter().any(|c| c == "InstrumentPlugin")
        || (plays_notes && counts.audio_inputs == 0)
    {
        "instrument".to_string()
    } else if classes.iter().any(|c| c == "AnalyserPlugin") {
        "analyzer".to_string()
    } else {
        "audio-effect".to_string()
    }];
    features.extend(
        classes
            .iter()
            .filter(|c| c.as_str() != "Plugin")
            .map(|c| format!("lv2:{c}")),
    );
    PluginInfo {
        key: PluginKey::new(PluginFormat::Lv2, plugin.uri()),
        path: path.to_path_buf(),
        name: plugin.name(),
        vendor,
        version,
        features,
    }
}

/// The main-thread half of an LV2 plugin: the description and the means to
/// instantiate it. The instance itself lives in [`Lv2Processor`].
pub(crate) struct Lv2Plugin {
    plugin: livi::Plugin,
    features: Arc<Features>,
    /// The URID the shared feature set gave `midi:MidiEvent`, so a note
    /// can be stamped as one.
    midi_urid: u32,
    /// Whether any atom input says it takes MIDI.
    accepts_notes: bool,
    /// Whether the Turtle declares `state:interface` — read before there is
    /// an instance to ask, because that is when the rack asks.
    keeps_state: bool,
    /// The state the document handed this plugin, kept until an instance
    /// exists to give it to — and afterwards, so that a plugin which has not
    /// run yet (or has stopped) still answers a snapshot with what it was.
    /// Refreshed from the instance by `HostedPlugin::deactivate`.
    pending_state: Option<Vec<u8>>,
    /// Which buffer each audio input port connects to, in port order — see
    /// [`open`], and [`Lv2Processor::input`] for why the main ones come
    /// first.
    input_order: Vec<usize>,
    /// How many of them are the main input; the rest are the sidechain.
    main_inputs: usize,
    /// The running instance's own `LV2_Handle`, while there is one, or null.
    ///
    /// What an editor asking for **`instance-access`** is handed. Written by
    /// [`activate`](Self::activate), cleared when that [`Lv2Processor`] is
    /// dropped — and an editor is closed before the instance it was handed
    /// can go (see `HostedPlugin::activate`), because the pointer the UI
    /// took at instantiate cannot be revoked afterwards.
    instance: Arc<AtomicPtr<c_void>>,
}

/// What `PluginHost::open` needs back for an LV2 plugin.
pub(crate) struct Opened {
    pub info: PluginInfo,
    pub params: Vec<HostedParam>,
    /// The **main** input's channels — every audio input that is not a
    /// sidechain. What `HostedPlugin::audio_inputs` reports.
    pub audio_inputs: u32,
    pub audio_outputs: u32,
    /// The main input and, when the Turtle declares one, the sidechain —
    /// the shape a CLAP plugin with a second input port presents, so that
    /// nothing above the host has to know which format declared its key
    /// with a port flag and which with a port property.
    pub input_ports: crate::plugin::PortLayout,
    pub accepts_notes: bool,
    pub keeps_state: bool,
    pub plugin: Lv2Plugin,
}

/// Reads one plugin out of a loaded world.
pub(crate) fn open(
    path: &Path,
    key: &PluginKey,
    world: &livi::World,
    features: &Arc<Features>,
) -> Result<Opened, HostError> {
    let plugin = world
        .plugin_by_uri(&key.id)
        .ok_or_else(|| HostError::NoSuchPlugin {
            key: key.clone(),
            path: path.to_path_buf(),
        })?;
    let info = describe(path, world, &plugin);
    let lilv = world.raw();
    let property = |name: &str| lilv.new_uri(&format!("{LV2_CORE}{name}"));
    let (toggled, integer, enumeration) = (
        property("toggled"),
        property("integer"),
        property("enumeration"),
    );
    let hidden = lilv.new_uri(NOT_ON_GUI);
    let params = plugin
        .ports_with_type(PortType::ControlInput)
        .map(|port| {
            let raw_port = plugin.raw().port_by_index(port.index.0);
            let has = |node: &livi::lilv::node::Node| {
                raw_port.as_ref().is_some_and(|p| p.has_property(node))
            };
            let (min, max) = (port.min_value.unwrap_or(0.0), port.max_value.unwrap_or(1.0));
            HostedParam {
                id: port.index.0 as u32,
                name: port.name.clone(),
                module: String::new(),
                min: f64::from(min),
                max: f64::from(max),
                default: f64::from(port.default_value),
                stepped: has(&toggled) || has(&integer) || has(&enumeration),
                hidden: has(&hidden),
                readonly: false,
            }
        })
        .collect();
    let midi = lilv.new_uri(MIDI_EVENT);
    let accepts_notes = plugin
        .ports_with_type(PortType::AtomSequenceInput)
        .any(|port| {
            plugin
                .raw()
                .port_by_index(port.index.0)
                .is_some_and(|p| p.supports_event(&midi))
        });
    let counts = *plugin.port_counts();
    let midi_urid = features.midi_urid();
    let keeps_state = crate::lv2_state::declares_interface(&plugin, world);
    // **Which inputs are the sidechain.** LV2 says so with a port property,
    // `lv2:isSideChain`, on an audio input; the host feeds those the key and
    // the rest the bus. `input_order` is the buffer each audio input port is
    // connected to, in port order — main ports first, then the key — so the
    // main input is one contiguous run of buffers whatever order the plugin
    // declared its ports in.
    let side_chain = lilv.new_uri(IS_SIDE_CHAIN);
    let is_key: Vec<bool> = plugin
        .ports_with_type(PortType::AudioInput)
        .map(|port| {
            plugin
                .raw()
                .port_by_index(port.index.0)
                .is_some_and(|p| p.has_property(&side_chain))
        })
        .collect();
    let main_inputs = is_key.iter().filter(|key| !**key).count();
    let key_inputs = is_key.len() - main_inputs;
    let (mut next_main, mut next_key) = (0, main_inputs);
    let input_order: Vec<usize> = is_key
        .iter()
        .map(|&key| {
            let slot = if key { &mut next_key } else { &mut next_main };
            let index = *slot;
            *slot += 1;
            index
        })
        .collect();
    let mut input_ports = crate::plugin::PortLayout::single(main_inputs as u32);
    if key_inputs > 0 {
        input_ports.channels.push(key_inputs as u32);
    }
    Ok(Opened {
        info,
        params,
        audio_inputs: main_inputs as u32,
        audio_outputs: counts.audio_outputs as u32,
        input_ports,
        accepts_notes,
        keeps_state,
        plugin: Lv2Plugin {
            plugin,
            features: Arc::clone(features),
            midi_urid,
            accepts_notes,
            keeps_state,
            pending_state: None,
            instance: Arc::new(AtomicPtr::new(std::ptr::null_mut())),
            input_order,
            main_inputs,
        },
    })
}

/// The feature set every LV2 plugin of one host shares.
pub(crate) fn build_features(world: &livi::World) -> Arc<Features> {
    world.build_features(FeaturesBuilder {
        min_block_length: 1,
        max_block_length: LV2_MAX_BLOCK,
    })
}

impl Lv2Plugin {
    /// The plugin's atom input and output ports, by index.
    ///
    /// What an editor's atoms are addressed to and where the plugin's come
    /// from — see [`crate::atom::AtomPipes`]. The **first** of each, which is
    /// the convention every LV2 host follows.
    pub(crate) fn atom_ports(&self) -> (Option<u32>, Option<u32>) {
        let first = |kind| {
            self.plugin
                .ports_with_type(kind)
                .map(|port| port.index.0 as u32)
                .next()
        };
        (
            first(PortType::AtomSequenceInput),
            first(PortType::AtomSequenceOutput),
        )
    }

    /// Whether this plugin ships an editor of its own this host can show.
    pub(crate) fn has_editor(&self) -> bool {
        crate::lv2_ui::find_x11_ui(&self.plugin).is_some()
    }

    /// Loads and starts that editor into `window`.
    pub(crate) fn open_editor(
        &self,
        plugin_uri: &str,
        bundle: &std::path::Path,
        values: Arc<ParamValues>,
        atoms: Arc<crate::atom::AtomPipes>,
        params: &[HostedParam],
        window: &crate::gui::PluginWindow,
    ) -> Result<crate::lv2_ui::Lv2Ui, crate::gui::GuiError> {
        let info =
            crate::lv2_ui::find_x11_ui(&self.plugin).ok_or(crate::gui::GuiError::NoEditor)?;
        // Every control port, because that is what a UI is told about and
        // what it writes back — an LV2 parameter *is* a control port index.
        let ports: Vec<u32> = params.iter().map(|param| param.id).collect();
        crate::lv2_ui::Lv2Ui::open(
            &info,
            plugin_uri,
            bundle,
            &self.features,
            values,
            atoms,
            &ports,
            window,
            self.instance.load(Ordering::Acquire),
        )
    }

    /// Instantiates and activates the plugin. See the module note on why
    /// this happens here and not at open.
    pub(crate) fn activate(
        &self,
        key: &PluginKey,
        values: Arc<ParamValues>,
        atoms: Arc<crate::atom::AtomPipes>,
        sample_rate: f64,
        max_block: usize,
    ) -> Result<Lv2Processor, HostError> {
        if max_block > LV2_MAX_BLOCK {
            return Err(HostError::Activate {
                key: key.clone(),
                why: format!(
                    "a block of {max_block} is over the {LV2_MAX_BLOCK} LV2 plugins were told to expect"
                ),
            });
        }
        // SAFETY: this runs the plugin's own `instantiate`, which is somebody
        // else's code; there is no safe version of that, and the features it
        // is handed are the ones its Turtle asked for.
        let instance = unsafe {
            self.plugin
                .instantiate(Arc::clone(&self.features), sample_rate)
        }
        .map_err(|e| HostError::Activate {
            key: key.clone(),
            why: format!("{e:?}"),
        })?;
        let counts = *self.plugin.port_counts();
        // **What the document remembers, before the first block.** A fresh
        // instance is the plugin at its own defaults; the state it was handed
        // (a sampler's file) goes in now, while nothing is running it, which
        // is the one moment LV2 lets a host call `restore` on any plugin at
        // all. A state the plugin will not take is skipped rather than
        // refused — a sampler with no file is closer to the song than no
        // sampler.
        if let Some(bytes) = &self.pending_state
            && let Some(state) = crate::lv2_state::Lv2State::decode(bytes)
        {
            // SAFETY: the instance exists and has never run; see above.
            let _ = unsafe {
                crate::lv2_state::restore(instance.raw().instance(), &self.features, &state)
            };
        }
        // The handle an editor can be given — see `Lv2Plugin::instance`.
        self.instance
            .store(instance.raw().instance().handle(), Ordering::Release);
        let mut processor = Lv2Processor {
            instance,
            handed_out: Arc::clone(&self.instance),
            _world: self.plugin.clone(),
            features: Arc::clone(&self.features),
            values,
            atoms,
            atom_in: (0..counts.atom_sequence_inputs)
                .map(|_| LV2AtomSequence::new(&self.features, ATOM_CAPACITY))
                .collect(),
            atom_out: (0..counts.atom_sequence_outputs)
                .map(|_| LV2AtomSequence::new(&self.features, ATOM_CAPACITY))
                .collect(),
            input: vec![vec![0.0; max_block]; counts.audio_inputs],
            input_order: self.input_order.clone(),
            main_inputs: self.main_inputs,
            output: vec![vec![0.0; max_block]; counts.audio_outputs],
            cv_in: vec![vec![0.0; max_block]; counts.cv_inputs],
            cv_out: vec![vec![0.0; max_block]; counts.cv_outputs],
            midi_urid: self.midi_urid,
            takes_notes: self.accepts_notes,
            max_block,
        };
        // Every control port starts at the plugin's default; the wire
        // carries the document's answer and is applied on the first block.
        for (id, value) in processor.values.all() {
            processor
                .instance
                .set_control_input(PortIndex(id as usize), value as f32);
        }
        Ok(processor)
    }
}

impl Lv2Plugin {
    pub(crate) fn keeps_state(&self) -> bool {
        self.keeps_state
    }

    /// What the document last handed this plugin, or what was read off the
    /// instance when it stopped. See the field.
    pub(crate) fn pending_state(&self) -> Option<&[u8]> {
        self.pending_state.as_deref()
    }

    /// Keeps `bytes` for the next instance. `false` for a plugin that
    /// declares no state interface: there is nothing to give it to.
    pub(crate) fn stash_state(&mut self, bytes: &[u8]) -> bool {
        if !self.keeps_state {
            return false;
        }
        self.pending_state = Some(bytes.to_vec());
        true
    }
}

impl Drop for Lv2Processor {
    fn drop(&mut self) {
        // Only if it is still this one: a newer instance may already have
        // put its own handle there, and that one is live.
        let mine = self.instance.raw().instance().handle();
        let _ = self.handed_out.compare_exchange(
            mine,
            std::ptr::null_mut(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// The audio half of an LV2 plugin — which, for LV2, is the whole plugin.
pub(crate) struct Lv2Processor {
    instance: livi::Instance,
    /// Where the instance's handle was published for editors — cleared on
    /// drop, so a later editor is not handed a freed instance.
    handed_out: Arc<AtomicPtr<c_void>>,
    /// The plugin this came from, kept **only** to keep its world alive.
    ///
    /// A lilv instance holds no reference to the world that loaded its
    /// library, and the world `dlclose`s that library when it is freed — so
    /// a processor parked in a graph while the host that opened it has been
    /// dropped would call `cleanup` into unmapped code. The plugin handle
    /// carries the world's lifetime, and this field is what makes the
    /// processor outlive-safe whatever order things are dropped in. Found
    /// the hard way: `PluginRack` drops its host before its parked
    /// processors, and did so with a segfault.
    _world: livi::Plugin,
    /// The shared feature set, for the URID map the state interface names
    /// its keys through — see [`crate::lv2_state`].
    features: Arc<livi::Features>,
    values: Arc<ParamValues>,
    /// The editor's end of the atom conversation — see [`crate::atom`].
    atoms: Arc<crate::atom::AtomPipes>,
    /// One sequence per atom input; the notes go into the **first**, which
    /// is the convention every LV2 host follows. The others are handed empty
    /// sequences, which is what a port that must be connected needs.
    atom_in: Vec<LV2AtomSequence>,
    atom_out: Vec<LV2AtomSequence>,
    /// One buffer per audio input port: the **main** ones first, then the
    /// sidechain, whatever order the Turtle declared them in.
    /// `input_order` says which buffer each port gets — see [`Lv2Plugin`].
    input: Vec<Vec<f32>>,
    input_order: Vec<usize>,
    main_inputs: usize,
    output: Vec<Vec<f32>>,
    cv_in: Vec<Vec<f32>>,
    cv_out: Vec<Vec<f32>>,
    midi_urid: u32,
    takes_notes: bool,
    max_block: usize,
}

impl Lv2Processor {
    pub(crate) fn max_block(&self) -> usize {
        self.max_block
    }

    /// The **main** input's buffers — where the bus goes. The sidechain, if
    /// there is one, is behind them and written by [`fill_key`](Self::fill_key).
    pub(crate) fn input(&mut self) -> &mut [Vec<f32>] {
        let main = self.main_inputs.min(self.input.len());
        &mut self.input[..main]
    }

    /// Puts `key` on every channel of the sidechain, or silence when there
    /// is none to put — **every block**, so a key handed over once does not
    /// go on ducking after the tap it came from is gone. Nothing to do on a
    /// plugin that declared no `lv2:isSideChain` port.
    pub(crate) fn fill_key(&mut self, key: Option<&[f32]>, frames: usize) {
        let main = self.main_inputs.min(self.input.len());
        for channel in &mut self.input[main..] {
            let frames = frames.min(channel.len());
            match key {
                Some(key) => {
                    let taken = frames.min(key.len());
                    channel[..taken].copy_from_slice(&key[..taken]);
                    channel[taken..frames].fill(0.0);
                }
                None => channel[..frames].fill(0.0),
            }
        }
    }

    pub(crate) fn output(&self) -> &[Vec<f32>] {
        &self.output
    }

    /// **Main thread, with the processor in hand.** The plugin's own state,
    /// encoded — see [`crate::lv2_state`]. `None` when it keeps none.
    ///
    /// Not RT: this runs the plugin's `save`, which may read files and
    /// allocate, and the specification forbids it while `run` executes.
    /// Having `&mut self` is what guarantees no `run` is in progress: the
    /// processor is either in a node on the audio thread or here, never both.
    pub(crate) fn save_state(&mut self) -> Option<Vec<u8>> {
        // SAFETY: `&mut self` — see above.
        let state =
            unsafe { crate::lv2_state::save(self.instance.raw().instance(), &self.features) }?;
        Some(state.encode())
    }

    /// **RT.** A MIDI message at `frame`. Dropped, not grown, when the block
    /// is full — see [`ATOM_CAPACITY`].
    fn push(&mut self, frame: usize, bytes: [u8; 3]) {
        if !self.takes_notes {
            return;
        }
        if let Some(first) = self.atom_in.first_mut() {
            let _ = first.push_midi_event::<3>(frame as i64, self.midi_urid, &bytes);
        }
    }

    pub(crate) fn note_on(&mut self, frame: usize, key: u8, velocity: f64) {
        let velocity = (velocity.clamp(0.0, 1.0) * 127.0).round() as u8;
        self.push(frame, [0x90, key.min(127), velocity.max(1)]);
    }

    pub(crate) fn note_off(&mut self, frame: usize, key: u8) {
        self.push(frame, [0x80, key.min(127), 0]);
    }

    /// A controller, as the MIDI it was. Channel 0, like the notes.
    pub(crate) fn controller(&mut self, frame: usize, controller: u8, value: u8) {
        self.push(frame, [0xB0, controller.min(127), value.min(127)]);
    }

    /// A bend, centred at zero, back into the fourteen bits MIDI carries.
    pub(crate) fn pitch_bend(&mut self, frame: usize, value: i16) {
        let raw = (i32::from(value.clamp(-8192, 8191)) + 8192) as u16;
        self.push(frame, [0xE0, (raw & 0x7F) as u8, ((raw >> 7) & 0x7F) as u8]);
    }

    pub(crate) fn channel_pressure(&mut self, frame: usize, value: u8) {
        self.push(frame, [0xD0, value.min(127), 0]);
    }

    /// **RT.** LV2 has no reset call. What every host sends instead is
    /// "all sound off" and "all notes off" at the top of the next block —
    /// after clearing whatever was queued, so the two land first and the
    /// sequence stays in time order.
    pub(crate) fn reset(&mut self) {
        for sequence in &mut self.atom_in {
            sequence.clear();
        }
        self.push(0, [0xB0, 120, 0]);
        self.push(0, [0xB0, 123, 0]);
    }

    /// **RT.** One block. The bus copies happen around this in
    /// `HostedProcessor`, which is shared with the CLAP arm.
    pub(crate) fn run(&mut self, frames: usize) {
        let frames = frames.min(self.max_block);
        self.atoms
            .runs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Whatever moved since the last block. A control port is a float the
        // plugin reads during `run`, so writing it now is the whole of a
        // parameter change — there is no event to build.
        let instance = &mut self.instance;
        self.values.drain(|id, value| {
            instance.set_control_input(PortIndex(id as usize), value as f32);
        });

        let Self {
            instance,
            atom_in,
            atom_out,
            input,
            input_order,
            output,
            cv_in,
            cv_out,
            ..
        } = self;
        let ports = livi::EmptyPortConnections::new()
            // In **port order**, which is how livi connects them; the
            // buffers themselves are kept main-first — see `input`.
            .with_audio_inputs(input_order.iter().map(|&index| &input[index][..frames]))
            .with_audio_outputs(output.iter_mut().map(|b| &mut b[..frames]))
            .with_atom_sequence_inputs(atom_in.iter())
            .with_atom_sequence_outputs(atom_out.iter_mut())
            .with_cv_inputs(cv_in.iter().map(|b| &b[..frames]))
            .with_cv_outputs(cv_out.iter_mut().map(|b| &mut b[..frames]));
        // SAFETY: runs the plugin's `run` with every port connected to a
        // buffer at least `frames` long, which is the contract. A refusal
        // (a port count that does not match) leaves the output as it was,
        // which for a block that was just filled with input is pass-through.
        let _ = unsafe { instance.run(frames, ports) };

        // **What the plugin said**, on its way to the editor. Read before the
        // input sequences are cleared, because both are this block's.
        if let Some(first) = atom_out.first() {
            let pipe = &self.atoms.to_editor;
            for event in first.iter() {
                pipe.push(event.event.body.mytype, event.data);
            }
        }

        for sequence in atom_in.iter_mut() {
            sequence.clear();
        }
        // **And what the editor said**, into the sequence the *next* block
        // will read. Filled here rather than at the top of the next `run`
        // because notes are added before a block runs, and an atom appended
        // after a note at frame 100 would put the sequence out of time order —
        // which an LV2 plugin is entitled to stop reading at. Written into the
        // sequence the instant it is emptied, they sit at frame zero ahead of
        // every note the next block brings.
        //
        // The cost is one block of latency on a message that says "load this
        // file", which is 2.7 ms at 128 frames and 48 kHz.
        if let Some(first) = atom_in.first_mut() {
            self.atoms.to_plugin.drain(|urid, body| {
                let Ok(event) =
                    livi::event::LV2AtomEventBuilder::<{ crate::atom::MAX_ATOM_BYTES }>::new(
                        0, urid, body,
                    )
                else {
                    return;
                };
                let _ = first.push_event(&event);
            });
        }
    }
}

/// The folders LV2 nominates on this platform, for [`crate::search_paths`].
pub(crate) fn search_paths(home: Option<&PathBuf>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(from_env) = std::env::var_os("LV2_PATH") {
        paths.extend(std::env::split_paths(&from_env).filter(|path| path.is_absolute()));
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = home {
            paths.push(home.join(".lv2"));
        }
        paths.push(PathBuf::from("/usr/lib/lv2"));
        paths.push(PathBuf::from("/usr/local/lib/lv2"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = home {
            paths.push(home.join("Library/Audio/Plug-Ins/LV2"));
        }
        paths.push(PathBuf::from("/Library/Audio/Plug-Ins/LV2"));
    }
    #[cfg(target_os = "windows")]
    {
        let _ = home;
        if let Some(appdata) = std::env::var_os("APPDATA") {
            paths.push(PathBuf::from(appdata).join("LV2"));
        }
        if let Some(common) = std::env::var_os("COMMONPROGRAMFILES") {
            paths.push(PathBuf::from(common).join("LV2"));
        }
    }
    paths
}
