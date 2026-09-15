//! A bridge with no SDK in it.
//!
//! **This is a test fixture, not a product.** It implements
//! [`fontelle_bridge_abi`] for a made-up format, `"vst2"` in name only —
//! the one format this build reaches through a bridge — whose "bundles" are
//! folders (named `.so`, the extension VST 2 has on Linux) holding a
//! `plugins.txt` listing the plugins in them — a gain and a sine, the same
//! two every fixture in this repository is made of. What it proves is the
//! *loader*: that Fontelle finds a bridge, reads its table, and hosts what
//! the bridge offers through exactly the `HostedPlugin`/`HostedProcessor`
//! that CLAP, LV2 and VST 3 go through. A real bridge — `fontelle-vst2`,
//! built in its own repository — implements the same table and never
//! touches this tree.

use std::ffi::{CStr, CString, c_char};
use std::sync::atomic::{AtomicU64, Ordering};

use fontelle_bridge_abi::{ABI_VERSION, FontelleBridge, Instance, ParamInfo, PluginInfo};

pub const GAIN_ID: &str = "test.bridge.gain";
pub const SINE_ID: &str = "test.bridge.sine";

/// How big the sine's editor says it is.
pub const EDITOR_WIDTH: u32 = 320;
pub const EDITOR_HEIGHT: u32 = 200;
/// What the sine's editor sets its level to on its first tick after being
/// opened — the one thing a fixture editor that draws nothing can do that a
/// host can see. The same trick `fontelle_testlv2::HELLO_FROM_THE_EDITOR`
/// plays: it appears only if the bridge was asked to open the editor, was
/// ticked, and the host read the parameter back afterwards.
pub const HELLO_FROM_THE_EDITOR: f64 = 0.75;
/// What a folder needs in it to be one of this bridge's bundles.
pub const MANIFEST: &str = "plugins.txt";
pub const EXTENSION: &str = "so";

const FORMAT: &CStr = c"vst2";
const NAME: &CStr = c"Fontelle Test Bridge";
const EXT: &CStr = c"so";
const GAIN: &CStr = c"test.bridge.gain";
const SINE: &CStr = c"test.bridge.sine";
const GAIN_NAME: &CStr = c"Bridged Test Gain";
const SINE_NAME: &CStr = c"Bridged Test Sine";
const VENDOR: &CStr = c"Fopull LLC";
const VERSION: &CStr = c"1.0.0";
const EMPTY: &CStr = c"";
const P_GAIN: &CStr = c"Gain";
const P_INVERT: &CStr = c"Invert";
const P_LEVEL: &CStr = c"Level";

enum Kind {
    Gain,
    Sine,
}

struct Plugin {
    kind: Kind,
    /// Parameters by id, as atomics so the main thread can write while the
    /// audio thread reads — the bridge's job, per the ABI's thread note.
    params: Vec<(u32, AtomicU64)>,
    rate: f64,
    phase: f64,
    key: Option<u8>,
    /// Pending notes for the coming block: (frame, key, on).
    notes: Vec<(u32, u8, bool)>,
    active: bool,
    /// The mod wheel, scaling the sine's level; **one** at rest, so a
    /// plugin nobody has touched is heard.
    wheel: f64,
    /// Channel pressure, `0..=1`, **ducking** the level — the opposite of
    /// the wheel, so a host that sent one as the other is told apart from
    /// one that got it right. The same trick both in-tree fixtures play.
    pressure: f64,
    /// The bend, in semitones.
    bend: f64,
    /// The window the editor was opened into, while it is open. Only the
    /// sine has an editor — see [`HELLO_FROM_THE_EDITOR`].
    editor: Option<u64>,
    /// Whether the open editor has greeted yet.
    greeted: bool,
}

impl Plugin {
    fn param(&self, id: u32) -> f64 {
        self.params
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, v)| f64::from_bits(v.load(Ordering::Relaxed)))
            .unwrap_or(0.0)
    }
}

unsafe fn plugin<'a>(instance: Instance) -> &'a mut Plugin {
    // SAFETY: every instance handed out by `open` is a leaked `Box<Plugin>`
    // that `close` reclaims; the host promises not to use it afterwards.
    unsafe { &mut *instance.cast::<Plugin>() }
}

unsafe extern "C" fn search_paths(out: *mut *const c_char, capacity: u32) -> u32 {
    static HOME: &CStr = c"/nonexistent/test-bridge-plugins";
    if capacity > 0 && !out.is_null() {
        unsafe { *out = HOME.as_ptr() };
    }
    1
}

