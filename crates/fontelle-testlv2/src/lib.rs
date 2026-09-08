//! Two tiny LV2 plugins, built only so [`fontelle_host`] has a real LV2
//! bundle to load.
//!
//! **This is a test fixture, not a product**, and it is `fontelle-testplug`'s
//! argument made a second time: a host is almost entirely foreign-function
//! boundary, and the only honest test of one loads a real plugin through the
//! real entry point across the real ABI. LV2's ABI is a C struct of function
//! pointers found by `lv2_descriptor`, and a bundle is a *folder* — the
//! shared library beside two Turtle files that describe it — so this crate
//! is both halves: the `cdylib`, and the two `.ttl` files as constants for a
//! test helper to write beside it.
//!
//! Written against the raw C structs rather than a plugin framework, because
//! the point is that the host reads what any LV2 plugin gives it, and a
//! framework's idea of a plugin is one more thing between the test and the
//! ABI.
//!
//! - **Fontelle Test Gain LV2** — an effect. Multiplies by a control port,
//!   inverts the phase from a toggled one, and keeps **state no port
//!   exposes** through `state:interface`: how many blocks it has run, and
//!   the folder it lives in as an `atom:Path` that goes through the host's
//!   `state:mapPath`. See [`STATE_RUNS_KEY`].
//! - **Fontelle Test Sine LV2** — an instrument. One sine at the last key it
//!   was given, at a level a control port sets, read out of an atom
//!   sequence of MIDI events **at the frame each one carries** — which is
//!   how a host's note placement is checked at all. Requires `urid:map`,
//!   because that is how a plugin learns which atoms are MIDI. It also
//!   hears the **wheels**: the mod wheel (CC 1) scales its level, a pitch
//!   bend bends it two semitones either way, and channel pressure *ducks*
//!   it — the opposite of the wheel, so a host that sent one as the other
//!   is told apart from one that got it right.

use std::ffi::{CStr, c_char, c_void};

use lv2_raw::{
    LV2AtomEvent, LV2AtomSequence, LV2Descriptor, LV2Feature, LV2Handle, LV2UIControllerRaw,
    LV2UIDescriptorRaw, LV2UIHandle, LV2UIIdleInterface, LV2UIWidget, LV2UIWriteFunctionRaw,
    LV2UridMap,
};

/// The bundle's manifest, and the file it points at. A test helper writes
/// both beside the built library to make a bundle the way an installer would.
pub const MANIFEST_TTL: &str = include_str!("../bundle/manifest.ttl");
pub const PLUGIN_TTL: &str = include_str!("../bundle/testlv2.ttl");
/// What the manifest calls the shared library.
pub const BINARY_NAME: &str = "fontelle_testlv2.so";

pub const GAIN_URI: &str = "http://fopull.com/fontelle/testlv2/gain";
/// The gain again, under a name that ships **no editor**.
///
/// The case a host has to survive as much as the other one: a plugin whose
/// only face is the one the host draws for it. Calf's whole suite is this.
pub const PLAIN_URI: &str = "http://fopull.com/fontelle/testlv2/plain";
pub const SINE_URI: &str = "http://fopull.com/fontelle/testlv2/sine";
/// The gain a third time, with an editor that **listens to nothing**.
///
/// Its UI descriptor has no `port_event` and no `extension_data` — both of
/// which LV2 allows to be NULL, and JuceOPL's are. A host that calls either
/// without looking dies at address zero, which the studio did three times in
/// one minute.
pub const DEAF_URI: &str = "http://fopull.com/fontelle/testlv2/deaf";
pub const DEAF_UI_URI: &str = "http://fopull.com/fontelle/testlv2/deaf#ui";

/// What the gain stores under `state:interface`: how many times `run` has
/// been called, as an `atom:Int`. **No control port exposes it**, which is the
/// point — a parameter would come back through the parameter wire whether or
/// not the host read the plugin's own state.
pub const STATE_RUNS_KEY: &str = "http://fopull.com/fontelle/testlv2/gain#runs";
/// And the bundle folder it was instantiated from, as an `atom:Path`.
///
/// Stored **through `state:mapPath`** when the host offers it — the way a
/// sampler stores the file it loaded — and checked on restore: a restore whose
/// path does not map back to the folder the plugin lives in is **refused**, so
/// a host's path round trip is asserted by the plugin itself.
pub const STATE_HOME_KEY: &str = "http://fopull.com/fontelle/testlv2/gain#home";

const GAIN_URI_C: &CStr = c"http://fopull.com/fontelle/testlv2/gain";
const PLAIN_URI_C: &CStr = c"http://fopull.com/fontelle/testlv2/plain";
const SINE_URI_C: &CStr = c"http://fopull.com/fontelle/testlv2/sine";
const DEAF_URI_C: &CStr = c"http://fopull.com/fontelle/testlv2/deaf";
const URID_MAP_URI: &CStr = c"http://lv2plug.in/ns/ext/urid#map";
const MIDI_EVENT_URI: &CStr = c"http://lv2plug.in/ns/ext/midi#MidiEvent";
const ATOM_SEQUENCE_URI: &CStr = c"http://lv2plug.in/ns/ext/atom#Sequence";
const ATOM_INT_URI: &CStr = c"http://lv2plug.in/ns/ext/atom#Int";
const ATOM_PATH_URI: &CStr = c"http://lv2plug.in/ns/ext/atom#Path";
const STATE_INTERFACE_URI: &CStr = c"http://lv2plug.in/ns/ext/state#interface";
const STATE_MAP_PATH_URI: &CStr = c"http://lv2plug.in/ns/ext/state#mapPath";
const STATE_FREE_PATH_URI: &CStr = c"http://lv2plug.in/ns/ext/state#freePath";
const STATE_RUNS_KEY_C: &CStr = c"http://fopull.com/fontelle/testlv2/gain#runs";
const STATE_HOME_KEY_C: &CStr = c"http://fopull.com/fontelle/testlv2/gain#home";

