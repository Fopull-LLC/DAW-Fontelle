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

use std::ffi::{CStr, c_void};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, Ordering};

use fontelle_types::{PluginFormat, PluginKey};
use livi::event::LV2AtomSequence;
use livi::{FeaturesBuilder, PortIndex, PortType};
// The two the host keeps per bundle, named through this module so the
// stub can stand in for them — see `lv2_stub.rs`.
pub(crate) use livi::{Features, World};

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

/// The most an atom port is given, whatever it asks for: a plugin that
/// wants more than this is moving whole files through a port, and a port
/// is allocated per instance whether or not anything is ever sent.
const ATOM_CAPACITY_MAX: usize = 64 * 1024 * 1024;

const MINIMUM_SIZE: &str = "http://lv2plug.in/ns/ext/resize-port#minimumSize";
const TIME_POSITION: &str = "http://lv2plug.in/ns/ext/time#Position";

/// A `time:Position`'s body: id and type, and seven properties of at most
/// twenty-four bytes each.
const TIME_BODY: usize = 8 + 7 * 24;

/// The song's position, written as a `time:Position` object at the top of
/// every block into the event input of a plugin that supports it.
///
/// > *"most times it just renders with nothing"* — and the sweep that went
/// > looking: Vaporizer2's LV2 asserted on a host position that never came,
/// > and 274 of the bundles on the devbox say they want one. Without it a
/// > tempo-synced LFO or arpeggiator runs at whatever it assumed.
///
/// **Ahead of the block's notes.** They are queued before `run`, and a
/// sequence is in time order; so the position is written into a scratch
/// sequence and the notes copied after it, all into buffers sized when the
/// plugin was activated — nothing allocates on the audio thread.
struct TimeWriter {
    transport: crate::PluginTransport,
    rate: f64,
    scratch: LV2AtomSequence,
    urids: [u32; 12],
}

impl TimeWriter {
    fn new(features: &Arc<Features>, capacity: usize, rate: f64) -> Self {
        let urid = |uri: &str| {
            let uri = std::ffi::CString::new(uri).expect("a URI has no NUL");
            features.urid(&uri)
        };
        const TIME: &str = "http://lv2plug.in/ns/ext/time#";
        const ATOM: &str = "http://lv2plug.in/ns/ext/atom#";
        Self {
            transport: crate::PluginTransport::default(),
            rate,
            scratch: LV2AtomSequence::new(features, capacity),
            urids: [
                urid(&format!("{ATOM}Object")),
                urid(TIME_POSITION),
                urid(&format!("{TIME}frame")),
                urid(&format!("{TIME}speed")),
                urid(&format!("{TIME}bar")),
                urid(&format!("{TIME}barBeat")),
                urid(&format!("{TIME}beatUnit")),
                urid(&format!("{TIME}beatsPerBar")),
                urid(&format!("{TIME}beatsPerMinute")),
                urid(&format!("{ATOM}Long")),
                urid(&format!("{ATOM}Float")),
                urid(&format!("{ATOM}Int")),
            ],
        }
    }