unsafe extern "C" fn scan_bundle(
    path: *const c_char,
    out: *mut *mut PluginInfo,
    count: *mut u32,
) -> i32 {
    let path = unsafe { CStr::from_ptr(path) }
        .to_string_lossy()
        .into_owned();
    let Ok(listing) = std::fs::read_to_string(std::path::Path::new(&path).join(MANIFEST)) else {
        return -1;
    };
    let mut infos = Vec::new();
    for line in listing.lines() {
        let (id, name) = match line.trim() {
            GAIN_ID => (GAIN, GAIN_NAME),
            SINE_ID => (SINE, SINE_NAME),
            _ => continue,
        };
        infos.push(PluginInfo {
            id: id.as_ptr(),
            name: name.as_ptr(),
            vendor: VENDOR.as_ptr(),
            version: VERSION.as_ptr(),
            is_instrument: u8::from(id == SINE),
        });
    }
    let mut infos = infos.into_boxed_slice();
    unsafe {
        *count = infos.len() as u32;
        *out = infos.as_mut_ptr();
    }
    std::mem::forget(infos);
    0
}

unsafe extern "C" fn free_infos(infos: *mut PluginInfo, count: u32) {
    if !infos.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(infos, count as usize)) });
    }
}

unsafe extern "C" fn open(_path: *const c_char, id: *const c_char) -> Instance {
    let id = unsafe { CStr::from_ptr(id) };
    let (kind, params) = if id == GAIN {
        (Kind::Gain, vec![(0, 1.0f64), (1, 0.0)])
    } else if id == SINE {
        (Kind::Sine, vec![(7, 0.5)])
    } else {
        return std::ptr::null_mut();
    };
    Box::into_raw(Box::new(Plugin {
        kind,
        params: params
            .into_iter()
            .map(|(id, v)| (id, AtomicU64::new(v.to_bits())))
            .collect(),
        rate: 48_000.0,
        phase: 0.0,
        key: None,
        notes: Vec::with_capacity(64),
        active: false,
        wheel: 1.0,
        pressure: 0.0,
        bend: 0.0,
        editor: None,
        greeted: false,
    }))
    .cast()
}

unsafe extern "C" fn close(instance: Instance) {
    drop(unsafe { Box::from_raw(instance.cast::<Plugin>()) });
}

unsafe extern "C" fn param_count(instance: Instance) -> u32 {
    unsafe { plugin(instance) }.params.len() as u32
}

unsafe extern "C" fn param_info(instance: Instance, index: u32, out: *mut ParamInfo) -> i32 {
    let p = unsafe { plugin(instance) };
    let Some((id, _)) = p.params.get(index as usize) else {
        return -1;
    };
    let info = match (&p.kind, *id) {
        (Kind::Gain, 0) => ParamInfo {
            id: 0,
            name: P_GAIN.as_ptr(),
            module: EMPTY.as_ptr(),
            min: 0.0,
            max: 2.0,
            default: 1.0,
            stepped: 0,
            hidden: 0,
            readonly: 0,
        },
        (Kind::Gain, _) => ParamInfo {
            id: 1,
            name: P_INVERT.as_ptr(),
            module: EMPTY.as_ptr(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            stepped: 1,
            hidden: 0,
            readonly: 0,
        },
        (Kind::Sine, _) => ParamInfo {
            id: 7,
            name: P_LEVEL.as_ptr(),
            module: EMPTY.as_ptr(),
            min: 0.0,
            max: 1.0,
            default: 0.5,
            stepped: 0,
            hidden: 0,
            readonly: 0,
        },
    };
    unsafe { out.write(info) };
    0
}

unsafe extern "C" fn audio_inputs(instance: Instance) -> u32 {
    match unsafe { plugin(instance) }.kind {
        Kind::Gain => 2,
        Kind::Sine => 0,
    }
}

unsafe extern "C" fn audio_outputs(_instance: Instance) -> u32 {
    2
}

unsafe extern "C" fn accepts_notes(instance: Instance) -> u8 {
    u8::from(matches!(unsafe { plugin(instance) }.kind, Kind::Sine))
}

unsafe extern "C" fn set_param(instance: Instance, id: u32, plain: f64) {
    let p = unsafe { plugin(instance) };
    if let Some((_, cell)) = p.params.iter().find(|(i, _)| *i == id) {
        cell.store(plain.to_bits(), Ordering::Relaxed);
    }
}

unsafe extern "C" fn get_param(instance: Instance, id: u32) -> f64 {
    unsafe { plugin(instance) }.param(id)
}

unsafe extern "C" fn display(
    instance: Instance,
    id: u32,
    value: f64,
    buffer: *mut c_char,
    capacity: u32,
) -> i32 {
    let p = unsafe { plugin(instance) };
    let text = match (&p.kind, id) {
        (Kind::Gain, 0) => format!("{value:.2}x"),
        _ => return -1,
    };
    let Ok(text) = CString::new(text) else {
        return -1;
    };
    let bytes = text.as_bytes_with_nul();
    if bytes.len() > capacity as usize {
        return -1;
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr().cast(), buffer, bytes.len()) };
    (bytes.len() - 1) as i32
}

