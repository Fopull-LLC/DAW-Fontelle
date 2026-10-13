//! The Fontelle Singer's DiffSinger runtime: community DiffSinger
//! voicebanks in OpenUtau's layout, sung through ONNX Runtime behind
//! [`fontelle_sing::Backend`].
//!
//! Written from the published format: DiffSinger's own deployment
//! exporters and docs (openvpi/DiffSinger, Apache-2.0) for the models'
//! inputs and outputs, and OpenUtau's voicebank layout (stakira/OpenUtau,
//! MIT) for where a bank keeps its files. No code is shared with any other
//! engine.
//!
//! ```no_run
//! use fontelle_diffsinger::{DiffSinger, Options};
//! use fontelle_sing::Backend;
//! let singer = DiffSinger::new(Options::new(vec!["/path/to/voicebanks".into()]));
//! for voice in singer.voices() {
//!     println!("{} ({})", voice.name, voice.languages.join(", "));
//! }
//! ```
//!
//! # What a bank answers to
//!
//! [`Capabilities`] are worked out per bank from its configs: pitch bend and
//! dynamics always (the f0 and the gain are this runtime's own); breathiness,
//! voicing and tension when the acoustic model takes them and the variance
//! model predicts them (the lanes move the prediction); gender when the
//! acoustic model takes a key shift; vibrato, portamento, velocity and
//! hand-placed phoneme timing always. Extra lanes: energy, pitch
//! expressiveness and a mix lane per speaker, when the bank has them. A
//! speaker is also a per-note option.
//!
//! # Determinism
//!
//! The same phrase, quality and seed sing the same samples: the noise the
//! models start from is drawn here from the seed (see [`onnx`]).

pub mod bank;
pub mod config;
pub mod curves;
pub mod dict;
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub mod glibc_compat;
pub mod inventory;
pub mod lyrics;
pub mod models;
pub mod noise;
pub mod onnx;
pub mod phonemize;
pub mod render;
pub mod timing;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use fontelle_sing::{Backend, Capabilities, Phrase, Quality, Session, SingError, Sung, VoiceInfo};

use crate::bank::{Bank, find_banks};
use crate::models::RuntimeOptions;
use crate::render::Loaded;

/// Bumped whenever a change here alters what is sung; part of
/// [`Backend::version`] and so of every render key.
pub const RUNTIME_REVISION: u32 = 1;

/// Where voicebanks and their dependencies are, and how models run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Folders holding voicebank folders (searched three levels down).
    pub voicebank_dirs: Vec<PathBuf>,
    /// Folders holding installed dependencies (`.oudep` contents, one
    /// folder each). Each voicebank folder's `Dependencies` subfolder and
    /// its parent's are searched too.
    pub dependency_dirs: Vec<PathBuf>,
    pub runtime: RuntimeOptions,
}

impl Options {
    pub fn new(voicebank_dirs: Vec<PathBuf>) -> Self {
        Self {
            voicebank_dirs,
            ..Self::default()
        }
    }

    fn all_dependency_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = self.dependency_dirs.clone();
        for d in &self.voicebank_dirs {
            dirs.push(d.join("Dependencies"));
            if let Some(parent) = d.parent() {
                dirs.push(parent.join("Dependencies"));
            }
        }
        dirs
    }
}

struct Entry {
    bank: Bank,
    models: Mutex<Option<Arc<Loaded>>>,
}

impl Entry {
    fn models(&self, options: &RuntimeOptions) -> Result<Arc<Loaded>, String> {
        let mut slot = self.models.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(m) = &*slot {
            return Ok(m.clone());
        }
        let loaded = Arc::new(Loaded::load(&self.bank, options)?);
        *slot = Some(loaded.clone());
        Ok(loaded)
    }
}

struct Inner {
    options: Options,
    banks: RwLock<Vec<Arc<Entry>>>,
    /// Banks that did not load, and why.
    failed: RwLock<Vec<(PathBuf, String)>>,
}

impl Inner {
    fn entry(&self, voice: &str) -> Option<Arc<Entry>> {
        self.banks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|e| e.bank.id == voice)
            .cloned()
    }
}

/// The DiffSinger backend. Cheap to clone; clones share their banks and
/// opened models.
#[derive(Clone)]
pub struct DiffSinger {
    inner: Arc<Inner>,
}

impl DiffSinger {
    /// Finds the voicebanks; no model is opened until a bank first sings.
    pub fn new(options: Options) -> Self {
        let s = Self {
            inner: Arc::new(Inner {
                options,
                banks: RwLock::new(Vec::new()),
                failed: RwLock::new(Vec::new()),
            }),
        };
        s.rescan();
        s
    }

