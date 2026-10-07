//! Singer engines, loaded the way bridges are.
//!
//! A vocal track's notes are sung by a **singer engine**: a shared library
//! implementing [`fontelle_singer_abi`], installed beside Fontelle in its own
//! data folder ([`singer_search_paths`]) and built from a repository of its
//! own under its own licence. Fontelle never links one and one never links
//! Fontelle; this file is the loader and a safe face on the table, nothing
//! more, exactly as `bridge.rs` is for plugin formats.
//!
//! An engine runs in this process (crash isolation is a later step behind
//! the same ABI). Rendering is offline work: [`SingerInstance::render`] is
//! called on worker threads, never the audio thread, one phrase at a time per
//! instance as the ABI promises the engine.
//!
//! `fontelle-testsinger` is an engine with no voice in it, and is what the
//! tests load.

use std::ffi::{CStr, CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use std::collections::HashMap;

use fontelle_singer_abi::{
    ABI_VERSION, EngineInstance, EntryFn, FontelleSingerEngine, PhraseNote, PhraseRequest,
    PhraseResult, QUALITY_FINAL, QUALITY_PREVIEW, SINGER_RESEARCH_ONLY, STATUS_CANCELLED,
    STATUS_OK, SingerEngineV2, SingerInfo,
};

/// An engine library that would not load, and why — shown where singers are
/// chosen, because the engine somebody just installed is the one they most
/// need told about.
#[derive(Debug, Clone)]
pub struct SingerFailure {
    pub path: PathBuf,
    pub why: String,
}

/// What an engine's ABI number means for this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbiVerdict {
    Fits,
    /// Built against a newer ABI than this Fontelle speaks.
    NeedsNewerFontelle,
    /// Built against an older one, no longer read.
    NeedsOlderFontelle,
}

impl AbiVerdict {
    /// What a settings page says about an engine built for `theirs`.
    pub fn explain(self, theirs: u32) -> String {
        match self {
            Self::Fits => String::new(),
            Self::NeedsNewerFontelle => format!(
                "built for singer ABI {theirs}, and this Fontelle speaks {ABI_VERSION} \
                 — it needs a newer Fontelle"
            ),
            Self::NeedsOlderFontelle => format!(
                "built for singer ABI {theirs}, and this Fontelle speaks {ABI_VERSION} \
                 — it needs an older Fontelle, or a newer build of the engine"
            ),
        }
    }
}

/// Whether an engine built for ABI `theirs` can be loaded here.
pub fn abi_verdict(theirs: u32) -> AbiVerdict {
    if theirs > ABI_VERSION {
        AbiVerdict::NeedsNewerFontelle
    } else if theirs < fontelle_singer_abi::MIN_ABI_VERSION {
        AbiVerdict::NeedsOlderFontelle
    } else {
        AbiVerdict::Fits
    }
}

/// One singer an engine offers, read once at load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingerDescription {
    /// Stable, saved in songs.
    pub id: String,
    pub name: String,
    /// BCP 47 tags; the first is the singer's default.
    pub languages: Vec<String>,
    pub lowest: u8,
    pub highest: u8,
    /// For testing only; its output must not be shared. The picker says so.
    pub research_only: bool,
}

/// One loaded engine.
pub struct SingerEngine {
    table: *const FontelleSingerEngine,
    name: String,
    /// Read after an instance exists, and again whenever one is made while
    /// it is still unknown: an engine may fingerprint the voice it loaded
    /// and fill this in late (the ABI's note on `version`).
    version: std::sync::Mutex<String>,
    path: PathBuf,
    singers: Vec<SingerDescription>,
    /// The ABI it was built for (1 or 2).
    abi: u32,
    /// What each singer answers to, asked once at load (ABI 2; nothing
    /// beyond notes and words for a version-1 engine).
    capabilities: HashMap<String, fontelle_sing::Capabilities>,
    /// Whether it draws a window of its own (ABI 2).
    has_editor: bool,
    /// The `Arc` it lives in, so a session (which keeps the library loaded)
    /// can be made from `&self` (`fontelle_sing::Backend::session`).
    me: std::sync::OnceLock<std::sync::Weak<SingerEngine>>,
    /// Declared last so it is dropped last: the table points into it.
    _library: libloading::Library,
}