unsafe extern "C" fn activate(instance: Instance, sample_rate: f64, _max_block: u32) -> i32 {
    let p = unsafe { plugin(instance) };
    if p.active {
        return -1;
    }
    p.rate = sample_rate;
    p.active = true;
    0
}

unsafe extern "C" fn deactivate(instance: Instance) {
    let p = unsafe { plugin(instance) };
    p.active = false;
    p.key = None;
    p.notes.clear();
}

unsafe extern "C" fn note_on(instance: Instance, frame: u32, key: u8, _velocity: f64) {
    let p = unsafe { plugin(instance) };
    if p.notes.len() < p.notes.capacity() {
        p.notes.push((frame, key, true));
    }
}

unsafe extern "C" fn note_off(instance: Instance, frame: u32, key: u8) {
    let p = unsafe { plugin(instance) };
    if p.notes.len() < p.notes.capacity() {
        p.notes.push((frame, key, false));
    }
}

unsafe extern "C" fn reset(instance: Instance) {
    let p = unsafe { plugin(instance) };
    p.key = None;
    p.notes.clear();
}

// ------------------------------------------------------- a performance

/// The wheel scales the level and everything else is ignored — a fixture
/// that answered every controller could not show a host dropping one.
/// Applied for the block rather than at `frame`: a wheel need not be
/// sample-accurate to be heard, and the notes are what the timestamps are
/// there to prove.
unsafe extern "C" fn controller(instance: Instance, _frame: u32, controller: u8, value: u8) {
    let p = unsafe { plugin(instance) };
    if controller == 1 {
        p.wheel = f64::from(value.min(127)) / 127.0;
    }
}

unsafe extern "C" fn pitch_bend(instance: Instance, _frame: u32, value: i16) {
    let p = unsafe { plugin(instance) };
    let span = if value < 0 { 8192.0 } else { 8191.0 };
    p.bend = f64::from(value.clamp(-8192, 8191)) / span * BEND_RANGE_SEMITONES;
}

unsafe extern "C" fn channel_pressure(instance: Instance, _frame: u32, value: u8) {
    let p = unsafe { plugin(instance) };
    p.pressure = f64::from(value.min(127)) / 127.0;
}

/// How far a full bend goes, in semitones — a keyboard's default, and what
/// the host converts a slide into for a bridge.
const BEND_RANGE_SEMITONES: f64 = 2.0;

unsafe extern "C" fn process(
    instance: Instance,
    inputs: *const *const f32,
    input_channels: u32,
    outputs: *const *mut f32,
    output_channels: u32,
    frames: u32,
) {
    let p = unsafe { plugin(instance) };
    let frames = frames as usize;
    // SAFETY: the host promises `input_channels` pointers to `frames`
    // floats each, and the same for the outputs.
    let outs: Vec<&mut [f32]> = (0..output_channels as usize)
        .map(|c| unsafe { std::slice::from_raw_parts_mut(*outputs.add(c), frames) })
        .collect();
    match p.kind {
        Kind::Gain => {
            let factor = p.param(0) * if p.param(1) >= 0.5 { -1.0 } else { 1.0 };
            for (c, out) in outs.into_iter().enumerate() {
                if c < input_channels as usize {
                    let input = unsafe { std::slice::from_raw_parts(*inputs.add(c), frames) };
                    for (o, i) in out.iter_mut().zip(input) {
                        *o = (f64::from(*i) * factor) as f32;
                    }
                } else {
                    out.fill(0.0);
                }
            }
        }
        Kind::Sine => {
            let level = p.param(7) * p.wheel * (1.0 - p.pressure.clamp(0.0, 1.0));
            let bend = p.bend;
            let mut outs = outs;
            let mut at = 0;
            // Taken out and put back, so the list keeps the capacity
            // `note_on` guards on.
            let mut notes = std::mem::take(&mut p.notes);
            for (frame, key, on) in
                notes
                    .iter()
                    .copied()
                    .chain(std::iter::once((frames as u32, 0, false)))
            {
                let frame = (frame as usize).min(frames);
                for i in at..frame {
                    let sample = match p.key {
                        Some(k) => {
                            let hz = 440.0 * 2f64.powf((f64::from(k) - 69.0 + bend) / 12.0);
                            let s = (p.phase * std::f64::consts::TAU).sin() * level;
                            p.phase = (p.phase + hz / p.rate).fract();
                            s as f32
                        }
                        None => 0.0,
                    };
                    for out in outs.iter_mut() {
                        out[i] = sample;
                    }
                }
                at = frame;
                if frame == frames && key == 0 && !on {
                    break;
                }
                if on {
                    p.key = Some(key);
                } else if p.key == Some(key) {
                    p.key = None;
                }
            }
            notes.clear();
            p.notes = notes;
        }
    }
}

