//! Singer engines as `fontelle_sing` backends, and ABI 2: an engine that
//! says what it honours, sings the whole phrase, and draws its own window
//! that reads the part and edits it through the host.
//!
//! `fontelle-testsinger` is both engines: copied under its own name it is a
//! version-1 engine (notes and lyrics); copied under a name with `abi2` in it
//! it is a version-2 one, with a lane, a setting and a note option of its
//! own and a window that paints a colour and edits the part when a note's
//! lyric asks it to (`!poke`, `!take`, `!play`, `!bad`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use fontelle_host::{EditorTake, SingerEditor, SingerEngine, SingerEngines};
use fontelle_sing::{
    Backend, CURVE_STEP, Curve, Lane, PartEdit, PartView, PhonemeEdit, Phrase, PhraseNote, Quality,
    SingError, Transport, ViewNote,
};

fn library_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    }
}

/// A singers folder with the test engine in it twice: as itself (ABI 1) and
/// as its ABI 2 self.
fn folder() -> PathBuf {
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let mut path = std::env::current_exe().expect("a test binary knows where it is");
            path.pop();
            path.pop();
            let built = path.join(library_name("fontelle_testsinger"));
            assert!(built.exists(), "build fontelle-testsinger first");
            let folder = path.join(format!("fontelle-test-singers-v2-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&folder);
            std::fs::copy(&built, folder.join(library_name("fontelle_testsinger"))).unwrap();
            std::fs::copy(
                &built,
                folder.join(library_name("fontelle_testsinger_abi2")),
            )
            .unwrap();
            folder
        })
        .clone()
}

fn engines() -> SingerEngines {
    SingerEngines::load(&[folder()])
}

fn v1(engines: &SingerEngines) -> Arc<SingerEngine> {
    engines
        .engine(fontelle_testsinger::ENGINE_NAME)
        .expect("the version-1 engine")
        .clone()
}

fn v2(engines: &SingerEngines) -> Arc<SingerEngine> {
    engines
        .engine(fontelle_testsinger::ENGINE2_NAME)
        .expect("the version-2 engine")
        .clone()
}

fn phrase(notes: Vec<PhraseNote>) -> Phrase {
    Phrase {
        voice: fontelle_testsinger::ALTO_ID.into(),
        language: "en".into(),
        notes,
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
    }
}

fn sing(backend: &dyn Backend, phrase: &Phrase) -> Result<fontelle_sing::Sung, SingError> {
    let mut session = backend.session().expect("a session");
    session.sing(phrase, Quality::Preview, 7, &|| false)
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt()
}

// ------------------------------------------------------------- loading

#[test]
fn both_versions_load_side_by_side_and_say_which_they_are() {
    let engines = engines();
    assert!(engines.failures.is_empty(), "{:?}", engines.failures);
    assert_eq!(engines.engines().len(), 2);
    assert_eq!(v1(&engines).abi_version(), 1);
    assert_eq!(v2(&engines).abi_version(), 2);
    assert!(!v1(&engines).has_editor());
    assert!(v2(&engines).has_editor());
}

#[test]
fn an_engine_is_a_backend_with_its_voices() {
    let engines = engines();
    let backend: Arc<dyn Backend> = v1(&engines);
    assert_eq!(backend.name(), fontelle_testsinger::ENGINE_NAME);
    assert_eq!(backend.version(), fontelle_testsinger::ENGINE_VERSION);
    let voices = backend.voices();
    assert_eq!(voices.len(), 2);
    assert_eq!(voices[0].id, fontelle_testsinger::ALTO_ID);
    assert_eq!(voices[0].languages, vec!["en", "ja"]);
    assert!(voices[1].research_only);
}

// --------------------------------------------------------- capabilities

#[test]
fn a_version_1_engine_honours_nothing_beyond_notes_and_words() {
    let engines = engines();
    let caps = v1(&engines).capabilities(fontelle_testsinger::ALTO_ID);
    assert!(caps.lanes.is_empty());
    assert!(!caps.vibrato && !caps.portamento && !caps.velocity && !caps.phoneme_timing);
    assert!(caps.extra_lanes.is_empty());
}