/// The `urid:map` feature out of a feature array, if the host passed one.
///
/// # Safety
/// `features` is a NULL-terminated array of pointers to valid features, per
/// the LV2 contract, or null.
unsafe fn find_urid_map<'a>(features: *const *const LV2Feature) -> Option<&'a LV2UridMap> {
    let mut cursor = features;
    unsafe {
        while !cursor.is_null() && !(*cursor).is_null() {
            let feature = &**cursor;
            if !feature.uri.is_null() && CStr::from_ptr(feature.uri) == URID_MAP_URI {
                return feature.data.cast::<LV2UridMap>().as_ref();
            }
            cursor = cursor.add(1);
        }
    }
    None
}

// ------------------------------------------------------------------ gain

struct Gain {
    /// First, so an editor handed the instance can check it is looking at
    /// one — see [`INSTANCE_MAGIC`].
    magic: u32,
    input: *const f32,
    output: *mut f32,
    gain: *const f32,
    invert: *const f32,
    /// The sidechain (port 4, `lv2:isSideChain`), or null on a plugin whose
    /// Turtle declares none — the plain one — or under a host that never
    /// connected it. The gain ducks by it: see `gain_run`.
    key: *const f32,
    /// How many blocks have run — the state no port exposes. See
    /// [`STATE_RUNS_KEY`].
    runs: i32,
    /// The folder this was instantiated from, as the host gave it. See
    /// [`STATE_HOME_KEY`].
    home: std::ffi::CString,
    /// The URIDs the state is stored under, or zero when the host gave no
    /// `urid:map` — in which case there is nothing to store it as, and the
    /// state interface stores nothing.
    runs_key: u32,
    home_key: u32,
    int_type: u32,
    path_type: u32,
}

/// What a [`Gain`] instance starts with, and what its editor looks for
/// through `instance-access`: a host that hands the editor anything but the
/// live instance hands it something without this at the front.
pub const INSTANCE_MAGIC: u32 = 0x4641_4345;

extern "C" fn gain_instantiate(
    _descriptor: *const LV2Descriptor,
    _rate: f64,
    bundle_path: *const c_char,
    features: *const *const LV2Feature,
) -> LV2Handle {
    // SAFETY: LV2's contract for `instantiate` — the bundle path is a valid C
    // string and the features a NULL-terminated array of valid features.
    let (runs_key, home_key, int_type, path_type) = match unsafe { find_urid_map(features) } {
        Some(map) => (
            (map.map)(map.handle, STATE_RUNS_KEY_C.as_ptr()),
            (map.map)(map.handle, STATE_HOME_KEY_C.as_ptr()),
            (map.map)(map.handle, ATOM_INT_URI.as_ptr()),
            (map.map)(map.handle, ATOM_PATH_URI.as_ptr()),
        ),
        None => (0, 0, 0, 0),
    };
    let home = if bundle_path.is_null() {
        std::ffi::CString::default()
    } else {
        unsafe { CStr::from_ptr(bundle_path) }.to_owned()
    };
    Box::into_raw(Box::new(Gain {
        magic: INSTANCE_MAGIC,
        input: std::ptr::null(),
        output: std::ptr::null_mut(),
        gain: std::ptr::null(),
        invert: std::ptr::null(),
        key: std::ptr::null(),
        runs: 0,
        home,
        runs_key,
        home_key,
        int_type,
        path_type,
    }))
    .cast()
}

extern "C" fn gain_connect_port(handle: LV2Handle, port: u32, data: *mut c_void) {
    // SAFETY: `handle` is what `gain_instantiate` returned and has not been
    // cleaned up — the host's obligation under the LV2 contract.
    let gain = unsafe { &mut *handle.cast::<Gain>() };
    match port {
        0 => gain.input = data.cast(),
        1 => gain.output = data.cast(),
        2 => gain.gain = data.cast(),
        3 => gain.invert = data.cast(),
        4 => gain.key = data.cast(),
        _ => {}
    }
}

extern "C" fn gain_run(handle: LV2Handle, samples: u32) {
    // SAFETY: as above, and every port was connected before `run` — the
    // host's obligation again. A null port is treated as "not connected"
    // rather than dereferenced, which is more than the contract requires.
    let gain = unsafe { &mut *handle.cast::<Gain>() };
    gain.runs = gain.runs.wrapping_add(1);
    if gain.input.is_null() || gain.output.is_null() {
        return;
    }
    let factor = unsafe { gain.gain.as_ref().copied().unwrap_or(1.0) };
    let invert = unsafe { gain.invert.as_ref().copied().unwrap_or(0.0) } >= 0.5;
    let sign = if invert { -1.0 } else { 1.0 };
    let input = unsafe { std::slice::from_raw_parts(gain.input, samples as usize) };
    let output = unsafe { std::slice::from_raw_parts_mut(gain.output, samples as usize) };
    // The key **ducks** the signal, sample for sample: none of it through at
    // full scale, all of it at silence. Heard rather than detected, so a
    // host that put the bus on the key port by mistake would be heard
    // silencing itself — see the CLAP fixture's `duck`.
    let key = (!gain.key.is_null())
        .then(|| unsafe { std::slice::from_raw_parts(gain.key, samples as usize) });
    for (frame, (o, i)) in output.iter_mut().zip(input).enumerate() {
        let duck = key.map_or(1.0, |key| 1.0 - key[frame].abs().min(1.0));
        *o = i * factor * sign * duck;
    }
}

extern "C" fn gain_cleanup(handle: LV2Handle) {
    // SAFETY: the host calls this exactly once, after which it never uses
    // the handle again.
    drop(unsafe { Box::from_raw(handle.cast::<Gain>()) });
}

// ------------------------------------------------------------------ sine

