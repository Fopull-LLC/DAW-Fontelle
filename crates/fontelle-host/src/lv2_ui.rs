//! An LV2 plugin's **own** editor (TDD §8.4, §16).
//!
//! [`crate::gui`] does this for CLAP. This is the same feature for the other
//! format, and it matters more there: a CLAP plugin's parameters describe it
//! completely, while an LV2 sampler's whole state is *a file it loaded*, and
//! there is no parameter for that. Without its own editor an LSP Multi-Sampler
//! is a rack of knobs attached to nothing.
//!
//! # Why this is not `suil`
//!
//! The usual way a host shows an LV2 UI is `suil`, which wraps a UI of one
//! toolkit inside a window of another. It is not installed on the machine this
//! was written for, and it does not need to be: **every LV2 UI worth showing
//! here is already an `ui:X11UI`** — an X11 window id is all it wants, which
//! is exactly what [`crate::gui::PluginWindow`] already hands a CLAP plugin.
//! Of the seventeen bundles installed on the reporter's machine that ship a
//! UI, seventeen ship X11. What suil would add is Gtk and Qt UIs, and a
//! wrapper for toolkits nothing here uses is a dependency for nobody.
//!
//! Calf ships **no** UI at all in its Turtle — not a Gtk one, none — so it
//! keeps the generated panel whatever this module does. That is a fact about
//! Calf, not a gap in here.
//!
//! # What a UI is, and what the host owes it
//!
//! A shared library with one symbol, `lv2ui_descriptor`, giving a struct of
//! function pointers: `instantiate`, `cleanup`, `port_event`, and whatever
//! `extension_data` will hand over. The host must
//!
//! - hand it **`ui:parent`**, the window to draw into;
//! - hand it **`urid:map`** — the *plugin's own* map, so that when the UI and
//!   the plugin talk about an atom they mean the same number;
//! - call **`idle`** rapidly, which is how it repaints, exactly as CLAP's
//!   timer is;
//! - call **`port_event`** when a parameter moves, so a knob the studio turned
//!   moves on the editor too;
//! - and take what the UI writes back and put it on the parameter wire.
//!
//! The last two are one loop, and the one rule that makes it not a fight over
//! a knob is at the bottom of [`Lv2Ui::tick`]: **what the editor wrote is
//! never sent back to it.**