#[test]
fn a_version_2_engine_says_what_it_honours_and_what_it_adds() {
    let engines = engines();
    let caps = v2(&engines).capabilities(fontelle_testsinger::ALTO_ID);
    assert!(caps.lanes.contains(&Lane::Dynamics));
    assert!(caps.lanes.contains(&Lane::PitchBend));
    assert!(!caps.lanes.contains(&Lane::Gender), "it ignores gender");
    assert!(caps.vibrato && caps.phoneme_timing);
    assert!(caps.phonemes.iter().any(|p| p == "a"));
    assert_eq!(caps.extra_lanes[0].id, fontelle_testsinger::GROWL_LANE);
    assert_eq!(caps.settings[0].id, fontelle_testsinger::BRIGHTNESS_SETTING);
    assert_eq!(caps.note_options[0].id, fontelle_testsinger::COLOUR_OPTION);
    // A voice it does not have answers nothing.
    assert_eq!(
        v2(&engines).capabilities("nobody"),
        fontelle_sing::Capabilities::default()
    );
}

// ------------------------------------------------------------- singing

#[test]
fn a_version_1_engine_sings_a_phrase_as_notes_and_words() {
    let engines = engines();
    let mut a = PhraseNote::new(0.0, 0.4, 69.0, "la");
    // Hand-placed phonemes reach a version-1 engine the only way it reads
    // them: as a bracketed lyric.
    a.phonemes = Some(vec![
        PhonemeEdit {
            symbol: "s".into(),
            offset: None,
        },
        PhonemeEdit {
            symbol: "o".into(),
            offset: None,
        },
    ]);
    let sung = sing(v1(&engines).as_ref(), &phrase(vec![a])).unwrap();
    assert_eq!(sung.sample_rate, fontelle_testsinger::SAMPLE_RATE);
    assert!(sung.start < 0.0);
    let symbols: Vec<&str> = sung.phonemes[0]
        .phonemes
        .iter()
        .map(|p| p.symbol.as_str())
        .collect();
    assert_eq!(symbols, vec!["s", "o"]);
}

#[test]
fn a_version_2_engine_sings_the_whole_phrase() {
    let engines = engines();
    let backend = v2(&engines);
    let plain = phrase(vec![PhraseNote::new(0.0, 0.5, 69.0, "la")]);
    let flat = sing(backend.as_ref(), &plain).unwrap();
    // A bend lane a semitone up is sung a semitone up.
    let mut bent = plain.clone();
    bent.curves.insert(
        Lane::PitchBend,
        Curve {
            start: -0.1,
            step: CURVE_STEP,
            values: vec![100.0; 200],
        },
    );
    let up = sing(backend.as_ref(), &bent).unwrap();
    let mid = |s: &fontelle_sing::Sung| {
        let i = ((0.25 - s.start) / s.pitch_step) as usize;
        s.pitch[i]
    };
    let ratio = mid(&up) / mid(&flat);
    assert!((ratio - 2f32.powf(1.0 / 12.0)).abs() < 0.01, "{ratio}");
    // A dynamics lane far down is far quieter.
    let mut quiet = plain.clone();
    quiet.curves.insert(
        Lane::Dynamics,
        Curve {
            start: -0.1,
            step: CURVE_STEP,
            values: vec![-40.0; 200],
        },
    );
    let soft = sing(backend.as_ref(), &quiet).unwrap();
    assert!(rms(&soft.samples) < rms(&flat.samples) * 0.1);
    // Hand-placed phonemes with timings come back as sung, timings and all.
    let mut placed = plain.clone();
    placed.notes[0].phonemes = Some(vec![
        PhonemeEdit {
            symbol: "k".into(),
            offset: Some(-0.04),
        },
        PhonemeEdit {
            symbol: "a".into(),
            offset: Some(0.0),
        },
    ]);
    let sung = sing(backend.as_ref(), &placed).unwrap();
    assert_eq!(sung.phonemes[0].phonemes[0].symbol, "k");
    assert_eq!(sung.phonemes[0].phonemes[0].offset, Some(-0.04));
}

