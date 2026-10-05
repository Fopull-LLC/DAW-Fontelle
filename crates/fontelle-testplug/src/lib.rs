//! Two tiny CLAP plugins, built only so [`fontelle_host`] has something real
//! to load.
//!
//! **This is a test fixture, not a product.** It ships as a `cdylib` because
//! that is what a CLAP bundle is, and `fontelle-host` dev-depends on it so
//! that cargo has built the `.clap` by the time a host test looks for it.
//!
//! It exists because the alternative was worse. A host is almost entirely
//! foreign-function boundary: dynamic loading, C vtables, an audio buffer
//! layout nobody in this workspace controls, and a threading contract that is
//! part of the CLAP specification rather than of this program. Mocking that
//! tests the mock. Testing against whatever the developer happens to have
//! installed tests that machine. So the host's tests load a real plugin,
//! through the real entry point, across the real ABI — one this repository
//! also wrote, so its answers are known.
//!
//! Two plugins in one bundle, deliberately: a bundle holding several plugins
//! is the ordinary case (every plugin suite ships one), and a scanner that
//! only ever saw bundles of one would be wrong in a way nobody noticed until
//! it met a real folder.
//!
//! - **Fontelle Test Gain** — an effect. Multiplies by a parameter, can invert
//!   the phase from a stepped one, implements the state extension, and has a
//!   **sidechain**: a mono input port beside its main pair that *ducks* the
//!   main output by whatever is on it, and a mono aux output that carries
//!   the key back out. A host that hands over the second input port and
//!   maps the bus to the first is heard doing so.
//! - **Fontelle Test Sine** — an instrument. Plays one sine at the key it is
//!   given, at a level a parameter sets, and implements *no* state extension —
//!   which is the case a host has to survive as much as the other one. It
//!   hears the **wheels** two ways, which is why it ships **twice**: under
//!   [`SinePlugin`]`<true>` its note port takes raw MIDI beside CLAP's own
//!   events, and under [`SINE_CLAP_ONLY`] it takes CLAP's alone — so a host
//!   is checked sending a mod wheel as the bytes it was to the one, and as a
//!   note expression to the other. The mod wheel (CC 1, or vibrato) scales
//!   the level, a bend (or tuning) bends it, and channel pressure (or the
//!   pressure expression) *ducks* it — the opposite of the wheel, so the two
//!   cannot be mistaken for each other by a host that sent the wrong thing.
//!   Its `reset` **leaves the note sounding**, as a JUCE plugin's does
//!   (OB-Xf's, found 2026-09-29), and it renders silence for an event list
//!   that is not in time order, or with no transport — all three are what
//!   real plugins do or depend on, and a host that leaned on the kinder
//!   answers was broken against them.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU32, Ordering};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::gui::{
    GuiApiType, GuiConfiguration, GuiSize, PluginGui, PluginGuiImpl, Window,
};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePorts,
    PluginNotePortsImpl,
};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams, PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::entry::prelude::*;
use clack_plugin::events::event_types::{
    MidiEvent, NoteExpressionEvent, NoteExpressionType, NoteOffEvent, NoteOnEvent, ParamValueEvent,
};
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};

fn store(cell: &AtomicU32, value: f32) {
    cell.store(value.to_bits(), Ordering::Relaxed);
}

fn load(cell: &AtomicU32) -> f32 {
    f32::from_bits(cell.load(Ordering::Relaxed))
}

// ---------------------------------------------------------------- gain effect

pub struct GainShared {
    gain: AtomicU32,
    /// A stepped parameter, so a host has one to draw as a switch.
    invert: AtomicU32,
    /// A multiplier kept in the plugin's **state and nowhere else** — not a
    /// parameter. Most of a real plugin is like this (Surge's wavetables, a
    /// sampler's file, OB-Xf's patch name), and a host that thinks a preset
    /// is its parameters loses it. Scales the output like the gain.
    trim: AtomicU32,
    /// A gain a preset has asked for and the next block will apply — NaN
    /// when there is none. **Surge XT loads a patch this way**: its
    /// `preset-load` queues the patch and the audio thread swaps it in, so a
    /// host that reads the parameters back straight after the call reads the
    /// patch that was there before.
    pending_gain: AtomicU32,
    /// Whether the audio thread has run it since it was activated.
    ///
    /// **Surge XT loads a state the same way it loads a preset once this is
    /// so**: handed to the audio thread, in on a later block. Before it has
    /// ever processed there is no later block, and it loads where it stands.
    /// So a host that restores a state into a running plugin — an undo, a
    /// preset of its own — and reads it straight back reads the state from
    /// before.
    processing: std::sync::atomic::AtomicBool,
    /// A whole state waiting for the next block: gain, invert, trim. The
    /// first is NaN when there is none.
    pending_state: [AtomicU32; 3],
    /// Whether it has been activated — see its latency.
    activated: std::sync::atomic::AtomicBool,
}

impl PluginShared<'_> for GainShared {}

pub struct GainMain<'a> {
    shared: &'a GainShared,
    host: HostMainThreadHandle<'a>,
    /// A preset asked for that the plugin finishes only in `on_main_thread`
    /// — see `DEFERRED_PRESET`.
    deferred: Option<f32>,
    /// Whether a state handed to it is applied only in `on_main_thread`, as
    /// a JUCE plugin's message thread does — see `DEFERS_STATES`.
    defers_states: bool,
    /// Such a state, waiting: gain, invert, trim. Saved back as it is while
    /// it waits, so a host that compares what it handed over with what the
    /// plugin saves believes it is in.
    deferred_state: Option<[f32; 3]>,
}

impl<'a> PluginMainThread<'a, GainShared> for GainMain<'a> {
    fn on_main_thread(&mut self) {
        if let Some(gain) = self.deferred.take() {
            store(&self.shared.pending_gain, gain);
        }
        if let Some([gain, invert, trim]) = self.deferred_state.take() {
            store(&self.shared.gain, gain);
            store(&self.shared.invert, invert);
            store(&self.shared.trim, trim);
        }
    }
}

pub struct GainPlugin;

/// What the fixture gain tells a host it delays by, in samples.
///
/// A round number that no real plugin would land on by accident, and one no
/// buffer size in this program shares, so a host that never asked reads zero
/// and is told apart from one that asked and was answered. The gain does not
/// actually delay anything — what is under test is the *reading*, and a
/// fixture that also delayed would make a latency test a delay test.
pub const GAIN_LATENCY_SAMPLES: u32 = 137;

