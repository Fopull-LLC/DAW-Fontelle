//! An LV2 plugin's **own** state, through `state:interface` (TDD §8.4).
//!
//! > *"put a sample into an LSP Multi-Sampler, save, quit, reopen the
//! > project, and hear the sample."*
//!
//! A control port is a float, and a sampler's whole state is *a file it
//! loaded*. LV2's answer is the state extension: the plugin's `extension_data`
//! hands over two functions, `save` and `restore`, each of which calls the
//! host back once per property — a key, a type, some bytes — and the host
//! keeps those however it likes. This module is that "however": a
//! [`Lv2State`] is the list of properties, and its [`encode`](Lv2State::encode)
//! is the blob that goes into `project.json` beside the control ports, exactly
//! where a CLAP plugin's blob already goes.
//!
//! # Why not lilv's state API
//!
//! lilv has `lilv_state_new_from_instance` and friends, and they do this and
//! more. They also serialise to **Turtle**, own the path mapping against a
//! directory of the host's choosing, and want the unmapped world of every
//! extension bundle to name their types — none of which fits a program that
//! keeps one blob in one JSON file and decides for itself what a path means
//! (INVARIANT 8, §17.4). The interface itself is two calls and two callbacks;
//! that is what is here, against no new dependency.
//!
//! # The two rules that make it safe
//!
//! - **The plugin must not be running.** The specification forbids calling
//!   `save` or `restore` while `run` is executing (`threadSafeRestore` is a
//!   plugin's opt-in and this build does not rely on it). For LV2 the whole
//!   instance rides in the [`crate::HostedProcessor`], so whoever calls in here
//!   is holding that processor on the main thread — see
//!   [`crate::ProcessorBay::recall`] for how it is fetched from a graph that
//!   is playing it.
//! - **Every path is mapped identity.** `state:mapPath` is offered — a sampler
//!   that finds it missing may refuse to store its file at all — and
//!   `abstract_path` answers with the absolute path it was given. That is the
//!   same rule an audio clip referenced in place follows (§17.4's headless
//!   default: *reference, never copy*): the project names the file by where
//!   it is. Making the abstract form project-relative, or copying the sample
//!   into the project, is the import prompt's decision and belongs to the
//!   pass that builds one for plugin-loaded files. `state:makePath` is not
//!   offered: nothing here has a folder to hand a plugin to write into.

use std::ffi::{CStr, CString, c_char, c_void};

use livi::Features;
use lv2_raw::LV2Feature;

const STATE_INTERFACE: &str = "http://lv2plug.in/ns/ext/state#interface";
const MAP_PATH: &CStr = c"http://lv2plug.in/ns/ext/state#mapPath";
const FREE_PATH: &CStr = c"http://lv2plug.in/ns/ext/state#freePath";

/// `LV2_STATE_IS_POD`: the value is plain bytes, not a pointer to something.
const IS_POD: u32 = 1;
/// `LV2_STATE_IS_PORTABLE`: the value means the same on another machine.
const IS_PORTABLE: u32 = 2;

const STATUS_SUCCESS: u32 = 0;
const STATUS_ERR_BAD_TYPE: u32 = 2;
const STATUS_ERR_BAD_FLAGS: u32 = 3;

/// One property a plugin stored: a key and a type, both URIs, and the bytes.
///
/// URIs rather than URIDs, because a URID is a number one *process* agreed
/// on and this is written to a file another process will read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lv2Property {
    pub key: String,
    pub type_uri: String,
    /// The `LV2_State_Flags` the plugin stored it with.
    pub flags: u32,
    pub value: Vec<u8>,
}

/// Everything a plugin stored, in the order it stored it.
///
/// Public, and decodable, so that what a plugin actually kept can be looked
/// at — the difference between *"the sampler forgot its file"* and *"the
/// sampler never stored one"* is in here and nowhere else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lv2State {
    pub properties: Vec<Lv2Property>,
}