#[test]
fn a_version_2_render_fails_and_cancels_the_way_version_1_does() {
    let engines = engines();
    let backend = v2(&engines);
    let failing = phrase(vec![PhraseNote::new(
        0.0,
        0.4,
        60.0,
        fontelle_testsinger::FAIL_LYRIC,
    )]);
    match sing(backend.as_ref(), &failing) {
        Err(SingError::Failed(why)) => assert!(why.contains("fail"), "{why}"),
        other => panic!("{:?}", other.map(|s| s.samples.len())),
    }
    let mut session = backend.session().unwrap();
    let r = session.sing(
        &phrase(vec![PhraseNote::new(0.0, 0.4, 60.0, "la")]),
        Quality::Final,
        1,
        &|| true,
    );
    assert_eq!(r.err(), Some(SingError::Cancelled));
}

// -------------------------------------------------------------- editor

fn view(lyrics: &[&str]) -> PartView {
    PartView {
        track: "Voice".into(),
        voice: fontelle_testsinger::ALTO_ID.into(),
        language: "en".into(),
        notes: lyrics
            .iter()
            .enumerate()
            .map(|(i, l)| ViewNote {
                id: 100 + i as u64,
                note: PhraseNote::new(i as f64 * 0.5, 0.5, 60.0, *l),
                sung: Vec::new(),
            })
            .collect(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
        selection: Vec::new(),
        playhead: 0.0,
        playing: false,
    }
}

fn editor(engines: &SingerEngines) -> SingerEditor {
    // No window to draw in: the engine is given none, and still edits.
    SingerEditor::open(&v2(engines), 0).expect("an editor")
}

#[test]
fn a_version_1_engine_has_no_window_to_open() {
    let engines = engines();
    assert!(SingerEditor::open(&v1(&engines), 0).is_err());
}

#[test]
fn the_engines_window_opens_at_the_size_it_asks_for() {
    let engines = engines();
    let editor = editor(&engines);
    assert_eq!(editor.size(), fontelle_testsinger::EDITOR_SIZE);
}

#[test]
fn an_edit_from_the_window_arrives_as_one_batch() {
    let engines = engines();
    let mut editor = editor(&engines);
    editor.set_part(&view(&["la", fontelle_testsinger::POKE_LYRIC]));
    let asked = editor.tick();
    assert_eq!(asked.edits.len(), 1, "one batch: {:?}", asked.edits);
    let batch = &asked.edits[0];
    assert!(batch.contains(&PartEdit::Lyrics(vec![(101, "poked".into())])));
    assert!(batch.iter().any(|e| matches!(
        e,
        PartEdit::Settings(s) if s.get(fontelle_testsinger::BRIGHTNESS_SETTING) == Some(&0.5)
    )));
    assert!(batch.iter().any(|e| matches!(
        e,
        PartEdit::ExtraCurve { id, curve: Some(_) } if id == fontelle_testsinger::GROWL_LANE
    )));
    // Nothing more until the part changes.
    assert!(editor.tick().edits.is_empty());
}

#[test]
fn an_edit_naming_a_note_that_is_not_there_is_refused() {
    let engines = engines();
    let mut editor = editor(&engines);
    editor.set_part(&view(&[fontelle_testsinger::BAD_LYRIC]));
    let asked = editor.tick();
    // The engine heard "no" to its bad batch and said so with a good one.
    assert_eq!(
        asked.edits,
        vec![vec![PartEdit::Lyrics(vec![(100, "refused".into())])]]
    );
}

#[test]
fn the_window_moves_the_transport_and_reads_takes() {
    let engines = engines();
    let mut editor = editor(&engines);
    editor.set_part(&view(&[fontelle_testsinger::PLAY_LYRIC]));
    assert_eq!(editor.tick().transport, vec![Transport::Play]);
    editor.set_takes(vec![EditorTake {
        start: 0.0,
        end: 1.0,
        sample_rate: 24_000,
        take_start: -0.05,
        samples: vec![0.1; 1234].into(),
        pitch: vec![440.0; 10].into(),
        pitch_step: 0.01,
    }]);
    editor.set_part(&view(&[fontelle_testsinger::TAKE_LYRIC]));
    let asked = editor.tick();
    assert_eq!(
        asked.edits,
        vec![vec![PartEdit::Lyrics(vec![(100, "take 1234".into())])]]
    );
}
