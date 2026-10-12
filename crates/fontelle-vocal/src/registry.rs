//! Who can sing a vocal track: the **Fontelle Singer**'s built-in voicebank
//! runtimes and every **external engine** installed, behind one registry.
//!
//! A song names its singer by a [`VocalVoice`]: `engine` is the key of the
//! backend that sings it — a built-in runtime's own key (`fontelle-utau`,
//! `fontelle-diffsinger`), or an external engine's name — and `singer` the
//! voice's id inside it. Keys starting `fontelle-` are reserved for the
//! built-ins, so an engine can never be mistaken for one.
//!
//! # Wiring a built-in runtime
//!
//! A runtime is a crate with a type implementing `fontelle_sing::Backend`
//! made over the voicebanks folder. Wiring it is one entry in
//! [`builtin_backends`]:
//!
//! ```ignore
//! BuiltinBackend {
//!     key: "fontelle-utau",
//!     kind: SingerKind::Utau,
//!     make: |voicebanks| std::sync::Arc::new(fontelle_utau::UtauBackend::new(voicebanks)),
//! },
//! ```
//!
//! `make` is called again after a bank is installed or removed
//! ([`SingerRegistry::rescan`]), so a backend may read the folder once when
//! made.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fontelle_host::{SingerEngine, SingerEngines};
use fontelle_model::VocalVoice;
use fontelle_sing::{Backend, Capabilities, VoiceInfo};

/// What kind of singer a voice is, for the picker's badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SingerKind {
    /// A classic UTAU voicebank, sung by the Fontelle Singer.
    Utau,
    /// A DiffSinger voicebank (OpenUtau layout), sung by the Fontelle Singer.
    DiffSinger,
    /// An external singer engine's voice.
    Engine,
}

impl SingerKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utau => "UTAU",
            Self::DiffSinger => "DiffSinger",
            Self::Engine => "Engine",
        }
    }
}

/// One of the Fontelle Singer's runtimes.
#[derive(Clone, Copy)]
pub struct BuiltinBackend {
    /// What a song stores as `VocalVoice::engine`; starts `fontelle-`.
    pub key: &'static str,
    pub kind: SingerKind,
    /// The backend over a voicebanks folder.
    pub make: fn(&Path) -> Arc<dyn Backend>,
}

/// The Fontelle Singer's runtimes in this build, in the order the picker
/// lists their banks. **Each built-in runtime is wired here** — see the
/// module doc.
pub fn builtin_backends() -> Vec<BuiltinBackend> {
    vec![
        BuiltinBackend {
            key: "fontelle-diffsinger",
            kind: SingerKind::DiffSinger,
            make: |voicebanks| {
                crate::leveled_kept(
                    Arc::new(fontelle_diffsinger::DiffSinger::new(
                        fontelle_diffsinger::Options::new(vec![voicebanks.to_path_buf()]),
                    )),
                    crate::default_levels_dir(),
                )
            },
        },
        BuiltinBackend {
            key: "fontelle-utau",
            kind: SingerKind::Utau,
            make: |voicebanks| {
                crate::leveled_kept(
                    Arc::new(fontelle_utau::UtauBackend::new(
                        vec![voicebanks.to_path_buf()],
                        fontelle_utau::cache::default_cache_dir(),
                    )),
                    crate::default_levels_dir(),
                )
            },
        },
    ]
}

/// What the built-in instrument is called.
pub const FONTELLE_SINGER: &str = "Fontelle Singer";

/// The instrument an engine is, by its display name: what it says before
/// any bracketed note (`"Some Engine (test singers, research only)"` is
/// the instrument "Some Engine"); the whole name when nothing comes
/// before the bracket. Kept generic: no engine is named in this tree.
pub fn instrument_name(engine_name: &str) -> String {
    let name = engine_name.trim();
    match name.find(" (") {
        Some(at) if !name[..at].trim().is_empty() => name[..at].trim().to_string(),
        _ => name.to_string(),
    }
}

/// One instrument a vocal track can be: the Fontelle Singer, or an
/// installed engine. All sing the same part, so a track changes between
/// them keeping its notes, lyrics, expression and lanes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingerInstrument {
    /// `None` for the Fontelle Singer; else the engine's name.
    pub engine: Option<String>,
    pub name: String,
    pub has_editor: bool,
}