impl clack_extensions::latency::PluginLatencyImpl for GainMain<'_> {
    /// Only once activated: CLAP's latency call is legal while the plugin is
    /// being activated or is active, and Surge XT says so aloud — *"It is
    /// wrong to query the latency before the plugin is activated"* — because
    /// before it knows the rate it cannot know the answer. Asked earlier,
    /// this says nothing, the way a plugin that means it would.
    fn get(&mut self) -> u32 {
        if self
            .shared
            .activated
            .load(std::sync::atomic::Ordering::Acquire)
        {
            GAIN_LATENCY_SAMPLES
        } else {
            0
        }
    }
}

impl Plugin for GainPlugin {
    type AudioProcessor<'a> = GainProcessor<'a>;
    type Shared<'a> = GainShared;
    type MainThread<'a> = GainMain<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&GainShared>) {
        builder
            .register::<PluginParams>()
            .register::<PluginState>()
            .register::<clack_extensions::latency::PluginLatency>()
            .register::<clack_extensions::preset_discovery::PluginPresetLoad>()
            .register::<PluginAudioPorts>();
    }
}

impl DefaultPluginFactory for GainPlugin {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new("com.fopull.fontelle.testgain", "Fontelle Test Gain")
            .with_vendor("Fopull LLC")
            .with_version("1.0.0")
            .with_features([clack_plugin::plugin::features::AUDIO_EFFECT])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<GainShared, PluginError> {
        Ok(GainShared {
            gain: AtomicU32::new(1.0f32.to_bits()),
            invert: AtomicU32::new(0.0f32.to_bits()),
            trim: AtomicU32::new(1.0f32.to_bits()),
            pending_gain: AtomicU32::new(f32::NAN.to_bits()),
            processing: std::sync::atomic::AtomicBool::new(false),
            pending_state: [
                AtomicU32::new(f32::NAN.to_bits()),
                AtomicU32::new(0),
                AtomicU32::new(0),
            ],
            activated: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn new_main_thread<'a>(
        host: HostMainThreadHandle<'a>,
        shared: &'a GainShared,
    ) -> Result<GainMain<'a>, PluginError> {
        Ok(GainMain {
            shared,
            host,
            deferred: None,
            defers_states: false,
            deferred_state: None,
        })
    }
}

pub struct GainProcessor<'a> {
    shared: &'a GainShared,
    /// This block's key, copied off the sidechain port before the main pair
    /// is processed. Sized at activation from the block the host promised,
    /// never grown.
    key: Vec<f32>,
    /// [`ABORTS_IN_PROCESS_ENV`], read once as it is activated.
    aborts: bool,
}

/// Set, the gain **aborts in its own `process`** — what padthv1 does when
/// `libstdc++` catches its pure virtual call: a crash whose faulting address
/// is in `libc`, so only the host's own record says which plugin it was.
pub const ABORTS_IN_PROCESS_ENV: &str = "FONTELLE_TESTPLUG_ABORTS_IN_PROCESS";

impl<'a> PluginAudioProcessor<'a, GainShared, GainMain<'a>> for GainProcessor<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &mut GainMain<'a>,
        shared: &'a GainShared,
        config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        shared
            .activated
            .store(true, std::sync::atomic::Ordering::Release);
        Ok(Self {
            shared,
            key: vec![0.0; config.max_frames_count as usize],
            aborts: std::env::var_os(ABORTS_IN_PROCESS_ENV).is_some(),
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        if self.aborts {
            std::process::abort();
        }
        self.shared
            .processing
            .store(true, std::sync::atomic::Ordering::Release);
        // A queued state, whole.
        let queued = load(&self.shared.pending_state[0]);
        if !queued.is_nan() {
            store(&self.shared.gain, queued);
            store(&self.shared.invert, load(&self.shared.pending_state[1]));
            store(&self.shared.trim, load(&self.shared.pending_state[2]));
            store(&self.shared.pending_state[0], f32::NAN);
        }
        // A queued preset goes in first, then the block's events — Surge's
        // order, and the one that shows a host echoing stale values back.
        let pending = load(&self.shared.pending_gain);
        if !pending.is_nan() {
            store(&self.shared.gain, pending);
            store(&self.shared.pending_gain, f32::NAN);
            let _ = events.output.try_push(ParamValueEvent::new(
                0,
                ClapId::new(0),
                clack_plugin::events::Pckn::match_all(),
                f64::from(pending),
                Default::default(),
            ));
        }
        for event in events.input {
            if let Some(value) = event.as_event::<ParamValueEvent>() {
                take_gain_param(self.shared, value);
            }
        }
        let gain = load(&self.shared.gain)
            * load(&self.shared.trim)
            * if load(&self.shared.invert) >= 0.5 {
                -1.0
            } else {
                1.0
            };

        // **The key first.** The sidechain is the second input port; what is
        // on it ducks the main pair. A host that handed over the main port
        // alone hands over no key, which reads as silence — and a host that
        // put the bus on the wrong port is heard at once.
        let frames = (audio.frames_count() as usize).min(self.key.len());
        self.key[..frames].fill(0.0);
        if let Some(port) = audio.input_port(SIDECHAIN_PORT)
            && let Some(channels) = port.channels()?.into_f32()
            && let Some(first) = channels.channel(0)
        {
            let taken = frames.min(first.len()).min(self.key.len());
            self.key[..taken].copy_from_slice(&first[..taken]);
        }

        for (index, mut port) in (&mut audio).into_iter().enumerate() {
            let Some(channels) = port.channels()?.into_f32() else {
                continue;
            };
            // Port 1 pairs the sidechain input with the aux output: the aux
            // carries the key back out, so a host that summed the extra
            // output into the bus would be heard putting the key there.
            let is_aux = index == SIDECHAIN_PORT;
            for pair in channels {
                match pair {
                    ChannelPair::InputOnly(_) => {}
                    ChannelPair::OutputOnly(buffer) => {
                        if is_aux {
                            let taken = buffer.len().min(self.key.len());
                            buffer[..taken].copy_from_slice(&self.key[..taken]);
                        } else {
                            buffer.fill(0.0);
                        }
                    }
                    ChannelPair::InputOutput(input, output) => {
                        for (frame, (input, output)) in input.iter().zip(output).enumerate() {
                            *output = if is_aux {
                                self.key.get(frame).copied().unwrap_or(0.0)
                            } else {
                                *input * gain * duck(self.key.get(frame).copied().unwrap_or(0.0))
                            };
                        }
                    }
                    ChannelPair::InPlace(buffer) => {
                        for (frame, sample) in buffer.iter_mut().enumerate() {
                            *sample = if is_aux {
                                self.key.get(frame).copied().unwrap_or(0.0)
                            } else {
                                *sample * gain * duck(self.key.get(frame).copied().unwrap_or(0.0))
                            };
                        }
                    }
                }
            }
        }
        Ok(ProcessStatus::Continue)
    }
}

