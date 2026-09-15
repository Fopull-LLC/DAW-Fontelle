//! The third format hosted in the tree: VST 3, through the `vst3` crate.
//!
//! > *"a lot of people may not want to switch to the daw if they can't use
//! > their paid vsts in it ... maximum compatibility is what's most
//! > important to me."*
//!
//! For two years §3.4 kept VST 3 behind a bridge because the SDK was GPLv3
//! or a signed proprietary agreement. On 31 October 2025 Steinberg released
//! SDK 3.8.0 under MIT (`docs/vst-plan.md` §1), which put the interfaces on
//! §3.4's allowlist and took away every reason for the bridge. The binding
//! is `vst3` (coupler-rs; MIT OR Apache-2.0, generated from the MIT-era
//! headers) — not `vst3-sys`, which is GPLv3. It gives both halves: the
//! plugin's interfaces to call, and [`vst3::Class`] to implement the host's
//! from Rust, which is everything a host is: `IHostApplication`,
//! `IComponentHandler`, `IEventList`, `IParameterChanges`, `IBStream`,
//! `IPlugFrame` and `IRunLoop`, all below.
//!
//! # What a VST 3 plugin is, for the host
//!
//! A bundle — a folder, `Name.vst3/Contents/<arch>/Name.so` — exporting
//! `GetPluginFactory`, and since SDK 3.7.9 a `moduleinfo.json` that lists
//! the classes without loading the library. The factory makes **classes**;
//! an instrument or effect is an *Audio Module Class* whose object is the
//! **component** (`IComponent` + `IAudioProcessor`), with an
//! **edit controller** (`IEditController`: parameters, the editor) usually
//! as a second object the component names, and sometimes the same one.
//! The two halves talk through `IConnectionPoint` in `IMessage`s the host
//! makes for them.
//!
//! # What is different about this arm
//!
//! - **Parameters are normalised on the wire.** Every value a VST 3 plugin
//!   exchanges with its host runs 0..1, and turning one into the plugin's
//!   own units is a controller call — main thread only. So a hosted VST 3
//!   parameter's range *is* 0..1 (0..steps for a stepped one), the RT
//!   thread converts by arithmetic alone, and the plugin's units appear
//!   through `getParamStringByValue` in [`Vst3Plugin::display`].
//! - **The wheels are parameters.** VST 3 has no controller events: a mod
//!   wheel, a bend and aftertouch are whichever parameters the plugin maps
//!   them to through `IMidiMapping`, resolved once at open and driven as
//!   parameter changes at the frame they happened.
//! - **A slide is exactly what VST 3 carries**: a note expression of
//!   `kTuningTypeID` addressed to the note, so a slide of an octave into a
//!   VST 3 instrument is an octave, as it is for CLAP.
//! - **State is two blobs**, the component's and the controller's, kept
//!   opaque the way an LV2 blob is.
//! - **Every bus is activated and handed a buffer** — the CLAP lesson of
//!   2026-09-05 applies word for word, and a DPF-built plugin adds one of
//!   its own: it asserts that `outputParameterChanges` is non-null, so the
//!   host hands every plugin a real output queue too.
//!
//! The editor is an `IPlugView` attached to the X11 window `gui.rs` makes,
//! with the host's `IPlugFrame` (a resize asked of the window) and
//! `IRunLoop` (the timers and descriptors a JUCE plugin's message loop
//! needs pumping) behind it — the same two host services CLAP's `timer`
//! and `posix-fd` are, on the same [`crate::gui::GuiPump`].

#![allow(non_snake_case)]
// The SDK's enums are `c_uint` here and `c_int` on Windows, so a cast that
// is a no-op on one platform is the conversion on the other.
#![allow(clippy::unnecessary_cast)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use fontelle_types::{PluginFormat, PluginKey};
use vst3::Steinberg::Linux::{
    IEventHandler, IRunLoop, IRunLoopTrait, ITimerHandler, ITimerHandlerTrait,
};
// The handler trait's `onFDIsSet` is called only where descriptors are
// watched — a POSIX thing, like CLAP's `posix-fd`.
#[cfg(unix)]
use vst3::Steinberg::Linux::IEventHandlerTrait;
use vst3::Steinberg::Vst::IAttributeList_::AttrID;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface};

use crate::param::{HostedParam, ParamValues};
use crate::plugin::{HostError, NoteDialect, PortLayout};
use crate::scan::PluginInfo;

/// The most events one block may carry into a plugin — see
/// `processor::MAX_EVENTS`, which this mirrors.
const MAX_EVENTS: usize = 256;
/// The most points one parameter's queue holds in a block: one from the
/// wire at the top, and a wheel moved a few times inside it.
const MAX_POINTS: usize = 8;
/// What a plugin is set up for at open, to read its latency before there
/// is a graph to activate it in.
const NOMINAL_RATE: f64 = 48_000.0;
const NOMINAL_BLOCK: i32 = 1024;

const K_AUDIO: MediaType = MediaTypes_::kAudio as MediaType;
const K_EVENT: MediaType = MediaTypes_::kEvent as MediaType;
const K_INPUT: BusDirection = BusDirections_::kInput as BusDirection;
const K_OUTPUT: BusDirection = BusDirections_::kOutput as BusDirection;

// ------------------------------------------------------------------ strings

fn wide_string(chars: &[TChar]) -> String {
    let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
    String::from_utf16_lossy(&chars[..end])
}

fn narrow_string(chars: &[char8]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn write_wide(text: &str, out: &mut [TChar]) {
    let mut n = 0;
    for (slot, unit) in out.iter_mut().zip(text.encode_utf16()) {
        *slot = unit;
        n += 1;
    }
    if n < out.len() {
        out[n] = 0;
    } else if let Some(last) = out.last_mut() {
        *last = 0;
    }
}

fn guid(tuid: TUID) -> vst3::com_scrape_types::Guid {
    tuid.map(|c| c as u8)
}

/// The class id as this program writes it: the four 32-bit words of the
/// uid, as eight hex digits each — `FUID::toString`'s spelling and what
/// `moduleinfo.json` carries.
///
/// The bytes of a `TUID` are laid out COM-style on Windows (the first
/// three words little-endian) and plainly everywhere else, so the same
/// plugin's raw bytes differ by platform. Spelling the *words* makes the
/// key the same string on every platform, which is what a project saved on
/// one and opened on another needs (INVARIANT 8 for plugins).
pub(crate) fn cid_string(tuid: TUID) -> String {
    let b = guid(tuid);
    let words = if cfg!(windows) {
        [
            u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            u32::from_be_bytes([b[5], b[4], b[7], b[6]]),
            u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
            u32::from_be_bytes([b[12], b[13], b[14], b[15]]),
        ]
    } else {
        [
            u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
            u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
            u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
            u32::from_be_bytes([b[12], b[13], b[14], b[15]]),
        ]
    };
    words.iter().map(|w| format!("{w:08X}")).collect()
}

/// The other platform family's spelling of the same id — a bundle whose
/// `moduleinfo.json` was written on Windows and is read here, or the
/// reverse. Compared as a second chance when the first does not match.
fn other_spelling(id: &str) -> Option<String> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let (a, rest) = id.split_at(8);
    let (b, rest) = rest.split_at(8);
    let swap4 = |s: &str| -> String {
        let mut out = String::new();
        for i in (0..8).step_by(2).rev() {
            out.push_str(&s[i..i + 2]);
        }
        out
    };
    let swap2 = |s: &str| -> String { format!("{}{}{}{}", &s[2..4], &s[0..2], &s[6..8], &s[4..6]) };
    Some(format!("{}{}{}", swap4(a), swap2(b), rest))
}

// ------------------------------------------------------------- the module

/// One loaded `.vst3` library and its factory.
///
/// Shared by every plugin opened out of it and every processor those hand
/// out, so the library outlives anything still calling into it — the same
/// shape [`crate::bridge::Bridges`] has.
pub(crate) struct Module {
    /// Released by hand in `drop`, **before** the module's exit point is
    /// called: the factory points into the library, and the SDK's order is
    /// release everything, then exit, then unload.
    factory: std::mem::ManuallyDrop<ComPtr<IPluginFactory>>,
    library: libloading::Library,
}

impl Drop for Module {
    fn drop(&mut self) {
        // SAFETY: dropped exactly once, here, and never used after.
        unsafe { std::mem::ManuallyDrop::drop(&mut self.factory) };
        exit_module(&self.library);
    }
}

fn exit_module(library: &libloading::Library) {
    #[cfg(target_os = "linux")]
    let name: &[u8] = b"ModuleExit";
    #[cfg(target_os = "windows")]
    let name: &[u8] = b"ExitDll";
    #[cfg(target_os = "macos")]
    let name: &[u8] = b"bundleExit";
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let name: &[u8] = b"ModuleExit";
    // SAFETY: the symbol's signature is the SDK's.
    if let Ok(exit) = unsafe { library.get::<unsafe extern "system" fn() -> bool>(name) } {
        unsafe { exit() };
    }
}