/// One voice anybody could choose.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryVoice {
    /// The key a song stores (`VocalVoice::engine`).
    pub backend: String,
    /// The backend's own name, for the picker.
    pub backend_name: String,
    pub kind: SingerKind,
    /// The Fontelle Singer's (free, built in) rather than an engine's.
    pub built_in: bool,
    /// Whether its engine has a window of its own.
    pub has_editor: bool,
    pub info: VoiceInfo,
}

/// See the module doc.
pub struct SingerRegistry {
    voicebanks: PathBuf,
    builtins: Vec<(BuiltinBackend, Arc<dyn Backend>)>,
    engines: Arc<SingerEngines>,
    capabilities: Mutex<HashMap<(String, String), Capabilities>>,
}

impl std::fmt::Debug for SingerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingerRegistry")
            .field("voicebanks", &self.voicebanks)
            .field(
                "builtins",
                &self.builtins.iter().map(|(b, _)| b.key).collect::<Vec<_>>(),
            )
            .field("engines", &self.engines.engines().len())
            .finish()
    }
}

impl SingerRegistry {
    pub fn new(
        voicebanks: &Path,
        builtins: Vec<BuiltinBackend>,
        engines: Arc<SingerEngines>,
    ) -> Self {
        let builtins = builtins
            .into_iter()
            .filter(|b| Self::is_builtin_key(b.key))
            .map(|b| (b, (b.make)(voicebanks)))
            .collect();
        Self {
            voicebanks: voicebanks.to_path_buf(),
            builtins,
            engines,
            capabilities: Mutex::new(HashMap::new()),
        }
    }

    /// Nobody to sing: no runtimes, no engines.
    pub fn empty() -> Self {
        Self::new(Path::new(""), Vec::new(), Arc::new(SingerEngines::none()))
    }

    /// Whether `key` names a built-in runtime's backend (reserved: no engine
    /// may use one).
    pub fn is_builtin_key(key: &str) -> bool {
        fontelle_model::is_builtin_engine(key)
    }

    /// Makes every built-in backend again over the voicebanks folder — after
    /// a bank was installed or removed.
    pub fn rescan(&mut self) {
        for (b, backend) in &mut self.builtins {
            *backend = (b.make)(&self.voicebanks);
        }
        if let Ok(mut caps) = self.capabilities.lock() {
            caps.clear();
        }
    }

    pub fn voicebanks(&self) -> &Path {
        &self.voicebanks
    }

    pub fn engines(&self) -> &Arc<SingerEngines> {
        &self.engines
    }

    /// The backend a song's `VocalVoice::engine` names.
    pub fn backend(&self, key: &str) -> Option<Arc<dyn Backend>> {
        if let Some((_, b)) = self.builtins.iter().find(|(b, _)| b.key == key) {
            return Some(Arc::clone(b));
        }
        self.engine(key).map(|e| Arc::clone(e) as Arc<dyn Backend>)
    }

    /// The external engine `key` names — for its window.
    pub fn engine(&self, key: &str) -> Option<&Arc<SingerEngine>> {
        if Self::is_builtin_key(key) {
            return None;
        }
        self.engines.engine(key)
    }

    pub fn kind(&self, key: &str) -> Option<SingerKind> {
        if let Some((b, _)) = self.builtins.iter().find(|(b, _)| b.key == key) {
            return Some(b.kind);
        }
        self.engine(key).map(|_| SingerKind::Engine)
    }

    /// Every voice: the Fontelle Singer's banks first (each runtime's in its
    /// own order), then each engine's singers.
    pub fn voices(&self) -> Vec<RegistryVoice> {
        let mut out = Vec::new();
        for (b, backend) in &self.builtins {
            for info in backend.voices() {
                out.push(RegistryVoice {
                    backend: b.key.to_string(),
                    backend_name: backend.name().to_string(),
                    kind: b.kind,
                    built_in: true,
                    has_editor: false,
                    info,
                });
            }
        }
        for engine in self.engines.engines() {
            if Self::is_builtin_key(engine.name()) {
                continue;
            }
            for info in Backend::voices(engine.as_ref()) {
                out.push(RegistryVoice {
                    backend: engine.name().to_string(),
                    backend_name: engine.name().to_string(),
                    kind: SingerKind::Engine,
                    built_in: false,
                    has_editor: engine.has_editor(),
                    info,
                });
            }
        }
        out
    }