struct Sine {
    midi_in: *const LV2AtomSequence,
    /// Where it echoes whatever MIDI it was given.
    ///
    /// **A plugin that answers.** LSP's sampler tells its editor which file it
    /// actually loaded by writing an atom back, and a host that carries atoms
    /// one way only leaves that editor showing an empty slot forever. This is
    /// the smallest plugin that says anything at all, so the host's other
    /// direction has something to carry.
    midi_out: *mut LV2AtomSequence,
    /// How big that port's buffer is, which the host sets before every run.
    midi_out_capacity: usize,
    output: *mut f32,
    level: *const f32,
    /// The URID the host gave `midi:MidiEvent`, so events can be told apart.
    midi_urid: u32,
    /// And the one it gave `atom:Sequence`.
    ///
    /// A host hands an output atom port over as a **chunk** — "here are N
    /// bytes, do what you like" — and a plugin that writes events into it must
    /// say so by stamping it a sequence. One that does not has written a
    /// sequence no host will read, which is a bug worth having a fixture for.
    sequence_urid: u32,
    rate: f32,
    phase: f32,
    /// The key sounding, if one is.
    key: Option<u8>,
    /// The mod wheel, `0..=1`, scaling the level. Full until told otherwise.
    wheel: f32,
    /// Channel pressure, `0..=1`, **ducking** the level — see the crate note.
    pressure: f32,
    /// The pitch bend, in semitones, over two either way.
    bend: f32,
}

extern "C" fn sine_instantiate(
    _descriptor: *const LV2Descriptor,
    rate: f64,
    _bundle_path: *const c_char,
    features: *const *const LV2Feature,
) -> LV2Handle {
    // Find `urid:map`; refuse to start without it, which is what the plugin's
    // `lv2:requiredFeature` told the host to expect.
    let mut midi_urid = 0;
    let mut sequence_urid = 0;
    let mut cursor = features;
    // SAFETY: `features` is a NULL-terminated array of pointers to valid
    // features, per the LV2 contract for `instantiate`.
    unsafe {
        while !cursor.is_null() && !(*cursor).is_null() {
            let feature = &**cursor;
            if !feature.uri.is_null() && CStr::from_ptr(feature.uri) == URID_MAP_URI {
                let map = &*feature.data.cast::<LV2UridMap>();
                midi_urid = (map.map)(map.handle, MIDI_EVENT_URI.as_ptr());
                sequence_urid = (map.map)(map.handle, ATOM_SEQUENCE_URI.as_ptr());
            }
            cursor = cursor.add(1);
        }
    }
    if midi_urid == 0 {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(Sine {
        midi_in: std::ptr::null(),
        midi_out: std::ptr::null_mut(),
        midi_out_capacity: 0,
        output: std::ptr::null_mut(),
        level: std::ptr::null(),
        midi_urid,
        sequence_urid,
        rate: rate as f32,
        phase: 0.0,
        key: None,
        wheel: 1.0,
        pressure: 0.0,
        bend: 0.0,
    }))
    .cast()
}

extern "C" fn sine_connect_port(handle: LV2Handle, port: u32, data: *mut c_void) {
    // SAFETY: see `gain_connect_port`.
    let sine = unsafe { &mut *handle.cast::<Sine>() };
    match port {
        0 => sine.midi_in = data.cast(),
        1 => sine.output = data.cast(),
        2 => sine.level = data.cast(),
        3 => sine.midi_out = data.cast(),
        _ => {}
    }
}

extern "C" fn sine_run(handle: LV2Handle, samples: u32) {
    // SAFETY: see `gain_run`.
    let sine = unsafe { &mut *handle.cast::<Sine>() };
    if sine.output.is_null() {
        return;
    }
    let level = unsafe { sine.level.as_ref().copied().unwrap_or(0.5) };
    let output = unsafe { std::slice::from_raw_parts_mut(sine.output, samples as usize) };

    // Walk the events, rendering up to each one before applying it: that is
    // what makes the note land on the frame the host put it on rather than at
    // the top of the block.
    let mut events: Vec<(usize, [u8; 3])> = Vec::new();
    if !sine.midi_in.is_null() {
        // SAFETY: an atom port is connected to an `LV2_Atom_Sequence` whose
        // `atom.size` bytes of body follow it; the events inside are padded to
        // eight bytes, which is what `pad` does below.
        unsafe {
            let sequence = &*sine.midi_in;
            let body_size = sequence.atom.size as usize;
            let body_start =
                (sine.midi_in as *const u8).add(std::mem::size_of::<LV2AtomSequence>());
            let mut offset = std::mem::size_of::<lv2_raw::LV2AtomSequenceBody>();
            while offset + std::mem::size_of::<LV2AtomEvent>() <= body_size {
                let event = &*body_start
                    .add(offset - std::mem::size_of::<lv2_raw::LV2AtomSequenceBody>())
                    .cast::<LV2AtomEvent>();
                let data = (event as *const LV2AtomEvent as *const u8)
                    .add(std::mem::size_of::<LV2AtomEvent>());
                if event.body.type_ == sine.midi_urid && event.body.size >= 3 {
                    let bytes = [*data, *data.add(1), *data.add(2)];
                    events.push((event.time_in_frames.max(0) as usize, bytes));
                }
                let total = std::mem::size_of::<LV2AtomEvent>() + event.body.size as usize;
                offset += (total + 7) & !7;
            }
        }
    }

    // Whatever it heard, said back — see `Sine::midi_out`.
    sine.echo(&events);

    let mut at = 0;
    for (frame, bytes) in events
        .into_iter()
        .chain(std::iter::once((samples as usize, [0; 3])))
    {
        let frame = frame.min(samples as usize);
        sine.render(&mut output[at..frame], level);
        at = frame;
        match bytes[0] & 0xF0 {
            0x90 if bytes[2] > 0 => sine.key = Some(bytes[1]),
            0x80 | 0x90 => {
                if sine.key == Some(bytes[1]) {
                    sine.key = None;
                }
            }
            // All notes off, which is the host's "reset".
            0xB0 if bytes[1] == 123 => sine.key = None,
            // The mod wheel — see the crate note.
            0xB0 if bytes[1] == 1 => sine.wheel = f32::from(bytes[2] & 0x7F) / 127.0,
            0xD0 => sine.pressure = f32::from(bytes[1] & 0x7F) / 127.0,
            0xE0 => {
                let raw = (i32::from(bytes[2] & 0x7F) << 7) | i32::from(bytes[1] & 0x7F);
                sine.bend = (raw - 8192) as f32 / 8192.0 * BEND_RANGE_SEMITONES;
            }
            _ => {}
        }
    }
}