/// Where the library is inside a bundle on this platform — or the path
/// itself, for the single-file `.vst3` Windows plugins were before bundles.
pub(crate) fn library_path(bundle: &Path) -> Result<PathBuf, String> {
    if bundle.is_file() {
        return Ok(bundle.to_path_buf());
    }
    let contents = bundle.join("Contents");
    let arch = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", arch) => format!("{arch}-linux"),
        ("windows", "x86_64") => "x86_64-win".to_string(),
        ("windows", "aarch64") => "arm64-win".to_string(),
        ("windows", _) => "x86-win".to_string(),
        ("macos", _) => "MacOS".to_string(),
        (os, arch) => format!("{arch}-{os}"),
    };
    let is_library = |path: &Path| {
        path.is_file()
            && if cfg!(windows) {
                path.extension().is_some_and(|e| e == "vst3")
            } else if cfg!(target_os = "macos") {
                true
            } else {
                path.extension().is_some_and(|e| e == "so")
            }
    };
    let first_library = |folder: &Path| {
        let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| is_library(path))
            .collect();
        found.sort();
        found.into_iter().next()
    };
    if let Some(found) = first_library(&contents.join(&arch)) {
        return Ok(found);
    }
    // Not the folder this platform expects: any library under `Contents`
    // is worth a try, and a folder with none is what a bundle built for
    // another platform looks like.
    let mut folders: Vec<PathBuf> = std::fs::read_dir(&contents)
        .map_err(|_| "the bundle has no Contents folder".to_string())?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    folders
        .iter()
        .find_map(|folder| first_library(folder))
        .ok_or_else(|| format!("no library for {arch} in the bundle"))
}

/// Loads the library and runs its entry point.
///
/// **This runs somebody else's code**, as opening any plugin does.
pub(crate) fn load_module(bundle: &Path) -> Result<Arc<Module>, String> {
    let path = library_path(bundle)?;
    // SAFETY: loading a plugin library runs its initialisers, which is the
    // whole point and cannot be made safe.
    let library = unsafe { libloading::Library::new(&path) }.map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    {
        // `ModuleEntry` is handed the `dlopen` handle, which the SDK's own
        // module code keeps for `dladdr`-style lookups of its own path.
        let raw: libloading::os::unix::Library = library.into();
        let handle = raw.into_raw();
        // SAFETY: the symbol's signature is the SDK's; the handle is live.
        let library: libloading::Library =
            unsafe { libloading::os::unix::Library::from_raw(handle) }.into();
        if let Ok(entry) =
            unsafe { library.get::<unsafe extern "system" fn(*mut c_void) -> bool>(b"ModuleEntry") }
            && !unsafe { entry(handle) }
        {
            return Err("the module's entry point refused".to_string());
        }
        return finish_load(library);
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(entry) = unsafe { library.get::<unsafe extern "system" fn() -> bool>(b"InitDll") }
            && !unsafe { entry() }
        {
            return Err("the module's entry point refused".to_string());
        }
        return finish_load(library);
    }
    #[cfg(target_os = "macos")]
    {
        // `bundleEntry` wants a `CFBundleRef`; the SDK's own implementation
        // only keeps it, and a null is what every non-CoreFoundation host
        // hands over.
        if let Ok(entry) =
            unsafe { library.get::<unsafe extern "system" fn(*mut c_void) -> bool>(b"bundleEntry") }
            && !unsafe { entry(std::ptr::null_mut()) }
        {
            return Err("the module's entry point refused".to_string());
        }
        return finish_load(library);
    }
    #[allow(unreachable_code)]
    finish_load(library)
}

fn finish_load(library: libloading::Library) -> Result<Arc<Module>, String> {
    // SAFETY: the symbol's signature is the SDK's.
    let get_factory = unsafe {
        library.get::<unsafe extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory")
    }
    .map_err(|_| "the library exports no GetPluginFactory".to_string())?;
    let factory = unsafe { ComPtr::from_raw(get_factory()) }
        .ok_or_else(|| "GetPluginFactory returned nothing".to_string())?;
    Ok(Arc::new(Module {
        factory: std::mem::ManuallyDrop::new(factory),
        library,
    }))
}

/// One class the factory lists.
struct ClassInfo {
    cid: TUID,
    category: String,
    name: String,
    vendor: String,
    version: String,
    sub_categories: String,
}

impl Module {
    fn classes(&self) -> Vec<ClassInfo> {
        let mut vendor_fallback = String::new();
        // SAFETY: the factory is valid for the life of the module.
        unsafe {
            let mut info: PFactoryInfo = std::mem::zeroed();
            if self.factory.getFactoryInfo(&mut info) == kResultOk {
                vendor_fallback = narrow_string(&info.vendor);
            }
        }
        let factory2 = self.factory.cast::<IPluginFactory2>();
        let count = unsafe { self.factory.countClasses() };
        let mut classes = Vec::with_capacity(count.max(0) as usize);
        for index in 0..count {
            if let Some(factory2) = &factory2 {
                let mut info: PClassInfo2 = unsafe { std::mem::zeroed() };
                if unsafe { factory2.getClassInfo2(index, &mut info) } == kResultOk {
                    classes.push(ClassInfo {
                        cid: info.cid,
                        category: narrow_string(&info.category),
                        name: narrow_string(&info.name),
                        vendor: {
                            let vendor = narrow_string(&info.vendor);
                            if vendor.is_empty() {
                                vendor_fallback.clone()
                            } else {
                                vendor
                            }
                        },
                        version: narrow_string(&info.version),
                        sub_categories: narrow_string(&info.subCategories),
                    });
                    continue;
                }
            }
            let mut info: PClassInfo = unsafe { std::mem::zeroed() };
            if unsafe { self.factory.getClassInfo(index, &mut info) } == kResultOk {
                classes.push(ClassInfo {
                    cid: info.cid,
                    category: narrow_string(&info.category),
                    name: narrow_string(&info.name),
                    vendor: vendor_fallback.clone(),
                    version: String::new(),
                    sub_categories: String::new(),
                });
            }
        }
        classes
    }

    fn plugin_infos(&self, bundle: &Path) -> Vec<PluginInfo> {
        self.classes()
            .into_iter()
            .filter(|class| class.category == "Audio Module Class")
            .map(|class| PluginInfo {
                key: PluginKey::new(PluginFormat::Vst3, cid_string(class.cid)),
                path: bundle.to_path_buf(),
                name: class.name,
                vendor: class.vendor,
                version: class.version,
                features: features_of(&class.sub_categories),
            })
            .collect()
    }

    /// Makes one object of class `cid`, asking for `I`.
    fn create<I: Interface>(&self, cid: &TUID) -> Option<ComPtr<I>> {
        let mut obj: *mut c_void = std::ptr::null_mut();
        // SAFETY: the factory's contract; both ids live for the call.
        let result = unsafe {
            self.factory
                .createInstance(cid.as_ptr(), I::IID.as_ptr() as *const c_char, &mut obj)
        };
        if result != kResultOk || obj.is_null() {
            return None;
        }
        unsafe { ComPtr::from_raw(obj as *mut I) }
    }
}

/// VST 3's sub-categories, as the feature words the rest of this crate
/// reads: `Instrument` says instrument, `Fx` says effect, and the rest are
/// kept as they were said, lowercased.
fn features_of(sub_categories: &str) -> Vec<String> {
    sub_categories
        .split('|')
        .filter(|s| !s.is_empty())
        .map(|s| match s {
            "Instrument" => "instrument".to_string(),
            "Fx" => "audio-effect".to_string(),
            "Synth" => "synthesizer".to_string(),
            "Analyzer" => "analyzer".to_string(),
            other => other.to_lowercase(),
        })
        .collect()
}

// -------------------------------------------------------------- scanning

/// Everything one bundle holds: read off its `moduleinfo.json` when it
/// ships one, and off the loaded factory otherwise.
pub(crate) fn read_vst3_bundle(bundle: &Path) -> Result<Vec<PluginInfo>, String> {
    for candidate in [
        bundle.join("Contents/Resources/moduleinfo.json"),
        bundle.join("Contents/moduleinfo.json"),
    ] {
        if let Ok(text) = std::fs::read_to_string(&candidate)
            && let Some(found) = read_moduleinfo(&text, bundle)
        {
            return Ok(found);
        }
    }
    let module = load_module(bundle)?;
    let found = module.plugin_infos(bundle);
    if found.is_empty() {
        return Err("the module lists no audio module class".to_string());
    }
    Ok(found)
}

