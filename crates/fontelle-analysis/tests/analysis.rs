//! The whole analysis and its cache (plan §2.6, §3.10, §4 P1).

#![cfg(feature = "model")]

use fontelle_analysis::analysis::{Analysis, ENGINE_VERSION, Mode, analyse};
use fontelle_analysis::cache::{cache_key, load, store};
use fontelle_analysis::confidence::Clarity;
use fontelle_analysis::testsignals;
use fontelle_analysis::transcribe::basic_pitch::BasicPitch;
use fontelle_types::KeyScale;
use std::sync::OnceLock;

fn model() -> &'static BasicPitch {
    static MODEL: OnceLock<BasicPitch> = OnceLock::new();
    MODEL.get_or_init(|| BasicPitch::load().expect("the model loads"))
}

fn run(audio: &[f32], sample_rate: u32) -> Analysis {
    analyse(model(), audio, sample_rate).unwrap()
}

#[test]
fn a_sung_line_is_a_melody_with_its_notes() {
    let fixture = testsignals::vibrato_melody(44_100);
    let a = run(&fixture.samples, 44_100);
    assert_eq!(a.mode, Mode::Melody);
    assert_eq!(a.melody.len(), fixture.notes.len(), "{:?}", a.melody);
    assert_eq!(a.engine, ENGINE_VERSION);
    assert!(!a.guessed);
    assert_eq!(a.extraction.clarity, Clarity::Clear);
    // C D E G F E: C major (or its relative).
    let key = a.key.unwrap();
    assert!(
        key.key == KeyScale::new(0, "major") || key.relative == KeyScale::new(0, "major"),
        "{key:?}"
    );
}

#[test]
fn chords_are_chords() {
    let fixture = testsignals::melody_and_chords(44_100);
    let a = run(&fixture.samples, 44_100);
    assert_eq!(a.mode, Mode::Chords);
    assert!(a.melody.is_empty());
    assert_eq!(a.notes.len(), fixture.notes.len());
    let first = a.chords.iter().find(|c| c.chord.is_some()).unwrap();
    assert_eq!(first.chord.unwrap().label(), "C", "{:?}", a.chords);
}

#[test]
fn noise_gives_a_low_confidence_guess_not_nothing() {
    let a = run(&testsignals::noise(44_100, 4.0, 0.3), 44_100);
    assert!(!a.notes.is_empty(), "noise still gets a guess");
    assert!(a.key.is_some());
    assert_eq!(
        a.extraction.clarity,
        Clarity::RoughGuess,
        "{:?}",
        a.extraction
    );
    let drums = run(&testsignals::drum_loop(44_100), 44_100);
    assert!(!drums.notes.is_empty(), "a drum loop still gets a guess");
    assert_eq!(
        drums.extraction.clarity,
        Clarity::RoughGuess,
        "{:?}",
        drums.extraction
    );
    assert!(drums.guessed);
}

#[test]
fn silence_gives_no_notes() {
    let a = run(&vec![0.0; 3 * 44_100], 44_100);
    assert!(a.notes.is_empty());
    assert!(a.melody.is_empty());
    assert!(a.key.is_none());
    assert!(!a.guessed);
    assert!((a.duration - 3.0).abs() < 1e-9);
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fontelle-analysis-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn same_bytes_same_analysis_from_cache() {
    let dir = scratch("cache");
    let fixture = testsignals::c_major_triad(44_100);
    let key = cache_key(&fixture.samples, 44_100);
    assert_eq!(key, cache_key(&fixture.samples.clone(), 44_100));
    assert!(
        key.ends_with(ENGINE_VERSION)
            || key.contains(&ENGINE_VERSION.replace('+', "_"))
            || key.len() >= 64
    );
    assert!(load(&dir, &key).is_none());

    let a = run(&fixture.samples, 44_100);
    store(&dir, &key, &a).unwrap();
    assert_eq!(load(&dir, &key), Some(a.clone()));

    // One sample changed, or another rate: another key.
    let mut changed = fixture.samples.clone();
    changed[1000] += 1e-6;
    assert_ne!(cache_key(&changed, 44_100), key);
    assert_ne!(cache_key(&fixture.samples, 48_000), key);

    // A damaged file is a miss, not a crash; so is another engine's.
    let path = std::fs::read_dir(&dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&path, b"{ not json").unwrap();
    assert!(load(&dir, &key).is_none());
    let mut other = a;
    other.engine = "an-older-engine".into();
    store(&dir, &key, &other).unwrap();
    assert!(load(&dir, &key).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