/// Which port the sidechain (input) and aux (output) are, in each direction.
const SIDECHAIN_PORT: usize = 1;

/// How much of the main signal a key sample lets through: none at full
/// scale, all of it at silence. A key that is *heard* rather than detected,
/// so a test can say exactly what it expects.
fn duck(key: f32) -> f32 {
    1.0 - key.abs().min(1.0)
}

/// The gain's ports: a main pair each way, and a **sidechain** input beside
/// an aux output.
///
/// CLAP has no sidechain flag — a sidechain is an input port that is not the
/// main one, and a compressor's key arrives on it. The second port in each
/// direction is what a host has to hand over *and* keep off the bus: the
/// bus goes to the main pair, the key comes in on the sidechain, and the aux
/// is rendered and dropped.
impl PluginAudioPortsImpl for GainMain<'_> {
    fn count(&mut self, _is_input: bool) -> u32 {
        2
    }

    fn get(&mut self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        match index {
            0 => writer.set(&AudioPortInfo {
                id: 0u32.into(),
                name: b"main",
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            }),
            1 => writer.set(&AudioPortInfo {
                id: 1u32.into(),
                name: if is_input { b"sidechain" } else { b"aux" },
                channel_count: 1,
                flags: AudioPortFlags::empty(),
                port_type: Some(AudioPortType::MONO),
                in_place_pair: None,
            }),
            _ => {}
        }
    }
}

impl PluginMainThreadParams for GainMain<'_> {
    fn count(&mut self) -> u32 {
        2
    }

    fn get_info(&mut self, param_index: u32, info: &mut ParamInfoWriter) {
        match param_index {
            0 => info.set(&ParamInfo {
                id: 0u32.into(),
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Gain",
                module: b"",
                min_value: 0.0,
                max_value: 4.0,
                default_value: 1.0,
            }),
            1 => info.set(&ParamInfo {
                id: 1u32.into(),
                flags: ParamInfoFlags::IS_AUTOMATABLE | ParamInfoFlags::IS_STEPPED,
                cookie: Default::default(),
                name: b"Invert",
                // **Slashes, the way a real plugin writes one.** CLAP's
                // `module` is a path, and plugins write it with separators —
                // Surge XT's are "/Macros/" and "/Global & FX/". A host that
                // shows the string raw puts "/Phase/" over a group of knobs.
                module: b"/Phase/",
                min_value: 0.0,
                max_value: 1.0,
                default_value: 0.0,
            }),
            _ => {}
        }
    }

    fn get_value(&mut self, param_id: ClapId) -> Option<f64> {
        match u32::from(param_id) {
            0 => Some(load(&self.shared.gain) as f64),
            1 => Some(load(&self.shared.invert) as f64),
            _ => None,
        }
    }

    fn value_to_text(
        &mut self,
        _param_id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        use std::fmt::Write;
        write!(writer, "{value:.2}x")
    }

    fn text_to_value(&mut self, _param_id: ClapId, text: &CStr) -> Option<f64> {
        text.to_str().ok()?.trim_end_matches('x').parse().ok()
    }

    fn flush(&mut self, input_events: &InputEvents, _output_events: &mut OutputEvents) {
        for event in input_events {
            if let Some(value) = event.as_event::<ParamValueEvent>() {
                store(&self.shared.gain, value.value() as f32);
            }
        }
    }
}

impl PluginAudioProcessorParams for GainProcessor<'_> {
    fn flush(&mut self, input_events: &InputEvents, _output_events: &mut OutputEvents) {
        for event in input_events {
            if let Some(value) = event.as_event::<ParamValueEvent>() {
                take_gain_param(self.shared, value);
            }
        }
    }
}

/// **Being told the switch is a gesture, whatever it is told.** Surge XT's
/// oscillator type is like this: an event for it sets up the oscillator
/// again, and what hung off the old setting — which lives in the state, not
/// in any parameter — goes back to where a fresh one starts. So a host that
/// loads a state and then "sets" every parameter to the value it already has
/// does not get the state back; here the trim returns to one.
fn take_gain_param(shared: &GainShared, event: &ParamValueEvent) {
    match u32::from(event.param_id().unwrap_or(0u32.into())) {
        0 => store(&shared.gain, event.value() as f32),
        1 => {
            store(&shared.invert, event.value() as f32);
            store(&shared.trim, 1.0);
        }
        _ => {}
    }
}

impl PluginStateImpl for GainMain<'_> {
    fn save(&mut self, output: &mut OutputStream) -> Result<(), PluginError> {
        use std::io::Write;
        let [gain, invert, trim] = self.deferred_state.unwrap_or_else(|| {
            [
                load(&self.shared.gain),
                load(&self.shared.invert),
                load(&self.shared.trim),
            ]
        });
        output.write_all(&gain.to_le_bytes())?;
        output.write_all(&invert.to_le_bytes())?;
        output.write_all(&trim.to_le_bytes())?;
        Ok(())
    }

    fn load(&mut self, input: &mut InputStream) -> Result<(), PluginError> {
        use std::io::Read;
        let mut bytes = [0u8; 8];
        input.read_exact(&mut bytes)?;
        let gain = f32::from_le_bytes(bytes[..4].try_into().unwrap());
        let invert = f32::from_le_bytes(bytes[4..].try_into().unwrap());
        // Written after the first two, so a state saved before it existed
        // still loads — and leaves the trim at one.
        let mut trim = [0u8; 4];
        let trim = match input.read_exact(&mut trim) {
            Ok(()) => f32::from_le_bytes(trim),
            Err(_) => 1.0,
        };
        if self.defers_states {
            self.deferred_state = Some([gain, invert, trim]);
            self.host.request_callback();
            return Ok(());
        }
        // Once the audio thread has run it, a state waits for the next
        // block — see `GainShared::processing`.
        if self
            .shared
            .processing
            .load(std::sync::atomic::Ordering::Acquire)
        {
            store(&self.shared.pending_state[1], invert);
            store(&self.shared.pending_state[2], trim);
            store(&self.shared.pending_state[0], gain);
            return Ok(());
        }
        store(&self.shared.gain, gain);
        store(&self.shared.invert, invert);
        store(&self.shared.trim, trim);
        Ok(())
    }
}

