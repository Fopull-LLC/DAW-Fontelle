//! The C ABI a **bridge** implements.
//!
//! > *"if vst3 and vst2 are legally murky we could always try and add it in
//! > a way that keeps it completely separate to the open source stuff and
//! > never gets included with it ... that way i can locally use vsts lv2s or
//! > clap plugins"*
//!
//! §3.4 is the problem: the VST3 SDK is GPLv3-or-proprietary, and linking
//! it would relicense this MIT/Apache tree; the VST2 SDK is no longer
//! offered at all. §8.4's answer was "VST3 through a bridge if it is ever
//! justified", and this crate is the bridge's contract.
//!
//! # The shape
//!
//! A bridge is a shared library Fontelle finds in its own data folder at run
//! time (`$XDG_DATA_HOME/fontelle/bridges/*.so`, or `FONTELLE_BRIDGES`),
//! which exports one symbol, [`ENTRY_SYMBOL`], returning a
//! [`FontelleBridge`]: a table of function pointers that scans, opens, runs
//! and saves plugins of **one** format. Everything the host needs is in the
//! table; the bridge never links against Fontelle and Fontelle never links
//! against the bridge. What the bridge links against — an SDK, a proprietary
//! runtime, `yabridge`'s output — is its own business and its own licence,
//! built from its own repository, and never enters this one.
//!
//! This crate is **the whole of the shared surface**: `#[repr(C)]` structs
//! and a version number, and nothing else. A bridge written in C includes
//! the equivalent header; one written in Rust depends on this crate, which
//! is MIT/Apache like the rest of the tree.
//!
//! # Threads
//!
//! The same contract every plugin API draws. [`FontelleBridge::process`],
//! [`note_on`](FontelleBridge::note_on), [`note_off`](FontelleBridge::note_off)
//! and [`reset`](FontelleBridge::reset) are called on the **audio thread**
//! and must not allocate, lock or block; everything else is called on the
//! main thread. A bridge may be asked to `set_param` on the main thread
//! while `process` runs on the audio thread, and synchronising the two is
//! the bridge's job — an atomic per parameter is the usual answer.
//!
//! # A performance
//!
//! Notes, and the three channel-wide things a hand does beside them: a
//! controller, a pitch bend, channel pressure ([`FontelleBridge::controller`]
//! onward). They are audio-thread calls in time order with the notes, and
//! they are **performance rather than automation** — a parameter the
//! document moves arrives through [`FontelleBridge::set_param`] by its own
//! id. What a plugin makes of a wheel is its own business; the host does
//! not invent a mapping onto a parameter.
//!
//! # Editors
//!
//! A bridged plugin's **own** editor, when it has one, is drawn into an X11
//! window Fontelle makes and owns, exactly as a CLAP or LV2 plugin's is
//! (`fontelle_host::gui`): the host hands the bridge the window id in
//! [`open_editor`](FontelleBridge::open_editor), the bridge embeds the
//! plugin's view in it and reports the size it wants, and the host calls
//! [`tick_editor`](FontelleBridge::tick_editor) once per frame for as long
//! as it is open — a bridged editor has no thread of its own any more than
//! a CLAP one does. The host reads every parameter back off the bridge
//! after each tick, which is how a knob moved in the editor reaches the
//! document. All of it is main thread.
//!
//! # Lifetimes
//!
//! Strings a bridge hands out in a [`PluginInfo`] or a [`ParamInfo`] stay
//! valid until the matching `free_infos` call or until the instance is
//! closed, respectively. State bytes stay valid until `free_state`. Nothing
//! handed *to* a bridge outlives the call it was passed in.

use std::ffi::{c_char, c_void};

/// The version of this contract. A bridge whose table says another number
/// is refused rather than read.
///
/// **2** added the editor entry points at the end of the table
/// ([`FontelleBridge::has_editor`] onward) — the four things both natively
/// hosted formats turned out to need, plus a resize. **3** added the rest
/// of a performance ([`FontelleBridge::controller`] onward): until it, the
/// table carried notes and nothing else, so a bridged instrument was the
/// one kind that could not be played with a wheel. A bridge built against
/// an older table is refused rather than read past its end.
pub const ABI_VERSION: u32 = 3;

