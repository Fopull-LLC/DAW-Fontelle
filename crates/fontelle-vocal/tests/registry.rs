//! Who can sing: the Fontelle Singer's built-in backends (classic UTAU and
//! DiffSinger voicebanks, each a `fontelle_sing::Backend` made over the
//! voicebanks folder) and every external engine installed, behind one
//! registry the scheduler and the picker read — and through which a
//! song's `VocalVoice` finds what sings it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontelle_host::SingerEngines;
use fontelle_model::VocalVoice;
use fontelle_sing::{
    Backend, Capabilities, Lane, Phrase, Quality, Session, SingError, Sung, VoiceInfo,
};
use fontelle_vocal::{BuiltinBackend, SingerKind, SingerRegistry};

fn library_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    }
}

/// Loaded once: copying over a library that is loaded rewrites code that
/// is running.
fn engines() -> Arc<SingerEngines> {
    static ENGINES: std::sync::OnceLock<Arc<SingerEngines>> = std::sync::OnceLock::new();
    Arc::clone(ENGINES.get_or_init(|| {
        let mut path = std::env::current_exe().unwrap();
        path.pop();
        path.pop();
        let built = path.join(library_name("fontelle_testsinger"));
        assert!(built.exists(), "run `cargo build -p fontelle-testsinger`");
        let folder = path.join(format!("fontelle-vocal-registry-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&folder);
        std::fs::copy(&built, folder.join(library_name("fontelle_testsinger"))).unwrap();
        std::fs::copy(
            &built,
            folder.join(library_name("fontelle_testsinger_abi2")),
        )
        .unwrap();
        Arc::new(SingerEngines::load(&[folder]))
    }))
}

/// A built-in backend whose voices are the folders under the voicebanks
/// folder whose names start with `prefix`.
struct Folders {
    name: &'static str,
    root: PathBuf,
    prefix: &'static str,
}

impl Backend for Folders {
    fn name(&self) -> &str {
        self.name
    }
    fn version(&self) -> String {
        "0.1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        let mut out: Vec<VoiceInfo> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(self.prefix).then(|| VoiceInfo {
                    id: name.clone(),
                    name: name.clone(),
                    languages: vec!["ja".into()],
                    lowest: 48,
                    highest: 72,
                    research_only: false,
                    terms: Some("Free to use.".into()),
                    image: None,
                })
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities {
            lanes: vec![Lane::Dynamics],
            ..Capabilities::default()
        }
    }
    fn session(&self) -> Result<Box<dyn Session>, String> {
        Ok(Box::new(Silent))
    }
}

struct Silent;

impl Session for Silent {
    fn sing(
        &mut self,
        _phrase: &Phrase,
        _quality: Quality,
        _seed: u64,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        Ok(Sung {
            sample_rate: 8_000,
            samples: vec![0.0; 80],
            ..Sung::default()
        })
    }
}

fn utau(root: &Path) -> Arc<dyn Backend> {
    Arc::new(Folders {
        name: "Test UTAU",
        root: root.to_path_buf(),
        prefix: "utau-",
    })
}

fn diffsinger(root: &Path) -> Arc<dyn Backend> {
    Arc::new(Folders {
        name: "Test DiffSinger",
        root: root.to_path_buf(),
        prefix: "ds-",
    })
}

fn builtins() -> Vec<BuiltinBackend> {
    vec![
        BuiltinBackend {
            key: "fontelle-utau",
            kind: SingerKind::Utau,
            make: utau,
        },
        BuiltinBackend {
            key: "fontelle-diffsinger",
            kind: SingerKind::DiffSinger,
            make: diffsinger,
        },
    ]
}

fn voicebanks(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-registry-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("utau-teto")).unwrap();
    std::fs::create_dir_all(dir.join("ds-qixuan")).unwrap();
    dir
}

