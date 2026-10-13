//! The backend over a folder of voicebanks, without real models.

mod common;

use std::collections::BTreeMap;

use common::{scratch, write, write_bank};
use fontelle_diffsinger::{DiffSinger, Options};
use fontelle_sing::{Backend, Lane, Phrase, PhraseNote, Quality, SingError};

fn phrase(voice: &str) -> Phrase {
    Phrase {
        voice: voice.into(),
        language: "en".into(),
        notes: vec![PhraseNote::new(0.0, 0.5, 60.0, "la")],
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
        start: 0.0,
        note_ids: Vec::new(),
    }
}

#[test]
fn voices_and_capabilities_come_from_the_folders() {
    let root = scratch("backend-voices");
    let (_bank, _deps) = write_bank(&root);
    // Dependencies sit beside the singers folder, where they are found.
    let singer = DiffSinger::new(Options::new(vec![root.join("singers")]));
    let voices = singer.voices();
    assert_eq!(voices.len(), 1);
    assert_eq!(voices[0].id, "Test Bank");
    assert!(
        singer.problems("Test Bank").is_empty(),
        "{:?}",
        singer.problems("Test Bank")
    );
    assert!(
        singer
            .capabilities("Test Bank")
            .lanes
            .contains(&Lane::Breathiness)
    );
    assert_eq!(singer.capabilities("nobody"), Default::default());
    assert!(singer.version().contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn singing_what_cannot_be_sung_fails_with_a_reason() {
    let root = scratch("backend-fails");
    let (_bank, _deps) = write_bank(&root);
    let singer = DiffSinger::new(Options::new(vec![root.join("singers")]));
    let mut session = singer.session().unwrap();
    let e = session
        .sing(&phrase("nobody"), Quality::Preview, 1, &|| false)
        .unwrap_err();
    assert!(
        matches!(e, SingError::Failed(ref w) if w.contains("nobody")),
        "{e:?}"
    );
    // The fixture's models are empty files: a clean failure, not a panic.
    let e = session
        .sing(&phrase("Test Bank"), Quality::Preview, 1, &|| false)
        .unwrap_err();
    assert!(matches!(e, SingError::Failed(_)), "{e:?}");
    assert_eq!(
        session.sing(&phrase("Test Bank"), Quality::Preview, 1, &|| true),
        Err(SingError::Cancelled)
    );
}

#[test]
fn a_bank_without_its_vocoder_is_listed_with_the_problem() {
    let root = scratch("backend-novocoder");
    let (_bank, deps) = write_bank(&root);
    std::fs::remove_dir_all(&deps).unwrap();
    let singer = DiffSinger::new(Options::new(vec![root.join("singers")]));
    assert_eq!(singer.voices().len(), 1);
    assert!(singer.problems("Test Bank")[0].contains("test_vocoder"));
    let e = singer
        .session()
        .unwrap()
        .sing(&phrase("Test Bank"), Quality::Final, 1, &|| false)
        .unwrap_err();
    assert!(matches!(e, SingError::Failed(ref w) if w.contains("test_vocoder")));
}

#[test]
fn rescanning_finds_new_banks_and_reports_broken_ones() {
    let root = scratch("backend-rescan");
    let singer = DiffSinger::new(Options::new(vec![root.join("singers")]));
    assert!(singer.voices().is_empty());
    write_bank(&root);
    write(
        &root.join("singers/broken/dsconfig.yaml"),
        "acoustic: missing.onnx\n",
    );
    singer.rescan();
    assert_eq!(singer.voices().len(), 1);
    let failed = singer.failed();
    assert_eq!(failed.len(), 1);
    assert!(failed[0].0.ends_with("broken"));
}

/// A bank names its folder and its vocoder's as what it is sung from, so
/// its loudness can be kept by their content; a voice it does not have
/// names nothing.
#[test]
fn a_bank_is_sung_from_its_folder_and_its_vocoders() {
    let root = scratch("backend-files");
    let (bank, deps) = write_bank(&root);
    let singer = DiffSinger::new(Options::new(vec![root.join("singers")]));
    let files = singer.voice_files("Test Bank");
    assert_eq!(files.len(), 2, "{files:?}");
    assert!(files.contains(&bank), "{files:?}");
    assert!(files.iter().any(|f| f.starts_with(&deps)), "{files:?}");
    assert!(singer.voice_files("nobody").is_empty());
}