use std::ffi::{CStr, CString, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use livi::Features;
use lv2_raw::{LV2Feature, LV2UIControllerRaw, LV2UIHandle, LV2UIWidget};

use crate::gui::{GuiError, GuiSize, PluginWindow};
use crate::param::ParamValues;

const X11_UI: &str = "http://lv2plug.in/ns/extensions/ui#X11UI";
const UI_PARENT: &CStr = c"http://lv2plug.in/ns/extensions/ui#parent";
const UI_RESIZE: &CStr = c"http://lv2plug.in/ns/extensions/ui#resize";
const UI_IDLE: &CStr = c"http://lv2plug.in/ns/extensions/ui#idleInterface";
const URID_MAP: &str = "http://lv2plug.in/ns/ext/urid#map";
const URID_UNMAP: &str = "http://lv2plug.in/ns/ext/urid#unmap";
const EVENT_TRANSFER: &CStr = c"http://lv2plug.in/ns/ext/atom#eventTransfer";
const INSTANCE_ACCESS: &CStr = c"http://lv2plug.in/ns/ext/instance-access";

/// `LV2UI_Descriptor`, with **every function pointer nullable**.
///
/// The specification says `port_event` *"may be NULL if the UI is not
/// interested in any port events"* and `extension_data` *"may be set to NULL
/// if the UI is not interested in supporting any extensions"* — and JuceOPL's
/// editor is both. `lv2_raw`'s descriptor types `port_event` as a plain
/// function pointer, which in Rust cannot be null, so reading a NULL through
/// it is undefined behaviour and the optimiser is entitled to fold any later
/// null check away. The release studio did exactly that: three crashes at
/// address zero in a minute, the first time a knob moved with that editor
/// open. So the descriptor is read through this struct, which is the C
/// header's shape with `Option` where C has a pointer that may be null; the
/// compiler then *cannot* assume anything.
#[repr(C)]
struct UiDescriptor {
    uri: *const std::ffi::c_char,
    instantiate: Option<
        unsafe extern "C" fn(
            descriptor: *const UiDescriptor,
            plugin_uri: *const std::ffi::c_char,
            bundle_path: *const std::ffi::c_char,
            write_function: lv2_raw::LV2UIWriteFunctionRaw,
            controller: LV2UIControllerRaw,
            widget: *mut LV2UIWidget,
            features: *const *const LV2Feature,
        ) -> LV2UIHandle,
    >,
    cleanup: Option<unsafe extern "C" fn(LV2UIHandle)>,
    port_event: Option<PortEvent>,
    extension_data: Option<unsafe extern "C" fn(*const std::ffi::c_char) -> *const c_void>,
}

/// `LV2UI_Idle_Interface`, likewise.
#[repr(C)]
struct IdleInterface {
    idle: Option<unsafe extern "C" fn(LV2UIHandle) -> c_int>,
}

/// A descriptor's `port_event` — see [`UiDescriptor`].
type PortEvent = unsafe extern "C" fn(LV2UIHandle, u32, u32, u32, *const c_void);

/// How far into one library's list of editors to look for the right one.
///
/// LSP's single `lsp-plugins-lv2ui.so` answers for every plugin it ships,
/// which is over three hundred. This is not a limit anybody should reach —
/// the walk ends at the null past the last descriptor — it is the bound that
/// stops a library which never returns one from hanging the studio.
const MAX_UIS_PER_BINARY: u32 = 4096;

/// Where a plugin's X11 editor lives.
pub(crate) struct Lv2UiInfo {
    /// The UI's own URI, which is not the plugin's.
    pub(crate) uri: CString,
    /// The shared library to load.
    pub(crate) binary: PathBuf,
}

/// The `ui:X11UI` this plugin ships, if it ships one.
///
/// Read off the plugin's own Turtle through lilv, like everything else about
/// an LV2 plugin — see [`crate::lv2`] on why the world holds one bundle.
pub(crate) fn find_x11_ui(plugin: &livi::Plugin) -> Option<Lv2UiInfo> {
    let raw = plugin.raw();
    let uis = raw.uis()?;
    for ui in uis.iter() {
        let is_x11 = ui
            .classes()
            .into_iter()
            .any(|class| class.as_uri().is_some_and(|uri| uri == X11_UI));
        if !is_x11 {
            continue;
        }
        let uri = ui.uri();
        let uri = uri.as_uri()?;
        let binary = ui.binary_uri().and_then(|node| {
            node.as_uri()
                .and_then(file_uri_to_path)
                .filter(|path| path.is_file())
        })?;
        return Some(Lv2UiInfo {
            uri: CString::new(uri).ok()?,
            binary,
        });
    }
    None
}

/// The reverse of [`crate::lv2::bundle_uri`]: a `file:` URI back to a path.
fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let mut out = Vec::with_capacity(rest.len());
    let mut bytes = rest.bytes();
    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            out.push(byte);
            continue;
        }
        let hex: String = [bytes.next()?, bytes.next()?]
            .iter()
            .map(|b| *b as char)
            .collect();
        out.push(u8::from_str_radix(&hex, 16).ok()?);
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

/// Everything the UI's callbacks reach the host through.
///
/// One allocation, pinned behind a `Box` for the life of the editor, and its
/// address is the `controller` the UI is given. Every callback below is
/// somebody else's code calling into this program on the main thread.
pub(crate) struct UiBridge {
    values: Arc<ParamValues>,
    /// The atoms the editor and the plugin exchange — see [`crate::atom`].
    atoms: Arc<crate::atom::AtomPipes>,
    /// The URID the **plugin's own** map gave `atom:eventTransfer`, which is
    /// the protocol number an editor writes an atom under. Taken from the
    /// plugin's map rather than one of our own, because otherwise the number
    /// the editor uses and the number this recognises would be two different
    /// numbers.
    event_transfer: u32,
    /// What the editor has written since the last look.
    ///
    /// Kept, rather than only applied, because of the one rule that makes the
    /// two-way loop stable: a value the editor sent must not be sent back to
    /// it. See [`Lv2Ui::tick`].
    written: Mutex<Vec<(u32, f32)>>,
    /// A size the editor asked its window to become, latest wins.
    resize: Mutex<Option<GuiSize>>,
}