/// The gain's own presets, the way a CLAP plugin offers a library: a
/// preset-discovery factory beside the plugin factory lists them, and the
/// `preset-load` extension loads one by the key the listing gave. Both live
/// **inside the plugin** (`Location::Plugin`), as Surge XT's factory set
/// does not but many do. Each sets the trim, which is the part of the gain a
/// host that loaded only parameters would miss.
const OWN_PRESETS: [(&CStr, &CStr, f32); 2] = [(c"Loud", c"loud", 2.0), (c"Quiet", c"quiet", 0.25)];

/// And one that sets a **parameter**, queued for the next block — see
/// `GainShared::pending_gain`. Loaded by key; not in the listing, so the
/// listing's own test is unchanged.
const QUEUED_PRESET: (&CStr, f32) = (c"queued", 0.5);

/// One the plugin finishes only when the host calls it back on the main
/// thread (`request_callback` → `on_main_thread`), as CLAP lets a plugin
/// defer work — a host that records the request and never answers it has a
/// preset that never lands.
const DEFERRED_PRESET: (&CStr, f32) = (c"deferred", 0.25);

/// Loaded by this key, the gain from then on applies a **state** it is handed
/// only in `on_main_thread`, and saves that state back meanwhile — OB-Xf's
/// way (JUCE's message thread). A host that took "it saves what I gave it"
/// for "it is in" moved on, and the state landed later, over whatever was
/// loaded next: an undo's patch over the preset chosen after it.
pub const DEFERS_STATES: &str = "defer-states";

impl clack_extensions::preset_discovery::PluginPresetLoadImpl for GainMain<'_> {
    fn load_from_location(
        &mut self,
        location: clack_extensions::preset_discovery::prelude::Location,
        load_key: Option<&CStr>,
    ) -> Result<(), PluginError> {
        use clack_extensions::preset_discovery::prelude::Location;
        if location != Location::Plugin {
            return Err(PluginError::Message("not one of the gain's presets"));
        }
        if load_key.is_some_and(|key| key.to_bytes() == DEFERS_STATES.as_bytes()) {
            self.defers_states = true;
            return Ok(());
        }
        if load_key == Some(DEFERRED_PRESET.0) {
            self.deferred = Some(DEFERRED_PRESET.1);
            self.host.request_callback();
            return Ok(());
        }
        if load_key == Some(QUEUED_PRESET.0) {
            store(&self.shared.pending_gain, QUEUED_PRESET.1);
            return Ok(());
        }
        let (_, _, trim) = OWN_PRESETS
            .iter()
            .find(|(_, key, _)| Some(*key) == load_key)
            .ok_or(PluginError::Message("no such preset"))?;
        store(&self.shared.trim, *trim);
        Ok(())
    }
}

pub struct TestPresets {
    descriptor: clack_extensions::preset_discovery::prelude::ProviderDescriptor,
}

impl TestPresets {
    fn new() -> Self {
        Self {
            descriptor: clack_extensions::preset_discovery::prelude::ProviderDescriptor::new(
                "com.fopull.fontelle.testgain.presets",
                "Fontelle Test Gain presets",
            ),
        }
    }
}

impl clack_extensions::preset_discovery::prelude::PresetDiscoveryFactoryImpl for TestPresets {
    fn provider_count(&self) -> u32 {
        1
    }

    fn provider_descriptor(
        &self,
        index: u32,
    ) -> Option<&clack_extensions::preset_discovery::prelude::ProviderDescriptor> {
        (index == 0).then_some(&self.descriptor)
    }

    fn create_provider<'a>(
        &'a self,
        indexer: clack_extensions::preset_discovery::prelude::IndexerInfo<'a>,
        provider_id: &CStr,
    ) -> Option<clack_extensions::preset_discovery::prelude::ProviderInstance<'a>> {
        use clack_extensions::preset_discovery::prelude::*;
        if Some(provider_id) != self.descriptor.id() {
            return None;
        }
        Some(ProviderInstance::new(
            indexer,
            &self.descriptor,
            |mut indexer| {
                indexer
                    .declare_location(LocationInfo {
                        name: c"Fontelle Test Gain",
                        flags: Flags::IS_FACTORY_CONTENT,
                        location: Location::Plugin,
                    })
                    .map_err(|_| PluginError::Message("the indexer refused the location"))?;
                Ok(GainPresetProvider)
            },
        ))
    }
}

struct GainPresetProvider;

impl<'a> clack_extensions::preset_discovery::prelude::ProviderImpl<'a> for GainPresetProvider {
    fn get_metadata(
        &mut self,
        location: clack_extensions::preset_discovery::prelude::Location,
        receiver: &mut clack_extensions::preset_discovery::prelude::MetadataReceiver,
    ) -> Result<(), PluginError> {
        use clack_extensions::preset_discovery::prelude::*;
        if location != Location::Plugin {
            return Ok(());
        }
        for (name, key, _) in OWN_PRESETS {
            receiver
                .begin_preset(Some(name), Some(key))
                .map_err(|_| PluginError::Message("the host refused a preset"))?
                .add_plugin_id(UniversalPluginId::clap(c"com.fopull.fontelle.testgain"))
                .add_feature(c"Soft and loud");
        }
        Ok(())
    }
}

// ------------------------------------------------------------ sine instrument

pub struct SineShared {
    level: AtomicU32,
    /// A parameter that **lies about its default**, on purpose.
    ///
    /// Its `param_info` says the default is zero; `get_value` says one. That
    /// is not a hypothetical: Surge XT's CLAP build reports `default_value`
    /// as zero for all seven hundred and seventy-five of its parameters,
    /// while `get_value` on a freshly instantiated plugin returns the real
    /// setting (Global Volume at -2.03 dB, Polyphony Limit at 16). A host
    /// that seeds its parameter wire from `param_info` and then sends the lot
    /// turns Surge's global volume down to -48 dB before the first block, and
    /// the plugin is silent — which is exactly what happened here.
    ///
    /// The sine is multiplied by this, so a host that gets it wrong renders
    /// nothing and a test can say so.
    output: AtomicU32,
}