/// Reads a `moduleinfo.json`. `None` when the file does not parse or lists
/// no audio module class, in which case the library is loaded instead.
///
/// The SDK's writer leaves a trailing comma after the last element of every
/// list and object, and its reader accepts them; `serde_json` does not, so
/// they are stripped first.
fn read_moduleinfo(text: &str, bundle: &Path) -> Option<Vec<PluginInfo>> {
    let json: serde_json::Value = serde_json::from_str(&strip_trailing_commas(text)).ok()?;
    let module_version = json.get("Version")?.as_str().unwrap_or("").to_string();
    let factory_vendor = json
        .get("Factory Info")
        .and_then(|f| f.get("Vendor"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let classes = json.get("Classes")?.as_array()?;
    let found: Vec<PluginInfo> = classes
        .iter()
        .filter(|class| {
            class.get("Category").and_then(|c| c.as_str()) == Some("Audio Module Class")
        })
        .filter_map(|class| {
            let cid = class.get("CID")?.as_str()?.to_ascii_uppercase();
            let sub: Vec<String> = class
                .get("Sub Categories")
                .and_then(|s| s.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let text = |key: &str, fallback: &str| {
                class
                    .get(key)
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or(fallback)
                    .to_string()
            };
            Some(PluginInfo {
                key: PluginKey::new(PluginFormat::Vst3, cid),
                path: bundle.to_path_buf(),
                name: text("Name", ""),
                vendor: text("Vendor", &factory_vendor),
                version: text("Version", &module_version),
                features: features_of(&sub.join("|")),
            })
        })
        .collect();
    (!found.is_empty()).then_some(found)
}

/// Removes a `,` that is followed, over whitespace, by `]` or `}` —
/// outside strings.
fn strip_trailing_commas(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            out.push(c as char);
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_string = true;
            out.push('"');
            i += 1;
            continue;
        }
        if c == b',' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b']' || bytes[j] == b'}') {
                i += 1;
                continue;
            }
        }
        // Non-ASCII bytes pass through untouched: the text is UTF-8 and only
        // ASCII punctuation is inspected.
        let ch_len = utf8_len(c);
        out.push_str(&text[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// The folders the VST 3 SDK says a plugin is installed in on this
/// platform, plus `VST3_PATH`, honoured the way `CLAP_PATH` is.
pub(crate) fn search_paths(home: Option<&PathBuf>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(from_env) = std::env::var_os("VST3_PATH") {
        paths.extend(std::env::split_paths(&from_env).filter(|path| path.is_absolute()));
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = home {
            paths.push(home.join(".vst3"));
        }
        paths.push(PathBuf::from("/usr/lib/vst3"));
        paths.push(PathBuf::from("/usr/local/lib/vst3"));
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = home {
            paths.push(home.join("Library/Audio/Plug-Ins/VST3"));
        }
        paths.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
    }
    #[cfg(target_os = "windows")]
    {
        let _ = home;
        if let Some(common) = std::env::var_os("COMMONPROGRAMFILES") {
            paths.push(PathBuf::from(common).join("VST3"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            paths.push(PathBuf::from(local).join("Programs/Common/VST3"));
        }
    }
    paths
}

// ------------------------------------------------ the host's own objects

/// What this program tells a plugin about itself, and the two things a
/// plugin asks it to make: an `IMessage` and its `IAttributeList`, which
/// are how a component and its controller talk.
struct HostApplication;

impl Class for HostApplication {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApplication {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        write_wide("Fontelle", unsafe { &mut *name });
        kResultOk
    }

    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        obj: *mut *mut c_void,
    ) -> tresult {
        let (cid, iid) = unsafe { (guid(*cid), guid(*iid)) };
        if cid == IMessage::IID && iid == IMessage::IID {
            let message = ComWrapper::new(Message::default());
            unsafe { *obj = message.to_com_ptr::<IMessage>().unwrap().into_raw() as *mut c_void };
            return kResultOk;
        }
        if cid == IAttributeList::IID && iid == IAttributeList::IID {
            let list = ComWrapper::new(Attributes::default());
            unsafe {
                *obj = list.to_com_ptr::<IAttributeList>().unwrap().into_raw() as *mut c_void
            };
            return kResultOk;
        }
        unsafe { *obj = std::ptr::null_mut() };
        kNoInterface
    }
}

/// An attribute list: four maps, because the four getters are typed.
#[derive(Default)]
struct Attributes {
    ints: Mutex<HashMap<String, i64>>,
    floats: Mutex<HashMap<String, f64>>,
    strings: Mutex<HashMap<String, Vec<u16>>>,
    binaries: Mutex<HashMap<String, Vec<u8>>>,
}

impl Class for Attributes {
    type Interfaces = (IAttributeList,);
}

unsafe fn attr_key(id: AttrID) -> String {
    if id.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned()
}

impl IAttributeListTrait for Attributes {
    unsafe fn setInt(&self, id: AttrID, value: int64) -> tresult {
        self.ints
            .lock()
            .unwrap()
            .insert(unsafe { attr_key(id) }, value);
        kResultOk
    }
    unsafe fn getInt(&self, id: AttrID, value: *mut int64) -> tresult {
        match self.ints.lock().unwrap().get(&unsafe { attr_key(id) }) {
            Some(v) => {
                unsafe { *value = *v };
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn setFloat(&self, id: AttrID, value: f64) -> tresult {
        self.floats
            .lock()
            .unwrap()
            .insert(unsafe { attr_key(id) }, value);
        kResultOk
    }
    unsafe fn getFloat(&self, id: AttrID, value: *mut f64) -> tresult {
        match self.floats.lock().unwrap().get(&unsafe { attr_key(id) }) {
            Some(v) => {
                unsafe { *value = *v };
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn setString(&self, id: AttrID, string: *const TChar) -> tresult {
        let mut chars = Vec::new();
        let mut p = string;
        while !p.is_null() && unsafe { *p } != 0 {
            chars.push(unsafe { *p });
            p = unsafe { p.add(1) };
        }
        self.strings
            .lock()
            .unwrap()
            .insert(unsafe { attr_key(id) }, chars);
        kResultOk
    }
    unsafe fn getString(&self, id: AttrID, string: *mut TChar, size_in_bytes: uint32) -> tresult {
        match self.strings.lock().unwrap().get(&unsafe { attr_key(id) }) {
            Some(chars) => {
                let room = (size_in_bytes as usize / 2).saturating_sub(1);
                let n = chars.len().min(room);
                unsafe {
                    std::ptr::copy_nonoverlapping(chars.as_ptr(), string, n);
                    *string.add(n) = 0;
                }
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn setBinary(&self, id: AttrID, data: *const c_void, size_in_bytes: uint32) -> tresult {
        let bytes = if data.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(data as *const u8, size_in_bytes as usize) }
                .to_vec()
        };
        self.binaries
            .lock()
            .unwrap()
            .insert(unsafe { attr_key(id) }, bytes);
        kResultOk
    }
    unsafe fn getBinary(
        &self,
        id: AttrID,
        data: *mut *const c_void,
        size_in_bytes: *mut uint32,
    ) -> tresult {
        match self.binaries.lock().unwrap().get(&unsafe { attr_key(id) }) {
            Some(bytes) => {
                unsafe {
                    *data = bytes.as_ptr() as *const c_void;
                    *size_in_bytes = bytes.len() as u32;
                }
                kResultOk
            }
            None => kResultFalse,
        }
    }
}

struct Message {
    id: Mutex<CString>,
    attributes: ComWrapper<Attributes>,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            id: Mutex::new(CString::default()),
            attributes: ComWrapper::new(Attributes::default()),
        }
    }
}

impl Class for Message {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.lock().unwrap().as_ptr()
    }
    unsafe fn setMessageID(&self, id: FIDString) {
        if !id.is_null() {
            *self.id.lock().unwrap() = unsafe { CStr::from_ptr(id) }.to_owned();
        }
    }
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        // Handed out **borrowed** — the SDK's own `HostMessage` does the
        // same — because a plugin calls this once per attribute it sets and
        // never releases what it was given.
        self.attributes
            .as_com_ref::<IAttributeList>()
            .unwrap()
            .as_ptr()
    }
}

/// What the controller may say back to the host.
///
/// The same rule [`crate::plugin::FontelleShared`] draws: every request is
/// *recorded* and acted on between frames. A `performEdit` is a knob moved
/// in the plugin's own editor and goes onto the wire, which is the same
/// thing as CLAP's parameter gesture; a `restartComponent` is a flag.
struct Handler {
    values: Arc<ParamValues>,
    /// `(id, divisor)` per parameter: what a normalised value is multiplied
    /// by to land on the wire — one for a continuous parameter, the step
    /// count for a stepped one.
    scale: Arc<Vec<(u32, f64)>>,
    wants_restart: AtomicBool,
    wants_reread: AtomicBool,
}

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        let scale = self
            .scale
            .iter()
            .find(|(i, _)| *i == id)
            .map_or(1.0, |(_, s)| *s);
        let plain = if scale > 1.0 {
            (value * scale).round()
        } else {
            value
        };
        if self.values.set(id, plain) {
            kResultOk
        } else {
            kInvalidArgument
        }
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        let flags = flags as u32;
        if flags & (RestartFlags_::kParamValuesChanged as u32) != 0 {
            self.wants_reread.store(true, Ordering::Release);
        }
        if flags
            & (RestartFlags_::kLatencyChanged as u32
                | RestartFlags_::kReloadComponent as u32
                | RestartFlags_::kIoChanged as u32
                | RestartFlags_::kParamTitlesChanged as u32)
            != 0
        {
            self.wants_restart.store(true, Ordering::Release);
        }
        kResultOk
    }
}

/// The frame a plugin's view sits in, and the run loop behind it.
///
/// One object for both because a view finds the run loop by asking its
/// frame for the interface, which is how the SDK lays it out. The pump is
/// [`crate::gui::GuiPump`], the same one CLAP's timers and descriptors go
/// through; the handlers are kept beside it by the pump's ids.
struct Frame {
    gui_resize: AtomicU64,
    pump: Mutex<crate::gui::GuiPump>,
    timers: Mutex<Vec<(u32, ComPtr<ITimerHandler>)>>,
    fds: Mutex<Vec<(i32, ComPtr<IEventHandler>)>>,
}

impl Frame {
    fn new() -> Self {
        Self {
            gui_resize: AtomicU64::new(0),
            pump: Mutex::new(crate::gui::GuiPump::default()),
            timers: Mutex::new(Vec::new()),
            fds: Mutex::new(Vec::new()),
        }
    }

    /// Drives every timer and descriptor that is due. Handlers are called
    /// **with no lock held**, because a handler is entitled to register or
    /// unregister another from inside its callback.
    fn tick(&self) {
        let due = self
            .pump
            .lock()
            .unwrap()
            .due_timers(std::time::Instant::now());
        let handlers: Vec<ComPtr<ITimerHandler>> = {
            let timers = self.timers.lock().unwrap();
            due.iter()
                .filter_map(|id| timers.iter().find(|(i, _)| i == id).map(|(_, h)| h.clone()))
                .collect()
        };
        for handler in handlers {
            unsafe { handler.onTimer() };
        }
        #[cfg(unix)]
        {
            let ready = self.pump.lock().unwrap().ready_fds();
            let handlers: Vec<(i32, ComPtr<IEventHandler>)> = {
                let fds = self.fds.lock().unwrap();
                ready
                    .iter()
                    .filter_map(|fd| fds.iter().find(|(f, _)| f == fd).cloned())
                    .collect()
            };
            for (fd, handler) in handlers {
                unsafe { handler.onFDIsSet(fd) };
            }
        }
    }

    fn clear(&self) {
        self.timers.lock().unwrap().clear();
        self.fds.lock().unwrap().clear();
        *self.pump.lock().unwrap() = crate::gui::GuiPump::default();
    }
}

impl Class for Frame {
    type Interfaces = (IPlugFrame, IRunLoop);
}

impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, _view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        let rect = unsafe { *new_size };
        let width = (rect.right - rect.left).max(0) as u64;
        let height = (rect.bottom - rect.top).max(0) as u64;
        // Packed so the pair cannot be read half-updated — see
        // `FontelleShared::request_resize`.
        self.gui_resize
            .store((width << 32) | height, Ordering::Release);
        kResultOk
    }
}

impl IRunLoopTrait for Frame {
    unsafe fn registerEventHandler(&self, handler: *mut IEventHandler, fd: i32) -> tresult {
        let Some(handler) = (unsafe { ComRef::from_raw(handler) }) else {
            return kInvalidArgument;
        };
        #[cfg(unix)]
        self.pump.lock().unwrap().register_fd(fd);
        self.fds.lock().unwrap().push((fd, handler.to_com_ptr()));
        kResultOk
    }
    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> tresult {
        let mut fds = self.fds.lock().unwrap();
        let before = fds.len();
        fds.retain(|(fd, held)| {
            let keep = held.as_ptr() != handler;
            #[cfg(unix)]
            if !keep {
                self.pump.lock().unwrap().unregister_fd(*fd);
            }
            #[cfg(not(unix))]
            let _ = fd;
            keep
        });
        if fds.len() != before {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn registerTimer(&self, handler: *mut ITimerHandler, milliseconds: u64) -> tresult {
        let Some(handler) = (unsafe { ComRef::from_raw(handler) }) else {
            return kInvalidArgument;
        };
        let id = self
            .pump
            .lock()
            .unwrap()
            .register_timer(milliseconds.min(u64::from(u32::MAX)) as u32);
        self.timers.lock().unwrap().push((id, handler.to_com_ptr()));
        kResultOk
    }
    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> tresult {
        let mut timers = self.timers.lock().unwrap();
        let before = timers.len();
        timers.retain(|(id, held)| {
            let keep = held.as_ptr() != handler;
            if !keep {
                self.pump.lock().unwrap().unregister_timer(*id);
            }
            keep
        });
        if timers.len() != before {
            kResultOk
        } else {
            kResultFalse
        }
    }
}

/// A byte stream in memory, for state both ways.
struct Stream {
    bytes: RefCell<Vec<u8>>,
    position: Cell<usize>,
}

impl Stream {
    fn new(bytes: Vec<u8>) -> ComWrapper<Self> {
        ComWrapper::new(Self {
            bytes: RefCell::new(bytes),
            position: Cell::new(0),
        })
    }
}

impl Class for Stream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for Stream {
    unsafe fn read(&self, buffer: *mut c_void, num_bytes: int32, num_read: *mut int32) -> tresult {
        let bytes = self.bytes.borrow();
        let at = self.position.get();
        let n = (num_bytes.max(0) as usize).min(bytes.len().saturating_sub(at));
        if n > 0 {
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr().add(at), buffer as *mut u8, n) };
        }
        self.position.set(at + n);
        if !num_read.is_null() {
            unsafe { *num_read = n as i32 };
        }
        kResultOk
    }
    unsafe fn write(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_written: *mut int32,
    ) -> tresult {
        let source =
            unsafe { std::slice::from_raw_parts(buffer as *const u8, num_bytes.max(0) as usize) };
        let mut bytes = self.bytes.borrow_mut();
        let at = self.position.get();
        if at + source.len() > bytes.len() {
            bytes.resize(at + source.len(), 0);
        }
        bytes[at..at + source.len()].copy_from_slice(source);
        self.position.set(at + source.len());
        if !num_written.is_null() {
            unsafe { *num_written = source.len() as i32 };
        }
        kResultOk
    }
    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let len = self.bytes.borrow().len() as i64;
        let current = self.position.get() as i64;
        let next = match mode as IBStream_::IStreamSeekMode {
            IBStream_::IStreamSeekMode_::kIBSeekSet => pos,
            IBStream_::IStreamSeekMode_::kIBSeekCur => current + pos,
            _ => len + pos,
        }
        .clamp(0, len);
        self.position.set(next as usize);
        if !result.is_null() {
            unsafe { *result = next };
        }
        kResultOk
    }
    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if pos.is_null() {
            return kInvalidArgument;
        }
        unsafe { *pos = self.position.get() as i64 };
        kResultOk
    }
}

/// The events of one block, sized once.
struct EventList {
    events: RefCell<Vec<Event>>,
}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.events.borrow().len() as i32
    }
    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        match self.events.borrow().get(index.max(0) as usize) {
            Some(event) => {
                unsafe { *e = *event };
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        let mut events = self.events.borrow_mut();
        if events.len() >= events.capacity() {
            return kResultFalse;
        }
        events.push(unsafe { *e });
        kResultOk
    }
}