/// How far a full bend goes, in semitones — a keyboard's default.
pub const BEND_RANGE_SEMITONES: f32 = 2.0;

impl Sine {
    fn render(&mut self, out: &mut [f32], level: f32) {
        let Some(key) = self.key else {
            out.fill(0.0);
            return;
        };
        let hz = 440.0 * 2f32.powf((key as f32 - 69.0 + self.bend) / 12.0);
        let level = level * self.wheel * (1.0 - self.pressure);
        for sample in out {
            *sample = (self.phase * std::f32::consts::TAU).sin() * level;
            self.phase = (self.phase + hz / self.rate).fract();
        }
    }

    /// Writes every event it was given straight back out again.
    ///
    /// An atom sequence built by hand, because that is what the port is: a
    /// header saying how many bytes of body follow, then events each padded
    /// out to eight. The host reads it the same way it reads any plugin's.
    fn echo(&mut self, events: &[(usize, [u8; 3])]) {
        if self.midi_out.is_null() {
            return;
        }
        // SAFETY: the host connects this port to a buffer at least
        // `midi_out_capacity` bytes long and sets `atom.size` to the capacity
        // of the body before every run, which is the LV2 contract for an
        // output atom port.
        unsafe {
            let sequence = &mut *self.midi_out;
            let capacity = self.midi_out_capacity.max(sequence.atom.size as usize);
            // **Stamped a sequence.** The host handed this over as a chunk;
            // a plugin that writes events into one and leaves it a chunk has
            // written a sequence nobody will read.
            sequence.atom.type_ = self.sequence_urid;
            sequence.body.unit = 0;
            sequence.body.pad = 0;
            let body_start = (self.midi_out as *mut u8).add(size_of::<LV2AtomSequence>());
            let mut written = 0usize;
            for (frame, bytes) in events {
                let total = size_of::<LV2AtomEvent>() + 3;
                let padded = (total + 7) & !7;
                if size_of::<lv2_raw::LV2AtomSequenceBody>() + written + padded > capacity {
                    break;
                }
                let event = body_start.add(written).cast::<LV2AtomEvent>();
                (*event).time_in_frames = *frame as i64;
                (*event).body.type_ = self.midi_urid;
                (*event).body.size = 3;
                let data = (event as *mut u8).add(size_of::<LV2AtomEvent>());
                for (index, byte) in bytes.iter().enumerate() {
                    *data.add(index) = *byte;
                }
                written += padded;
            }
            sequence.atom.size = (size_of::<lv2_raw::LV2AtomSequenceBody>() + written) as u32;
        }
    }
}

extern "C" fn sine_cleanup(handle: LV2Handle) {
    // SAFETY: see `gain_cleanup`.
    drop(unsafe { Box::from_raw(handle.cast::<Sine>()) });
}

// ------------------------------------------------------ the gain's own state

use lv2_raw::sys::{
    LV2_Feature as SysFeature, LV2_State_Flags, LV2_State_Free_Path, LV2_State_Handle,
    LV2_State_Interface, LV2_State_Map_Path, LV2_State_Retrieve_Function, LV2_State_Status,
    LV2_State_Status_LV2_STATE_ERR_BAD_TYPE as STATE_ERR_BAD_TYPE,
    LV2_State_Status_LV2_STATE_ERR_NO_FEATURE as STATE_ERR_NO_FEATURE,
    LV2_State_Status_LV2_STATE_ERR_NO_PROPERTY as STATE_ERR_NO_PROPERTY,
    LV2_State_Status_LV2_STATE_SUCCESS as STATE_SUCCESS, LV2_State_Store_Function,
};

/// The `state:mapPath` and `state:freePath` features out of the array a
/// host passes to `save` and `restore`, if it passed them.
///
/// # Safety
/// `features` is a NULL-terminated array of pointers to valid features, or
/// null — LV2's contract for both calls.
unsafe fn find_path_features<'a>(
    features: *const *const SysFeature,
) -> (
    Option<&'a LV2_State_Map_Path>,
    Option<&'a LV2_State_Free_Path>,
) {
    let (mut map, mut free) = (None, None);
    let mut cursor = features;
    unsafe {
        while !cursor.is_null() && !(*cursor).is_null() {
            let feature = &**cursor;
            if !feature.URI.is_null() {
                let uri = CStr::from_ptr(feature.URI);
                if uri == STATE_MAP_PATH_URI {
                    map = feature.data.cast::<LV2_State_Map_Path>().as_ref();
                } else if uri == STATE_FREE_PATH_URI {
                    free = feature.data.cast::<LV2_State_Free_Path>().as_ref();
                }
            }
            cursor = cursor.add(1);
        }
    }
    (map, free)
}

/// Frees a path the host handed back, the way the specification says to:
/// through `state:freePath` when the host offers it, and not at all
/// otherwise — a host that maps paths and offers no way to free them has
/// asked for the leak.
unsafe fn give_back(free: Option<&LV2_State_Free_Path>, path: *mut c_char) {
    if let Some(free) = free
        && let Some(free_path) = free.free_path
    {
        unsafe { free_path(free.handle, path) };
    }
}