impl PluginShared<'_> for SineShared {}

pub struct SineMain<'a, const MIDI: bool> {
    shared: &'a SineShared,
}

impl<'a, const MIDI: bool> PluginMainThread<'a, SineShared> for SineMain<'a, MIDI> {}

/// The sine, generic over whether its note port takes **MIDI** beside
/// CLAP's own events. See the crate note on why it ships both ways.
pub struct SinePlugin<const MIDI: bool>;

/// The id the CLAP-only sine ships under. The MIDI-speaking one is
/// `com.fopull.fontelle.testsine`.
pub const SINE_CLAP_ONLY: &str = "com.fopull.fontelle.testsine.clap";

impl<const MIDI: bool> Plugin for SinePlugin<MIDI> {
    type AudioProcessor<'a> = SineProcessor<'a>;
    type Shared<'a> = SineShared;
    type MainThread<'a> = SineMain<'a, MIDI>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&SineShared>) {
        builder
            .register::<PluginParams>()
            .register::<PluginAudioPorts>()
            .register::<PluginNotePorts>()
            .register::<clack_extensions::render::PluginRender>();
    }
}

impl<const MIDI: bool> DefaultPluginFactory for SinePlugin<MIDI> {
    fn get_descriptor() -> PluginDescriptor {
        let (id, name) = if MIDI {
            ("com.fopull.fontelle.testsine", "Fontelle Test Sine")
        } else {
            (SINE_CLAP_ONLY, "Fontelle Test Sine (CLAP only)")
        };
        PluginDescriptor::new(id, name)
            .with_vendor("Fopull LLC")
            .with_version("1.0.0")
            .with_features([
                clack_plugin::plugin::features::INSTRUMENT,
                clack_plugin::plugin::features::SYNTHESIZER,
            ])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<SineShared, PluginError> {
        // One line per instance made — see `CREATE_LOG_ENV`.
        if let Some(file) = std::env::var_os(CREATE_LOG_ENV) {
            use std::io::Write;
            if let Ok(mut out) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)
            {
                // The operating system's name for the thread: tests running
                // side by side share the file, and each counts its own.
                let name = std::fs::read_to_string("/proc/thread-self/comm")
                    .map(|name| name.trim().to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                let _ = out.write_all(format!("{name}\n").as_bytes());
            }
        }
        Ok(SineShared {
            level: AtomicU32::new(0.5f32.to_bits()),
            output: AtomicU32::new(1.0f32.to_bits()),
        })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a SineShared,
    ) -> Result<SineMain<'a, MIDI>, PluginError> {
        Ok(SineMain { shared })
    }
}

/// How far a full bend goes, in semitones — a keyboard's default. A tuning
/// expression arrives in semitones already.
pub const BEND_RANGE_SEMITONES: f32 = 2.0;

pub struct SineProcessor<'a> {
    shared: &'a SineShared,
    sample_rate: f32,
    key: Option<u16>,
    phase: f32,
    /// The mod wheel or vibrato expression, `0..=1`, scaling the level. Full
    /// until told otherwise.
    wheel: f32,
    /// Channel pressure or the pressure expression, `0..=1`, **ducking** the
    /// level — see the crate note.
    pressure: f32,
    /// The bend, in semitones.
    bend: f32,
}