/// One parameter's points in a block.
struct ValueQueue {
    id: Cell<ParamID>,
    points: RefCell<Vec<(i32, f64)>>,
}

impl Class for ValueQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ValueQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }
    unsafe fn getPointCount(&self) -> int32 {
        self.points.borrow().len() as i32
    }
    unsafe fn getPoint(
        &self,
        index: int32,
        sample_offset: *mut int32,
        value: *mut ParamValue,
    ) -> tresult {
        match self.points.borrow().get(index.max(0) as usize) {
            Some((offset, v)) => {
                unsafe {
                    *sample_offset = *offset;
                    *value = *v;
                }
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn addPoint(
        &self,
        sample_offset: int32,
        value: ParamValue,
        index: *mut int32,
    ) -> tresult {
        let mut points = self.points.borrow_mut();
        if points.len() >= points.capacity() {
            return kResultFalse;
        }
        points.push((sample_offset, value));
        if !index.is_null() {
            unsafe { *index = points.len() as i32 - 1 };
        }
        kResultOk
    }
}

/// The parameter changes of one block: a queue per parameter that moved,
/// out of a pool sized to the plugin's parameter count at activation.
struct ParameterChanges {
    pool: Vec<ComWrapper<ValueQueue>>,
    used: Cell<usize>,
}

impl ParameterChanges {
    fn new(capacity: usize) -> ComWrapper<Self> {
        ComWrapper::new(Self {
            pool: (0..capacity)
                .map(|_| {
                    ComWrapper::new(ValueQueue {
                        id: Cell::new(0),
                        points: RefCell::new(Vec::with_capacity(MAX_POINTS)),
                    })
                })
                .collect(),
            used: Cell::new(0),
        })
    }

    /// The queue for `id`, opened if it is not yet in use this block.
    fn queue_for(&self, id: ParamID) -> Option<&ComWrapper<ValueQueue>> {
        let used = self.used.get();
        if let Some(queue) = self.pool[..used].iter().find(|q| q.id.get() == id) {
            return Some(queue);
        }
        let queue = self.pool.get(used)?;
        queue.id.set(id);
        queue.points.borrow_mut().clear();
        self.used.set(used + 1);
        Some(queue)
    }

    fn add(&self, id: ParamID, offset: i32, value: f64) {
        if let Some(queue) = self.queue_for(id) {
            let mut points = queue.points.borrow_mut();
            if points.len() < points.capacity() {
                points.push((offset, value));
            }
        }
    }

    fn clear(&self) {
        self.used.set(0);
    }
}

impl Class for ParameterChanges {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for ParameterChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.used.get() as i32
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        let index = index.max(0) as usize;
        if index >= self.used.get() {
            return std::ptr::null_mut();
        }
        self.pool[index]
            .as_com_ref::<IParamValueQueue>()
            .unwrap()
            .as_ptr()
    }
    unsafe fn addParameterData(
        &self,
        id: *const ParamID,
        index: *mut int32,
    ) -> *mut IParamValueQueue {
        if id.is_null() {
            return std::ptr::null_mut();
        }
        let id = unsafe { *id };
        let Some(queue) = self.queue_for(id) else {
            return std::ptr::null_mut();
        };
        if !index.is_null() {
            let position = self.pool[..self.used.get()]
                .iter()
                .position(|q| q.id.get() == id)
                .unwrap_or(0);
            unsafe { *index = position as i32 };
        }
        queue.as_com_ref::<IParamValueQueue>().unwrap().as_ptr()
    }
}

// --------------------------------------------------------------- opening

/// What both halves of a VST 3 plugin hold: the module, the component and
/// its processor interface. The component is main-thread for everything
/// but `process`, which the specification puts on the audio thread; the
/// plugin is what synchronises the two, as CLAP's is.
pub(crate) struct Shared {
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    /// Last, so it is dropped last: the two above point into it. Never
    /// read — it is here to be held.
    _module: Arc<Module>,
}

/// Which parameters the plugin maps the three channel-wide controllers to.
#[derive(Debug, Clone, Copy, Default)]
struct MidiMap {
    wheel: Option<ParamID>,
    bend: Option<ParamID>,
    pressure: Option<ParamID>,
}

/// One bus, as declared.
#[derive(Debug, Clone)]
struct Bus {
    channels: u32,
    main: bool,
}

/// The main-thread half.
pub(crate) struct Vst3Plugin {
    controller: ComPtr<IEditController>,
    /// Whether the controller is the component itself: it is then not
    /// terminated twice, and the connection points are not connected.
    combined: bool,
    #[allow(dead_code)]
    host: ComWrapper<HostApplication>,
    handler: ComWrapper<Handler>,
    frame: ComWrapper<Frame>,
    view: Option<ComPtr<IPlugView>>,
    has_editor: Option<bool>,
    midi_map: MidiMap,
    audio_in: Vec<Bus>,
    audio_out: Vec<Bus>,
    event_in: u32,
    event_out: u32,
    scale: Arc<Vec<(u32, f64)>>,
    ids: Vec<u32>,
    /// Which parameters the document has set since the component last
    /// heard them — see [`Vst3Plugin::activate`].
    dirty: Vec<u32>,
    active: bool,
    /// Last, so it is dropped last: everything above points into the
    /// module it keeps loaded.
    shared: Arc<Shared>,
}

pub(crate) struct Opened {
    pub info: PluginInfo,
    pub params: Vec<HostedParam>,
    pub input_ports: PortLayout,
    pub output_ports: PortLayout,
    pub accepts_notes: bool,
    pub latency: u32,
    pub values: Arc<ParamValues>,
    pub plugin: Vst3Plugin,
}

fn instantiate(key: &PluginKey, why: &str) -> HostError {
    HostError::Instantiate {
        key: key.clone(),
        why: why.to_string(),
    }
}

pub(crate) fn open(
    module: &Arc<Module>,
    path: &Path,
    key: &PluginKey,
) -> Result<Opened, HostError> {
    let classes = module.classes();
    let wanted = key.id.to_ascii_uppercase();
    let other = other_spelling(&wanted);
    let class = classes
        .iter()
        .filter(|class| class.category == "Audio Module Class")
        .find(|class| {
            let spelled = cid_string(class.cid);
            spelled == wanted || other.as_deref() == Some(spelled.as_str())
        })
        .ok_or_else(|| HostError::NoSuchPlugin {
            key: key.clone(),
            path: path.to_path_buf(),
        })?;
    let info = PluginInfo {
        key: key.clone(),
        path: path.to_path_buf(),
        name: class.name.clone(),
        vendor: class.vendor.clone(),
        version: class.version.clone(),
        features: features_of(&class.sub_categories),
    };

    let host = ComWrapper::new(HostApplication);
    let host_ptr = host.to_com_ptr::<IHostApplication>().unwrap();
    let host_unknown = host_ptr.as_ptr() as *mut FUnknown;

    let component = module
        .create::<IComponent>(&class.cid)
        .ok_or_else(|| instantiate(key, "the factory would not make the component"))?;
    // SAFETY: every call below is the SDK's contract on a live object.
    if unsafe { component.initialize(host_unknown) } != kResultOk {
        return Err(instantiate(key, "the component would not initialize"));
    }
    let processor = component.cast::<IAudioProcessor>().ok_or_else(|| {
        unsafe { component.terminate() };
        instantiate(key, "the component is not an audio processor")
    })?;

    // The controller: the same object, or the class the component names.
    let (controller, combined) = match component.cast::<IEditController>() {
        Some(controller) => (controller, true),
        None => {
            let mut cid: TUID = [0; 16];
            if unsafe { component.getControllerClassId(&mut cid) } != kResultOk {
                unsafe { component.terminate() };
                return Err(instantiate(key, "the component names no controller"));
            }
            let controller = module.create::<IEditController>(&cid).ok_or_else(|| {
                unsafe { component.terminate() };
                instantiate(key, "the factory would not make the controller")
            })?;
            if unsafe { controller.initialize(host_unknown) } != kResultOk {
                unsafe { component.terminate() };
                return Err(instantiate(key, "the controller would not initialize"));
            }
            // The two halves talk through their connection points, when
            // they have them, in messages the host application makes.
            if let (Some(a), Some(b)) = (
                component.cast::<IConnectionPoint>(),
                controller.cast::<IConnectionPoint>(),
            ) {
                unsafe {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                }
            }
            // The controller starts from the component's state, so its
            // parameters read what the component will do.
            let stream = Stream::new(Vec::new());
            let stream_ptr = stream.to_com_ptr::<IBStream>().unwrap();
            if unsafe { component.getState(stream_ptr.as_ptr()) } == kResultOk {
                stream.position.set(0);
                unsafe { controller.setComponentState(stream_ptr.as_ptr()) };
            }
            (controller, false)
        }
    };

    // Buses: everything declared, main and aux alike.
    let buses = |media: MediaType, dir: BusDirection| -> Vec<Bus> {
        let count = unsafe { component.getBusCount(media, dir) };
        (0..count)
            .map(|index| {
                let mut info: BusInfo = unsafe { std::mem::zeroed() };
                if unsafe { component.getBusInfo(media, dir, index, &mut info) } == kResultOk {
                    Bus {
                        channels: info.channelCount.max(0) as u32,
                        main: info.busType == BusTypes_::kMain as BusType,
                    }
                } else {
                    Bus {
                        channels: 0,
                        main: false,
                    }
                }
            })
            .collect()
    };
    let audio_in = buses(K_AUDIO, K_INPUT);
    let audio_out = buses(K_AUDIO, K_OUTPUT);
    let event_in = unsafe { component.getBusCount(K_EVENT, K_INPUT) }.max(0) as u32;
    let event_out = unsafe { component.getBusCount(K_EVENT, K_OUTPUT) }.max(0) as u32;
    let layout = |buses: &[Bus]| PortLayout {
        channels: buses.iter().map(|bus| bus.channels).collect(),
        main: buses.iter().position(|bus| bus.main).unwrap_or(0),
    };
    let input_ports = layout(&audio_in);
    let output_ports = layout(&audio_out);

    // Parameters, off the controller, with the module names its units give.
    let units = unit_names(&controller);
    let count = unsafe { controller.getParameterCount() }.max(0);
    let mut params = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut info: ParameterInfo = unsafe { std::mem::zeroed() };
        if unsafe { controller.getParameterInfo(index, &mut info) } != kResultOk {
            continue;
        }
        let steps = info.stepCount.max(0) as f64;
        let stepped = info.stepCount > 0;
        // The current value, not the description's default — the lesson
        // Surge XT's CLAP build taught `read_params`, applied here too.
        let now = unsafe { controller.getParamNormalized(info.id) };
        let now = if now.is_finite() {
            now.clamp(0.0, 1.0)
        } else {
            info.defaultNormalizedValue.clamp(0.0, 1.0)
        };
        let flags = info.flags as u32;
        params.push(HostedParam {
            id: info.id,
            name: wide_string(&info.title),
            module: units.get(&info.unitId).cloned().unwrap_or_default(),
            min: 0.0,
            max: if stepped { steps } else { 1.0 },
            default: if stepped { (now * steps).round() } else { now },
            stepped,
            hidden: flags & (ParameterInfo_::ParameterFlags_::kIsHidden as u32) != 0,
            readonly: flags & (ParameterInfo_::ParameterFlags_::kIsReadOnly as u32) != 0,
        });
    }
    let scale: Arc<Vec<(u32, f64)>> = Arc::new(
        params
            .iter()
            .map(|param| (param.id, if param.stepped { param.max } else { 1.0 }))
            .collect(),
    );
    let ids = params.iter().map(|param| param.id).collect();
    let values = Arc::new(ParamValues::new(&params));

    let handler = ComWrapper::new(Handler {
        values: Arc::clone(&values),
        scale: Arc::clone(&scale),
        wants_restart: AtomicBool::new(false),
        wants_reread: AtomicBool::new(false),
    });
    unsafe {
        controller.setComponentHandler(handler.to_com_ptr::<IComponentHandler>().unwrap().as_ptr())
    };

    // The wheels: whichever parameters the plugin maps them to.
    let midi_map = controller
        .cast::<IMidiMapping>()
        .map(|mapping| {
            let lookup = |controller: CtrlNumber| -> Option<ParamID> {
                let mut id: ParamID = 0;
                let result =
                    unsafe { mapping.getMidiControllerAssignment(0, 0, controller, &mut id) };
                (result == kResultTrue).then_some(id)
            };
            MidiMap {
                wheel: lookup(ControllerNumbers_::kCtrlModWheel as CtrlNumber),
                bend: lookup(ControllerNumbers_::kPitchBend as CtrlNumber),
                pressure: lookup(ControllerNumbers_::kAfterTouch as CtrlNumber),
            }
        })
        .unwrap_or_default();

    // Set up once, so the latency can be read before there is a graph.
    let mut setup = ProcessSetup {
        processMode: ProcessModes_::kRealtime as i32,
        symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
        maxSamplesPerBlock: NOMINAL_BLOCK,
        sampleRate: NOMINAL_RATE,
    };
    unsafe { processor.setupProcessing(&mut setup) };
    let latency = unsafe { processor.getLatencySamples() };

    let shared = Arc::new(Shared {
        component,
        processor,
        _module: Arc::clone(module),
    });
    Ok(Opened {
        info,
        params,
        input_ports,
        output_ports,
        accepts_notes: event_in > 0,
        latency,
        values,
        plugin: Vst3Plugin {
            controller,
            combined,
            host,
            handler,
            frame: ComWrapper::new(Frame::new()),
            view: None,
            has_editor: None,
            midi_map,
            audio_in,
            audio_out,
            event_in,
            event_out,
            scale,
            ids,
            dirty: Vec::new(),
            active: false,
            shared,
        },
    })
}