    /// Looks for voicebanks again (after an install). Banks already known
    /// keep their opened models.
    pub fn rescan(&self) {
        let options = &self.inner.options;
        let deps = options.all_dependency_dirs();
        let old = self
            .inner
            .banks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let mut banks: Vec<Arc<Entry>> = Vec::new();
        let mut failed = Vec::new();
        for dir in find_banks(&options.voicebank_dirs) {
            if let Some(e) = old.iter().find(|e| e.bank.dir == dir) {
                banks.push(e.clone());
                continue;
            }
            match Bank::load(&dir, &deps) {
                // The first of two banks with one folder name wins.
                Ok(bank) if banks.iter().any(|e| e.bank.id == bank.id) => failed.push((
                    dir,
                    format!("another voicebank is already called \"{}\"", bank.id),
                )),
                Ok(bank) => banks.push(Arc::new(Entry {
                    bank,
                    models: Mutex::new(None),
                })),
                Err(e) => failed.push((dir, e)),
            }
        }
        *self.inner.banks.write().unwrap_or_else(|e| e.into_inner()) = banks;
        *self.inner.failed.write().unwrap_or_else(|e| e.into_inner()) = failed;
    }

    /// Folders that look like voicebanks but did not load, and why.
    pub fn failed(&self) -> Vec<(PathBuf, String)> {
        self.inner
            .failed
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// What is wrong with a bank short of it not loading (no vocoder, a
    /// model folder that is not used), for the picker.
    pub fn problems(&self, voice: &str) -> Vec<String> {
        let Some(e) = self.inner.entry(voice) else {
            return Vec::new();
        };
        let mut p = e.bank.problems.clone();
        if let Err(why) = &e.bank.vocoder {
            p.insert(0, why.clone());
        }
        p
    }

    /// The bank's folder.
    pub fn bank_dir(&self, voice: &str) -> Option<PathBuf> {
        self.inner.entry(voice).map(|e| e.bank.dir.clone())
    }

    /// Opens a bank's models now rather than on its first phrase.
    pub fn preload(&self, voice: &str) -> Result<(), String> {
        let e = self
            .inner
            .entry(voice)
            .ok_or_else(|| format!("no DiffSinger voicebank \"{voice}\""))?;
        e.models(&self.inner.options.runtime).map(|_| ())
    }

    /// Closes a bank's models (they reopen when it next sings).
    pub fn unload(&self, voice: &str) {
        if let Some(e) = self.inner.entry(voice) {
            *e.models.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }
}

impl Backend for DiffSinger {
    fn name(&self) -> &str {
        "DiffSinger"
    }

    fn version(&self) -> String {
        format!(
            "fontelle-diffsinger {} r{RUNTIME_REVISION}",
            env!("CARGO_PKG_VERSION")
        )
    }

    fn voices(&self) -> Vec<VoiceInfo> {
        self.inner
            .banks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|e| e.bank.voice_info())
            .collect()
    }

    fn capabilities(&self, voice: &str) -> Capabilities {
        self.inner
            .entry(voice)
            .map(|e| e.bank.capabilities())
            .unwrap_or_default()
    }

    fn voice_files(&self, voice: &str) -> Vec<PathBuf> {
        let Some(e) = self.inner.entry(voice) else {
            return Vec::new();
        };
        let mut files = vec![e.bank.dir.clone()];
        if let Ok(v) = &e.bank.vocoder {
            files.push(v.dir.clone());
        }
        files
    }

    fn session(&self) -> Result<Box<dyn Session>, String> {
        Ok(Box::new(DiffSingerSession {
            inner: self.inner.clone(),
        }))
    }
}

/// One worker's session. Sessions share the opened models; ONNX Runtime
/// runs one phrase on a model at a time.
pub struct DiffSingerSession {
    inner: Arc<Inner>,
}

impl Session for DiffSingerSession {
    fn sing(
        &mut self,
        phrase: &Phrase,
        quality: Quality,
        seed: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        let entry = self.inner.entry(&phrase.voice).ok_or_else(|| {
            SingError::Failed(format!("no DiffSinger voicebank \"{}\"", phrase.voice))
        })?;
        if let Err(why) = &entry.bank.vocoder {
            return Err(SingError::Failed(why.clone()));
        }
        if cancelled() {
            return Err(SingError::Cancelled);
        }
        let models = entry
            .models(&self.inner.options.runtime)
            .map_err(SingError::Failed)?;
        render::sing(&entry.bank, &models, phrase, quality, seed, cancelled)
    }
}
pub mod kana;