impl<'a, const MIDI: bool> PluginAudioProcessor<'a, SineShared, SineMain<'a, MIDI>>
    for SineProcessor<'a>
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &mut SineMain<'a, MIDI>,
        shared: &'a SineShared,
        config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            shared,
            sample_rate: config.sample_rate as f32,
            key: None,
            phase: 0.0,
            wheel: 1.0,
            pressure: 0.0,
            bend: 0.0,
        })
    }

    /// Leaves the note sounding. CLAP says a reset kills voices; JUCE's
    /// `AudioProcessor::reset` is empty unless a plugin fills it, and OB-Xf
    /// does not — so a host that trusts a reset to end the notes it started
    /// leaves them stuck. This is that plugin.
    fn reset(&mut self) {
        self.phase = 0.0;
    }

    fn process(
        &mut self,
        process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        // CLAP allows a free-running host to pass no transport, and a synced
        // plugin then free-runs. This host always passes one; heard here as
        // silence if it does not.
        let untimed = process.transport.is_none();
        // CLAP: the input list is sorted by time. A plugin that splits its
        // block at each event takes that at its word, so one out of order is
        // a sub-block of negative length. Heard here as silence.
        let mut last = 0;
        let mut ordered = true;
        for event in events.input {
            let time = event.header().time();
            ordered &= time >= last;
            last = time;
        }
        // Notes are kept with the frame they happened on rather than applied
        // up front, because a host's whole job with an event list is to say
        // *when* — and a fixture that ignored the timestamps could not tell a
        // host that got them right from one that did not.
        let mut schedule = [(0u32, 0u8); MAX_SCHEDULED];
        let mut scheduled = 0;
        for event in events.input {
            if let Some(note) = event.as_event::<NoteOnEvent>()
                && let Some(turned) = own_knob(note.pckn().key.into_specific())
            {
                // Not a note: the plugin's own window moving its level, which
                // a host hears only if it listens — see `TURNS_AND_SAYS`.
                store(&self.shared.level, turned as f32);
                if note.pckn().key.into_specific() == Some(u16::from(TURNS_AND_SAYS)) {
                    let _ = events.output.try_push(ParamValueEvent::new(
                        event.header().time(),
                        ClapId::new(7),
                        clack_plugin::events::Pckn::match_all(),
                        turned,
                        Default::default(),
                    ));
                }
            } else if let Some(note) = event.as_event::<NoteOnEvent>() {
                if scheduled < MAX_SCHEDULED {
                    let key = note.pckn().key.into_specific().unwrap_or(60);
                    schedule[scheduled] = (event.header().time(), key.min(127) as u8 | 0x80);
                    scheduled += 1;
                }
            } else if event.as_event::<NoteOffEvent>().is_some() {
                if scheduled < MAX_SCHEDULED {
                    schedule[scheduled] = (event.header().time(), 0);
                    scheduled += 1;
                }
            } else if let Some(value) = event.as_event::<ParamValueEvent>() {
                match u32::from(value.param_id().unwrap_or(ClapId::new(7))) {
                    OUTPUT_PARAM => store(&self.shared.output, value.value() as f32),
                    _ => store(&self.shared.level, value.value() as f32),
                }
            } else if let Some(midi) = event.as_event::<MidiEvent>() {
                // The wheels as MIDI — what a host sends a port that takes
                // it. Applied for the block: a fixture's wheel need not be
                // sample-accurate to be heard.
                let data = midi.data();
                match data[0] & 0xF0 {
                    0xB0 if data[1] == 1 => self.wheel = f32::from(data[2] & 0x7F) / 127.0,
                    0xD0 => self.pressure = f32::from(data[1] & 0x7F) / 127.0,
                    0xE0 => {
                        let raw = (i32::from(data[2] & 0x7F) << 7) | i32::from(data[1] & 0x7F);
                        self.bend = (raw - 8192) as f32 / 8192.0 * BEND_RANGE_SEMITONES;
                    }
                    _ => {}
                }
            } else if let Some(expression) = event.as_event::<NoteExpressionEvent>() {
                // And as note expressions — what a host sends a port that
                // takes only CLAP's own events.
                match expression.expression_type() {
                    Some(NoteExpressionType::Vibrato) => self.wheel = expression.value() as f32,
                    Some(NoteExpressionType::Pressure) => {
                        self.pressure = expression.value() as f32;
                    }
                    Some(NoteExpressionType::Tuning) => self.bend = expression.value() as f32,
                    _ => {}
                }
            }
        }

        // Handed fewer ports than it declared, it renders nothing — see the
        // note on `PluginAudioPortsImpl`. Silence rather than an error, so a
        // host that got this wrong hears the problem rather than reading it.
        let refused = audio.output_port_count() < 2 || !ordered || untimed;
        let level = if refused {
            0.0
        } else {
            load(&self.shared.level) * load(&self.shared.output)
        } * self.wheel
            * (1.0 - self.pressure.clamp(0.0, 1.0));
        let bend = self.bend;
        let sample_rate = self.sample_rate;
        let mut end_key = self.key;
        let mut end_phase = self.phase;

        for mut port in &mut audio {
            let Some(channels) = port.channels()?.into_f32() else {
                continue;
            };
            for pair in channels {
                let buffer = match pair {
                    ChannelPair::OutputOnly(buffer) => buffer,
                    ChannelPair::InputOutput(_, buffer) => buffer,
                    ChannelPair::InPlace(buffer) => buffer,
                    ChannelPair::InputOnly(_) => continue,
                };
                let mut key = self.key;
                let mut phase = self.phase;
                let mut next = 0;
                for (frame, sample) in buffer.iter_mut().enumerate() {
                    while next < scheduled && schedule[next].0 as usize <= frame {
                        let (_, what) = schedule[next];
                        if what & 0x80 != 0 {
                            key = Some((what & 0x7f) as u16);
                            phase = 0.0;
                        } else {
                            key = None;
                        }
                        next += 1;
                    }
                    match key {
                        Some(key) => {
                            let step = 440.0 * 2.0f32.powf((key as f32 - 69.0 + bend) / 12.0)
                                / sample_rate
                                * std::f32::consts::TAU;
                            *sample = phase.sin() * level;
                            phase += step;
                        }
                        None => *sample = 0.0,
                    }
                }
                end_key = key;
                end_phase = phase;
            }
        }

        self.key = end_key;
        self.phase = end_phase;

        Ok(ProcessStatus::Continue)
    }
}

/// The most notes the fixture keeps track of in one block.
const MAX_SCHEDULED: usize = 64;

/// The sine's ports: a main pair, and a mono **sub** beside it.
///
/// The second port is there because the OneTrick drum synths have one — an
/// individual output beside the main pair — and a host that hands over only
/// the main port crashes them (see `every_port_a_plugin_declares_is_handed_
/// over`). The sine **refuses to render** when handed fewer ports than it
/// declared, which is the observable form of that rule.
impl<const MIDI: bool> PluginAudioPortsImpl for SineMain<'_, MIDI> {
    fn count(&mut self, is_input: bool) -> u32 {
        if is_input { 0 } else { 2 }
    }

    fn get(&mut self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if is_input {
            return;
        }
        match index {
            0 => writer.set(&AudioPortInfo {
                id: 0u32.into(),
                name: b"main",
                channel_count: 2,
                flags: AudioPortFlags::IS_MAIN,
                port_type: Some(AudioPortType::STEREO),
                in_place_pair: None,
            }),
            1 => writer.set(&AudioPortInfo {
                id: 1u32.into(),
                name: b"sub",
                channel_count: 1,
                flags: AudioPortFlags::empty(),
                port_type: Some(AudioPortType::MONO),
                in_place_pair: None,
            }),
            _ => {}
        }
    }
}

/// The note port: CLAP's own events, and — under the MIDI-speaking id —
/// raw MIDI beside them. **Preferred CLAP either way**, because the
/// preference is about notes and a host that reads MIDI *support* off the
/// preference would send this one expressions it also understands.
impl<const MIDI: bool> PluginNotePortsImpl for SineMain<'_, MIDI> {
    fn count(&mut self, is_input: bool) -> u32 {
        if is_input { 1 } else { 0 }
    }

    fn get(&mut self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if !is_input || index != 0 {
            return;
        }
        writer.set(&NotePortInfo {
            id: 0u32.into(),
            name: b"notes",
            preferred_dialect: Some(NoteDialect::Clap),
            supported_dialects: if MIDI {
                NoteDialects::CLAP | NoteDialects::MIDI
            } else {
                NoteDialects::CLAP
            },
        });
    }
}

impl PluginAudioProcessorParams for SineProcessor<'_> {
    fn flush(&mut self, input_events: &InputEvents, _output_events: &mut OutputEvents) {
        for event in input_events {
            if let Some(value) = event.as_event::<ParamValueEvent>() {
                match u32::from(value.param_id().unwrap_or(ClapId::new(7))) {
                    OUTPUT_PARAM => store(&self.shared.output, value.value() as f32),
                    _ => store(&self.shared.level, value.value() as f32),
                }
            }
        }
    }
}