unsafe extern "C" fn save_state(instance: Instance, out: *mut *mut u8, len: *mut u32) -> i32 {
    let p = unsafe { plugin(instance) };
    if !matches!(p.kind, Kind::Gain) {
        return -1;
    }
    // Eight bytes of gain, eight of invert: a blob the host cannot read.
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(&p.param(0).to_le_bytes());
    bytes.extend_from_slice(&p.param(1).to_le_bytes());
    let mut bytes = bytes.into_boxed_slice();
    unsafe {
        *len = bytes.len() as u32;
        *out = bytes.as_mut_ptr();
    }
    std::mem::forget(bytes);
    0
}

unsafe extern "C" fn free_state(bytes: *mut u8, len: u32) {
    if !bytes.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(bytes, len as usize)) });
    }
}

unsafe extern "C" fn load_state(instance: Instance, bytes: *const u8, len: u32) -> i32 {
    let p = unsafe { plugin(instance) };
    if !matches!(p.kind, Kind::Gain) || len != 16 {
        return -1;
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, 16) };
    let gain = f64::from_le_bytes(bytes[..8].try_into().unwrap());
    let invert = f64::from_le_bytes(bytes[8..].try_into().unwrap());
    unsafe {
        set_param(instance, 0, gain);
        set_param(instance, 1, invert);
    }
    0
}

// ------------------------------------------------------------ the editor

unsafe extern "C" fn has_editor(instance: Instance) -> u8 {
    u8::from(matches!(unsafe { plugin(instance) }.kind, Kind::Sine))
}

/// Opens the sine's editor: it draws nothing, remembers the window, and
/// reports a size. The gain refuses, because it has none.
unsafe extern "C" fn open_editor(
    instance: Instance,
    parent: u64,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let p = unsafe { plugin(instance) };
    if !matches!(p.kind, Kind::Sine) {
        return -1;
    }
    p.editor = Some(parent);
    p.greeted = false;
    if !width.is_null() && !height.is_null() {
        unsafe {
            *width = EDITOR_WIDTH;
            *height = EDITOR_HEIGHT;
        }
    }
    0
}

unsafe extern "C" fn close_editor(instance: Instance) {
    let p = unsafe { plugin(instance) };
    p.editor = None;
    p.greeted = false;
}

/// One frame: the first one after an open writes the greeting. A tick with
/// no editor open is a host bug and does nothing.
unsafe extern "C" fn tick_editor(instance: Instance) {
    let p = unsafe { plugin(instance) };
    if p.editor.is_none() || p.greeted {
        return;
    }
    p.greeted = true;
    unsafe { set_param(instance, 7, HELLO_FROM_THE_EDITOR) };
}

unsafe extern "C" fn resize_editor(_instance: Instance, _width: u32, _height: u32) -> i32 {
    // A fixed size, like a plugin whose editor is a picture.
    -1
}

static BRIDGE: FontelleBridge = FontelleBridge {
    abi_version: ABI_VERSION,
    format: FORMAT.as_ptr(),
    name: NAME.as_ptr(),
    extension: EXT.as_ptr(),
    search_paths,
    scan_bundle,
    free_infos,
    open,
    close,
    param_count,
    param_info,
    audio_inputs,
    audio_outputs,
    accepts_notes,
    set_param,
    get_param,
    display,
    activate,
    deactivate,
    note_on,
    note_off,
    reset,
    process,
    save_state,
    free_state,
    load_state,
    has_editor,
    open_editor,
    close_editor,
    tick_editor,
    resize_editor,
    controller,
    pitch_bend,
    channel_pressure,
};

/// The one symbol a host looks for.
///
/// # Safety
/// Called through `dlsym`; the table it returns lives as long as the library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontelle_bridge_entry() -> *const FontelleBridge {
    &BRIDGE
}