/// `LV2_State_Interface::save`: the run counter as an `atom:Int`, and the
/// bundle folder as an `atom:Path` abstracted through the host's `mapPath`.
///
/// Both stored `IS_POD | IS_PORTABLE`, which is what a host that writes them
/// to a project file needs to hear.
unsafe extern "C" fn gain_state_save(
    instance: LV2Handle,
    store: LV2_State_Store_Function,
    handle: LV2_State_Handle,
    _flags: u32,
    features: *const *const SysFeature,
) -> LV2_State_Status {
    // SAFETY: `instance` is what `gain_instantiate` returned; `store` is the
    // host's callback for this call, per the contract.
    let gain = unsafe { &*instance.cast::<Gain>() };
    let Some(store) = store else {
        return STATE_ERR_NO_FEATURE;
    };
    if gain.runs_key == 0 {
        // No `urid:map` at instantiate: nothing can be named, so nothing is
        // stored, and that is a success with an empty state.
        return STATE_SUCCESS;
    }
    let flags: u32 =
        (LV2_State_Flags::LV2_STATE_IS_POD | LV2_State_Flags::LV2_STATE_IS_PORTABLE).into();
    let status = unsafe {
        store(
            handle,
            gain.runs_key,
            (&raw const gain.runs).cast(),
            size_of::<i32>(),
            gain.int_type,
            flags,
        )
    };
    if status != STATE_SUCCESS {
        return status;
    }
    // The path, through the host's map when it offers one. An `atom:Path`
    // is stored **with** its terminating NUL, like every atom string.
    let (map, free) = unsafe { find_path_features(features) };
    match map.and_then(|map| map.abstract_path.map(|f| (map, f))) {
        Some((map, abstract_path)) => {
            let mapped = unsafe { abstract_path(map.handle, gain.home.as_ptr()) };
            if mapped.is_null() {
                return STATE_ERR_BAD_TYPE;
            }
            let bytes = unsafe { CStr::from_ptr(mapped) }
                .to_bytes_with_nul()
                .to_vec();
            let status = unsafe {
                store(
                    handle,
                    gain.home_key,
                    bytes.as_ptr().cast(),
                    bytes.len(),
                    gain.path_type,
                    flags,
                )
            };
            unsafe { give_back(free, mapped) };
            status
        }
        None => {
            let bytes = gain.home.as_bytes_with_nul();
            unsafe {
                store(
                    handle,
                    gain.home_key,
                    bytes.as_ptr().cast(),
                    bytes.len(),
                    gain.path_type,
                    flags,
                )
            }
        }
    }
}

/// `LV2_State_Interface::restore`: the counter comes back; the path must
/// map back to the folder this plugin lives in, or the restore is refused.
unsafe extern "C" fn gain_state_restore(
    instance: LV2Handle,
    retrieve: LV2_State_Retrieve_Function,
    handle: LV2_State_Handle,
    _flags: u32,
    features: *const *const SysFeature,
) -> LV2_State_Status {
    // SAFETY: as `gain_state_save`.
    let gain = unsafe { &mut *instance.cast::<Gain>() };
    let Some(retrieve) = retrieve else {
        return STATE_ERR_NO_FEATURE;
    };
    if gain.runs_key == 0 {
        return STATE_SUCCESS;
    }
    let (mut size, mut type_, mut flags) = (0usize, 0u32, 0u32);
    let value = unsafe {
        retrieve(
            handle,
            gain.runs_key,
            &raw mut size,
            &raw mut type_,
            &raw mut flags,
        )
    };
    if value.is_null() {
        return STATE_ERR_NO_PROPERTY;
    }
    if type_ != gain.int_type || size < size_of::<i32>() {
        return STATE_ERR_BAD_TYPE;
    }
    gain.runs = unsafe { value.cast::<i32>().read_unaligned() };

    let value = unsafe {
        retrieve(
            handle,
            gain.home_key,
            &raw mut size,
            &raw mut type_,
            &raw mut flags,
        )
    };
    if value.is_null() {
        return STATE_ERR_NO_PROPERTY;
    }
    if type_ != gain.path_type || size == 0 {
        return STATE_ERR_BAD_TYPE;
    }
    let stored = unsafe { std::slice::from_raw_parts(value.cast::<u8>(), size) };
    let Ok(stored) = CStr::from_bytes_until_nul(stored) else {
        return STATE_ERR_BAD_TYPE;
    };
    // Back through the host's map, if it offers one; the abstract form
    // otherwise.
    let (map, free) = unsafe { find_path_features(features) };
    let home_again = match map.and_then(|map| map.absolute_path.map(|f| (map, f))) {
        Some((map, absolute_path)) => {
            let mapped = unsafe { absolute_path(map.handle, stored.as_ptr()) };
            if mapped.is_null() {
                return STATE_ERR_BAD_TYPE;
            }
            let owned = unsafe { CStr::from_ptr(mapped) }.to_owned();
            unsafe { give_back(free, mapped) };
            owned
        }
        None => stored.to_owned(),
    };
    if home_again.as_c_str() != gain.home.as_c_str() {
        // The round trip lost the path: the plugin says so, in the only way
        // the interface lets it.
        return STATE_ERR_BAD_TYPE;
    }
    STATE_SUCCESS
}

struct SyncStateInterface(LV2_State_Interface);
// SAFETY: two function pointers, immutable for the life of the library.
unsafe impl Sync for SyncStateInterface {}
static GAIN_STATE_INTERFACE: SyncStateInterface = SyncStateInterface(LV2_State_Interface {
    save: Some(gain_state_save),
    restore: Some(gain_state_restore),
});

/// The gain's `extension_data`: `state:interface`, and nothing else.
extern "C" fn gain_extension_data(uri: *const c_char) -> *const c_void {
    if uri.is_null() {
        return std::ptr::null();
    }
    if unsafe { CStr::from_ptr(uri) } == STATE_INTERFACE_URI {
        return (&raw const GAIN_STATE_INTERFACE.0).cast();
    }
    std::ptr::null()
}

extern "C" fn no_extension_data(_uri: *const c_char) -> *const c_void {
    std::ptr::null()
}

// ------------------------------------------------------------------ entry

/// A descriptor a `static` can hold.
///
/// SAFETY: an `LV2Descriptor` is `'static` C strings and function pointers,
/// immutable for the life of the library and readable from any thread.
struct SyncDescriptor(LV2Descriptor);
unsafe impl Sync for SyncDescriptor {}

static GAIN_DESCRIPTOR: SyncDescriptor = SyncDescriptor(LV2Descriptor {
    uri: GAIN_URI_C.as_ptr(),
    instantiate: gain_instantiate,
    connect_port: gain_connect_port,
    activate: None,
    run: gain_run,
    deactivate: None,
    cleanup: gain_cleanup,
    extension_data: Some(gain_extension_data),
});

static SINE_DESCRIPTOR: SyncDescriptor = SyncDescriptor(LV2Descriptor {
    uri: SINE_URI_C.as_ptr(),
    instantiate: sine_instantiate,
    connect_port: sine_connect_port,
    activate: None,
    run: sine_run,
    deactivate: None,
    cleanup: sine_cleanup,
    extension_data: Some(no_extension_data),
});