    /// **RT.** Puts this block's position at frame zero of `sequence`, ahead
    /// of whatever is already in it.
    fn put_ahead_of(&mut self, sequence: &mut LV2AtomSequence) {
        let [
            object,
            position,
            frame,
            speed,
            bar,
            bar_beat,
            beat_unit,
            beats_per_bar,
            bpm,
            long,
            float,
            int,
        ] = self.urids;
        let t = self.transport;
        // An object's body: its id and type, then each property as a key, a
        // context, and an atom whose body is padded to eight bytes.
        let mut body = [0u8; TIME_BODY];
        let mut at = 0;
        let mut put = |bytes: &[u8]| {
            body[at..at + bytes.len()].copy_from_slice(bytes);
            at += bytes.len();
        };
        put(&0u32.to_ne_bytes());
        put(&position.to_ne_bytes());
        let mut property = |key: u32, kind: u32, value: &[u8]| {
            put(&key.to_ne_bytes());
            put(&0u32.to_ne_bytes());
            put(&(value.len() as u32).to_ne_bytes());
            put(&kind.to_ne_bytes());
            put(value);
            put(&[0u8; 8][..(8 - value.len() % 8) % 8]);
        };
        property(
            frame,
            long,
            &((t.seconds * self.rate).round() as i64).to_ne_bytes(),
        );
        property(
            speed,
            float,
            &(if t.playing { 1.0f32 } else { 0.0 }).to_ne_bytes(),
        );
        property(bar, long, &i64::from(t.bar_number).to_ne_bytes());
        property(
            bar_beat,
            float,
            &((t.beats - t.bar_start_beats) as f32).to_ne_bytes(),
        );
        property(
            beat_unit,
            int,
            &i32::from(t.denominator.max(1)).to_ne_bytes(),
        );
        property(
            beats_per_bar,
            float,
            &f32::from(t.numerator.max(1)).to_ne_bytes(),
        );
        property(bpm, float, &(t.tempo as f32).to_ne_bytes());
        let used = at;
        let Ok(event) =
            livi::event::LV2AtomEventBuilder::<TIME_BODY>::new(0, object, &body[..used])
        else {
            return;
        };
        self.scratch.clear();
        if self.scratch.push_event(&event).is_err() {
            return;
        }
        // The notes after it, as they are: the bytes of the events, which
        // already carry their frames and their padding.
        // SAFETY: both buffers are livi sequences — a 16-byte header, then
        // events — and the copy is bounded by the scratch's capacity.
        unsafe {
            let from = sequence.as_ptr();
            let events = ((*from).atom.size as usize).saturating_sub(8);
            let to = self.scratch.as_mut_ptr();
            let used = (*to).atom.size as usize;
            if 8 + used + events > self.scratch.capacity() + 8 {
                return;
            }
            let source = (from as *const u8).add(16);
            let target = (to as *mut u8).add(8 + used);
            std::ptr::copy_nonoverlapping(source, target, events);
            (*to).atom.size += events as u32;
        }
        std::mem::swap(sequence, &mut self.scratch);
    }
}

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
    // Sorted: lilv hands a plugin's classes back in an order that changes
    // from one run to the next, and a scan that differs every time it is
    // made cannot be compared with the one before it.
    let mut classes: Vec<String> = classes
        .iter()
        .filter(|c| c.as_str() != "Plugin")
        .map(|c| format!("lv2:{c}"))
        .collect();
    classes.sort();
    classes.dedup();
    features.extend(classes);
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
    /// Each atom input's and output's buffer size, in port order — see
    /// `open`.
    atom_in_sizes: Vec<usize>,
    atom_out_sizes: Vec<usize>,
    /// Whether its event input supports `time:Position`.
    wants_time: bool,
    /// The port designated `lv2:freeWheeling`, if it has one — see `open`.
    free_wheeling: Option<u32>,
    /// The output port it says its latency on, if it has one — see `open`.
    latency_port: Option<u32>,
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
    // **The host's own port.** One designated `lv2:freeWheeling` says
    // whether the host is rendering faster than real time; the host sets it
    // every block (see `Lv2Processor::run`), so it is no parameter for a
    // person to move or a document to keep.
    let free_wheeling = plugin
        .raw()
        .port_by_designation(Some(&property("InputPort")), &property("freeWheeling"))
        .map(|port| port.index() as u32);
    // **What it delays by**: an output control port designated
    // `lv2:latency` — or, from before designations, one with the
    // `lv2:reportsLatency` property — which the plugin writes in `run`.
    let reports_latency = property("reportsLatency");
    let latency_port = plugin
        .raw()
        .port_by_designation(Some(&property("OutputPort")), &property("latency"))
        .map(|port| port.index() as u32)
        .or_else(|| {
            plugin
                .ports_with_type(PortType::ControlOutput)
                .find(|port| {
                    plugin
                        .raw()
                        .port_by_index(port.index.0)
                        .is_some_and(|p| p.has_property(&reports_latency))
                })
                .map(|port| port.index.0 as u32)
        });
    let params = plugin
        .ports_with_type(PortType::ControlInput)
        .filter(|port| Some(port.index.0 as u32) != free_wheeling)
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
    // **Each atom port as big as the plugin said it needs.** LV2's
    // resize-port extension lets a port declare `rsz:minimumSize`, and a
    // plugin that does may write that far without asking: Vaporizer2 says
    // 62,552 bytes, LSP's file ports several megabytes. Every atom port used
    // to be [`ATOM_CAPACITY`] bytes, and such a plugin overran the heap.
    let minimum_size = lilv.new_uri(MINIMUM_SIZE);
    let atom_sizes = |kind| -> Vec<usize> {
        plugin
            .ports_with_type(kind)
            .map(|port| {
                plugin
                    .raw()
                    .port_by_index(port.index.0)
                    .and_then(|p| p.get(&minimum_size))
                    .and_then(|node| node.as_int())
                    .map_or(ATOM_CAPACITY, |bytes| {
                        (bytes.max(0) as usize).clamp(ATOM_CAPACITY, ATOM_CAPACITY_MAX)
                    })
            })
            .collect()
    };
    let atom_in_sizes = atom_sizes(PortType::AtomSequenceInput);
    // Whether the event input takes the song's position — see `TimeWriter`.
    let position = lilv.new_uri(TIME_POSITION);
    let wants_time = plugin
        .ports_with_type(PortType::AtomSequenceInput)
        .next()
        .and_then(|port| plugin.raw().port_by_index(port.index.0))
        .is_some_and(|p| p.supports_event(&position));
    let atom_out_sizes = atom_sizes(PortType::AtomSequenceOutput);
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
            atom_in_sizes,
            atom_out_sizes,
            wants_time,
            free_wheeling,
            latency_port,
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
            instance: std::mem::ManuallyDrop::new(instance),
            owner: std::thread::current().id(),
            handed_out: Arc::clone(&self.instance),
            _world: self.plugin.clone(),
            features: Arc::clone(&self.features),
            values,
            atoms,
            atom_in: (0..counts.atom_sequence_inputs)
                .map(|at| {
                    let size = self.atom_in_sizes.get(at).copied().unwrap_or(ATOM_CAPACITY);
                    LV2AtomSequence::new(&self.features, size)
                })
                .collect(),
            atom_out: (0..counts.atom_sequence_outputs)
                .map(|at| {
                    let size = self
                        .atom_out_sizes
                        .get(at)
                        .copied()
                        .unwrap_or(ATOM_CAPACITY);
                    LV2AtomSequence::new(&self.features, size)
                })
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
            time: self.wants_time.then(|| {
                TimeWriter::new(
                    &self.features,
                    self.atom_in_sizes.first().copied().unwrap_or(ATOM_CAPACITY),
                    sample_rate,
                )
            }),
            free_wheeling: self.free_wheeling.map(|port| PortIndex(port as usize)),
            offline: false,
            latency_port: self.latency_port.map(|port| PortIndex(port as usize)),
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
        // > *"most times it just renders with nothing"* — and the sweep
        // > that went looking for why.
        //
        // The last hold on a processor can go on the audio thread: a graph
        // still queued when the studio let go, a stream torn down on a
        // device change. Freed there, drumkv1's `cleanup` destroyed its Qt
        // application off the thread that made it and the process died;
        // Calf's corrupted its heap. Off its own thread an instance is
        // leaked instead — what `clack` does for a CLAP plugin in the same
        // place. It is a few kilobytes at a moment that is rare (the studio
        // keeps a processor's host alive and frees it at home otherwise).
        if std::thread::current().id() == self.owner {
            // SAFETY: dropped exactly once, here, and never used after.
            unsafe { std::mem::ManuallyDrop::drop(&mut self.instance) };
        }
    }
}