    /// Every instrument a vocal track can be: the Fontelle Singer first
    /// (always — its voicebanks can be installed later), then each engine.
    pub fn instruments(&self) -> Vec<SingerInstrument> {
        let mut out = vec![SingerInstrument {
            engine: None,
            name: FONTELLE_SINGER.to_string(),
            has_editor: false,
        }];
        for engine in self.engines.engines() {
            if Self::is_builtin_key(engine.name()) {
                continue;
            }
            out.push(SingerInstrument {
                engine: Some(engine.name().to_string()),
                name: instrument_name(engine.name()),
                has_editor: engine.has_editor(),
            });
        }
        out
    }

    /// The voice `instrument` (`None`: the Fontelle Singer; else an
    /// engine's name) starts with: one of **its own** singers, in
    /// `language` when one sings it, an ordinary singer before a
    /// research-only one; `None` when it has no singer.
    pub fn default_voice_for(
        &self,
        instrument: Option<&str>,
        language: Option<&str>,
    ) -> Option<VocalVoice> {
        self.default_voice_among(instrument, language, &|_| true)
    }

    /// [`default_voice_for`](Self::default_voice_for) among the voices
    /// `keep` lets through (those whose terms are settled, say).
    pub fn default_voice_among(
        &self,
        instrument: Option<&str>,
        language: Option<&str>,
        keep: &dyn Fn(&RegistryVoice) -> bool,
    ) -> Option<VocalVoice> {
        let voices: Vec<RegistryVoice> = self
            .voices()
            .into_iter()
            .filter(|v| match instrument {
                None => v.built_in,
                Some(engine) => !v.built_in && v.backend == engine,
            })
            .filter(|v| keep(v))
            .collect();
        let sings =
            |v: &&RegistryVoice| language.is_some_and(|l| v.info.languages.iter().any(|x| x == l));
        let pick = voices
            .iter()
            .find(|v| sings(v) && !v.info.research_only)
            .or_else(|| voices.iter().find(sings))
            .or_else(|| voices.iter().find(|v| !v.info.research_only))
            .or_else(|| voices.first())?;
        let language = language
            .filter(|l| pick.info.languages.iter().any(|x| x == l))
            .map(str::to_string)
            .or_else(|| pick.info.languages.first().cloned())
            .unwrap_or_else(|| "en".into());
        Some(VocalVoice::new(
            pick.backend.clone(),
            pick.info.id.clone(),
            language,
        ))
    }

    /// What `voice` answers to, asked once and kept.
    pub fn capabilities(&self, voice: &VocalVoice) -> Capabilities {
        let key = (voice.engine.clone(), voice.singer.clone());
        if let Some(caps) = self
            .capabilities
            .lock()
            .ok()
            .and_then(|c| c.get(&key).cloned())
        {
            return caps;
        }
        let Some(backend) = self.backend(&voice.engine) else {
            return Capabilities::default();
        };
        let caps = backend.capabilities(&voice.singer);
        if let Ok(mut cache) = self.capabilities.lock() {
            cache.insert(key, caps.clone());
        }
        caps
    }

    /// The voice a new vocal track starts with: the first free voicebank,
    /// else the first engine singer that is not research-only, else any.
    pub fn default_voice(&self) -> Option<VocalVoice> {
        self.default_voice_where(&|_| true)
    }

    /// [`default_voice`](Self::default_voice) among the voices `keep` lets
    /// through.
    pub fn default_voice_where(&self, keep: &dyn Fn(&RegistryVoice) -> bool) -> Option<VocalVoice> {
        let voices: Vec<RegistryVoice> = self.voices().into_iter().filter(|v| keep(v)).collect();
        let pick = voices
            .iter()
            .find(|v| v.built_in && !v.info.research_only)
            .or_else(|| voices.iter().find(|v| !v.info.research_only))
            .or_else(|| voices.first())?;
        Some(VocalVoice::new(
            pick.backend.clone(),
            pick.info.id.clone(),
            pick.info
                .languages
                .first()
                .cloned()
                .unwrap_or_else(|| "en".into()),
        ))
    }
}