/// Every unit's path, `Parent/Child`, by id — the grouping a parameter's
/// `unitId` names, when the controller describes its units at all.
fn unit_names(controller: &ComPtr<IEditController>) -> HashMap<UnitID, String> {
    let mut names = HashMap::new();
    let Some(units) = controller.cast::<IUnitInfo>() else {
        return names;
    };
    let count = unsafe { units.getUnitCount() }.max(0);
    let mut parents: HashMap<UnitID, (UnitID, String)> = HashMap::new();
    for index in 0..count {
        let mut info: UnitInfo = unsafe { std::mem::zeroed() };
        if unsafe { units.getUnitInfo(index, &mut info) } == kResultOk {
            parents.insert(info.id, (info.parentUnitId, wide_string(&info.name)));
        }
    }
    for id in parents.keys() {
        let mut path = Vec::new();
        let mut at = *id;
        let mut hops = 0;
        while let Some((parent, name)) = parents.get(&at) {
            if at == kRootUnitId || hops > 16 {
                break;
            }
            path.push(name.clone());
            at = *parent;
            hops += 1;
        }
        path.reverse();
        names.insert(*id, path.join("/"));
    }
    names
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        self.close_editor();
        // SAFETY: the SDK's teardown order — disconnect, terminate the
        // controller, then the component; the module outlives both.
        unsafe {
            if !self.combined {
                if let (Some(a), Some(b)) = (
                    self.shared.component.cast::<IConnectionPoint>(),
                    self.controller.cast::<IConnectionPoint>(),
                ) {
                    a.disconnect(b.as_ptr());
                    b.disconnect(a.as_ptr());
                }
                self.controller.setComponentHandler(std::ptr::null_mut());
                self.controller.terminate();
            }
            if self.active {
                self.shared.processor.setProcessing(0);
                self.shared.component.setActive(0);
            }
            self.shared.component.terminate();
        }
    }
}