/// The first four bytes of an encoded state, so a CLAP blob or a damaged one
/// is refused rather than read as an empty list.
const MAGIC: &[u8; 4] = b"FLV2";
const VERSION: u32 = 1;

impl Lv2State {
    /// The bytes the project keeps. Its own little format — a count, then
    /// length-prefixed key, type and value per property — because there is
    /// no standard one for a state held outside lilv, and JSON would double
    /// a sample's path in base64 for no reason.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            16 + self
                .properties
                .iter()
                .map(|p| 16 + p.key.len() + p.type_uri.len() + p.value.len())
                .sum::<usize>(),
        );
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(self.properties.len() as u32).to_le_bytes());
        for property in &self.properties {
            for text in [property.key.as_bytes(), property.type_uri.as_bytes()] {
                out.extend_from_slice(&(text.len() as u32).to_le_bytes());
                out.extend_from_slice(text);
            }
            out.extend_from_slice(&property.flags.to_le_bytes());
            out.extend_from_slice(&(property.value.len() as u32).to_le_bytes());
            out.extend_from_slice(&property.value);
        }
        out
    }

    /// Reads [`encode`](Self::encode)'s bytes back. `None` for anything that
    /// is not one of them, rather than a partial list.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut cursor = Cursor { bytes, at: 0 };
        if cursor.take(4)? != MAGIC {
            return None;
        }
        if cursor.u32()? != VERSION {
            return None;
        }
        let count = cursor.u32()? as usize;
        let mut properties = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            let key = String::from_utf8(cursor.blob()?.to_vec()).ok()?;
            let type_uri = String::from_utf8(cursor.blob()?.to_vec()).ok()?;
            let flags = cursor.u32()?;
            let value = cursor.blob()?.to_vec();
            properties.push(Lv2Property {
                key,
                type_uri,
                flags,
                value,
            });
        }
        (cursor.at == bytes.len()).then_some(Self { properties })
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    fn blob(&mut self) -> Option<&'a [u8]> {
        let len = self.u32()? as usize;
        self.take(len)
    }
}

// ------------------------------------------------------------ the C ABI ---
//
// Written out here rather than taken from a crate: `lv2_raw` at the version
// `livi` pins has no state module, and these are three small structs fixed by
// the specification's `state.h`. Every function pointer is an `Option`,
// because every one of them may be NULL on the plugin's side and reading a
// null through a type that cannot be null is undefined behaviour the
// optimiser is entitled to exploit — see `lv2_ui` for the crash that taught
// this.

type StoreFn = unsafe extern "C" fn(
    handle: *mut c_void,
    key: u32,
    value: *const c_void,
    size: usize,
    type_: u32,
    flags: u32,
) -> u32;

type RetrieveFn = unsafe extern "C" fn(
    handle: *mut c_void,
    key: u32,
    size: *mut usize,
    type_: *mut u32,
    flags: *mut u32,
) -> *const c_void;

/// `LV2_State_Interface`.
#[repr(C)]
struct Interface {
    save: Option<
        unsafe extern "C" fn(
            instance: *mut c_void,
            store: Option<StoreFn>,
            handle: *mut c_void,
            flags: u32,
            features: *const *const LV2Feature,
        ) -> u32,
    >,
    restore: Option<
        unsafe extern "C" fn(
            instance: *mut c_void,
            retrieve: Option<RetrieveFn>,
            handle: *mut c_void,
            flags: u32,
            features: *const *const LV2Feature,
        ) -> u32,
    >,
}

type PathFn = unsafe extern "C" fn(handle: *mut c_void, path: *const c_char) -> *mut c_char;

/// `LV2_State_Map_Path`.
#[repr(C)]
struct MapPath {
    handle: *mut c_void,
    abstract_path: Option<PathFn>,
    absolute_path: Option<PathFn>,
}