/// The symbol a bridge exports.
pub const ENTRY_SYMBOL: &str = "fontelle_bridge_entry";

/// The entry point's signature.
pub type EntryFn = unsafe extern "C" fn() -> *const FontelleBridge;

/// What a scan learns about one plugin.
#[repr(C)]
pub struct PluginInfo {
    /// The plugin's own stable id, NUL-terminated — a VST3 class id, say.
    pub id: *const c_char,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub version: *const c_char,
    /// Non-zero for an instrument.
    pub is_instrument: u8,
}

/// One parameter, as the plugin describes it.
#[repr(C)]
pub struct ParamInfo {
    /// The plugin's stable id for it.
    pub id: u32,
    pub name: *const c_char,
    /// Its module or group, `/`-separated, or an empty string.
    pub module: *const c_char,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub stepped: u8,
    pub hidden: u8,
    pub readonly: u8,
}

/// An opened plugin, opaque to the host.
pub type Instance = *mut c_void;

/// The table a bridge returns from its entry point.
///
/// Every pointer is required. Return codes are zero for success and
/// negative for failure unless the field says otherwise.
#[repr(C)]
pub struct FontelleBridge {
    /// Must equal [`ABI_VERSION`].
    pub abi_version: u32,
    /// The format tag this bridge serves: `"vst3"` or `"vst2"`, NUL-terminated.
    /// The same tag a `PluginKey` is written with.
    pub format: *const c_char,
    /// What the bridge calls itself, for a settings page.
    pub name: *const c_char,
    /// The file extension of a bundle of this format, without the dot.
    pub extension: *const c_char,

    /// Writes the folders this format is installed in on this machine into
    /// `out`, up to `capacity` of them, and returns how many there are.
    /// Strings stay valid for the life of the bridge.
    pub search_paths: unsafe extern "C" fn(out: *mut *const c_char, capacity: u32) -> u32,

    /// Everything one bundle holds. On success `*out` points at `*count`
    /// entries the host frees with `free_infos`.
    pub scan_bundle: unsafe extern "C" fn(
        path: *const c_char,
        out: *mut *mut PluginInfo,
        count: *mut u32,
    ) -> i32,
    pub free_infos: unsafe extern "C" fn(infos: *mut PluginInfo, count: u32),

    /// Opens one plugin out of `path`. Null on failure.
    pub open: unsafe extern "C" fn(path: *const c_char, id: *const c_char) -> Instance,
    pub close: unsafe extern "C" fn(instance: Instance),

    pub param_count: unsafe extern "C" fn(instance: Instance) -> u32,
    /// Describes the `index`th parameter. Strings stay valid until `close`.
    pub param_info:
        unsafe extern "C" fn(instance: Instance, index: u32, out: *mut ParamInfo) -> i32,
    pub audio_inputs: unsafe extern "C" fn(instance: Instance) -> u32,
    pub audio_outputs: unsafe extern "C" fn(instance: Instance) -> u32,
    pub accepts_notes: unsafe extern "C" fn(instance: Instance) -> u8,

    /// Main thread. Sets a parameter in the plugin's own units; a running
    /// plugin hears it on its next block.
    pub set_param: unsafe extern "C" fn(instance: Instance, id: u32, plain: f64),
    pub get_param: unsafe extern "C" fn(instance: Instance, id: u32) -> f64,
    /// Formats a value the way the plugin would. Writes a NUL-terminated
    /// string into `buffer` and returns its length, or a negative number if
    /// the plugin has no formatter.
    pub display: unsafe extern "C" fn(
        instance: Instance,
        id: u32,
        value: f64,
        buffer: *mut c_char,
        capacity: u32,
    ) -> i32,

    /// Prepares the plugin to run. Nothing below may be called before this
    /// has succeeded, or after `deactivate`.
    pub activate: unsafe extern "C" fn(instance: Instance, sample_rate: f64, max_block: u32) -> i32,
    pub deactivate: unsafe extern "C" fn(instance: Instance),