/// The audio half of an LV2 plugin — which, for LV2, is the whole plugin.
pub(crate) struct Lv2Processor {
    /// Freed by hand in `drop`, and only on [`owner`](Self::owner).
    instance: std::mem::ManuallyDrop<livi::Instance>,
    /// The thread that made the instance — the studio's main thread. An LV2
    /// plugin's `cleanup` may assume it runs there (drumkv1 tears down its
    /// Qt application in it), so an instance let go of anywhere else is
    /// leaked rather than freed: see `Drop`.
    owner: std::thread::ThreadId,
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
    /// The song's position, for a plugin that asked — see [`TimeWriter`].
    time: Option<TimeWriter>,
    /// Where the plugin is told it is free-wheeling, if it asked to be.
    free_wheeling: Option<PortIndex>,
    /// Whether the blocks are a render rather than playback — see `run`.
    offline: bool,
    /// Where it says what it delays by — see [`latency`](Self::latency).
    latency_port: Option<PortIndex>,
}

impl Lv2Processor {
    /// Hands the running instance a preset's own state, with the processor
    /// out of the graph — the only way LV2 allows it (see
    /// `ProcessorBay::recall`). Port values are not set here: they reach the
    /// plugin as parameters, through the wire every other value takes.
    pub(crate) fn restore_preset(&mut self, preset: &Lv2Preset) {
        let instance = self.instance.raw().instance();
        let Some(descriptor) = instance.descriptor() else {
            return;
        };
        // lilv's `Instance` keeps its pointer to itself; the C struct it
        // points at is public in `lilv.h`, and `lilv_state_restore` reads its
        // descriptor and handle and nothing else.
        let mut raw = livi::lilv::sys::LilvInstanceImpl {
            lv2_descriptor: (descriptor as *const lv2_raw::LV2Descriptor).cast(),
            lv2_handle: instance.handle(),
            pimpl: std::ptr::null_mut(),
        };
        let mut map = forwarding_map(&self.features);
        let map_feature = lv2_raw::LV2Feature {
            uri: c"http://lv2plug.in/ns/ext/urid#map".as_ptr(),
            data: (&mut map as *mut lv2_raw::LV2UridMap).cast(),
        };
        let features = [&map_feature as *const lv2_raw::LV2Feature, std::ptr::null()];
        // SAFETY: the state is lilv's own and alive; the instance is not
        // running (the caller holds the processor); the feature list is
        // null-terminated and outlives the call.
        unsafe {
            livi::lilv::sys::lilv_state_restore(
                preset.0.state.as_ptr(),
                &mut raw,
                Some(ignore_port_value),
                std::ptr::null_mut(),
                0,
                features.as_ptr(),
            );
        }
    }

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