/// A key that is not a note to the sine but a knob turned **in its own
/// window**: the level goes to [`OWN_LEVEL_SAID`], and the plugin reports it
/// the way CLAP says a plugin reports a change it made itself — an output
/// event. A host that throws a plugin's output events away never hears it,
/// and saves the level it last set instead.
/// When this names a file, every instance of the sine made appends a line —
/// so a test can count them (a render that opened a second one of the
/// studio's plugins, which some plugins do not survive).
pub const CREATE_LOG_ENV: &str = "FONTELLE_TESTPLUG_CREATE_LOG";

/// When this names a file, every render mode the sine is set to appends a
/// line — `offline` or `realtime` — so a test can see a render tell it.
pub const RENDER_LOG_ENV: &str = "FONTELLE_TESTPLUG_RENDER_LOG";

impl<const MIDI: bool> clack_extensions::render::PluginRenderImpl for SineMain<'_, MIDI> {
    fn has_hard_realtime_requirement(&self) -> bool {
        false
    }

    fn set(&mut self, mode: clack_extensions::render::RenderMode) -> Result<(), PluginError> {
        if let Some(file) = std::env::var_os(RENDER_LOG_ENV) {
            use std::io::Write;
            if let Ok(mut out) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(file)
            {
                let said = match mode {
                    clack_extensions::render::RenderMode::Offline => "offline\n",
                    clack_extensions::render::RenderMode::Realtime => "realtime\n",
                };
                let _ = out.write_all(said.as_bytes());
            }
        }
        Ok(())
    }
}

pub const TURNS_AND_SAYS: u8 = 1;
pub const OWN_LEVEL_SAID: f64 = 0.125;

/// And one it does **not** report: a preset chosen in the plugin's own
/// browser, which a host only learns by asking. The level goes to
/// [`OWN_LEVEL_UNSAID`].
pub const TURNS_QUIETLY: u8 = 2;
pub const OWN_LEVEL_UNSAID: f64 = 0.375;

fn own_knob(key: Option<u16>) -> Option<f64> {
    match key? {
        k if k == u16::from(TURNS_AND_SAYS) => Some(OWN_LEVEL_SAID),
        k if k == u16::from(TURNS_QUIETLY) => Some(OWN_LEVEL_UNSAID),
        _ => None,
    }
}

/// The sine's second parameter — see [`SineShared::output`].
pub const OUTPUT_PARAM: u32 = 8;

impl<const MIDI: bool> PluginMainThreadParams for SineMain<'_, MIDI> {
    fn count(&mut self) -> u32 {
        2
    }

    fn get_info(&mut self, param_index: u32, info: &mut ParamInfoWriter) {
        match param_index {
            0 => info.set(&ParamInfo {
                id: 7u32.into(),
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Level",
                module: b"",
                min_value: 0.0,
                max_value: 1.0,
                default_value: 0.5,
            }),
            // **Zero, and a lie.** See `SineShared::output`.
            1 => info.set(&ParamInfo {
                id: OUTPUT_PARAM.into(),
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Output",
                module: b"",
                min_value: 0.0,
                max_value: 1.0,
                default_value: 0.0,
            }),
            _ => {}
        }
    }

    fn get_value(&mut self, param_id: ClapId) -> Option<f64> {
        match u32::from(param_id) {
            7 => Some(load(&self.shared.level) as f64),
            OUTPUT_PARAM => Some(load(&self.shared.output) as f64),
            _ => None,
        }
    }

    fn value_to_text(
        &mut self,
        _param_id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> std::fmt::Result {
        use std::fmt::Write;
        write!(writer, "{value:.2}")
    }

    fn text_to_value(&mut self, _param_id: ClapId, text: &CStr) -> Option<f64> {
        text.to_str().ok()?.parse().ok()
    }

    fn flush(&mut self, input_events: &InputEvents, _output_events: &mut OutputEvents) {
        for event in input_events {
            if let Some(value) = event.as_event::<ParamValueEvent>() {
                match u32::from(value.param_id().unwrap_or(ClapId::new(7))) {
                    OUTPUT_PARAM => store(&self.shared.output, value.value() as f32),
                    _ => store(&self.shared.level, value.value() as f32),
                }
            }
        }
    }
}

// ------------------------------------------------------------------- the entry

// ------------------------------------------------------ a plugin with a face

/// A plugin with an editor of its own, whose `show` says **no**.
///
/// The way clap-helpers' default `guiShow` does — it returns `false` unless a
/// plugin overrides it, and SpectMorph and the OneTrick drum synths never do:
/// they put their window up in `set_parent` and have nothing left to show. A
/// host that reads that `false` as failure tears down the editor it has just
/// embedded, and the window with it, which is *"just closing their window as
/// soon as it opens"*. This fixture answers exactly the way they do, so that
/// rule is a test rather than a plugin somebody has installed.
///
/// A **note effect** rather than an instrument or an audio effect, so it
/// appears on neither of the two lists the studio's menus count.
pub struct FacePlugin;

pub struct FaceShared;

impl PluginShared<'_> for FaceShared {}

pub struct FaceMain {
    /// Whether the host embedded it. Reset by `destroy`, as a real one is.
    parented: bool,
}

impl PluginMainThread<'_, FaceShared> for FaceMain {}

impl Plugin for FacePlugin {
    type AudioProcessor<'a> = FaceProcessor;
    type Shared<'a> = FaceShared;
    type MainThread<'a> = FaceMain;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&FaceShared>) {
        builder.register::<PluginGui>();
    }
}

impl DefaultPluginFactory for FacePlugin {
    fn get_descriptor() -> PluginDescriptor {
        PluginDescriptor::new("com.fopull.fontelle.testface", "Fontelle Test Face")
            .with_vendor("Fopull LLC")
            .with_version("1.0.0")
            .with_features([clack_plugin::plugin::features::NOTE_EFFECT])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<FaceShared, PluginError> {
        Ok(FaceShared)
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        _shared: &'a FaceShared,
    ) -> Result<FaceMain, PluginError> {
        Ok(FaceMain { parented: false })
    }
}

pub struct FaceProcessor;

impl<'a> PluginAudioProcessor<'a, FaceShared, FaceMain> for FaceProcessor {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &mut FaceMain,
        _shared: &'a FaceShared,
        _config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self)
    }

    fn process(
        &mut self,
        _process: Process,
        _audio: Audio,
        _events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        Ok(ProcessStatus::Continue)
    }
}