/// The gain's code under a second URI — see [`PLAIN_URI`].
static PLAIN_DESCRIPTOR: SyncDescriptor = SyncDescriptor(LV2Descriptor {
    uri: PLAIN_URI_C.as_ptr(),
    instantiate: gain_instantiate,
    connect_port: gain_connect_port,
    activate: None,
    run: gain_run,
    deactivate: None,
    cleanup: gain_cleanup,
    extension_data: Some(no_extension_data),
});

/// And a third time, with the editor that listens to nothing — see
/// [`DEAF_URI`]. No state interface either: `keeps_state` has to stay
/// `false` for a plugin that merely shares the gain's code.
static DEAF_DESCRIPTOR: SyncDescriptor = SyncDescriptor(LV2Descriptor {
    uri: DEAF_URI_C.as_ptr(),
    instantiate: gain_instantiate,
    connect_port: gain_connect_port,
    activate: None,
    run: gain_run,
    deactivate: None,
    cleanup: gain_cleanup,
    extension_data: Some(no_extension_data),
});

static DESCRIPTORS: [&SyncDescriptor; 4] = [
    &GAIN_DESCRIPTOR,
    &SINE_DESCRIPTOR,
    &PLAIN_DESCRIPTOR,
    &DEAF_DESCRIPTOR,
];

/// The one symbol an LV2 host looks for.
///
/// # Safety
/// Called by a host through `dlsym`; returns a pointer to a descriptor that
/// lives as long as the library, or null past the last one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv2_descriptor(index: u32) -> *const LV2Descriptor {
    DESCRIPTORS
        .get(index as usize)
        .map_or(std::ptr::null(), |cell| &cell.0 as *const LV2Descriptor)
}

// --------------------------------------------------------------- the editor

/// What the gain's editor calls itself.
pub const GAIN_UI_URI: &str = "http://fopull.com/fontelle/testlv2/gain#ui";
const GAIN_UI_URI_C: &CStr = c"http://fopull.com/fontelle/testlv2/gain#ui";
const UI_PARENT_URI: &CStr = c"http://lv2plug.in/ns/extensions/ui#parent";
const UI_IDLE_URI: &CStr = c"http://lv2plug.in/ns/extensions/ui#idleInterface";
const INSTANCE_ACCESS_URI: &CStr = c"http://lv2plug.in/ns/ext/instance-access";

/// The gain's control ports, by index, as the editor speaks about them.
pub const GAIN_PORT: u32 = 2;
pub const INVERT_PORT: u32 = 3;

/// What the editor writes to [`GAIN_PORT`] on its first idle, when the host
/// gave it a window to live in.
///
/// **This is the fixture's whole trick.** A host that finds the editor, loads
/// it, hands it a parent window and then drives its idle loop is the only host
/// that makes this number appear — so one assertion covers four things that
/// are otherwise only observable by looking at a window.
pub const HELLO_FROM_THE_EDITOR: f32 = 2.0;

/// What it writes instead when the host also handed it the **running
/// plugin** through `instance-access`, and the handle really is a [`Gain`].
///
/// The feature every DPF-built editor requires (Cardinal, Dexed, drumsynth):
/// a host that does not pass it gets *"Host does not support
/// instance-access, cannot use UI"* and no editor at all.
pub const HELLO_FROM_THE_INSTANCE: f32 = 3.0;

/// An editor for the gain, drawing nothing at all.
///
/// **Deliberately not a window.** What is being tested is the host's half:
/// that it finds an `ui:X11UI`, loads the binary, passes `ui:parent`, drives
/// `idle`, carries a port write back to the plugin's parameters, and tells the
/// editor when one of them moves. Every one of those is observable through the
/// two ports below, and none of them needs an X server — so the tests run on a
/// machine with no display, which is where they mostly run.
///
/// What it does instead is **mirror**: whatever the host says [`GAIN_PORT`]
/// has become, it writes to [`INVERT_PORT`]. A round trip in one assertion.
struct GainUi {
    write: LV2UIWriteFunctionRaw,
    controller: LV2UIControllerRaw,
    /// Whether the host gave it a window. See [`HELLO_FROM_THE_EDITOR`].
    parented: bool,
    /// Whether the host gave it the live plugin, and the plugin was a
    /// [`Gain`]. See [`HELLO_FROM_THE_INSTANCE`].
    has_instance: bool,
    greeted: bool,
}

impl GainUi {
    fn write_port(&self, port: u32, value: f32) {
        let Some(write) = self.write else {
            return;
        };
        write(
            self.controller,
            port,
            std::mem::size_of::<f32>() as u32,
            0,
            (&raw const value).cast(),
        );
    }
}

extern "C" fn ui_instantiate(
    _descriptor: *const LV2UIDescriptorRaw,
    _plugin_uri: *const c_char,
    _bundle_path: *const c_char,
    write_function: LV2UIWriteFunctionRaw,
    controller: LV2UIControllerRaw,
    widget: *mut LV2UIWidget,
    features: *const *const LV2Feature,
) -> LV2UIHandle {
    let mut parented = false;
    let mut has_instance = false;
    let mut parent: *mut c_void = std::ptr::null_mut();
    if !features.is_null() {
        let mut cursor = features;
        // SAFETY: the host passes a null-terminated array of pointers to
        // features that outlive this call, which is LV2's contract.
        while let Some(feature) = unsafe { (*cursor).as_ref() } {
            let uri = unsafe { CStr::from_ptr(feature.uri) };
            if uri == UI_PARENT_URI {
                parented = true;
                parent = feature.data;
            }
            if uri == INSTANCE_ACCESS_URI {
                // SAFETY: `instance-access` carries the plugin's own
                // `LV2_Handle`, which for this bundle is a `Gain` this same
                // library made. Checked by its magic rather than trusted.
                has_instance = unsafe { feature.data.cast::<Gain>().as_ref() }
                    .is_some_and(|gain| gain.magic == INSTANCE_MAGIC);
            }
            cursor = unsafe { cursor.add(1) };
        }
    }
    // An X11 UI's widget *is* the window it drew into. This one drew nothing,
    // so it hands back the window it was given — which is what a UI that
    // reparents rather than creating its own does, and what makes this
    // testable with no X server at all.
    if !widget.is_null() {
        unsafe { *widget = parent };
    }
    Box::into_raw(Box::new(GainUi {
        write: write_function,
        controller,
        parented,
        has_instance,
        greeted: false,
    }))
    .cast()
}