impl Vst3Plugin {
    fn scale_of(&self, id: u32) -> f64 {
        self.scale
            .iter()
            .find(|(i, _)| *i == id)
            .map_or(1.0, |(_, s)| *s)
    }

    /// Tells the controller a value now — so its editor follows the knob —
    /// whether or not the plugin is running. A running plugin's processor
    /// hears it through the wire on its next block.
    pub(crate) fn set_param(&mut self, id: u32, value: f64) {
        let normalised = (value / self.scale_of(id)).clamp(0.0, 1.0);
        unsafe { self.controller.setParamNormalized(id, normalised) };
        if !self.active && !self.dirty.contains(&id) {
            self.dirty.push(id);
        }
    }

    pub(crate) fn get_param(&self, id: u32) -> f64 {
        let normalised = unsafe { self.controller.getParamNormalized(id) };
        let scale = self.scale_of(id);
        if scale > 1.0 {
            (normalised * scale).round()
        } else {
            normalised
        }
    }

    pub(crate) fn display(&self, id: u32, value: f64) -> Option<String> {
        let normalised = (value / self.scale_of(id)).clamp(0.0, 1.0);
        let mut text: String128 = [0; 128];
        let result = unsafe {
            self.controller
                .getParamStringByValue(id, normalised, &mut text)
        };
        (result == kResultOk).then(|| wide_string(&text))
    }

    /// Both halves' state, framed so they can be told apart on the way back.
    pub(crate) fn save_state(&self) -> Option<Vec<u8>> {
        let component = {
            let stream = Stream::new(Vec::new());
            let ptr = stream.to_com_ptr::<IBStream>().unwrap();
            if unsafe { self.shared.component.getState(ptr.as_ptr()) } != kResultOk {
                return None;
            }
            stream.bytes.borrow().clone()
        };
        let controller = {
            let stream = Stream::new(Vec::new());
            let ptr = stream.to_com_ptr::<IBStream>().unwrap();
            unsafe { self.controller.getState(ptr.as_ptr()) };
            stream.bytes.borrow().clone()
        };
        let mut out = STATE_MAGIC.to_vec();
        out.extend_from_slice(&(component.len() as u32).to_le_bytes());
        out.extend_from_slice(&component);
        out.extend_from_slice(&(controller.len() as u32).to_le_bytes());
        out.extend_from_slice(&controller);
        Some(out)
    }

    pub(crate) fn load_state(&mut self, bytes: &[u8]) -> bool {
        let Some((component, controller)) = split_state(bytes) else {
            return false;
        };
        // The component is about to hear the whole of it directly.
        self.dirty.clear();
        let stream = Stream::new(component.to_vec());
        let ptr = stream.to_com_ptr::<IBStream>().unwrap();
        let loaded = unsafe { self.shared.component.setState(ptr.as_ptr()) } == kResultOk;
        stream.position.set(0);
        unsafe { self.controller.setComponentState(ptr.as_ptr()) };
        if !controller.is_empty() {
            let stream = Stream::new(controller.to_vec());
            let ptr = stream.to_com_ptr::<IBStream>().unwrap();
            unsafe { self.controller.setState(ptr.as_ptr()) };
        }
        loaded
    }

    /// Reads every parameter back off the controller onto the wire.
    pub(crate) fn reread_params(&self, values: &ParamValues) {
        for id in &self.ids {
            values.set(*id, self.get_param(*id));
        }
    }

    /// Whether the plugin has asked to be restarted since last asked.
    pub(crate) fn take_restart(&self) -> bool {
        self.handler.wants_restart.swap(false, Ordering::AcqRel)
    }

    /// The between-frames service: a parameter reload the plugin announced.
    pub(crate) fn service(&self, values: &ParamValues) {
        if self.handler.wants_reread.swap(false, Ordering::AcqRel) {
            self.reread_params(values);
        }
    }

    pub(crate) fn activate(
        &mut self,
        key: &PluginKey,
        values: Arc<ParamValues>,
        sample_rate: f64,
        max_block: usize,
    ) -> Result<(Vst3Processor, u32), HostError> {
        let component = &self.shared.component;
        let processor = &self.shared.processor;
        let refuse = |why: &str| HostError::Activate {
            key: key.clone(),
            why: why.to_string(),
        };
        // SAFETY: the SDK's activation sequence on a live, inactive component.
        unsafe {
            if processor.canProcessSampleSize(SymbolicSampleSizes_::kSample32 as i32) != kResultOk {
                return Err(refuse("the plugin processes only 64-bit samples"));
            }
            // Every bus, main and aux alike — the CLAP lesson.
            for (index, _) in self.audio_in.iter().enumerate() {
                component.activateBus(K_AUDIO, K_INPUT, index as i32, 1);
            }
            for (index, _) in self.audio_out.iter().enumerate() {
                component.activateBus(K_AUDIO, K_OUTPUT, index as i32, 1);
            }
            for index in 0..self.event_in {
                component.activateBus(K_EVENT, K_INPUT, index as i32, 1);
            }
            for index in 0..self.event_out {
                component.activateBus(K_EVENT, K_OUTPUT, index as i32, 1);
            }
            // The arrangements it reported, handed back to it: a plugin that
            // wants to be told is told what it said.
            let mut ins: Vec<SpeakerArrangement> = (0..self.audio_in.len())
                .map(|i| {
                    let mut arr: SpeakerArrangement = 0;
                    processor.getBusArrangement(K_INPUT, i as i32, &mut arr);
                    arr
                })
                .collect();
            let mut outs: Vec<SpeakerArrangement> = (0..self.audio_out.len())
                .map(|i| {
                    let mut arr: SpeakerArrangement = 0;
                    processor.getBusArrangement(K_OUTPUT, i as i32, &mut arr);
                    arr
                })
                .collect();
            processor.setBusArrangements(
                ins.as_mut_ptr(),
                ins.len() as i32,
                outs.as_mut_ptr(),
                outs.len() as i32,
            );
            let mut setup = ProcessSetup {
                processMode: ProcessModes_::kRealtime as i32,
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
                maxSamplesPerBlock: max_block as i32,
                sampleRate: sample_rate,
            };
            if processor.setupProcessing(&mut setup) != kResultOk {
                return Err(refuse("the plugin refused the sample rate or block size"));
            }
            if component.setActive(1) != kResultOk {
                return Err(refuse("the plugin would not activate"));
            }
            processor.setProcessing(1);
        }
        self.active = true;
        // **Only what the document changed** goes to the component on its
        // first block — not every parameter. The wire was seeded from the
        // controller at open, and sending the lot back is not a no-op:
        // Surge XT's normalised values do not all round-trip through its
        // own `setParamNormalized`, and a flood of 2,855 of them turned its
        // default patch down to a whisper (a peak of 0.01 against 0.36 for
        // the same note with nothing sent). What a knob or a restore set
        // since open is what the component has not heard, and that is
        // exactly what is marked; a state load clears the list because the
        // component read that itself.
        for id in self.dirty.drain(..) {
            if let Some(value) = values.get(id) {
                values.set(id, value);
            }
        }
        let latency = unsafe { processor.getLatencySamples() };
        let processor = Vst3Processor::new(
            Arc::clone(&self.shared),
            values,
            Arc::clone(&self.scale),
            self.midi_map,
            &self.audio_in,
            &self.audio_out,
            self.event_in > 0,
            sample_rate,
            max_block,
        );
        Ok((processor, latency))
    }