    /// **Audio thread.** A note at `frame` within the coming block, in time
    /// order. `velocity` runs 0..1.
    pub note_on: unsafe extern "C" fn(instance: Instance, frame: u32, key: u8, velocity: f64),
    pub note_off: unsafe extern "C" fn(instance: Instance, frame: u32, key: u8),
    /// **Audio thread.** Everything sounding stops.
    pub reset: unsafe extern "C" fn(instance: Instance),
    /// **Audio thread.** One block: `inputs` channels in, `outputs` channels
    /// out, each `frames` long, exactly the counts the plugin declared.
    pub process: unsafe extern "C" fn(
        instance: Instance,
        inputs: *const *const f32,
        input_channels: u32,
        outputs: *const *mut f32,
        output_channels: u32,
        frames: u32,
    ),

    /// The plugin's own state. On success `*out` points at `*len` bytes the
    /// host frees with `free_state`. A negative return means the plugin
    /// keeps none.
    pub save_state:
        unsafe extern "C" fn(instance: Instance, out: *mut *mut u8, len: *mut u32) -> i32,
    pub free_state: unsafe extern "C" fn(bytes: *mut u8, len: u32),
    pub load_state: unsafe extern "C" fn(instance: Instance, bytes: *const u8, len: u32) -> i32,

    // ---- ABI 2: the plugin's own editor. See the crate note.
    /// Whether the plugin has an editor the bridge can embed in an X11
    /// window. `1` or `0`.
    pub has_editor: unsafe extern "C" fn(instance: Instance) -> u8,
    /// Opens the editor into the X11 window `parent`, and writes the size
    /// it wants into `width` and `height`. Negative when the plugin has no
    /// editor or refused; nothing is open afterwards in that case.
    pub open_editor: unsafe extern "C" fn(
        instance: Instance,
        parent: u64,
        width: *mut u32,
        height: *mut u32,
    ) -> i32,
    /// Closes an open editor. Called before the window it was given goes.
    pub close_editor: unsafe extern "C" fn(instance: Instance),
    /// One frame of the editor: repaint, take a click. Called only between
    /// `open_editor` and `close_editor`.
    pub tick_editor: unsafe extern "C" fn(instance: Instance),
    /// The window is now `width` by `height`. Negative if the editor cannot
    /// be that size.
    pub resize_editor: unsafe extern "C" fn(instance: Instance, width: u32, height: u32) -> i32,

    // ---- ABI 3: the rest of a performance. See the crate note.
    /// **Audio thread.** A continuous controller at `frame` within the
    /// coming block — the mod wheel is 1 — `value` in `0..=127`, on the
    /// same channel the notes are. In time order with the notes.
    ///
    /// **A performance, not automation**: a knob the *document* moves
    /// arrives through [`set_param`](Self::set_param) by its own id, while
    /// this is the raw fact that a hand moved a wheel, for the plugin to
    /// interpret. The sustain pedal never arrives here — holding notes is
    /// the host's bookkeeping.
    pub controller: unsafe extern "C" fn(instance: Instance, frame: u32, controller: u8, value: u8),
    /// **Audio thread.** A pitch bend, centred at zero over `-8192..=8191`,
    /// reaching whatever range the plugin bends by — two semitones is what
    /// a keyboard defaults to.
    ///
    /// A **slide note** arrives here too: the table carries one pitch for
    /// the instrument rather than one per note, so the host converts a
    /// slide into the bend that reaches it, and a slide past the bend's
    /// range lands at the limit. Per-note pitch is what an ABI 4 would add,
    /// once there is a bridge that wants it.
    pub pitch_bend: unsafe extern "C" fn(instance: Instance, frame: u32, value: i16),
    /// **Audio thread.** Channel aftertouch at `frame`, `0..=127`.
    pub channel_pressure: unsafe extern "C" fn(instance: Instance, frame: u32, value: u8),
}

// SAFETY: the table is function pointers and `'static` strings, immutable
// for the life of the library that returned it.
unsafe impl Sync for FontelleBridge {}
unsafe impl Send for FontelleBridge {}