/// **The UI thread is the main thread**, and this is what LV2 hands the UI to
/// write a port with.
///
/// # Safety
/// Called by the UI with the `controller` it was given at instantiate, which
/// is the address of a [`UiBridge`] that outlives it — [`Lv2Ui`] owns both and
/// destroys the UI first.
extern "C" fn write_port(
    controller: LV2UIControllerRaw,
    port_index: u32,
    buffer_size: u32,
    protocol: u32,
    buffer: *const c_void,
) {
    let Some(bridge) = (unsafe { controller.cast::<UiBridge>().as_ref() }) else {
        return;
    };
    if buffer.is_null() {
        return;
    }
    // **An atom.** This is the half a float cannot do — "load this file" — and
    // it goes to the plugin's own atom port, in one piece, at the top of its
    // next block. See `crate::atom`.
    if protocol != 0 && protocol == bridge.event_transfer {
        if trace() {
            eprintln!(
                "[atom] editor wrote {buffer_size} bytes to port {port_index} (atom port {:?}); plugin ran {} blocks; to plugin {}/{} taken, to editor {}/{} taken",
                bridge.atoms.in_port,
                bridge.atoms.runs.load(std::sync::atomic::Ordering::Relaxed),
                bridge.atoms.to_plugin.carried(),
                bridge.atoms.to_plugin.taken(),
                bridge.atoms.to_editor.carried(),
                bridge.atoms.to_editor.taken(),
            );
        }
        if bridge.atoms.in_port != Some(port_index) {
            return;
        }
        // What arrives is a whole `LV2_Atom`: a size and a type, then that
        // many bytes. The pipe carries the type beside the body, so the two
        // words of header are read here and not copied.
        if (buffer_size as usize) < size_of::<AtomHeader>() {
            return;
        }
        let header = unsafe { *buffer.cast::<AtomHeader>() };
        let body = unsafe {
            std::slice::from_raw_parts(
                buffer.cast::<u8>().add(size_of::<AtomHeader>()),
                (header.size as usize).min(buffer_size as usize - size_of::<AtomHeader>()),
            )
        };
        bridge.atoms.to_plugin.push(header.atom_type, body);
        return;
    }
    // Protocol 0 is `ui:floatProtocol` — one control port, one float. Any
    // other is one this build does not know, and LV2 says a host must ignore
    // what it does not understand rather than guess at it.
    if protocol != 0 || buffer_size as usize != size_of::<f32>() {
        return;
    }
    let value = unsafe { *buffer.cast::<f32>() };
    if !value.is_finite() {
        return;
    }
    bridge.values.set(port_index, f64::from(value));
    if let Ok(mut written) = bridge.written.lock() {
        written.push((port_index, value));
    }
}

use crate::atom::trace;

/// The two words every atom starts with.
#[repr(C)]
#[derive(Clone, Copy)]
struct AtomHeader {
    size: u32,
    atom_type: u32,
}

/// `ui:resize`, which is how an LV2 editor asks for a different window.
#[repr(C)]
struct Lv2UiResize {
    handle: *mut c_void,
    ui_resize: extern "C" fn(*mut c_void, c_int, c_int) -> c_int,
}

extern "C" fn resize(handle: *mut c_void, width: c_int, height: c_int) -> c_int {
    let Some(bridge) = (unsafe { handle.cast::<UiBridge>().as_ref() }) else {
        return 1;
    };
    if width <= 0 || height <= 0 {
        return 1;
    }
    if let Ok(mut wanted) = bridge.resize.lock() {
        *wanted = Some(GuiSize {
            width: width as u32,
            height: height as u32,
        });
    }
    0
}