/// How big the face says it is.
pub const FACE_WIDTH: u32 = 320;
pub const FACE_HEIGHT: u32 = 200;

/// The window kind the face embeds into: the platform's own, which is what
/// the plugins it stands for offer and what the host asks for
/// (`fontelle_host`'s `embedded_api`). X11 alone was a face that had no
/// editor on Windows — CI: *"assertion failed: plugin.has_editor()"*.
fn face_api() -> GuiApiType<'static> {
    if cfg!(windows) {
        GuiApiType::WIN32
    } else if cfg!(target_os = "macos") {
        GuiApiType::COCOA
    } else {
        GuiApiType::X11
    }
}

impl PluginGuiImpl for FaceMain {
    fn is_api_supported(&mut self, configuration: GuiConfiguration) -> bool {
        configuration.api_type == face_api() && !configuration.is_floating
    }

    fn get_preferred_api(&mut self) -> Option<GuiConfiguration<'_>> {
        Some(GuiConfiguration {
            api_type: face_api(),
            is_floating: false,
        })
    }

    fn create(&mut self, configuration: GuiConfiguration) -> Result<(), PluginError> {
        if self.is_api_supported(configuration) {
            Ok(())
        } else {
            Err(PluginError::Message("not that kind of window"))
        }
    }

    fn destroy(&mut self) {
        self.parented = false;
    }

    fn set_scale(&mut self, _scale: f64) -> Result<(), PluginError> {
        Ok(())
    }

    fn get_size(&mut self) -> Option<GuiSize> {
        Some(GuiSize {
            width: FACE_WIDTH,
            height: FACE_HEIGHT,
        })
    }

    fn set_size(&mut self, _size: GuiSize) -> Result<(), PluginError> {
        Ok(())
    }

    fn set_parent(&mut self, _window: Window) -> Result<(), PluginError> {
        self.parented = true;
        Ok(())
    }

    fn set_transient(&mut self, _window: Window) -> Result<(), PluginError> {
        Ok(())
    }

    /// **`false`, like clap-helpers' default.** The window went up in
    /// `set_parent`; there is nothing left to show, and the plugin says so
    /// the only way the API lets it.
    fn show(&mut self) -> Result<(), PluginError> {
        Err(PluginError::Message(
            "nothing to show: it went up in set_parent",
        ))
    }

    fn hide(&mut self) -> Result<(), PluginError> {
        Err(PluginError::Message("nothing to hide"))
    }
}

pub struct TestEntry {
    factory: PluginFactoryWrapper<TestFactory>,
    presets:
        clack_extensions::preset_discovery::prelude::PresetDiscoveryFactoryWrapper<TestPresets>,
}

/// A copy of this bundle whose file name contains this **crashes** as it is
/// loaded — what a broken plugin somewhere on a user's disk does to a host
/// that scans in its own process.
pub const CRASHES_ON_LOAD: &str = "crashes-on-load";
/// And one whose name contains this never comes back from being loaded.
pub const HANGS_ON_LOAD: &str = "hangs-on-load";

impl Entry for TestEntry {
    fn new(bundle_path: Option<&CStr>) -> Result<Self, EntryLoadError> {
        let named = bundle_path
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        if named.contains(CRASHES_ON_LOAD) {
            std::process::abort();
        }
        if named.contains(HANGS_ON_LOAD) {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        Ok(Self {
            factory: PluginFactoryWrapper::new(TestFactory::new()),
            presets:
                clack_extensions::preset_discovery::prelude::PresetDiscoveryFactoryWrapper::new(
                    TestPresets::new(),
                ),
        })
    }

    fn declare_factories<'a>(&'a self, builder: &mut EntryFactories<'a>) {
        builder.register_factory(&self.factory);
        builder.register_factory(&self.presets);
    }
}

pub struct TestFactory {
    gain: PluginDescriptor,
    sine: PluginDescriptor,
    face: PluginDescriptor,
    /// The sine again, speaking only CLAP — see [`SINE_CLAP_ONLY`].
    sine_clap: PluginDescriptor,
}

impl TestFactory {
    fn new() -> Self {
        Self {
            gain: GainPlugin::get_descriptor(),
            sine: SinePlugin::<true>::get_descriptor(),
            face: FacePlugin::get_descriptor(),
            sine_clap: SinePlugin::<false>::get_descriptor(),
        }
    }
}

impl PluginFactoryImpl for TestFactory {
    fn plugin_count(&self) -> u32 {
        4
    }

    fn plugin_descriptor(&self, index: u32) -> Option<&PluginDescriptor> {
        match index {
            0 => Some(&self.gain),
            1 => Some(&self.sine),
            2 => Some(&self.face),
            3 => Some(&self.sine_clap),
            _ => None,
        }
    }

    fn create_plugin<'a>(
        &'a self,
        host_info: HostInfo<'a>,
        plugin_id: &CStr,
    ) -> Option<PluginInstance<'a>> {
        if plugin_id == self.gain.id().unwrap_or_default() {
            Some(PluginInstance::new::<GainPlugin>(
                host_info,
                &self.gain,
                |host| GainPlugin::new_shared(host),
                |host, shared| GainPlugin::new_main_thread(host, shared),
            ))
        } else if plugin_id == self.sine.id().unwrap_or_default() {
            Some(PluginInstance::new::<SinePlugin<true>>(
                host_info,
                &self.sine,
                |host| SinePlugin::<true>::new_shared(host),
                |host, shared| SinePlugin::<true>::new_main_thread(host, shared),
            ))
        } else if plugin_id == self.sine_clap.id().unwrap_or_default() {
            Some(PluginInstance::new::<SinePlugin<false>>(
                host_info,
                &self.sine_clap,
                |host| SinePlugin::<false>::new_shared(host),
                |host, shared| SinePlugin::<false>::new_main_thread(host, shared),
            ))
        } else if plugin_id == self.face.id().unwrap_or_default() {
            Some(PluginInstance::new::<FacePlugin>(
                host_info,
                &self.face,
                |host| FacePlugin::new_shared(host),
                |host, shared| FacePlugin::new_main_thread(host, shared),
            ))
        } else {
            None
        }
    }
}

clack_export_entry!(TestEntry);