    /// **RT.** One raw MIDI message, channel and all — what an MPE zone
    /// speaks through (`HostedProcessor::set_mpe`).
    pub(crate) fn midi(&mut self, frame: usize, bytes: [u8; 3]) {
        self.push(frame, bytes);
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

    /// **RT.** Where the song is, for the next block — see [`TimeWriter`].
    pub(crate) fn set_transport(&mut self, transport: &crate::PluginTransport) {
        self.offline = transport.offline;
        if let Some(time) = &mut self.time {
            time.transport = *transport;
        }
    }

    /// What the plugin says it delays by, in frames — off its latency port,
    /// as it was left by the last `run`. Zero for a plugin with none.
    pub(crate) fn latency(&self) -> u32 {
        self.latency_port
            .and_then(|port| self.instance.control_output(port))
            .filter(|frames| frames.is_finite() && *frames > 0.0)
            .map_or(0, |frames| frames.round().min(u32::MAX as f32) as u32)
    }

    /// Runs one silent block so a plugin with a latency port has written
    /// it — LV2 has a plugin say its latency in `run` and nowhere else.
    pub(crate) fn learn_latency(&mut self) {
        if self.latency_port.is_none() {
            return;
        }
        for buffer in &mut self.input {
            buffer.fill(0.0);
        }
        self.run(self.max_block);
        for buffer in &mut self.output {
            buffer.fill(0.0);
        }
    }

    /// See [`crate::HostedProcessor::finish_work`]. Each block is one
    /// round of the conversation — the work the last one asked for done
    /// before it, its answer handed over after it — and a plugin may ask
    /// again on hearing the answer, so there are a few.
    pub(crate) fn finish_work(&mut self) {
        const ROUNDS: usize = 4;
        let offline = std::mem::replace(&mut self.offline, true);
        for _ in 0..ROUNDS {
            for buffer in &mut self.input {
                buffer.fill(0.0);
            }
            self.run(self.max_block);
        }
        self.offline = offline;
        for buffer in &mut self.output {
            buffer.fill(0.0);
        }
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
        if let (Some(time), Some(first)) = (&mut self.time, self.atom_in.first_mut()) {
            time.put_ahead_of(first);
        }
        if let Some(port) = self.free_wheeling {
            instance.set_control_input(port, if self.offline { 1.0 } else { 0.0 });
        }
        // **Offline, the worker keeps step.** livi's worker thread looks for
        // work a tenth of a second apart, which is fine for playback and
        // seconds of audio in a render: setBfree, building its organ after a
        // state restore, rendered at a different level every time it was
        // asked, and a sampler would render the silence before its files
        // were read. So a render does the work itself, before the block (what
        // was asked since the last one, a restore's included) and after it
        // (what this block asked), and the answer is handed over at the end
        // of the next — what every host that renders does. Not real-time
        // safe, and it need not be: nobody is listening.
        if self.offline {
            self.features.worker_manager().run_workers();
        }

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
        if self.offline {
            self.features.worker_manager().run_workers();
        }

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
        // Fedora, openSUSE and RHEL package 64-bit plugins under `lib64`.
        paths.push(PathBuf::from("/usr/lib64/lv2"));
        paths.push(PathBuf::from("/usr/local/lib64/lv2"));
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

// ------------------------------------------------------------ own presets

/// An LV2 preset's state as lilv read it off the Turtle — its properties as
/// well as its port values, mapped through the host's own URID map so they
/// mean to the plugin what they meant to whoever wrote them. lilv offers no
/// way to list what is inside one, so it is kept whole and handed to
/// `lilv_state_restore`.
#[derive(Clone)]
pub struct Lv2Preset(Arc<LilvState>);

/// lilv's state, and the plugin it was read for.
///
/// **The plugin handle is what keeps the world alive**, and the state needs
/// it: its nodes are the world's, and `lilv_state_free` frees them through
/// it. A state that outlived its world crashed in `sord_node_free` the
/// moment a rack, which drops its host before its libraries, was dropped —
/// the `Lv2Processor::_world` trap again. Dropped after `Drop::drop` has
/// freed the state, as fields are.
struct LilvState {
    state: std::ptr::NonNull<livi::lilv::sys::LilvState>,
    _world: livi::Plugin,
}

// SAFETY: a `LilvState` is data lilv read out of the world; nothing in it is
// tied to a thread, it is only read after it is made, and the world it
// points into is kept alive beside it.
unsafe impl Send for LilvState {}
unsafe impl Sync for LilvState {}

impl Drop for LilvState {
    fn drop(&mut self) {
        // SAFETY: made by `lilv_state_new_from_world` and freed once, here.
        unsafe { livi::lilv::sys::lilv_state_free(self.state.as_ptr()) }
    }
}

impl PartialEq for Lv2Preset {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for Lv2Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Lv2Preset")
    }
}

extern "C" fn map_through_features(
    handle: lv2_raw::LV2UridMapHandle,
    uri: *const std::os::raw::c_char,
) -> lv2_raw::LV2Urid {
    if handle.is_null() || uri.is_null() {
        return 0;
    }
    // SAFETY: `handle` is the `Features` `forwarding_map` was given, alive
    // for the call; `uri` is the NUL-terminated string LV2 promises.
    unsafe { (*(handle as *const Features)).urid(CStr::from_ptr(uri)) }
}

/// A URID map that answers with `features`' own — the one the plugin was
/// instantiated with, so a key lilv maps is the key the plugin knows.
fn forwarding_map(features: &Arc<Features>) -> lv2_raw::LV2UridMap {
    lv2_raw::LV2UridMap {
        handle: Arc::as_ptr(features) as lv2_raw::LV2UridMapHandle,
        map: map_through_features,
    }
}

unsafe extern "C" fn ignore_port_value(
    _symbol: *const std::os::raw::c_char,
    _user: *mut c_void,
    _value: *const c_void,
    _size: u32,
    _type: u32,
) {
}

/// The `pset:Preset`s that apply to the plugin `uri` names, read whole: a
/// label, a bank's label as the category, each port value by index, and
/// lilv's state.
pub(crate) fn presets(world: &World, features: &Arc<Features>, uri: &str) -> Vec<crate::OwnPreset> {
    let Some(handle) = world.plugin_by_uri(uri) else {
        return Vec::new();
    };
    let lilv = world.raw();
    let plugin = handle.raw();
    let preset_class = lilv.new_uri("http://lv2plug.in/ns/ext/presets#Preset");
    let label = lilv.new_uri("http://www.w3.org/2000/01/rdf-schema#label");
    let port = lilv.new_uri("http://lv2plug.in/ns/lv2core#port");
    let symbol = lilv.new_uri("http://lv2plug.in/ns/lv2core#symbol");
    let value = lilv.new_uri("http://lv2plug.in/ns/ext/presets#value");
    let bank = lilv.new_uri("http://lv2plug.in/ns/ext/presets#bank");
    let Some(presets) = plugin.related(Some(&preset_class)) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for preset in presets.iter() {
        let Some(preset_uri) = preset.as_uri().map(str::to_string) else {
            continue;
        };
        let _ = lilv.load_resource(&preset);
        let name = lilv
            .get(Some(&preset), Some(&label), None)
            .and_then(|node| node.as_str().map(str::to_string))
            .unwrap_or_else(|| {
                preset_uri
                    .rsplit(['#', '/'])
                    .next()
                    .unwrap_or(&preset_uri)
                    .to_string()
            });
        let category = lilv
            .get(Some(&preset), Some(&bank), None)
            .and_then(|bank| lilv.get(Some(&bank), Some(&label), None))
            .and_then(|node| node.as_str().map(str::to_string))
            .unwrap_or_default();
        let mut ports = Vec::new();
        for entry in lilv.find_nodes(Some(&preset), &port, None).iter() {
            let (Some(name), Some(set)) = (
                lilv.get(Some(&entry), Some(&symbol), None),
                lilv.get(Some(&entry), Some(&value), None),
            ) else {
                continue;
            };
            let Some(target) = plugin.port_by_symbol(&name) else {
                continue;
            };
            let set = set
                .as_float()
                .or_else(|| set.as_int().map(|v| v as f32))
                .or_else(|| set.as_bool().map(|v| if v { 1.0 } else { 0.0 }));
            if let Some(set) = set {
                ports.push((target.index() as u32, set));
            }
        }
        let mut map = forwarding_map(features);
        // SAFETY: the world and the node are alive; the map is valid for the
        // call and lilv keeps no pointer to it.
        let state = unsafe {
            livi::lilv::sys::lilv_state_new_from_world(
                lilv.as_ptr(),
                (&mut map as *mut lv2_raw::LV2UridMap).cast(),
                preset.as_ptr(),
            )
        };
        found.push(crate::OwnPreset {
            name,
            category,
            source: crate::OwnPresetSource::Lv2 {
                uri: preset_uri,
                ports,
                state: std::ptr::NonNull::new(state).map(|state| {
                    Lv2Preset(Arc::new(LilvState {
                        state,
                        _world: handle.clone(),
                    }))
                }),
            },
        });
    }
    found
}