/// `LV2_State_Free_Path`.
#[repr(C)]
struct FreePath {
    handle: *mut c_void,
    free_path: Option<unsafe extern "C" fn(handle: *mut c_void, path: *mut c_char)>,
}

/// Identity, both ways — see the module note on why.
///
/// # Safety
/// Called by the plugin with a valid C string, or null, per the contract.
unsafe extern "C" fn identity_path(_handle: *mut c_void, path: *const c_char) -> *mut c_char {
    if path.is_null() {
        return std::ptr::null_mut();
    }
    unsafe { CStr::from_ptr(path) }.to_owned().into_raw()
}

/// Frees what [`identity_path`] handed out, and only that: LV2 says a path
/// the host returned is freed through the host's own `freePath`.
///
/// # Safety
/// `path` came from [`identity_path`], or is null.
unsafe extern "C" fn free_path(_handle: *mut c_void, path: *mut c_char) {
    if !path.is_null() {
        drop(unsafe { CString::from_raw(path) });
    }
}

/// The feature array a `save` or `restore` is handed, and what it points at.
///
/// Boxed so the addresses are stable for the life of the call; the plugin
/// keeps nothing past it.
struct PathFeatures {
    _map: Box<MapPath>,
    _free: Box<FreePath>,
    #[allow(clippy::vec_box)]
    _owned: Vec<Box<LV2Feature>>,
    pointers: Vec<*const LV2Feature>,
}

impl PathFeatures {
    fn new() -> Self {
        let map = Box::new(MapPath {
            handle: std::ptr::null_mut(),
            abstract_path: Some(identity_path),
            absolute_path: Some(identity_path),
        });
        let free = Box::new(FreePath {
            handle: std::ptr::null_mut(),
            free_path: Some(free_path),
        });
        let owned = vec![
            Box::new(LV2Feature {
                uri: MAP_PATH.as_ptr(),
                data: (&raw const *map as *mut MapPath).cast(),
            }),
            Box::new(LV2Feature {
                uri: FREE_PATH.as_ptr(),
                data: (&raw const *free as *mut FreePath).cast(),
            }),
        ];
        let mut pointers: Vec<*const LV2Feature> =
            owned.iter().map(|feature| &raw const **feature).collect();
        pointers.push(std::ptr::null());
        Self {
            _map: map,
            _free: free,
            _owned: owned,
            pointers,
        }
    }
}

/// What the plugin's `save` writes into, one callback at a time.
struct Store<'a> {
    features: &'a Features,
    properties: Vec<Lv2Property>,
}

/// `LV2_State_Store_Function`.
///
/// # Safety
/// Called by the plugin with the `handle` it was given, which is a [`Store`]
/// that outlives the `save` call, and `size` bytes at `value`.
unsafe extern "C" fn store(
    handle: *mut c_void,
    key: u32,
    value: *const c_void,
    size: usize,
    type_: u32,
    flags: u32,
) -> u32 {
    let Some(store) = (unsafe { handle.cast::<Store>().as_mut() }) else {
        return STATUS_ERR_BAD_TYPE;
    };
    // A value that is not plain bytes is a pointer into the plugin, and a
    // pointer cannot be written to a file. Refused, as the specification
    // says a host that cannot store it should.
    if flags & IS_POD == 0 {
        return STATUS_ERR_BAD_FLAGS;
    }
    let (Some(key_uri), Some(type_uri)) = (store.features.uri(key), store.features.uri(type_))
    else {
        return STATUS_ERR_BAD_TYPE;
    };
    let value = if value.is_null() || size == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(value.cast::<u8>(), size) }.to_vec()
    };
    store.properties.push(Lv2Property {
        key: key_uri.to_string(),
        type_uri: type_uri.to_string(),
        flags,
        value,
    });
    STATUS_SUCCESS
}