#[test]
fn every_voice_is_listed_built_in_banks_first_then_engines() {
    let root = voicebanks("list");
    let registry = SingerRegistry::new(&root, builtins(), engines());
    let all = registry.voices();
    let keys: Vec<(&str, &str)> = all
        .iter()
        .map(|v| (v.backend.as_str(), v.info.id.as_str()))
        .collect();
    assert_eq!(keys[0], ("fontelle-utau", "utau-teto"));
    assert_eq!(keys[1], ("fontelle-diffsinger", "ds-qixuan"));
    assert_eq!(all[0].kind, SingerKind::Utau);
    assert_eq!(all[1].kind, SingerKind::DiffSinger);
    assert!(all[0].built_in && !all[2].built_in);
    // Both test engines, each singer.
    let engine_voices: Vec<&str> = all
        .iter()
        .filter(|v| !v.built_in)
        .map(|v| v.backend.as_str())
        .collect();
    assert_eq!(engine_voices.len(), 4);
    assert!(engine_voices.contains(&fontelle_testsinger::ENGINE2_NAME));
    assert!(
        all.iter()
            .all(|v| !v.built_in == (v.kind == SingerKind::Engine))
    );
    // The engine with a window says so.
    let two = all
        .iter()
        .find(|v| v.backend == fontelle_testsinger::ENGINE2_NAME)
        .unwrap();
    assert!(two.has_editor);
}

#[test]
fn a_songs_voice_finds_its_backend_and_what_it_answers_to() {
    let root = voicebanks("find");
    let registry = SingerRegistry::new(&root, builtins(), engines());
    let voice = VocalVoice::new("fontelle-utau", "utau-teto", "ja");
    let backend = registry.backend(&voice.engine).expect("built in");
    assert_eq!(backend.name(), "Test UTAU");
    assert_eq!(registry.capabilities(&voice).lanes, vec![Lane::Dynamics]);
    let engine = VocalVoice::new(
        fontelle_testsinger::ENGINE2_NAME,
        fontelle_testsinger::ALTO_ID,
        "en",
    );
    assert!(registry.backend(&engine.engine).is_some());
    assert!(
        registry.engine(&engine.engine).is_some(),
        "an engine, for its window"
    );
    assert!(registry.engine("fontelle-utau").is_none());
    assert!(registry.backend("Nobody").is_none());
    assert_eq!(
        registry.capabilities(&VocalVoice::new("Nobody", "x", "en")),
        Capabilities::default()
    );
}

#[test]
fn a_new_track_gets_a_free_voicebank_before_any_engine() {
    let root = voicebanks("default");
    let registry = SingerRegistry::new(&root, builtins(), engines());
    assert_eq!(
        registry.default_voice(),
        Some(VocalVoice::new("fontelle-utau", "utau-teto", "ja"))
    );
    // With no banks installed, the first engine's ordinary singer.
    let empty =
        std::env::temp_dir().join(format!("fontelle-registry-empty-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&empty);
    let registry = SingerRegistry::new(&empty, builtins(), engines());
    let voice = registry.default_voice().unwrap();
    assert_eq!(voice.singer, fontelle_testsinger::ALTO_ID);
}

#[test]
fn a_bank_installed_later_is_found_on_a_rescan() {
    let root = voicebanks("rescan");
    let mut registry = SingerRegistry::new(&root, builtins(), Arc::new(SingerEngines::none()));
    assert_eq!(registry.voices().len(), 2);
    std::fs::create_dir_all(root.join("utau-defoko")).unwrap();
    registry.rescan();
    assert_eq!(registry.voices().len(), 3);
}

#[test]
fn an_engine_named_like_a_built_in_backend_is_not_let_in() {
    let registry = SingerRegistry::new(
        &voicebanks("clash"),
        builtins(),
        Arc::new(SingerEngines::none()),
    );
    assert!(registry.backend("fontelle-utau").is_some());
    assert!(SingerRegistry::is_builtin_key("fontelle-utau"));
    assert!(!SingerRegistry::is_builtin_key(
        fontelle_testsinger::ENGINE_NAME
    ));
}

#[test]
fn the_built_in_list_is_where_the_two_runtimes_are_wired() {
    // Every entry the release wires in has a key a song can store and a kind
    // the picker can badge.
    let built_in = fontelle_vocal::builtin_backends();
    for b in &built_in {
        assert!(b.key.starts_with("fontelle-"), "{}", b.key);
        assert_ne!(b.kind, SingerKind::Engine);
    }
    let keys: Vec<_> = built_in.iter().map(|b| b.key).collect();
    assert_eq!(keys, ["fontelle-diffsinger", "fontelle-utau"]);
}

#[test]
fn the_built_in_runtimes_find_nothing_in_an_empty_folder() {
    let dir = std::env::temp_dir().join(format!("fontelle-builtin-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for b in fontelle_vocal::builtin_backends() {
        assert!((b.make)(&dir).voices().is_empty(), "{}", b.key);
    }
    std::fs::remove_dir_all(&dir).ok();
}
