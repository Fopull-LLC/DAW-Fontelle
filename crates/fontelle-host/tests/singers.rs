//! Singer engines: found in Fontelle's own folder, loaded, asked who they
//! have and made to sing — through the real C ABI, with
//! `fontelle-testsinger` built as the library a real engine would be.
//!
//! `fontelle-singer-abi` is the contract; what these prove is the loader and
//! the seam, the same thing `tests/bridges.rs` proves for plugin bridges.

use std::path::PathBuf;
use std::sync::atomic::AtomicU32;

use fontelle_host::{
    AbiVerdict, PhraseSpec, RenderFailure, SingerEngines, SpecNote, abi_verdict,
    singer_search_paths_from,
};
use fontelle_singer_abi::{ABI_VERSION, QUALITY_FINAL, QUALITY_PREVIEW};

fn library_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    }
}

/// A folder of Fontelle's singer-engine kind with the test engine in it.
fn singer_folder() -> PathBuf {
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let mut path = std::env::current_exe().expect("a test binary knows where it is");
            path.pop();
            path.pop();
            let built = path.join(library_name("fontelle_testsinger"));
            assert!(
                built.exists(),
                "the test singer has not been built: {} — run `cargo build -p fontelle-testsinger`",
                built.display()
            );
            let folder = path.join(format!("fontelle-test-singers-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&folder);
            std::fs::copy(&built, folder.join(library_name("fontelle_testsinger"))).unwrap();
            folder
        })
        .clone()
}

fn engines() -> SingerEngines {
    SingerEngines::load(&[singer_folder()])
}

fn la(start: f64, pitch: f32, lyric: &str) -> SpecNote {
    SpecNote {
        start,
        duration: 0.4,
        pitch,
        lyric: lyric.to_string(),
    }
}

#[test]
fn an_engine_in_the_folder_is_found_with_its_singers() {
    let engines = engines();
    assert!(engines.failures.is_empty(), "{:?}", engines.failures);
    assert_eq!(engines.engines().len(), 1);
    let engine = &engines.engines()[0];
    assert_eq!(engine.name(), fontelle_testsinger::ENGINE_NAME);
    assert_eq!(engine.version(), fontelle_testsinger::ENGINE_VERSION);
    let singers = engine.singers();
    assert_eq!(singers.len(), 2);
    assert_eq!(singers[0].id, fontelle_testsinger::ALTO_ID);
    assert_eq!(singers[0].name, fontelle_testsinger::ALTO_NAME);
    assert_eq!(singers[0].languages, vec!["en", "ja"]);
    assert!(!singers[0].research_only);
    assert!(singers[1].research_only, "the flag reaches the host");
    assert!(singers[0].lowest < singers[0].highest);
    // Looked up by the names a song stores.
    assert!(engines.engine(fontelle_testsinger::ENGINE_NAME).is_some());
    assert!(engines.engine("Nobody's Engine").is_none());
}

#[test]
fn a_missing_folder_is_no_engines_and_no_failure() {
    let engines = SingerEngines::load(&[PathBuf::from("/nonexistent/fontelle/singers")]);
    assert!(engines.engines().is_empty());
    assert!(engines.failures.is_empty());
    assert!(SingerEngines::none().engines().is_empty());
}