/// What the plugin's `restore` reads from: the stored properties, with their
/// URIs mapped back to this process's URIDs.
struct Retrieve {
    entries: Vec<(u32, u32, u32, Vec<u8>)>,
}

/// `LV2_State_Retrieve_Function`.
///
/// # Safety
/// Called by the plugin with the `handle` it was given, which is a
/// [`Retrieve`] that outlives the `restore` call; the out-pointers are valid
/// or null.
unsafe extern "C" fn retrieve(
    handle: *mut c_void,
    key: u32,
    size: *mut usize,
    type_: *mut u32,
    flags: *mut u32,
) -> *const c_void {
    let Some(table) = (unsafe { handle.cast::<Retrieve>().as_ref() }) else {
        return std::ptr::null();
    };
    let Some((_, stored_type, stored_flags, value)) = table
        .entries
        .iter()
        .find(|(stored_key, ..)| *stored_key == key)
    else {
        return std::ptr::null();
    };
    unsafe {
        if let Some(size) = size.as_mut() {
            *size = value.len();
        }
        if let Some(type_) = type_.as_mut() {
            *type_ = *stored_type;
        }
        if let Some(flags) = flags.as_mut() {
            *flags = *stored_flags;
        }
    }
    value.as_ptr().cast()
}

/// Whether the plugin's Turtle declares `state:interface`.
///
/// Off the description rather than the instance, so a plugin can say it
/// keeps state before it has been activated — which is when the rack asks.
pub(crate) fn declares_interface(plugin: &livi::Plugin, world: &livi::World) -> bool {
    plugin
        .raw()
        .has_extension_data(&world.raw().new_uri(STATE_INTERFACE))
}

/// Asks a plugin for its state. `None` when it has no interface, or refused.
///
/// # Safety
/// The instance must not be running — see the module note.
pub(crate) unsafe fn save(
    instance: &livi::lilv::instance::Instance,
    features: &Features,
) -> Option<Lv2State> {
    let interface = unsafe { instance.extension_data::<Interface>(STATE_INTERFACE) }?;
    let save = unsafe { interface.as_ref() }.save?;
    let mut store_into = Store {
        features,
        properties: Vec::new(),
    };
    let paths = PathFeatures::new();
    let status = unsafe {
        save(
            instance.handle(),
            Some(store),
            (&raw mut store_into).cast(),
            IS_POD | IS_PORTABLE,
            paths.pointers.as_ptr(),
        )
    };
    (status == STATUS_SUCCESS).then_some(Lv2State {
        properties: store_into.properties,
    })
}

/// Hands a plugin its state back. `false` when it has no interface, or
/// refused what it was given.
///
/// A key or type whose URI this process cannot map is left out rather than
/// failing the lot: the plugin is told to fall back to a default for a
/// property it cannot find, which the specification requires of it.
///
/// # Safety
/// The instance must not be running — see the module note.
pub(crate) unsafe fn restore(
    instance: &livi::lilv::instance::Instance,
    features: &Features,
    state: &Lv2State,
) -> bool {
    let Some(interface) = (unsafe { instance.extension_data::<Interface>(STATE_INTERFACE) }) else {
        return false;
    };
    let Some(restore) = unsafe { interface.as_ref() }.restore else {
        return false;
    };
    let mut table = Retrieve {
        entries: Vec::with_capacity(state.properties.len()),
    };
    for property in &state.properties {
        let (Ok(key), Ok(type_uri)) = (
            CString::new(property.key.as_str()),
            CString::new(property.type_uri.as_str()),
        ) else {
            continue;
        };
        table.entries.push((
            features.urid(&key),
            features.urid(&type_uri),
            property.flags,
            property.value.clone(),
        ));
    }
    let paths = PathFeatures::new();
    let status = unsafe {
        restore(
            instance.handle(),
            Some(retrieve),
            (&raw mut table).cast(),
            0,
            paths.pointers.as_ptr(),
        )
    };
    status == STATUS_SUCCESS
}
