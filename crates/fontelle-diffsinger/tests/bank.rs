//! Loading a voicebank folder and what it says it can do.

mod common;

use common::{scratch, write, write_bank};
use fontelle_diffsinger::bank::{
    Bank, LANE_EXPRESSIVENESS, LANE_SPEAKER_PREFIX, OPTION_SPEAKER, SETTING_NATURAL_PITCH,
    SETTING_STEPS, find_banks, find_vocoder,
};
use fontelle_sing::Lane;

#[test]
fn a_bank_loads_its_models_folders_and_vocoder() {
    let root = scratch("loads");
    let (dir, deps) = write_bank(&root);
    let bank = Bank::load(&dir, std::slice::from_ref(&deps)).unwrap();
    assert_eq!(bank.id, "Test Bank");
    assert!(bank.dur.is_some() && bank.pitch.is_some() && bank.variance.is_some());
    assert!(bank.problems.is_empty(), "{:?}", bank.problems);
    let vocoder = bank.vocoder.as_ref().unwrap();
    assert_eq!(vocoder.config.model, "v.onnx");
    assert!(
        vocoder
            .model_path()
            .ends_with("installed under another name/v.onnx")
    );
    // Languages: the phonemizer's first, then the rest by id.
    assert_eq!(bank.languages, ["ja", "en"]);
    assert_eq!(bank.language_for("en-US"), "en");
    assert_eq!(bank.language_for("fr"), "ja");
    // Speakers from the voice colours.
    assert_eq!(bank.speakers.len(), 2);
    assert_eq!(bank.speakers[0].label, "Soft");
    assert_eq!(bank.speakers[1].id, "soprano");
    assert!(bank.terms.as_deref().unwrap().contains("Free for any use"));
}

#[test]
fn a_bank_says_what_it_honours() {
    let root = scratch("caps");
    let (dir, deps) = write_bank(&root);
    let bank = Bank::load(&dir, &[deps]).unwrap();
    let c = bank.capabilities();
    for lane in [
        Lane::PitchBend,
        Lane::Dynamics,
        Lane::Breathiness,
        Lane::Voicing,
        Lane::Gender,
    ] {
        assert!(c.lanes.contains(&lane), "{lane:?}");
    }
    assert!(
        !c.lanes.contains(&Lane::Tension),
        "the acoustic model takes no tension"
    );
    assert!(c.vibrato && c.portamento && c.velocity && c.phoneme_timing);
    assert!(c.phonemes.contains(&"en/hh".to_string()));
    assert!(!c.phonemes.contains(&"<PAD>".to_string()));
    let lanes: Vec<&str> = c.extra_lanes.iter().map(|l| l.id.as_str()).collect();
    assert!(lanes.contains(&LANE_EXPRESSIVENESS));
    assert!(lanes.contains(&format!("{LANE_SPEAKER_PREFIX}alto").as_str()));
    assert!(lanes.contains(&format!("{LANE_SPEAKER_PREFIX}soprano").as_str()));
    let speaker = c
        .note_options
        .iter()
        .find(|o| o.id == OPTION_SPEAKER)
        .unwrap();
    assert_eq!(speaker.choices[0], ("alto".to_string(), "Soft".to_string()));
    let settings: Vec<&str> = c.settings.iter().map(|s| s.id.as_str()).collect();
    assert!(settings.contains(&SETTING_STEPS));
    assert!(settings.contains(&SETTING_NATURAL_PITCH));
    let info = bank.voice_info();
    assert_eq!(info.id, "Test Bank");
    assert_eq!(info.name, "Test Singer");
    assert_eq!(info.languages[0], "ja");
    assert!(info.image.unwrap().ends_with("icon.png"));
    assert!(!info.research_only);
    assert!(info.lowest < info.highest);
}

#[test]
fn dictionaries_are_found_per_language_and_kept() {
    let root = scratch("dicts");
    let (dir, deps) = write_bank(&root);
    let bank = Bank::load(&dir, &[deps]).unwrap();
    assert!(
        bank.dictionary_path("en")
            .unwrap()
            .ends_with("dsdur/dsdict-en.yaml")
    );
    assert!(
        bank.dictionary_path("ja")
            .unwrap()
            .ends_with("dsdur/dsdict.yaml")
    );
    let en = bank.dictionary("en").unwrap();
    assert!(en.query("hello").is_some());
    let again = bank.dictionary("en").unwrap();
    assert!(std::sync::Arc::ptr_eq(&en, &again));
    assert!(bank.dictionary("ja").unwrap().query("ka").is_some());
}

#[test]
fn speakers_resolve_by_suffix_in_each_folder() {
    let root = scratch("speakers");
    let (dir, deps) = write_bank(&root);
    let bank = Bank::load(&dir, &[deps]).unwrap();
    // The root lists alto first; the duration folder lists soprano first.
    assert_eq!(bank.acoustic.speaker_index("alto"), 0);
    let dur = bank.dur.as_ref().unwrap();
    assert_eq!(dur.speaker_index("alto"), 1);
    assert_eq!(dur.speaker_index("emb/soprano"), 0);
    assert_eq!(dur.speaker_index("nobody"), 0, "the first, when unknown");
    let e = dur.embeddings().unwrap();
    assert_eq!(e, vec![vec![0.0, 1.0, 0.0, 0.0], vec![1.0, 0.0, 0.0, 0.0]]);
}

#[test]
fn a_missing_vocoder_or_model_folder_is_a_problem_not_a_failure() {
    let root = scratch("missing");
    let (dir, _deps) = write_bank(&root);
    std::fs::remove_file(dir.join("dspitch/p.onnx")).unwrap();
    let bank = Bank::load(&dir, &[]).unwrap();
    let why = bank.vocoder.as_ref().unwrap_err();
    assert!(why.contains("test_vocoder"));
    // Says what to do with it: the .oudep dropped on the window.
    assert!(why.contains("drop"), "{why}");
    assert!(bank.pitch.is_none());
    assert!(!bank.problems.is_empty());
    // A bank without its acoustic model is not a bank.
    std::fs::remove_file(dir.join("a.onnx")).unwrap();
    assert!(Bank::load(&dir, &[]).is_err());
}

#[test]
fn banks_are_found_below_the_folders_given() {
    let root = scratch("find");
    let (dir, deps) = write_bank(&root);
    // Not banks: a UTAU bank, and a folder with an unrelated dsconfig.
    write(&root.join("singers/utau/oto.ini"), "a.wav=a,0,0,0,0,0\n");
    write(&root.join("singers/odd/dsconfig.yaml"), "phonemes: x.txt\n");
    let found = find_banks(&[root.join("singers")]);
    assert_eq!(found, vec![dir.clone()]);
    // A bank's own vocoder wins over a dependency.
    write(
        &dir.join("dsvocoder/vocoder.yaml"),
        "name: own\nmodel: own.onnx\n",
    );
    write(&dir.join("dsvocoder/own.onnx"), "");
    let v = find_vocoder("test_vocoder", &dir, std::slice::from_ref(&deps)).unwrap();
    assert_eq!(v.config.model, "own.onnx");
    std::fs::remove_dir_all(dir.join("dsvocoder")).unwrap();
    let v = find_vocoder("test_vocoder", &dir, &[deps]).unwrap();
    assert_eq!(v.config.model, "v.onnx");
}