/// One LV2 editor, running.
pub(crate) struct Lv2Ui {
    /// Kept only to keep the UI's code mapped: every function pointer below
    /// lives in it, and dropping it `dlclose`s the library out from under
    /// them.
    _library: libloading::Library,
    descriptor: *const UiDescriptor,
    handle: LV2UIHandle,
    idle: Option<unsafe extern "C" fn(LV2UIHandle) -> c_int>,
    /// Pinned: the UI holds its address as `controller`.
    bridge: Box<UiBridge>,
    /// The features handed to `instantiate`, kept alive for the life of the
    /// UI — LV2 lets a plugin keep a feature's data, and several do.
    _features: FeatureSet,
    /// Every control port, and what the editor was last told it is.
    ///
    /// `NaN` for "never told", so the first tick sends the lot — which is what
    /// makes an editor open showing the values the song is at rather than its
    /// own defaults.
    last_sent: Vec<(u32, f32)>,
    /// The editor said it had been closed. Read once and remembered, because
    /// `idle` must not be called again after it says so.
    closed: bool,
    /// Where an atom is reassembled before being handed to the editor.
    ///
    /// Kept rather than made each time: the pipe carries a type and a body,
    /// and `port_event` wants the whole atom in one buffer. Main thread only,
    /// so growing it is nobody's problem.
    scratch: Vec<u8>,
}

/// The feature array handed to a UI, and everything it points at.
///
/// One struct because the array is pointers into the rest of it: moving any of
/// them after `instantiate` would leave the UI holding a dangling feature.
struct FeatureSet {
    /// The null-terminated array the UI was given.
    _pointers: Vec<*const LV2Feature>,
    /// **Boxed on purpose.** These are what `_pointers` points at, and a
    /// `Vec<LV2Feature>` would move them the next time it grew — which is
    /// only safe here by an argument about the order the pushes happen in.
    /// A box makes the address stable by construction instead.
    #[allow(clippy::vec_box)]
    _owned: Vec<Box<LV2Feature>>,
    _resize: Box<Lv2UiResize>,
}