#[test]
fn a_library_that_is_not_a_singer_engine_is_a_failure_not_a_crash() {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let other = path.join(library_name("fontelle_testbridge"));
    if !other.exists() {
        return;
    }
    let folder = path.join(format!("fontelle-test-not-singers-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    std::fs::copy(&other, folder.join(library_name("not_a_singer"))).unwrap();
    let engines = SingerEngines::load(&[folder]);
    assert!(engines.engines().is_empty());
    assert_eq!(engines.failures.len(), 1);
    assert!(
        engines.failures[0].why.contains("not a singer engine"),
        "{}",
        engines.failures[0].why
    );
}

#[test]
fn an_engine_built_for_another_abi_says_which_fontelle_it_needs() {
    assert_eq!(abi_verdict(ABI_VERSION), AbiVerdict::Fits);
    assert_eq!(abi_verdict(ABI_VERSION + 1), AbiVerdict::NeedsNewerFontelle);
    assert_eq!(abi_verdict(ABI_VERSION - 1), AbiVerdict::NeedsOlderFontelle);
    assert!(
        AbiVerdict::NeedsNewerFontelle
            .explain(2)
            .contains("newer Fontelle")
    );
    assert!(
        AbiVerdict::NeedsOlderFontelle
            .explain(0)
            .contains("older Fontelle")
    );
}

#[test]
fn a_phrase_is_sung_through_the_abi() {
    let engines = engines();
    let engine = &engines.engines()[0];
    let mut singer = engine.instance().expect("an instance");
    let spec = PhraseSpec {
        singer: fontelle_testsinger::ALTO_ID.into(),
        language: "en".into(),
        notes: vec![
            la(0.0, 69.0, "la"),
            la(0.4, 71.0, "-"),
            la(0.8, 72.0, "[s a]"),
        ],
    };
    let cancel = AtomicU32::new(0);
    let sung = singer.render(&spec, QUALITY_PREVIEW, 9, &cancel).unwrap();
    assert_eq!(sung.sample_rate, fontelle_testsinger::SAMPLE_RATE);
    assert!(sung.start < 0.0);
    assert!(!sung.samples.is_empty());
    assert!(sung.pitch_step > 0.0 && !sung.pitch.is_empty());
    // What it sang, per note, for the roll.
    assert_eq!(sung.phonemes.len(), 2, "{:?}", sung.phonemes);
    assert_eq!(sung.phonemes[0].note, 0);
    assert_eq!(sung.phonemes[0].phonemes, vec!["l", "a"]);
    assert!(!sung.phonemes[0].guessed);
    assert_eq!(sung.phonemes[1].note, 2);
    assert_eq!(sung.phonemes[1].phonemes, vec!["s", "a"]);
    // Final is a different take of the same phrase.
    let fine = singer.render(&spec, QUALITY_FINAL, 9, &cancel).unwrap();
    assert_eq!(fine.samples.len(), sung.samples.len());
    assert_ne!(fine.samples, sung.samples);
}

#[test]
fn a_cancelled_render_and_a_failed_one_are_told_apart() {
    let engines = engines();
    let mut singer = engines.engines()[0].instance().unwrap();
    let spec = |lyric: &str| PhraseSpec {
        singer: fontelle_testsinger::ALTO_ID.into(),
        language: "en".into(),
        notes: vec![la(0.0, 60.0, lyric)],
    };
    let raised = AtomicU32::new(1);
    assert!(matches!(
        singer.render(&spec("la"), QUALITY_PREVIEW, 1, &raised),
        Err(RenderFailure::Cancelled)
    ));
    let calm = AtomicU32::new(0);
    match singer.render(
        &spec(fontelle_testsinger::FAIL_LYRIC),
        QUALITY_PREVIEW,
        1,
        &calm,
    ) {
        Err(RenderFailure::Failed(why)) => assert!(why.contains("fail"), "{why}"),
        other => panic!(
            "expected a failure, got {:?}",
            other.map(|s| s.samples.len())
        ),
    }
    // And the instance still sings afterwards.
    assert!(
        singer
            .render(&spec("la"), QUALITY_PREVIEW, 1, &calm)
            .is_ok()
    );
}

#[test]
fn singers_are_looked_for_in_fontelles_data_folder_and_the_environment() {
    let env = |vars: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    };
    // Linux and macOS: the XDG data folder, else ~/.local/share — the
    // settings' own rule (`Settings::data_dir_on`).
    let paths = singer_search_paths_from(
        &env(&[("HOME", "/home/ty"), ("XDG_DATA_HOME", "/data")]),
        false,
        &|_| false,
    );
    assert_eq!(paths, vec![PathBuf::from("/data/fontelle/singers")]);
    let paths = singer_search_paths_from(&env(&[("HOME", "/home/ty")]), false, &|_| false);
    assert_eq!(
        paths,
        vec![PathBuf::from("/home/ty/.local/share/fontelle/singers")]
    );
    // A relative XDG value is ignored, as the spec says.
    let paths = singer_search_paths_from(
        &env(&[("HOME", "/home/ty"), ("XDG_DATA_HOME", "rel")]),
        false,
        &|_| false,
    );
    assert_eq!(
        paths,
        vec![PathBuf::from("/home/ty/.local/share/fontelle/singers")]
    );
    // FONTELLE_SINGERS first, then the data folder.
    let list = std::env::join_paths(["/opt/a", "/opt/b"]).unwrap();
    let list: &'static str = Box::leak(list.into_string().unwrap().into_boxed_str());
    let vars: &'static [(&'static str, &'static str)] =
        Box::leak(Box::new([("HOME", "/home/ty"), ("FONTELLE_SINGERS", list)]));
    let paths = singer_search_paths_from(&env(vars), false, &|_| false);
    assert_eq!(
        paths,
        vec![
            PathBuf::from("/opt/a"),
            PathBuf::from("/opt/b"),
            PathBuf::from("/home/ty/.local/share/fontelle/singers"),
        ]
    );
    // Windows: %LOCALAPPDATA%\fontelle\singers.
    let paths = singer_search_paths_from(
        &env(&[("LOCALAPPDATA", "C:\\Users\\ty\\AppData\\Local")]),
        true,
        &|_| false,
    );
    assert_eq!(
        paths,
        vec![
            PathBuf::from("C:\\Users\\ty\\AppData\\Local")
                .join("fontelle")
                .join("singers")
        ]
    );
}

/// A word the engine did not know comes back marked as guessed, and the
/// keys a host does not know are ignored rather than refused.
#[test]
fn a_guessed_pronunciation_is_said_so() {
    let engines = engines();
    let mut singer = engines.engines()[0].instance().unwrap();
    let spec = PhraseSpec {
        singer: fontelle_testsinger::ALTO_ID.into(),
        language: "en".into(),
        notes: vec![la(0.0, 60.0, "la"), la(0.4, 62.0, "xylophone")],
    };
    let sung = singer
        .render(&spec, QUALITY_PREVIEW, 1, &AtomicU32::new(0))
        .unwrap();
    assert!(!sung.phonemes[0].guessed);
    assert!(sung.phonemes[1].guessed, "{:?}", sung.phonemes);
}

/// An engine with helper libraries lives in a folder of its own: only the
/// library named after the folder is opened, never the helpers beside it.
#[test]
fn an_engine_in_a_folder_of_its_own_is_loaded_and_its_helpers_are_not() {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let root = path.join(format!("fontelle-test-singer-dirs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let engine_dir = root.join("toy");
    std::fs::create_dir_all(&engine_dir).unwrap();
    std::fs::copy(
        path.join(library_name("fontelle_testsinger")),
        engine_dir.join(library_name("toy")),
    )
    .unwrap();
    // A helper that is not an engine: opened, it would be a failure.
    let helper = path.join(library_name("fontelle_testbridge"));
    if helper.exists() {
        std::fs::copy(&helper, engine_dir.join(library_name("onnxruntime_helper"))).unwrap();
    }
    // A folder whose library is not named after it is left alone too.
    let stray = root.join("stray");
    std::fs::create_dir_all(&stray).unwrap();
    if helper.exists() {
        std::fs::copy(&helper, stray.join(library_name("something_else"))).unwrap();
    }
    let engines = SingerEngines::load(&[root]);
    assert!(engines.failures.is_empty(), "{:?}", engines.failures);
    assert_eq!(engines.engines().len(), 1);
    assert_eq!(
        engines.engines()[0].name(),
        fontelle_testsinger::ENGINE_NAME
    );
}