extern "C" fn ui_cleanup(handle: LV2UIHandle) {
    if handle.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(handle.cast::<GainUi>()) });
}

extern "C" fn ui_port_event(
    handle: LV2UIHandle,
    port_index: u32,
    buffer_size: u32,
    format: u32,
    buffer: *const c_void,
) {
    // Format 0 is a control port carrying one float, which is the only thing
    // this editor understands.
    if format != 0 || buffer_size as usize != std::mem::size_of::<f32>() || buffer.is_null() {
        return;
    }
    let Some(ui) = (unsafe { handle.cast::<GainUi>().as_ref() }) else {
        return;
    };
    if port_index != GAIN_PORT {
        return;
    }
    let value = unsafe { *buffer.cast::<f32>() };
    ui.write_port(INVERT_PORT, value);
}

extern "C" fn ui_idle(handle: LV2UIHandle) -> std::os::raw::c_int {
    let Some(ui) = (unsafe { handle.cast::<GainUi>().as_mut() }) else {
        return 0;
    };
    if !ui.greeted {
        ui.greeted = true;
        ui.write_port(
            GAIN_PORT,
            match (ui.parented, ui.has_instance) {
                (true, true) => HELLO_FROM_THE_INSTANCE,
                (true, false) => HELLO_FROM_THE_EDITOR,
                (false, _) => 0.0,
            },
        );
    }
    0
}

struct SyncIdle(LV2UIIdleInterface);
unsafe impl Sync for SyncIdle {}
static IDLE_INTERFACE: SyncIdle = SyncIdle(LV2UIIdleInterface { idle: ui_idle });

extern "C" fn ui_extension_data(uri: *const c_char) -> *const c_void {
    if uri.is_null() {
        return std::ptr::null();
    }
    if unsafe { CStr::from_ptr(uri) } == UI_IDLE_URI {
        return (&raw const IDLE_INTERFACE.0).cast();
    }
    std::ptr::null()
}

// ------------------------------------------------- the editor that speaks atoms

/// What the sine's editor calls itself.
pub const SINE_UI_URI: &str = "http://fopull.com/fontelle/testlv2/sine#ui";
const SINE_UI_URI_C: &CStr = c"http://fopull.com/fontelle/testlv2/sine#ui";
const EVENT_TRANSFER_URI: &CStr = c"http://lv2plug.in/ns/ext/atom#eventTransfer";

/// The sine's ports, as its editor speaks about them.
pub const SINE_MIDI_IN_PORT: u32 = 0;
pub const SINE_LEVEL_PORT: u32 = 2;
pub const SINE_MIDI_OUT_PORT: u32 = 3;

/// What the sine's editor sets the level to once it has been handed an atom
/// of the plugin's own.
///
/// A number no default and no test sets any other way, so seeing it means one
/// thing: an atom went out to the plugin, the plugin answered, and the answer
/// came back to the editor.
pub const HEARD_AN_ATOM: f32 = 0.875;

/// An editor whose whole conversation with its plugin is atoms.
///
/// **This is the half a control port cannot do.** An LSP sampler is handed a
/// file by its editor writing a `patch:Set` atom, and answers with one saying
/// what it loaded; a host that carries floats and drops atoms opens that
/// editor onto a sampler that can never be given a sample. So this fixture
/// sends the smallest atom that has an audible consequence — a MIDI note-on —
/// and reports back, in the one way a test can see, that the plugin's reply
/// arrived.
struct SineUi {
    write: LV2UIWriteFunctionRaw,
    controller: LV2UIControllerRaw,
    /// The URIDs the host's map gave, so the atoms this writes and the atoms
    /// the plugin reads are the same kind of thing.
    event_transfer: u32,
    midi: u32,
    greeted: bool,
}

extern "C" fn sine_ui_instantiate(
    _descriptor: *const LV2UIDescriptorRaw,
    _plugin_uri: *const c_char,
    _bundle_path: *const c_char,
    write_function: LV2UIWriteFunctionRaw,
    controller: LV2UIControllerRaw,
    widget: *mut LV2UIWidget,
    features: *const *const LV2Feature,
) -> LV2UIHandle {
    let mut parent: *mut c_void = std::ptr::null_mut();
    let (mut event_transfer, mut midi) = (0, 0);
    if !features.is_null() {
        let mut cursor = features;
        // SAFETY: see `ui_instantiate` — LV2's contract for the array.
        while let Some(feature) = unsafe { (*cursor).as_ref() } {
            let uri = unsafe { CStr::from_ptr(feature.uri) };
            if uri == UI_PARENT_URI {
                parent = feature.data;
            } else if uri == URID_MAP_URI {
                let map = unsafe { &*feature.data.cast::<LV2UridMap>() };
                event_transfer = (map.map)(map.handle, EVENT_TRANSFER_URI.as_ptr());
                midi = (map.map)(map.handle, MIDI_EVENT_URI.as_ptr());
            }
            cursor = unsafe { cursor.add(1) };
        }
    }
    // Without a map it cannot name an atom, and an editor that cannot name
    // one has nothing to say. Refusing is what the Turtle's
    // `lv2:requiredFeature urid:map` told the host to expect.
    if event_transfer == 0 || midi == 0 {
        return std::ptr::null_mut();
    }
    if !widget.is_null() {
        unsafe { *widget = parent };
    }
    Box::into_raw(Box::new(SineUi {
        write: write_function,
        controller,
        event_transfer,
        midi,
        greeted: false,
    }))
    .cast()
}

extern "C" fn sine_ui_cleanup(handle: LV2UIHandle) {
    if handle.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(handle.cast::<SineUi>()) });
}