    pub(crate) fn deactivate(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        unsafe {
            self.shared.processor.setProcessing(0);
            self.shared.component.setActive(0);
        }
    }

    // ------------------------------------------------------------ editor

    fn platform_type() -> &'static CStr {
        if cfg!(target_os = "windows") {
            c"HWND"
        } else if cfg!(target_os = "macos") {
            c"NSView"
        } else {
            c"X11EmbedWindowID"
        }
    }

    fn create_view(&self) -> Option<ComPtr<IPlugView>> {
        let raw = unsafe { self.controller.createView(c"editor".as_ptr()) };
        unsafe { ComPtr::from_raw(raw) }
    }

    /// Whether the plugin offers a view, **and** one in the shape this host
    /// can embed. The only way to ask is to make one, so the answer is kept.
    pub(crate) fn has_editor(&mut self) -> bool {
        if let Some(answer) = self.has_editor {
            return answer;
        }
        let answer = match self.create_view() {
            Some(view) => {
                let supported =
                    unsafe { view.isPlatformTypeSupported(Self::platform_type().as_ptr()) };
                supported == kResultTrue
            }
            None => false,
        };
        self.has_editor = Some(answer);
        answer
    }

    pub(crate) fn open_editor(
        &mut self,
        window: &crate::gui::PluginWindow,
        scale: f64,
    ) -> Result<crate::gui::GuiSize, crate::gui::GuiError> {
        use crate::gui::GuiError;
        if !self.has_editor() {
            return Err(GuiError::NoEditor);
        }
        let view = self.create_view().ok_or(GuiError::Refused("create"))?;
        self.frame.clear();
        unsafe {
            view.setFrame(self.frame.to_com_ptr::<IPlugFrame>().unwrap().as_ptr());
            if let Some(scaling) = view.cast::<IPlugViewContentScaleSupport>() {
                scaling.setContentScaleFactor(scale as f32);
            }
            let parent = window.id() as usize as *mut c_void;
            if view.attached(parent, Self::platform_type().as_ptr()) != kResultOk {
                view.setFrame(std::ptr::null_mut());
                return Err(GuiError::Refused("attach"));
            }
        }
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let size = if unsafe { view.getSize(&mut rect) } == kResultOk {
            let width = (rect.right - rect.left).max(0) as u32;
            let height = (rect.bottom - rect.top).max(0) as u32;
            if width == 0 || height == 0 {
                crate::gui::GuiSize::FALLBACK
            } else {
                crate::gui::GuiSize { width, height }
            }
        } else {
            crate::gui::GuiSize::FALLBACK
        };
        self.view = Some(view);
        Ok(size)
    }

    pub(crate) fn close_editor(&mut self) {
        if let Some(view) = self.view.take() {
            unsafe {
                view.removed();
                view.setFrame(std::ptr::null_mut());
            }
        }
        self.frame.clear();
    }

    pub(crate) fn tick_editor(&self) {
        if self.view.is_some() {
            self.frame.tick();
        }
    }

    pub(crate) fn take_resize(&self) -> Option<crate::gui::GuiSize> {
        let packed = self.frame.gui_resize.swap(0, Ordering::AcqRel);
        (packed != 0).then_some(crate::gui::GuiSize {
            width: (packed >> 32) as u32,
            height: (packed & 0xffff_ffff) as u32,
        })
    }

    pub(crate) fn editor_resizable(&self) -> bool {
        self.view
            .as_ref()
            .is_some_and(|view| unsafe { view.canResize() } == kResultTrue)
    }

    pub(crate) fn resize_editor(&self, size: crate::gui::GuiSize) -> bool {
        let Some(view) = &self.view else {
            return false;
        };
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: size.width as i32,
            bottom: size.height as i32,
        };
        unsafe { view.onSize(&mut rect) == kResultOk }
    }
}

const STATE_MAGIC: &[u8; 4] = b"FV3S";

fn split_state(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = bytes.strip_prefix(STATE_MAGIC)?;
    let (len, rest) = rest.split_first_chunk::<4>()?;
    let len = u32::from_le_bytes(*len) as usize;
    let (component, rest) = rest.split_at_checked(len)?;
    let (len, rest) = rest.split_first_chunk::<4>()?;
    let len = u32::from_le_bytes(*len) as usize;
    let (controller, _) = rest.split_at_checked(len)?;
    Some((component, controller))
}

/// How the plugin's note port is spoken to — the wheels reach it through
/// its MIDI mapping, which is the nearest thing VST 3 has to a port that
/// takes MIDI.
pub(crate) fn note_dialect(accepts_notes: bool) -> Option<NoteDialect> {
    accepts_notes.then_some(NoteDialect::Midi)
}

// ------------------------------------------------------------ the audio half

/// The audio half: buffers for every bus, the event list and the parameter
/// changes of a block, all sized at activation.
pub(crate) struct Vst3Processor {
    shared: Arc<Shared>,
    values: Arc<ParamValues>,
    scale: Arc<Vec<(u32, f64)>>,
    midi_map: MidiMap,
    input: Vec<Vec<Vec<f32>>>,
    output: Vec<Vec<Vec<f32>>>,
    in_ptrs: Vec<Vec<*mut f32>>,
    out_ptrs: Vec<Vec<*mut f32>>,
    in_buses: Vec<AudioBusBuffers>,
    out_buses: Vec<AudioBusBuffers>,
    main_in: usize,
    main_out: usize,
    key_in: Option<usize>,
    events: ComWrapper<EventList>,
    replies: ComWrapper<EventList>,
    changes_in: ComWrapper<ParameterChanges>,
    changes_out: ComWrapper<ParameterChanges>,
    context: ProcessContext,
    has_notes: bool,
    sounding: [bool; 128],
    max_block: usize,
}

// SAFETY: the pointer tables point into `input`/`output`, which move with
// the struct and are rewritten before every use; the COM objects are used
// from this thread only while `run` executes, which is the specification's
// contract for `process`.
unsafe impl Send for Vst3Processor {}

impl Vst3Processor {
    #[allow(clippy::too_many_arguments)]
    fn new(
        shared: Arc<Shared>,
        values: Arc<ParamValues>,
        scale: Arc<Vec<(u32, f64)>>,
        midi_map: MidiMap,
        audio_in: &[Bus],
        audio_out: &[Bus],
        has_notes: bool,
        sample_rate: f64,
        max_block: usize,
    ) -> Self {
        let buffers = |buses: &[Bus]| -> Vec<Vec<Vec<f32>>> {
            buses
                .iter()
                .map(|bus| vec![vec![0.0f32; max_block]; bus.channels as usize])
                .collect()
        };
        let input = buffers(audio_in);
        let output = buffers(audio_out);
        let main_in = audio_in.iter().position(|bus| bus.main).unwrap_or(0);
        let main_out = audio_out.iter().position(|bus| bus.main).unwrap_or(0);
        let key_in = (0..audio_in.len()).find(|&index| index != main_in);
        let mut context: ProcessContext = unsafe { std::mem::zeroed() };
        context.sampleRate = sample_rate;
        context.state = ProcessContext_::StatesAndFlags_::kPlaying as u32
            | ProcessContext_::StatesAndFlags_::kContTimeValid as u32;
        Self {
            in_ptrs: input
                .iter()
                .map(|bus| vec![std::ptr::null_mut(); bus.len()])
                .collect(),
            out_ptrs: output
                .iter()
                .map(|bus| vec![std::ptr::null_mut(); bus.len()])
                .collect(),
            in_buses: (0..input.len())
                .map(|_| AudioBusBuffers {
                    numChannels: 0,
                    silenceFlags: 0,
                    __field0: AudioBusBuffers__type0 {
                        channelBuffers32: std::ptr::null_mut(),
                    },
                })
                .collect(),
            out_buses: (0..output.len())
                .map(|_| AudioBusBuffers {
                    numChannels: 0,
                    silenceFlags: 0,
                    __field0: AudioBusBuffers__type0 {
                        channelBuffers32: std::ptr::null_mut(),
                    },
                })
                .collect(),
            input,
            output,
            main_in,
            main_out,
            key_in,
            events: ComWrapper::new(EventList {
                events: RefCell::new(Vec::with_capacity(MAX_EVENTS)),
            }),
            replies: ComWrapper::new(EventList {
                events: RefCell::new(Vec::with_capacity(MAX_EVENTS)),
            }),
            changes_in: ParameterChanges::new(scale.len().max(1)),
            changes_out: ParameterChanges::new(scale.len().max(1)),
            context,
            has_notes,
            sounding: [false; 128],
            max_block,
            shared,
            values,
            scale,
            midi_map,
        }
    }

