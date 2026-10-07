//! The vocal track in the studio: a channel that sings its notes' lyrics
//! through an installed singer engine — added from the instrument menu,
//! given a singer, written in the roll, heard in time and exported.
//!
//! The engine is `fontelle-testsinger`, loaded as a library from a singers
//! folder exactly as a real one would be; with no folder, the track still
//! takes notes and lyrics and says why it is silent.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fontelle_app::Session;
use fontelle_model::VocalVoice;
use fontelle_types::{InstrumentKind, NoteId, PPQN};
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::{DocumentHost, StudioHost, VocalRenderState};

fn library_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    }
}

fn scratch(name: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("fontelle-vocal-app-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn singer_folder() -> PathBuf {
    static FOLDER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOLDER
        .get_or_init(|| {
            let mut path = std::env::current_exe().unwrap();
            path.pop();
            path.pop();
            let built = path.join(library_name("fontelle_testsinger"));
            assert!(built.exists(), "run `cargo build -p fontelle-testsinger`");
            let folder = scratch("singers");
            std::fs::copy(&built, folder.join(library_name("fontelle_testsinger"))).unwrap();
            folder
        })
        .clone()
}

/// A saved session (an export goes inside the bundle) with a vocal track
/// added from the instrument menu and selected.
fn vocal_session(name: &str, engines: bool) -> (Session, PathBuf) {
    let dir = scratch(name);
    let bundle = dir.join("Song.fontelle");
    let project = common::a_clip_project(4);
    fontelle_app::save_project(&project, &bundle).unwrap();
    let mut session = common::a_session_in(project, Some(bundle.clone()));
    session.set_singer_folders(if engines {
        vec![singer_folder()]
    } else {
        vec![dir.join("no-singers")]
    });
    // Never the machine's own voicebanks: an installed bank would be the
    // first singer and change what these tests hear.
    session.set_voicebanks(dir.join("no-voicebanks"), Vec::new());
    session.add_channel_of(InstrumentKind::Vocal).unwrap();
    (session, bundle)
}

fn vocal_index(session: &Session) -> usize {
    (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .expect("a vocal channel")
}

fn draw(session: &mut Session, beat: i64, key: u8) -> NoteId {
    session.edit(RollEdit::Add {
        note: fontelle_model::Note {
            start: beat * PPQN,
            length: PPQN,
            key,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            path: Vec::new(),
            channel: None,
            // What the roll's template carried from the last note clicked:
            // a drawn note sings the language's syllable, not that.
            lyric: "hel-".into(),
            voice: None,
        },
    })[0]
}

fn lyric(session: &Session, id: NoteId) -> String {
    session.notes()[id].lyric.clone()
}

/// Pumps the session as the window would until `done`.
fn pump_until(session: &mut Session, done: impl Fn(&Session) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(session) {
        assert!(Instant::now() < deadline, "the vocal track never settled");
        session.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_vocal_track_arrives_with_the_first_singer_in_its_own_language() {
    let (session, _) = vocal_session("arrives", true);
    let index = vocal_index(&session);
    assert_eq!(
        session.vocal_voice(index),
        Some(VocalVoice::new(
            fontelle_testsinger::ENGINE_NAME,
            fontelle_testsinger::ALTO_ID,
            "en"
        ))
    );
    let singers = session.singers();
    assert_eq!(singers.len(), 2);
    assert_eq!(singers[0].name, fontelle_testsinger::ALTO_NAME);
    assert_eq!(singers[0].languages, vec!["en", "ja"]);
    assert!(!singers[0].research_only);
    assert!(singers[1].research_only, "the picker can badge it");
}

#[test]
fn choosing_a_singer_and_a_language_is_one_undo() {
    let (mut session, _) = vocal_session("choose", true);
    let index = vocal_index(&session);
    let ja = VocalVoice::new(
        fontelle_testsinger::ENGINE_NAME,
        fontelle_testsinger::RESEARCH_ID,
        "ja",
    );
    session.set_singer(index, Some(ja.clone()));
    assert_eq!(session.vocal_voice(index), Some(ja));
    session.undo();
    assert_eq!(
        session.vocal_voice(index).map(|v| v.singer),
        Some(fontelle_testsinger::ALTO_ID.to_string())
    );
}

#[test]
fn a_drawn_note_sings_the_languages_syllable() {
    let (mut session, _) = vocal_session("syllable", true);
    let id = draw(&mut session, 0, 60);
    assert_eq!(lyric(&session, id), "la");
    let index = vocal_index(&session);
    session.set_singer(
        index,
        Some(VocalVoice::new(
            fontelle_testsinger::ENGINE_NAME,
            fontelle_testsinger::ALTO_ID,
            "ja",
        )),
    );
    let id = draw(&mut session, 2, 62);
    assert_eq!(lyric(&session, id), "ら");
}

#[test]
fn a_typed_line_spreads_over_the_selected_notes_in_one_undo() {
    let (mut session, _) = vocal_session("line", true);
    // Drawn out of order: the line goes in time order, not selection order.
    let c = draw(&mut session, 2, 64);
    let a = draw(&mut session, 0, 60);
    let b = draw(&mut session, 1, 62);
    let said = session.set_lyric_line(&[c, a, b], "a-ma-zing").unwrap();
    assert!(!said.is_empty());
    assert_eq!(lyric(&session, a), "a-");
    assert_eq!(lyric(&session, b), "ma-");
    assert_eq!(lyric(&session, c), "zing");
    session.undo();
    assert_eq!(lyric(&session, a), "la");
    assert_eq!(lyric(&session, c), "la");
    // One note's own word, typed in its field.
    session.set_lyrics(&[b], vec!["oh".into()]).unwrap();
    assert_eq!(lyric(&session, b), "oh");
}

#[test]
fn the_roll_sees_each_phrase_sung_with_its_phonemes_and_pitch() {
    let (mut session, _) = vocal_session("view", true);
    let a = draw(&mut session, 0, 69);
    draw(&mut session, 1, 71);
    // A rest of two beats: a second phrase.
    draw(&mut session, 4, 72);
    let first = session.vocal_view().expect("a vocal channel is selected");
    assert_eq!(first.phrases.len(), 2);
    assert!(first.caption.contains(fontelle_testsinger::ALTO_NAME));
    // The arrangement's block says who sings and the first words.
    let index = vocal_index(&session);
    assert!(
        session.channels()[index].has_instrument,
        "a singer is its instrument"
    );
    let caption = session
        .clips()
        .iter()
        .map(|c| c.name.clone())
        .find(|n| n.contains(fontelle_testsinger::ALTO_NAME))
        .expect("the open clip is captioned with its singer");
    assert!(caption.contains("la"), "{caption}");
    pump_until(&mut session, |s| {
        s.vocal_view()
            .is_some_and(|v| v.phrases.iter().all(|p| p.state == VocalRenderState::Final))
    });
    let view = session.vocal_view().unwrap();
    assert_eq!(view.phrases[0].start, 0);
    assert_eq!(view.phrases[0].end, 2 * PPQN);
    assert_eq!(view.phrases[1].start, 4 * PPQN);
    let phonemes = view
        .phonemes
        .iter()
        .find(|(id, _)| *id == a)
        .map(|(_, p)| p.clone());
    assert_eq!(phonemes.as_deref(), Some("l a"));
    // The pitch it sang, as keys over the notes: A4 over the first.
    let line = &view.pitch[0];
    assert!(!line.is_empty());
    let over_a = line
        .iter()
        .find(|(t, _)| *t > PPQN / 4 && *t < PPQN / 2)
        .expect("a point over the first note");
    assert!((over_a.1 - 69.0).abs() < 0.1, "{}", over_a.1);
}

#[test]
fn with_no_engine_the_track_still_takes_notes_and_says_why_it_is_silent() {
    let (mut session, _) = vocal_session("none", false);
    let index = vocal_index(&session);
    assert_eq!(session.vocal_voice(index), None);
    assert!(session.singers().is_empty());
    let id = draw(&mut session, 0, 60);
    assert_eq!(lyric(&session, id), "la");
    let view = session.vocal_view().unwrap();
    assert!(
        view.message
            .as_deref()
            .is_some_and(|m| m.contains("No singer engine")),
        "{:?}",
        view.message
    );
}

fn read_wav(path: &Path) -> Vec<f32> {
    let decoded = fontelle_assets::import_audio(path).expect("a WAV");
    let channels = usize::from(decoded.channels.max(1));
    decoded
        .samples
        .chunks(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}

#[test]
fn an_export_waits_for_the_final_takes_and_sings() {
    let (mut session, bundle) = vocal_session("export", true);
    // Mute the clip's own piano: what is heard is the voice.
    for i in 0..session.channels().len() {
        if session.channel_kind(i) != Some(InstrumentKind::Vocal) {
            session.toggle_mute(i);
        }
    }
    draw(&mut session, 2, 69);
    draw(&mut session, 3, 69);
    // Straight away, before anything has been sung.
    session.export_wav().expect("exports");
    let audio = read_wav(&bundle.join("renders").join("Song.wav"));
    let sr = 48_000.0;
    let quiet: f32 = audio[..(0.8 * sr) as usize].iter().map(|s| s.abs()).sum();
    assert!(quiet < 1.0, "silence before the phrase: {quiet}");
    let sung: f32 = audio[(1.1 * sr) as usize..(1.9 * sr) as usize]
        .iter()
        .map(|s| s.abs())
        .sum();
    assert!(sung > 200.0, "the phrase is in the export: {sung}");
}

/// A word the engine did not know is marked on its note, so the writer can
/// give its phonemes in brackets.
#[test]
fn a_guessed_word_is_marked_on_its_note() {
    let (mut session, _) = vocal_session("guessed", true);
    let known = draw(&mut session, 0, 60);
    let odd = draw(&mut session, 1, 62);
    session
        .set_lyrics(&[odd], vec!["xylophone".into()])
        .unwrap();
    pump_until(&mut session, |s| {
        s.vocal_view()
            .is_some_and(|v| v.phrases.iter().all(|p| p.state == VocalRenderState::Final))
    });
    let view = session.vocal_view().unwrap();
    assert!(view.guessed.contains(&odd), "{:?}", view.guessed);
    assert!(!view.guessed.contains(&known));
}