extern "C" fn sine_ui_port_event(
    handle: LV2UIHandle,
    _port_index: u32,
    _buffer_size: u32,
    format: u32,
    buffer: *const c_void,
) {
    let Some(ui) = (unsafe { handle.cast::<SineUi>().as_ref() }) else {
        return;
    };
    // Only an atom is news. A control value arriving here is the level this
    // editor set itself.
    if format != ui.event_transfer || buffer.is_null() {
        return;
    }
    let Some(write) = ui.write else {
        return;
    };
    let level = HEARD_AN_ATOM;
    write(
        ui.controller,
        SINE_LEVEL_PORT,
        size_of::<f32>() as u32,
        0,
        (&raw const level).cast(),
    );
}

extern "C" fn sine_ui_idle(handle: LV2UIHandle) -> std::os::raw::c_int {
    let Some(ui) = (unsafe { handle.cast::<SineUi>().as_mut() }) else {
        return 0;
    };
    if ui.greeted {
        return 0;
    }
    ui.greeted = true;
    let Some(write) = ui.write else {
        return 0;
    };
    // One MIDI note-on, as a complete atom: the two-word header the type and
    // size live in, then the three bytes themselves.
    #[repr(C)]
    struct MidiAtom {
        size: u32,
        atom_type: u32,
        bytes: [u8; 3],
    }
    let atom = MidiAtom {
        size: 3,
        atom_type: ui.midi,
        bytes: [0x90, 69, 100],
    };
    write(
        ui.controller,
        SINE_MIDI_IN_PORT,
        size_of::<MidiAtom>() as u32,
        ui.event_transfer,
        (&raw const atom).cast(),
    );
    0
}

static SINE_IDLE_INTERFACE: SyncIdle = SyncIdle(LV2UIIdleInterface { idle: sine_ui_idle });

extern "C" fn sine_ui_extension_data(uri: *const c_char) -> *const c_void {
    if uri.is_null() {
        return std::ptr::null();
    }
    if unsafe { CStr::from_ptr(uri) } == UI_IDLE_URI {
        return (&raw const SINE_IDLE_INTERFACE.0).cast();
    }
    std::ptr::null()
}

struct SyncUiDescriptor(LV2UIDescriptorRaw);
unsafe impl Sync for SyncUiDescriptor {}

// --------------------------------------------- the editor that listens to nothing

const DEAF_UI_URI_C: &CStr = c"http://fopull.com/fontelle/testlv2/deaf#ui";

/// An editor with **no `port_event` and no `extension_data`**.
///
/// LV2 says of `port_event`: *"This member may be NULL if the UI is not
/// interested in any port events"*, and of `extension_data`: *"may be set to
/// NULL if the UI is not interested in supporting any extensions"*. JuceOPL's
/// editor is both, and a host that reads either through a type that cannot be
/// null calls address zero the first time a knob moves. This is that editor,
/// so the rule has a fixture rather than a coredump.
extern "C" fn deaf_ui_instantiate(
    _descriptor: *const LV2UIDescriptorRaw,
    _plugin_uri: *const c_char,
    _bundle_path: *const c_char,
    _write_function: LV2UIWriteFunctionRaw,
    _controller: LV2UIControllerRaw,
    widget: *mut LV2UIWidget,
    features: *const *const LV2Feature,
) -> LV2UIHandle {
    let mut parent: *mut c_void = std::ptr::null_mut();
    if !features.is_null() {
        let mut cursor = features;
        // SAFETY: see `ui_instantiate` — LV2's contract for the array.
        while let Some(feature) = unsafe { (*cursor).as_ref() } {
            if unsafe { CStr::from_ptr(feature.uri) } == UI_PARENT_URI {
                parent = feature.data;
            }
            cursor = unsafe { cursor.add(1) };
        }
    }
    if !widget.is_null() {
        unsafe { *widget = parent };
    }
    // A handle that is not null and owns nothing: there is nothing to keep.
    Box::into_raw(Box::new(0u8)).cast()
}

extern "C" fn deaf_ui_cleanup(handle: LV2UIHandle) {
    if handle.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(handle.cast::<u8>()) });
}

static DEAF_UI_DESCRIPTOR: SyncUiDescriptor = SyncUiDescriptor(LV2UIDescriptorRaw {
    uri: DEAF_UI_URI_C.as_ptr(),
    instantiate_raw: deaf_ui_instantiate,
    cleanup: deaf_ui_cleanup,
    port_event: None,
    extension_data: None,
});

static GAIN_UI_DESCRIPTOR: SyncUiDescriptor = SyncUiDescriptor(LV2UIDescriptorRaw {
    uri: GAIN_UI_URI_C.as_ptr(),
    instantiate_raw: ui_instantiate,
    cleanup: ui_cleanup,
    port_event: Some(ui_port_event),
    extension_data: Some(ui_extension_data),
});

static SINE_UI_DESCRIPTOR: SyncUiDescriptor = SyncUiDescriptor(LV2UIDescriptorRaw {
    uri: SINE_UI_URI_C.as_ptr(),
    instantiate_raw: sine_ui_instantiate,
    cleanup: sine_ui_cleanup,
    port_event: Some(sine_ui_port_event),
    extension_data: Some(sine_ui_extension_data),
});

/// The one symbol a host looks for when it wants a plugin's own editor.
///
/// **Two of them, from one library**, which is the ordinary case and the one
/// a host gets wrong: LSP ships a single `lsp-plugins-lv2ui.so` answering for
/// three hundred plugins, so a host that takes the first descriptor rather
/// than the one whose URI it asked for opens the wrong editor.
///
/// # Safety
/// Called by a host through `dlsym`; returns a pointer to a descriptor that
/// lives as long as the library, or null past the last one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lv2ui_descriptor(index: u32) -> *const LV2UIDescriptorRaw {
    match index {
        0 => &raw const GAIN_UI_DESCRIPTOR.0,
        1 => &raw const SINE_UI_DESCRIPTOR.0,
        2 => &raw const DEAF_UI_DESCRIPTOR.0,
        _ => std::ptr::null(),
    }
}