    pub(crate) fn max_block(&self) -> usize {
        self.max_block
    }

    /// The main input bus's channels, for the bus copy.
    pub(crate) fn main_input(&mut self) -> &mut [Vec<f32>] {
        match self.input.get_mut(self.main_in) {
            Some(bus) => bus,
            None => &mut [],
        }
    }

    pub(crate) fn main_output(&self) -> &[Vec<f32>] {
        self.output.get(self.main_out).map_or(&[], |bus| bus)
    }

    /// Fills the sidechain bus with `key`, or silence.
    pub(crate) fn fill_key(&mut self, key: Option<&[f32]>, frames: usize) {
        let Some(index) = self.key_in else {
            return;
        };
        for channel in &mut self.input[index] {
            let len = channel.len();
            match key {
                Some(key) => {
                    let n = frames.min(key.len()).min(len);
                    channel[..n].copy_from_slice(&key[..n]);
                    channel[n..frames.min(len)].fill(0.0);
                }
                None => channel[..frames.min(len)].fill(0.0),
            }
        }
    }

    fn push_event(&mut self, event: Event) {
        let mut events = self.events.events.borrow_mut();
        if events.len() < events.capacity() {
            events.push(event);
        }
    }

    fn event(frame: usize, kind: Event_::EventTypes, body: Event__type0) -> Event {
        Event {
            busIndex: 0,
            sampleOffset: frame as i32,
            ppqPosition: 0.0,
            flags: 0,
            r#type: kind as u16,
            __field0: body,
        }
    }

    pub(crate) fn note_on(&mut self, frame: usize, key: u8, velocity: f64) {
        if !self.has_notes {
            return;
        }
        self.sounding[usize::from(key.min(127))] = true;
        self.push_event(Self::event(
            frame,
            Event_::EventTypes_::kNoteOnEvent,
            Event__type0 {
                noteOn: NoteOnEvent {
                    channel: 0,
                    pitch: i16::from(key),
                    tuning: 0.0,
                    velocity: velocity as f32,
                    length: 0,
                    // The key doubles as the note id, so a slide can name
                    // the note it bends.
                    noteId: i32::from(key),
                },
            },
        ));
    }

    pub(crate) fn note_off(&mut self, frame: usize, key: u8) {
        if !self.has_notes {
            return;
        }
        self.sounding[usize::from(key.min(127))] = false;
        self.push_event(Self::event(
            frame,
            Event_::EventTypes_::kNoteOffEvent,
            Event__type0 {
                noteOff: NoteOffEvent {
                    channel: 0,
                    pitch: i16::from(key),
                    velocity: 0.0,
                    noteId: i32::from(key),
                    tuning: 0.0,
                },
            },
        ));
    }

    /// A slide: `kTuningTypeID` on the note, where 0.5 is untuned and the
    /// ends are ±120 semitones — the SDK's own convention for the type.
    pub(crate) fn note_tuning(&mut self, frame: usize, key: u8, semitones: f64) {
        if !self.has_notes {
            return;
        }
        self.push_event(Self::event(
            frame,
            Event_::EventTypes_::kNoteExpressionValueEvent,
            Event__type0 {
                noteExpressionValue: NoteExpressionValueEvent {
                    typeId: NoteExpressionTypeIDs_::kTuningTypeID as u32,
                    noteId: i32::from(key),
                    value: (0.5 + semitones / 240.0).clamp(0.0, 1.0),
                },
            },
        ));
    }

    fn mapped_change(&mut self, id: Option<ParamID>, frame: usize, normalised: f64) {
        if let Some(id) = id {
            self.changes_in.add(id, frame as i32, normalised);
        }
    }

    pub(crate) fn controller(&mut self, frame: usize, controller: u8, value: u8) {
        if controller == 1 {
            self.mapped_change(self.midi_map.wheel, frame, f64::from(value & 0x7F) / 127.0);
        }
    }

    pub(crate) fn pitch_bend(&mut self, frame: usize, value: i16) {
        let normalised = (f64::from(value) + 8192.0) / 16383.0;
        self.mapped_change(self.midi_map.bend, frame, normalised.clamp(0.0, 1.0));
    }

    pub(crate) fn channel_pressure(&mut self, frame: usize, value: u8) {
        self.mapped_change(
            self.midi_map.pressure,
            frame,
            f64::from(value & 0x7F) / 127.0,
        );
    }

    /// Everything sounding stops: VST 3 has no all-notes-off event, so it
    /// is a note-off for every key this processor turned on.
    pub(crate) fn reset(&mut self) {
        for key in 0..128u8 {
            if self.sounding[usize::from(key)] {
                self.note_off(0, key);
            }
        }
    }

    /// **RT.** One block. Whatever moved on the wire goes at the top of the
    /// block, before the notes, which is the order the list has to be in.
    pub(crate) fn run(&mut self, frames: usize, with_input: bool) {
        let frames = frames.min(self.max_block);
        // The wire, normalised: a stepped parameter's position over its
        // step count, a continuous one as it is.
        let scale = &self.scale;
        let changes = &self.changes_in;
        self.values.drain(|id, value| {
            let divisor = scale
                .iter()
                .find(|(i, _)| *i == id)
                .map_or(1.0, |(_, s)| *s);
            let normalised = if divisor > 1.0 {
                value / divisor
            } else {
                value
            };
            // Ahead of anything a wheel put at a later frame: a value at
            // the top of the block is a point at offset zero, and
            // `queue_for` keeps the queue's points in the order they were
            // added — so the wire's point is inserted first.
            changes.add_first(id, normalised.clamp(0.0, 1.0));
        });
        if !with_input {
            for bus in &mut self.input {
                for channel in bus {
                    channel[..frames].fill(0.0);
                }
            }
        }
        for (pointers, bus) in self.in_ptrs.iter_mut().zip(&mut self.input) {
            for (pointer, channel) in pointers.iter_mut().zip(bus) {
                *pointer = channel.as_mut_ptr();
            }
        }
        for (pointers, bus) in self.out_ptrs.iter_mut().zip(&mut self.output) {
            for (pointer, channel) in pointers.iter_mut().zip(bus) {
                *pointer = channel.as_mut_ptr();
            }
        }
        for (buffers, pointers) in self.in_buses.iter_mut().zip(&mut self.in_ptrs) {
            buffers.numChannels = pointers.len() as i32;
            buffers.silenceFlags = 0;
            buffers.__field0.channelBuffers32 = pointers.as_mut_ptr();
        }
        for (buffers, pointers) in self.out_buses.iter_mut().zip(&mut self.out_ptrs) {
            buffers.numChannels = pointers.len() as i32;
            buffers.silenceFlags = 0;
            buffers.__field0.channelBuffers32 = pointers.as_mut_ptr();
        }
        self.replies.events.borrow_mut().clear();
        self.changes_out.clear();
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as i32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            numSamples: frames as i32,
            numInputs: self.in_buses.len() as i32,
            numOutputs: self.out_buses.len() as i32,
            inputs: self.in_buses.as_mut_ptr(),
            outputs: self.out_buses.as_mut_ptr(),
            inputParameterChanges: self
                .changes_in
                .as_com_ref::<IParameterChanges>()
                .unwrap()
                .as_ptr(),
            outputParameterChanges: self
                .changes_out
                .as_com_ref::<IParameterChanges>()
                .unwrap()
                .as_ptr(),
            inputEvents: self.events.as_com_ref::<IEventList>().unwrap().as_ptr(),
            outputEvents: self.replies.as_com_ref::<IEventList>().unwrap().as_ptr(),
            processContext: &mut self.context,
        };
        // SAFETY: every pointer in `data` is into this struct and outlives
        // the call; the counts are exactly the buses the plugin declared.
        unsafe { self.shared.processor.process(&mut data) };
        self.context.continousTimeSamples += frames as i64;
        self.context.projectTimeSamples += frames as i64;
        self.events.events.borrow_mut().clear();
        self.changes_in.clear();
    }
}

impl ParameterChanges {
    /// A point at offset zero, ahead of any the block already holds for
    /// the same parameter.
    fn add_first(&self, id: ParamID, value: f64) {
        if let Some(queue) = self.queue_for(id) {
            let mut points = queue.points.borrow_mut();
            if points.len() < points.capacity() {
                points.insert(0, (0, value));
            } else if let Some(first) = points.first_mut() {
                *first = (0, value);
            }
        }
    }
}