// SAFETY: the table is function pointers and static strings (the ABI's
// promise); the library handle is `Send + Sync` already. Instances, which
// are not shareable, are `SingerInstance`.
unsafe impl Send for SingerEngine {}
unsafe impl Sync for SingerEngine {}

impl std::fmt::Debug for SingerEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingerEngine")
            .field("name", &self.name)
            .field("version", &self.version())
            .field("path", &self.path)
            .finish()
    }
}

impl SingerEngine {
    fn table(&self) -> &FontelleSingerEngine {
        // SAFETY: from the library's entry point; the library lives as long
        // as `self`.
        unsafe { &*self.table }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Part of every cache key: a new version sings again.
    pub fn version(&self) -> String {
        self.version.lock().map(|v| v.clone()).unwrap_or_default()
    }

    fn refresh_version(&self) {
        let now = text(self.table().version);
        if let Ok(mut version) = self.version.lock()
            && version.is_empty()
            && !now.is_empty()
        {
            *version = now;
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The singer ABI it was built for.
    pub fn abi_version(&self) -> u32 {
        self.abi
    }

    /// Whether it has a window of its own to open ([`SingerEditor`]).
    pub fn has_editor(&self) -> bool {
        self.has_editor
    }

    /// What `singer` answers to: an ABI 2 engine's own answer, asked once at
    /// load; a version-1 engine's is nothing beyond notes and words.
    pub fn singer_capabilities(&self, singer: &str) -> fontelle_sing::Capabilities {
        self.capabilities.get(singer).cloned().unwrap_or_default()
    }

    /// The version-2 table, when the engine has a window.
    pub(crate) fn editor_table(&self) -> Option<&SingerEngineV2> {
        self.v2().filter(|_| self.has_editor)
    }

    /// A bare instance, for a window (which never shares one with a render
    /// worker).
    pub(crate) fn raw_instance(&self) -> Result<*mut EngineInstance, String> {
        // SAFETY: `create` is required; null means no instance.
        let instance = unsafe { (self.table().create)() };
        if instance.is_null() {
            return Err(format!("{} would not start", self.name));
        }
        self.refresh_version();
        Ok(instance)
    }

    pub(crate) fn destroy_raw(&self, instance: *mut EngineInstance) {
        // SAFETY: made by `raw_instance`, destroyed once.
        unsafe { (self.table().destroy)(instance) };
    }

    fn v2(&self) -> Option<&SingerEngineV2> {
        if self.abi < 2 {
            return None;
        }
        let v2 = self.table().v2;
        // SAFETY: an ABI 2 table's `v2` is null or points at a static table.
        (!v2.is_null()).then(|| unsafe { &*v2 })
    }

    pub fn singers(&self) -> &[SingerDescription] {
        &self.singers
    }

    pub fn singer(&self, id: &str) -> Option<&SingerDescription> {
        self.singers.iter().find(|s| s.id == id)
    }

    /// A new instance, to render on one thread.
    pub fn instance(self: &Arc<Self>) -> Result<SingerInstance, String> {
        // SAFETY: `create` is required by the ABI; null means no instance.
        let instance = unsafe { (self.table().create)() };
        if instance.is_null() {
            return Err(format!("{} would not start", self.name));
        }
        self.refresh_version();
        Ok(SingerInstance {
            engine: Arc::clone(self),
            instance,
        })
    }
}

/// Every engine this process has loaded.
#[derive(Default, Debug)]
pub struct SingerEngines {
    folders: Vec<PathBuf>,
    engines: Vec<Arc<SingerEngine>>,
    pub failures: Vec<SingerFailure>,
}

impl SingerEngines {
    /// None at all — a build until somebody installs one.
    pub fn none() -> Self {
        Self::default()
    }

    /// Loads every library in `folders`. A folder that is not there is not a
    /// failure; a library that is there and is not an engine is.
    ///
    /// **This runs somebody else's code**, as opening a plugin does.
    pub fn load(folders: &[PathBuf]) -> Self {
        let mut out = Self::none();
        out.folders = folders.to_vec();
        for folder in folders {
            let paths = candidates(folder);
            for path in paths {
                match load_one(&path) {
                    Ok(engine) => {
                        // A song names its engine by name, so two engines of
                        // one name cannot both be reachable: the first wins
                        // and the second is reported.
                        if out.engines.iter().any(|e| e.name == engine.name) {
                            out.failures.push(SingerFailure {
                                path,
                                why: format!("another engine called {} is loaded", engine.name),
                            });
                        } else {
                            let engine = Arc::new(engine);
                            let _ = engine.me.set(Arc::downgrade(&engine));
                            out.engines.push(engine);
                        }
                    }
                    Err(why) => out.failures.push(SingerFailure { path, why }),
                }
            }
        }
        out
    }

    pub fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    pub fn engines(&self) -> &[Arc<SingerEngine>] {
        &self.engines
    }

    /// The engine a song names.
    pub fn engine(&self, name: &str) -> Option<&Arc<SingerEngine>> {
        self.engines.iter().find(|e| e.name == name)
    }

    pub fn is_empty(&self) -> bool {
        self.engines.is_empty()
    }
}

/// The libraries in `folder` that are engines by the ABI's rule: every
/// library directly in it, and in each subfolder only the one named after
/// it (`<name>/<name>.so` or `<name>/lib<name>.so`) — so an engine's helper
/// libraries, installed beside it in its own folder, are never opened.
fn candidates(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let ext = library_extension();
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_library(&path) {
            paths.push(path);
        } else if path.is_dir() {
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            for file in [format!("{name}.{ext}"), format!("lib{name}.{ext}")] {
                let inside = path.join(file);
                if inside.is_file() {
                    paths.push(inside);
                    break;
                }
            }
        }
    }
    paths.sort();
    paths
}

fn library_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

fn is_library(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == library_extension())
}

fn load_one(path: &Path) -> Result<SingerEngine, String> {
    // SAFETY: loading a library runs its initialisers; there is no safe
    // version of this. The folder is Fontelle's own.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
    let entry: libloading::Symbol<EntryFn> =
        unsafe { library.get(fontelle_singer_abi::ENTRY_SYMBOL.as_bytes()) }
            .map_err(|_| "not a singer engine (no entry point)".to_string())?;
    let table = unsafe { entry() };
    if table.is_null() {
        return Err("the engine's entry point returned nothing".to_string());
    }
    // Only the version is read before it is known to fit: it is the first
    // field of every version of the table.
    let theirs = unsafe { (*table).abi_version };
    let verdict = abi_verdict(theirs);
    if verdict != AbiVerdict::Fits {
        return Err(verdict.explain(theirs));
    }
    let t = unsafe { &*table };
    let name = text(t.name);
    if name.is_empty() {
        return Err("the engine has no name".to_string());
    }
    // Who it has, read once through a throwaway instance — and its
    // version after that, which an engine may only know once it has one.
    let instance = unsafe { (t.create)() };
    if instance.is_null() {
        return Err(format!("{name} would not start"));
    }
    let version = text(t.version);
    let count = unsafe { (t.singer_count)(instance) }.min(4096);
    let mut singers = Vec::new();
    for index in 0..count {
        let mut info = SingerInfo {
            id: std::ptr::null(),
            name: std::ptr::null(),
            languages: std::ptr::null(),
            lowest: 0,
            highest: 127,
            flags: 0,
        };
        if !unsafe { (t.singer_info)(instance, index, &mut info) } {
            continue;
        }
        let id = text(info.id);
        if id.is_empty() {
            continue;
        }
        let shown = text(info.name);
        singers.push(SingerDescription {
            name: if shown.is_empty() { id.clone() } else { shown },
            id,
            languages: text(info.languages)
                .split(',')
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            lowest: info.lowest,
            highest: info.highest.max(info.lowest),
            research_only: info.flags & SINGER_RESEARCH_ONLY != 0,
        });
    }
    // Version 2: what each singer answers to, and whether there is a window
    // — asked here, on the thread that loads, as the ABI wants.
    let mut capabilities = HashMap::new();
    let mut has_editor = false;
    if theirs >= 2 && !t.v2.is_null() {
        let v2 = unsafe { &*t.v2 };
        for singer in &singers {
            let id = CString::new(singer.id.replace('\0', "")).unwrap_or_default();
            let raw = unsafe { (v2.capabilities)(instance, id.as_ptr()) };
            if raw.is_null() {
                continue;
            }
            let json = text(raw);
            unsafe { (v2.free_string)(instance, raw) };
            match serde_json::from_str::<fontelle_sing::Capabilities>(&json) {
                Ok(caps) => {
                    capabilities.insert(singer.id.clone(), caps);
                }
                Err(e) => eprintln!(
                    "Fontelle: {name}'s capabilities for {} did not read: {e}",
                    singer.id
                ),
            }
        }
        has_editor = unsafe { (v2.has_editor)(instance) };
    }
    unsafe { (t.destroy)(instance) };
    Ok(SingerEngine {
        table,
        name,
        version: std::sync::Mutex::new(version),
        path: path.to_path_buf(),
        singers,
        abi: theirs,
        capabilities,
        has_editor,
        me: std::sync::OnceLock::new(),
        _library: library,
    })
}

fn text(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    // SAFETY: the ABI promises NUL-terminated UTF-8 for as long as whatever
    // handed it out.
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

/// One note of a phrase, in phrase-relative seconds (the ABI's
/// `PhraseNote`, owned).
#[derive(Debug, Clone, PartialEq)]
pub struct SpecNote {
    pub start: f64,
    pub duration: f64,
    /// MIDI pitch, the fraction cents.
    pub pitch: f32,
    pub lyric: String,
}

/// What to sing: the ABI's `PhraseRequest` without its run-time parts.
#[derive(Debug, Clone, PartialEq)]
pub struct PhraseSpec {
    pub singer: String,
    pub language: String,
    pub notes: Vec<SpecNote>,
}

/// What an engine sang, copied out of its buffers.
#[derive(Debug, Clone, Default)]
pub struct Sung {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
    /// Seconds from the phrase's first note to the first sample.
    pub start: f64,
    /// Hz every `pitch_step` seconds from `start`, 0 where unvoiced.
    pub pitch: Vec<f32>,
    pub pitch_step: f64,
    /// What each note was sung as, for the roll.
    pub phonemes: Vec<SungPhonemes>,
}

/// One entry of the engine's phonemes report.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SungPhonemes {
    /// Index into the phrase's notes.
    pub note: usize,
    pub phonemes: Vec<String>,
    /// The engine did not know the word and guessed how to say it.
    pub guessed: bool,
    /// Where each phoneme starts, seconds from the note's start, where the
    /// engine said (the report's optional `offsets`); may be shorter than
    /// `phonemes`.
    pub offsets: Vec<Option<f64>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderFailure {
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for RenderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

/// An engine instance. `Send`, not `Sync`: one render at a time, which is
/// what the ABI promises the engine.
pub struct SingerInstance {
    engine: Arc<SingerEngine>,
    instance: *mut EngineInstance,
}

// SAFETY: an instance is used by one thread at a time (`&mut self` on every
// call), which is the ABI's threading rule.
unsafe impl Send for SingerInstance {}

impl SingerInstance {
    pub fn engine(&self) -> &Arc<SingerEngine> {
        &self.engine
    }

    /// Sings `spec`. Blocks for as long as the engine takes; `cancel` raised
    /// from another thread asks it to stop.
    pub fn render(
        &mut self,
        spec: &PhraseSpec,
        quality: u32,
        seed: u64,
        cancel: &AtomicU32,
    ) -> Result<Sung, RenderFailure> {
        let c = |s: &str| CString::new(s.replace('\0', "")).unwrap_or_default();
        let singer = c(&spec.singer);
        let language = c(&spec.language);
        let lyrics: Vec<CString> = spec.notes.iter().map(|n| c(&n.lyric)).collect();
        let notes: Vec<PhraseNote> = spec
            .notes
            .iter()
            .zip(&lyrics)
            .map(|(n, lyric)| PhraseNote {
                start: n.start,
                duration: n.duration,
                pitch: n.pitch,
                lyric: lyric.as_ptr(),
            })
            .collect();
        let request = PhraseRequest {
            singer_id: singer.as_ptr(),
            language: language.as_ptr(),
            notes: notes.as_ptr(),
            note_count: notes.len() as u32,
            seed,
            quality,
            cancel: cancel as *const AtomicU32,
        };
        let table = self.engine.table();
        let mut result = PhraseResult::EMPTY;
        // SAFETY: every pointer in `request` outlives the call; `result` is
        // ours to fill.
        let status = unsafe { (table.render)(self.instance, &request, &mut result) };
        self.take_result(status, result)
    }
}

impl SingerInstance {
    /// Sings `phrase` (ABI 2: the whole of it as JSON) with `cancel` handed
    /// to the engine as its cancel flag.
    fn render_phrase(
        &mut self,
        phrase: &fontelle_sing::Phrase,
        quality: u32,
        seed: u64,
        cancel: &AtomicU32,
    ) -> Result<Sung, RenderFailure> {
        let Some(v2) = self.engine.v2() else {
            return self.render(&spec_of(phrase), quality, seed, cancel);
        };
        let json = serde_json::to_string(phrase)
            .map_err(|e| RenderFailure::Failed(format!("the phrase would not write: {e}")))?;
        let json = CString::new(json).unwrap_or_default();
        let mut result = PhraseResult::EMPTY;
        // SAFETY: the JSON and the flag outlive the call; `result` is ours.
        let status = unsafe {
            (v2.render_phrase)(
                self.instance,
                json.as_ptr(),
                quality,
                seed,
                cancel as *const AtomicU32,
                &mut result,
            )
        };
        self.take_result(status, result)
    }

    /// What `status` and `result` say, copied out and the result freed.
    fn take_result(
        &mut self,
        status: i32,
        mut result: PhraseResult,
    ) -> Result<Sung, RenderFailure> {
        let table = self.engine.table();
        if status == STATUS_CANCELLED {
            return Err(RenderFailure::Cancelled);
        }
        if status != STATUS_OK {
            // On this thread, straight after the failing call (the ABI's
            // rule).
            let why = text(unsafe { (table.last_error)(self.instance) });
            return Err(RenderFailure::Failed(if why.is_empty() {
                format!("{} could not sing this phrase", self.engine.name)
            } else {
                why
            }));
        }
        let copy = |p: *const f32, n: u64| -> Vec<f32> {
            if p.is_null() || n == 0 {
                Vec::new()
            } else {
                // SAFETY: the engine owns `n` floats at `p` until
                // `free_result`.
                unsafe { std::slice::from_raw_parts(p, n as usize) }.to_vec()
            }
        };
        let sung = Sung {
            sample_rate: result.sample_rate,
            samples: copy(result.samples, result.frame_count),
            start: if result.start.is_finite() {
                result.start
            } else {
                0.0
            },
            pitch: copy(result.pitch, result.pitch_count),
            pitch_step: result.pitch_step,
            phonemes: parse_phonemes(&text(result.phonemes)),
        };
        unsafe { (table.free_result)(self.instance, &mut result) };
        if sung.sample_rate == 0 {
            return Err(RenderFailure::Failed(format!(
                "{} answered with no sample rate",
                self.engine.name
            )));
        }
        Ok(sung)
    }
}

/// A phrase as a version-1 engine reads it: notes and words. Phonemes set
/// by hand reach it the one way it understands them, as a bracketed lyric.
fn spec_of(phrase: &fontelle_sing::Phrase) -> PhraseSpec {
    PhraseSpec {
        singer: phrase.voice.clone(),
        language: phrase.language.clone(),
        notes: phrase
            .notes
            .iter()
            .map(|n| SpecNote {
                start: n.start,
                duration: n.duration,
                pitch: n.pitch,
                lyric: match &n.phonemes {
                    Some(p) if !p.is_empty() => format!(
                        "[{}]",
                        p.iter()
                            .map(|e| e.symbol.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    ),
                    _ => n.lyric.clone(),
                },
            })
            .collect(),
    }
}

fn quality_code(quality: fontelle_sing::Quality) -> u32 {
    match quality {
        fontelle_sing::Quality::Preview => QUALITY_PREVIEW,
        fontelle_sing::Quality::Final => QUALITY_FINAL,
    }
}

impl From<Sung> for fontelle_sing::Sung {
    fn from(sung: Sung) -> Self {
        fontelle_sing::Sung {
            sample_rate: sung.sample_rate,
            samples: sung.samples,
            start: sung.start,
            pitch: sung.pitch,
            pitch_step: sung.pitch_step,
            phonemes: sung
                .phonemes
                .into_iter()
                .map(|p| fontelle_sing::SungNote {
                    note: p.note,
                    phonemes: p
                        .phonemes
                        .into_iter()
                        .enumerate()
                        .map(|(i, symbol)| fontelle_sing::SungPhoneme {
                            symbol,
                            offset: p.offsets.get(i).copied().flatten(),
                        })
                        .collect(),
                    guessed: p.guessed,
                })
                .collect(),
        }
    }
}

impl From<RenderFailure> for fontelle_sing::SingError {
    fn from(failure: RenderFailure) -> Self {
        match failure {
            RenderFailure::Cancelled => Self::Cancelled,
            RenderFailure::Failed(why) => Self::Failed(why),
        }
    }
}

/// An external engine is a backend like any other: the scheduler, the cache
/// and the roll never know it is a library. Version 1 is sung notes and
/// words; version 2 the whole phrase.
impl fontelle_sing::Backend for SingerEngine {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> String {
        SingerEngine::version(self)
    }

    fn voices(&self) -> Vec<fontelle_sing::VoiceInfo> {
        self.singers
            .iter()
            .map(|s| fontelle_sing::VoiceInfo {
                id: s.id.clone(),
                name: s.name.clone(),
                languages: s.languages.clone(),
                lowest: s.lowest,
                highest: s.highest,
                research_only: s.research_only,
                terms: None,
                image: None,
            })
            .collect()
    }

    fn capabilities(&self, voice: &str) -> fontelle_sing::Capabilities {
        self.singer_capabilities(voice)
    }

    fn session(&self) -> Result<Box<dyn fontelle_sing::Session>, String> {
        // Every loaded engine lives in an `Arc` (`SingerEngines::load`), and
        // an instance holds one so the library outlives it.
        let me = self
            .me
            .get()
            .and_then(std::sync::Weak::upgrade)
            .ok_or_else(|| format!("{} is being unloaded", self.name))?;
        Ok(Box::new(me.instance()?))
    }
}

impl fontelle_sing::Session for SingerInstance {
    fn sing(
        &mut self,
        phrase: &fontelle_sing::Phrase,
        quality: fontelle_sing::Quality,
        seed: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<fontelle_sing::Sung, fontelle_sing::SingError> {
        // The engine polls a flag, and a closure cannot be handed across the
        // ABI: asked before and after. A scheduler that can be quicker calls
        // `sing_flagged`.
        if cancelled() {
            return Err(fontelle_sing::SingError::Cancelled);
        }
        let flag = AtomicU32::new(0);
        let sung = self.sing_flagged(phrase, quality, seed, &flag)?;
        if cancelled() {
            return Err(fontelle_sing::SingError::Cancelled);
        }
        Ok(sung)
    }

    fn sing_flagged(
        &mut self,
        phrase: &fontelle_sing::Phrase,
        quality: fontelle_sing::Quality,
        seed: u64,
        cancel: &AtomicU32,
    ) -> Result<fontelle_sing::Sung, fontelle_sing::SingError> {
        self.render_phrase(phrase, quality_code(quality), seed, cancel)
            .map(Into::into)
            .map_err(Into::into)
    }
}

impl Drop for SingerInstance {
    fn drop(&mut self) {
        // SAFETY: made by `create`, destroyed once.
        unsafe { (self.engine.table().destroy)(self.instance) };
    }
}

/// The ABI's phonemes JSON — `[{"note": i, "phonemes": ["s", "a"]}]` — read
/// leniently: anything malformed is shown as nothing rather than refused.
fn parse_phonemes(json: &str) -> Vec<SungPhonemes> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(json)
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let note = item.get("note")?.as_u64()? as usize;
            let phonemes = item
                .get("phonemes")?
                .as_array()?
                .iter()
                .filter_map(|p| p.as_str().map(str::to_string))
                .collect();
            let guessed = item
                .get("guessed")
                .and_then(|g| g.as_bool())
                .unwrap_or(false);
            // Where each phoneme starts, seconds from its note, when the
            // engine says (`null` for one it does not).
            let offsets = item
                .get("offsets")
                .and_then(|o| o.as_array())
                .map(|o| o.iter().map(serde_json::Value::as_f64).collect())
                .unwrap_or_default();
            // Any other key is the engine's to add and ours to ignore.
            Some(SungPhonemes {
                note,
                phonemes,
                guessed,
                offsets,
            })
        })
        .collect()
}

fn is_absolute(value: &str, windows: bool) -> bool {
    if windows {
        let b = value.as_bytes();
        (b.len() >= 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
            || value.starts_with("\\\\")
    } else {
        value.starts_with('/')
    }
}

/// Fontelle's own data folder (`<data>/fontelle`) by the settings' rule
/// (`Settings::data_dir_on` in `fontelle-app`): an absolute
/// `$XDG_DATA_HOME`, else `~/.local/share` — macOS included, as the app's
/// other folders are — and on Windows `%LOCALAPPDATA%`, unless a
/// `~/.local/share/fontelle` is already there. Singer engines live in its
/// `singers`, voicebanks in its `voicebanks`.
pub fn fontelle_data_dir_from(
    env: &dyn Fn(&str) -> Option<String>,
    windows: bool,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let absolute = |value: &str| is_absolute(value, windows);
    if let Some(xdg) = env("XDG_DATA_HOME").filter(|v| absolute(v)) {
        return Some(PathBuf::from(xdg).join("fontelle"));
    }
    let under_home = env("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".local/share").join("fontelle"));
    if windows {
        if let Some(old) = under_home.as_ref().filter(|d| exists(d)) {
            return Some(old.clone());
        }
        if let Some(base) = env("LOCALAPPDATA").filter(|v| absolute(v)) {
            return Some(PathBuf::from(base).join("fontelle"));
        }
    }
    under_home
}

/// [`fontelle_data_dir_from`] for this process.
pub fn fontelle_data_dir() -> Option<PathBuf> {
    fontelle_data_dir_from(&|key| std::env::var(key).ok(), cfg!(windows), &|path| {
        path.is_dir()
    })
}

/// Where Fontelle looks for singer engines: `FONTELLE_SINGERS` (a path list)
/// first, then `singers` in its own data folder.
pub fn singer_search_paths() -> Vec<PathBuf> {
    singer_search_paths_from(&|key| std::env::var(key).ok(), cfg!(windows), &|path| {
        path.is_dir()
    })
}

/// [`singer_search_paths`] for a given environment and platform. The data
/// folder follows the settings' own rule (`Settings::data_dir_on` in
/// `fontelle-app`): an absolute `$XDG_DATA_HOME`, else `~/.local/share` —
/// macOS included, as the app's other folders are — and on Windows
/// `%LOCALAPPDATA%`, unless a `~/.local/share/fontelle` is already there.
pub fn singer_search_paths_from(
    env: &dyn Fn(&str) -> Option<String>,
    windows: bool,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(list) = env("FONTELLE_SINGERS") {
        paths.extend(
            std::env::split_paths(&list).filter(|p| is_absolute(&p.to_string_lossy(), windows)),
        );
    }
    let data = fontelle_data_dir_from(env, windows, exists);
    if let Some(data) = data {
        paths.push(data.join("singers"));
    }
    paths.dedup();
    paths
}
