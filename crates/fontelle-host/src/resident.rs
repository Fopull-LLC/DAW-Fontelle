//! Plugin libraries, once loaded, stay loaded for the life of the process.
//!
//! A host used to unload a library when the last thing it opened from it
//! was closed — a render's rack, a scan, a test — and a library is not
//! always finished with when its plugins are. Some set up something shared
//! and process-wide on the way in: ZamHeadX2's entry sets up FFTW, FFTW kept
//! a pointer into a library that had been unloaded, and the next plugin to
//! use FFTW took the studio down (`tests/scan_isolation.rs`). Every host
//! that has met this keeps them; so does this one now.
//!
//! One of each is kept, which is enough: the operating system counts the
//! loads, and a library is unloaded only when the count reaches nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clack_host::prelude::PluginEntry;

static CLAP: Mutex<Vec<(PathBuf, PluginEntry)>> = Mutex::new(Vec::new());
static VST3: Mutex<Vec<(PathBuf, Arc<crate::vst3::Module>)>> = Mutex::new(Vec::new());
static LV2: Mutex<Option<HashMap<PathBuf, Vec<ParkedWorld>>>> = Mutex::new(None);

/// A lilv world nobody is using, kept so its libraries stay loaded.
struct ParkedWorld(crate::lv2::World);

// SAFETY: a parked world is never used again by anybody — it is only kept
// alive — so moving it to whichever thread drops the last host is sound.
unsafe impl Send for ParkedWorld {}

/// Keeps `entry`'s library loaded, if one from `path` is not already kept.
pub(crate) fn keep_clap(path: &Path, entry: &PluginEntry) {
    let mut kept = CLAP.lock().unwrap_or_else(|e| e.into_inner());
    if !kept.iter().any(|(held, _)| held == path) {
        kept.push((path.to_path_buf(), entry.clone()));
    }
}

/// Keeps `module` loaded, if one from `path` is not already kept.
pub(crate) fn keep_vst3(path: &Path, module: &Arc<crate::vst3::Module>) {
    let mut kept = VST3.lock().unwrap_or_else(|e| e.into_inner());
    if !kept.iter().any(|(held, _)| held == path) {
        kept.push((path.to_path_buf(), Arc::clone(module)));
    }
}

/// Takes the worlds of a host that is going, to be handed to the next host
/// that needs one: **a lilv world is never freed**. Freeing one tore down
/// state some plugins still pointed into (DrumSynth died in `sord_free`),
/// and a world kept to be used again costs nothing a new one would not.
pub(crate) fn park_worlds(worlds: HashMap<PathBuf, crate::lv2::World>) {
    let mut parked = LV2.lock().unwrap_or_else(|e| e.into_inner());
    let parked = parked.get_or_insert_with(HashMap::new);
    for (path, world) in worlds {
        parked.entry(path).or_default().push(ParkedWorld(world));
    }
}

/// A parked world for `path`, if there is one — before a new one is made.
pub(crate) fn take_world(path: &Path) -> Option<crate::lv2::World> {
    let mut parked = LV2.lock().unwrap_or_else(|e| e.into_inner());
    parked
        .as_mut()?
        .get_mut(path)?
        .pop()
        .map(|ParkedWorld(world)| world)
}