impl Lv2Ui {
    /// Loads and starts the editor, into `window`.
    // Eight, because an LV2 UI is instantiated with eight things and there is
    // no grouping of them that is not just this list with a name.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open(
        info: &Lv2UiInfo,
        plugin_uri: &str,
        bundle: &Path,
        features: &Features,
        values: Arc<ParamValues>,
        atoms: Arc<crate::atom::AtomPipes>,
        control_ports: &[u32],
        window: &PluginWindow,
        instance: *mut c_void,
    ) -> Result<Self, GuiError> {
        // SAFETY: there is no safe version of this — an LV2 UI is a shared
        // library whose code runs on load, exactly as a CLAP bundle's does.
        // See `crate::plugin::load_entry`, which says the same thing.
        let library = unsafe { libloading::Library::new(&info.binary) }
            .map_err(|_| GuiError::Refused("load"))?;
        type DescriptorFn = unsafe extern "C" fn(u32) -> *const UiDescriptor;
        let entry: libloading::Symbol<DescriptorFn> =
            unsafe { library.get(b"lv2ui_descriptor\0") }.map_err(|_| GuiError::Refused("load"))?;

        // A binary may hold several UIs — LSP ships **one** library for all
        // three hundred of its plugins — so the right one is found by URI
        // rather than by taking the first, and the walk has to be long enough
        // to reach the end of a list that size. It stops at the null the
        // specification puts past the last one; the bound is only so that a
        // library which never returns null cannot hang the studio.
        let mut descriptor = std::ptr::null();
        for index in 0..MAX_UIS_PER_BINARY {
            let candidate = unsafe { entry(index) };
            let Some(found) = (unsafe { candidate.as_ref() }) else {
                break;
            };
            if unsafe { CStr::from_ptr(found.uri) } == info.uri.as_c_str() {
                descriptor = candidate;
                break;
            }
        }
        let Some(found) = (unsafe { descriptor.as_ref() }) else {
            return Err(GuiError::Refused("find"));
        };
        let Some(instantiate) = found.instantiate else {
            return Err(GuiError::Refused("create"));
        };

        let bridge = Box::new(UiBridge {
            values,
            event_transfer: features.urid(EVENT_TRANSFER),
            atoms,
            written: Mutex::new(Vec::new()),
            resize: Mutex::new(None),
        });
        let controller: LV2UIControllerRaw = (&raw const *bridge).cast();

        let mut owned: Vec<Box<LV2Feature>> = Vec::new();
        let mut pointers: Vec<*const LV2Feature> = Vec::new();
        // **The plugin's own map**, so an atom the editor names and an atom
        // the plugin names are the same number. A UI given a map of its own
        // would agree with nobody.
        let dummy = LV2Feature {
            uri: c"".as_ptr(),
            data: std::ptr::null_mut(),
        };
        for feature in features.iter_features(&dummy) {
            let uri = unsafe { CStr::from_ptr(feature.uri) };
            if uri
                .to_str()
                .is_ok_and(|uri| uri == URID_MAP || uri == URID_UNMAP)
            {
                pointers.push(feature as *const LV2Feature);
            }
        }
        // The window. An X11 UI's parent *is* the window id, carried in the
        // pointer-sized `data` field, which is what the extension says.
        owned.push(Box::new(LV2Feature {
            uri: UI_PARENT.as_ptr(),
            data: window.id() as usize as *mut c_void,
        }));
        // "This host calls `idle`" — declared as a feature as well as read as
        // an extension, because several UIs list it as a *required feature*
        // and refuse to start without it (x42's do).
        owned.push(Box::new(LV2Feature {
            uri: UI_IDLE.as_ptr(),
            data: std::ptr::null_mut(),
        }));
        let resize_feature = Box::new(Lv2UiResize {
            handle: (&raw const *bridge as *mut UiBridge).cast(),
            ui_resize: resize,
        });
        owned.push(Box::new(LV2Feature {
            uri: UI_RESIZE.as_ptr(),
            data: (&raw const *resize_feature as *mut Lv2UiResize).cast(),
        }));
        // **The running plugin itself**, when there is one. `instance-access`
        // is discouraged by the specification and required by every DPF-built
        // editor there is — Cardinal, Dexed, drumsynth, and a couple of dozen
        // more of the bundles installed on the reporter's machine — which
        // refuse to open without it: *"Host does not support instance-access,
        // cannot use UI"*. The pointer is the instance's own `LV2_Handle`;
        // whoever drops that instance has to close this editor first.
        if !instance.is_null() {
            owned.push(Box::new(LV2Feature {
                uri: INSTANCE_ACCESS.as_ptr(),
                data: instance,
            }));
        }
        for feature in &owned {
            pointers.push(&raw const **feature);
        }
        pointers.push(std::ptr::null());

        let plugin_uri = CString::new(plugin_uri).map_err(|_| GuiError::Refused("name"))?;
        let mut bundle_path = bundle.to_string_lossy().into_owned();
        if !bundle_path.ends_with('/') {
            bundle_path.push('/');
        }
        let bundle_path = CString::new(bundle_path).map_err(|_| GuiError::Refused("find"))?;
        let mut widget: LV2UIWidget = std::ptr::null_mut();
        // SAFETY: the UI's own `instantiate`, with the arguments the
        // specification lists, every one of which outlives the call and —
        // for the controller and the features — the UI itself.
        let handle = unsafe {
            instantiate(
                descriptor,
                plugin_uri.as_ptr(),
                bundle_path.as_ptr(),
                Some(write_port),
                controller,
                &raw mut widget,
                pointers.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(GuiError::Refused("create"));
        }

        // Both halves may be NULL — no `extension_data` at all, or one that
        // answers nothing for the idle interface — and an editor with no
        // `idle` is simply never driven, which is what it asked for.
        let idle = found
            .extension_data
            .and_then(|data| unsafe { data(UI_IDLE.as_ptr()).cast::<IdleInterface>().as_ref() })
            .and_then(|interface| interface.idle);

        Ok(Self {
            _library: library,
            descriptor,
            handle,
            idle,
            bridge,
            _features: FeatureSet {
                _pointers: pointers,
                _owned: owned,
                _resize: resize_feature,
            },
            last_sent: control_ports.iter().map(|port| (*port, f32::NAN)).collect(),
            closed: false,
            scratch: Vec::with_capacity(crate::atom::MAX_ATOM_BYTES + 8),
        })
    }

    /// One frame of the editor. `false` once it says it has closed itself.
    ///
    /// Three things in an order that matters: the editor draws, what it wrote
    /// is taken account of, and only then is it told what changed.
    pub(crate) fn tick(&mut self) -> bool {
        if self.closed {
            return false;
        }
        // SAFETY: the UI's own idle, with the handle it gave us.
        if let Some(idle) = self.idle
            && unsafe { idle(self.handle) } != 0
        {
            self.closed = true;
            return false;
        }
        // **What the editor wrote is not news to the editor.** Folding its own
        // writes into `last_sent` before the comparison below is the whole of
        // what stops a knob under the mouse from fighting the host for its
        // position.
        self.absorb_writes();

        // `None` for an editor that listens to nothing — see `UiDescriptor`.
        let port_event = unsafe { self.descriptor.as_ref() }.and_then(|found| found.port_event);
        if let Some(port_event) = port_event {
            for (port, sent) in self.last_sent.iter_mut() {
                let Some(value) = self.bridge.values.get(*port) else {
                    continue;
                };
                let value = value as f32;
                if *sent == value {
                    continue;
                }
                *sent = value;
                // SAFETY: the UI's own `port_event`, with its handle and a
                // float that outlives the call.
                unsafe {
                    port_event(
                        self.handle,
                        *port,
                        size_of::<f32>() as u32,
                        0,
                        (&raw const value).cast(),
                    );
                }
            }
        }
        // **And whatever the plugin said**, which is the direction that tells
        // an editor which file it actually loaded. Sent after the control
        // ports so that an atom and the values around it arrive in the order
        // the plugin produced them.
        if let Some(port_event) = port_event
            && let Some(port) = self.bridge.atoms.out_port
        {
            let mut scratch = std::mem::take(&mut self.scratch);
            self.bridge.atoms.to_editor.drain(|urid, body| {
                scratch.clear();
                scratch.extend_from_slice(&(body.len() as u32).to_ne_bytes());
                scratch.extend_from_slice(&urid.to_ne_bytes());
                scratch.extend_from_slice(body);
                // SAFETY: as above; the atom lives in `scratch` for the call.
                unsafe {
                    port_event(
                        self.handle,
                        port,
                        scratch.len() as u32,
                        self.bridge.event_transfer,
                        scratch.as_ptr().cast(),
                    );
                }
            });
            self.scratch = scratch;
        }
        // And again, because a `port_event` is entitled to answer with a write
        // — the fixture's editor does exactly that.
        self.absorb_writes();
        true
    }

    fn absorb_writes(&mut self) {
        let Ok(mut written) = self.bridge.written.lock() else {
            return;
        };
        for (port, value) in written.drain(..) {
            if let Some(sent) = self.last_sent.iter_mut().find(|(p, _)| *p == port) {
                sent.1 = value;
            }
        }
    }

    /// A size the editor asked its window to be, since last time.
    pub(crate) fn take_resize(&self) -> Option<GuiSize> {
        self.bridge.resize.lock().ok()?.take()
    }
}

impl Drop for Lv2Ui {
    fn drop(&mut self) {
        if let Some(descriptor) = unsafe { self.descriptor.as_ref() }
            && let Some(cleanup) = descriptor.cleanup
            && !self.handle.is_null()
        {
            // SAFETY: the UI's own `cleanup`, once, with the handle it gave.
            unsafe { cleanup(self.handle) };
        }
    }
}
