//! The Fontelle Singer and each installed singer engine as instruments of
//! their own — added from the instrument menu as different things, named
//! for what they are on the track and its clips — over one vocal part, so
//! changing a track from one to the other keeps its notes, lyrics,
//! expression and lanes and goes on singing, through the new one, in one
//! undo step.
//!
//! > *"i thought we agreed to split generic vocals and [the paid engine] into
//! > separate addable instruments, that can be changed between and it will
//! > still sing correctly, but actually using them is separate
//! > instruments"*
//!
//! The engines are `fontelle-testsinger` (its ABI 1 self and its ABI 2
//! self); the Fontelle Singer's runtime is a fake that hums whatever is in
//! the test's own voicebanks folder — never the machine's.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fontelle_app::Session;
use fontelle_model::{LaneCurve, LaneKey, VocalVoice};
use fontelle_sing::{
    Backend, Capabilities, Lane, Phrase, Quality, Session as SingSession, SingError, Sung,
    VoiceInfo,
};
use fontelle_types::{InstrumentKind, NoteId, PPQN};
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::{DocumentHost, FONTELLE_SINGER, StudioHost, VocalRenderState};
use fontelle_vocal::{BuiltinBackend, SingerKind as Kind};

use fontelle_testsinger::{ALTO_ID, ALTO_NAME, ENGINE_NAME, ENGINE2_NAME, GROWL_LANE};

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
    let path = std::env::temp_dir().join(format!(
        "fontelle-vocal-instruments-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// Both test engines, copied once.
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
            std::fs::copy(
                &built,
                folder.join(library_name("fontelle_testsinger_abi2")),
            )
            .unwrap();
            folder
        })
        .clone()
}

/// The Fontelle Singer's runtime for the test: each installed bank is a
/// voice (by its folder, singing Japanese), hummed with no phonemes said —
/// which is how a take is told apart from an engine's.
struct Banks(PathBuf);

impl Backend for Banks {
    fn name(&self) -> &str {
        "Test Banks"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        fontelle_vocal::voicebanks::list(&self.0)
            .into_iter()
            .map(|b| VoiceInfo {
                id: b.id.clone(),
                name: b.name.clone(),
                languages: vec!["ja".into()],
                lowest: 48,
                highest: 72,
                research_only: false,
                terms: None,
                image: None,
            })
            .collect()
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities {
            lanes: vec![Lane::Dynamics],
            ..Capabilities::default()
        }
    }
    fn session(&self) -> Result<Box<dyn SingSession>, String> {
        Ok(Box::new(Hum))
    }
}

struct Hum;

impl SingSession for Hum {
    fn sing(
        &mut self,
        phrase: &Phrase,
        _quality: Quality,
        _seed: u64,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        let end = phrase.notes.last().map_or(0.0, |n| n.end());
        Ok(Sung {
            sample_rate: 8_000,
            samples: vec![0.1; (end * 8_000.0) as usize + 1],
            ..Sung::default()
        })
    }
}

fn banks(root: &Path) -> Arc<dyn Backend> {
    Arc::new(Banks(root.to_path_buf()))
}

fn builtins() -> Vec<BuiltinBackend> {
    vec![BuiltinBackend {
        key: "fontelle-utau",
        kind: Kind::Utau,
        make: banks,
    }]
}

/// A bank the way Teto's package leaves it: its files a folder down, a
/// Shift-JIS `character.txt` naming it and its picture.
fn install_teto(voicebanks: &Path) -> PathBuf {
    let inner = voicebanks
        .join("KasaneTeto_English")
        .join("\u{91cd}\u{97f3}\u{30c6}\u{30c8}");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("oto.ini"), b"a.wav=- a,20,100,-200,50,30\n").unwrap();
    std::fs::write(inner.join("teto.bmp"), b"BM").unwrap();
    let text = encoding_rs::SHIFT_JIS
        .encode("name=\u{91cd}\u{97f3}\u{30c6}\u{30c8}\r\nimage=teto.bmp\r\n")
        .0
        .into_owned();
    std::fs::write(inner.join("character.txt"), text).unwrap();
    inner
}

fn session(name: &str) -> (Session, PathBuf) {
    let dir = scratch(name);
    let project = common::a_clip_project(4);
    let bundle = dir.join("Song.fontelle");
    fontelle_app::save_project(&project, &bundle).unwrap();
    let mut session = common::a_session_in(project, Some(bundle));
    session.set_singer_folders(vec![singer_folder()]);
    let voicebanks = dir.join("voicebanks");
    install_teto(&voicebanks);
    session.set_voicebanks(voicebanks, builtins());
    session.set_headless_singer_editors(true);
    (session, dir)
}

fn vocal_index(session: &Session) -> usize {
    (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .expect("a vocal channel")
}

fn draw(session: &mut Session, beat: i64, key: u8, lyric: &str) -> NoteId {
    let id = session.edit(RollEdit::Add {
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
            lyric: String::new(),
            voice: None,
        },
    })[0];
    session.set_lyrics(&[id], vec![lyric.into()]).unwrap();
    id
}

fn pump_until(session: &mut Session, what: &str, done: impl Fn(&mut Session) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(session) {
        assert!(Instant::now() < deadline, "never: {what}");
        session.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn all_final(session: &mut Session) -> bool {
    session.vocal_view().is_some_and(|v| {
        !v.phrases.is_empty() && v.phrases.iter().all(|p| p.state == VocalRenderState::Final)
    })
}

fn caption(session: &Session) -> String {
    session
        .clips()
        .iter()
        .map(|c| c.name.clone())
        .find(|n| n.contains('\u{b7}'))
        .expect("a sung clip, captioned")
}

#[test]
fn the_instruments_are_the_fontelle_singer_and_each_engine_by_name() {
    let (session, _) = session("list");
    let instruments = session.singer_instruments();
    assert_eq!(instruments[0].engine, None);
    assert_eq!(instruments[0].name, FONTELLE_SINGER);
    let engines: Vec<(Option<&str>, &str, bool)> = instruments[1..]
        .iter()
        .map(|i| (i.engine.as_deref(), i.name.as_str(), i.has_editor))
        .collect();
    assert!(engines.contains(&(Some(ENGINE_NAME), ENGINE_NAME, false)));
    assert!(engines.contains(&(Some(ENGINE2_NAME), ENGINE2_NAME, true)));
}

#[test]
fn the_fontelle_singer_is_added_with_a_voicebank_and_named_for_itself() {
    let (mut session, _) = session("add-fontelle");
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    assert!(
        session.channels()[index].name.starts_with(FONTELLE_SINGER),
        "{}",
        session.channels()[index].name
    );
    assert_eq!(session.vocal_instrument(index), Some(None));
    let voice = session.vocal_voice(index).expect("a voicebank sings it");
    assert_eq!(voice.engine, "fontelle-utau");
    assert_eq!(voice.singer, "KasaneTeto_English");
    // Its words on the arrangement say which instrument sings them.
    draw(&mut session, 0, 64, "la");
    let caption = caption(&session);
    assert!(
        caption.starts_with(&format!("{FONTELLE_SINGER} \u{b7} \u{91cd}\u{97f3}")),
        "{caption}"
    );
    assert!(caption.ends_with("la"), "{caption}");
}

#[test]
fn an_engine_is_added_as_its_own_instrument_with_its_own_singer() {
    let (mut session, _) = session("add-engine");
    session.add_singer_channel(Some(ENGINE2_NAME)).unwrap();
    let index = vocal_index(&session);
    assert!(
        session.channels()[index].name.starts_with(ENGINE2_NAME),
        "{}",
        session.channels()[index].name
    );
    assert_eq!(
        session.vocal_instrument(index),
        Some(Some(ENGINE2_NAME.to_string()))
    );
    assert_eq!(
        session.vocal_voice(index),
        Some(VocalVoice::new(ENGINE2_NAME, ALTO_ID, "en"))
    );
    draw(&mut session, 0, 64, "la");
    let caption = caption(&session);
    assert!(
        caption.starts_with(&format!("{ENGINE2_NAME} \u{b7} {ALTO_NAME} \u{b7} ")),
        "{caption}"
    );
    // Its window is offered from the track, by the instrument's name.
    let view = session.vocal_view().unwrap();
    assert_eq!(view.editor.as_deref(), Some(ENGINE2_NAME));
}

/// The change Ty asked for: the part goes on singing through the other
/// instrument, every note, word and lane kept, and Ctrl+Z puts it back.
#[test]
fn changing_instrument_keeps_the_part_and_sings_it_through_the_new_one() {
    let (mut session, _) = session("switch");
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    let a = draw(&mut session, 0, 64, "la");
    let b = draw(&mut session, 1, 67, "li");
    let swell = LaneCurve::from_points(vec![(0, 0.2), (PPQN * 2, 0.9)]);
    session.set_vocal_lane(LaneKey::Standard(Lane::Dynamics), Some(swell.clone()));
    session.end_vocal_gesture();
    pump_until(&mut session, "the voicebank sang", all_final);
    assert!(session.vocal_view().unwrap().phonemes.is_empty(), "hummed");

    let before = session.undo_depth();
    session.set_singer_instrument(index, Some(ENGINE_NAME));
    assert_eq!(session.undo_depth(), before + 1, "one undo step");
    assert_eq!(
        session.vocal_instrument(index),
        Some(Some(ENGINE_NAME.to_string()))
    );
    // The part's language goes with it: the engine's singer who sings it.
    assert_eq!(
        session.vocal_voice(index),
        Some(VocalVoice::new(ENGINE_NAME, ALTO_ID, "ja"))
    );
    assert_eq!(session.notes()[a].lyric, "la");
    assert_eq!(session.notes()[b].lyric, "li");
    let view = session.vocal_view().unwrap();
    assert_eq!(view.backend, ENGINE_NAME);
    assert_eq!(
        view.part.curve(&LaneKey::Standard(Lane::Dynamics)),
        Some(&swell)
    );
    pump_until(&mut session, "the engine sang the same part", |s| {
        all_final(s) && s.vocal_view().is_some_and(|v| v.phonemes.len() == 2)
    });

    // On to the ABI 2 engine: its own lanes are offered on the same part.
    session.set_singer_instrument(index, Some(ENGINE2_NAME));
    let view = session.vocal_view().unwrap();
    assert_eq!(view.backend, ENGINE2_NAME);
    assert_eq!(view.capabilities.extra_lanes[0].id, GROWL_LANE);
    pump_until(&mut session, "the second engine sang it", |s| {
        all_final(s) && s.vocal_view().is_some_and(|v| v.phonemes.len() == 2)
    });

    // Back to the Fontelle Singer: a voicebank again, still singing.
    session.set_singer_instrument(index, None);
    assert_eq!(session.vocal_instrument(index), Some(None));
    assert_eq!(session.vocal_voice(index).unwrap().engine, "fontelle-utau");
    pump_until(&mut session, "hummed again", |s| {
        all_final(s) && s.vocal_view().is_some_and(|v| v.phonemes.is_empty())
    });

    // Each change one step back.
    session.undo();
    assert_eq!(session.vocal_voice(index).unwrap().engine, ENGINE2_NAME);
    session.undo();
    assert_eq!(session.vocal_voice(index).unwrap().engine, ENGINE_NAME);
    session.undo();
    assert_eq!(session.vocal_instrument(index), Some(None));
    assert_eq!(session.notes()[a].lyric, "la");
    // Choosing the instrument it already is changes nothing.
    let before = session.undo_depth();
    session.set_singer_instrument(index, None);
    assert_eq!(session.undo_depth(), before);
}

/// Another instrument made a vocal track keeps whatever it had, and an
/// engine with nobody chosen is still that engine's instrument.
#[test]
fn an_instrument_turned_into_an_engine_and_nobody_chosen_stays_that_engine() {
    let (mut session, _) = session("nobody");
    session.add_channel_of(InstrumentKind::Osc3).unwrap();
    let index = (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Osc3))
        .unwrap();
    session.set_singer_instrument(index, Some(ENGINE2_NAME));
    assert_eq!(session.channel_kind(index), Some(InstrumentKind::Vocal));
    assert_eq!(
        session.vocal_instrument(index),
        Some(Some(ENGINE2_NAME.to_string()))
    );
    session.undo();
    assert_eq!(session.channel_kind(index), Some(InstrumentKind::Osc3));
    session.redo();
    // "No singer" on an engine's track keeps the engine.
    session.set_singer(index, Some(VocalVoice::new(ENGINE2_NAME, "", "en")));
    assert_eq!(
        session.vocal_instrument(index),
        Some(Some(ENGINE2_NAME.to_string()))
    );
    session.select_channel(index);
    let view = session.vocal_view().unwrap();
    assert!(
        view.message
            .as_deref()
            .is_some_and(|m| m.contains("Choose a singer")),
        "{:?}",
        view.message
    );
}

/// A song saved with each instrument opens as each — and an old song's
/// vocal track opens as whichever its voice names.
#[test]
fn a_saved_song_opens_with_each_track_the_instrument_it_was() {
    let (mut session, dir) = session("round-trip");
    session.add_singer_channel(None).unwrap();
    session.add_singer_channel(Some(ENGINE2_NAME)).unwrap();
    let path = dir.join("Again.fontelle");
    fontelle_app::save_project(session.project(), &path).unwrap();
    let opened = fontelle_app::open_project(&path).unwrap().project;
    let mut again = common::a_session_in(opened, Some(path));
    again.set_singer_folders(vec![singer_folder()]);
    again.set_voicebanks(dir.join("voicebanks"), builtins());
    let vocal: Vec<Option<Option<String>>> = (0..again.channels().len())
        .filter(|i| again.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .map(|i| again.vocal_instrument(i))
        .collect();
    assert_eq!(
        vocal,
        vec![Some(None), Some(Some(ENGINE2_NAME.to_string()))]
    );
    // A track with no voice at all (an old song) is the Fontelle Singer's.
    let index = vocal_index(&again);
    again.set_singer(index, None);
    assert_eq!(again.vocal_instrument(index), Some(None));
}

/// The picker's card and the Installed page's row show one bank alike:
/// the name its `character.txt` gives, its folder under it, and the
/// picture from the folder its files are in.
#[test]
fn a_banks_card_and_its_installed_row_show_the_same_name_folder_and_face() {
    let (session, dir) = session("faces");
    let inner = dir
        .join("voicebanks")
        .join("KasaneTeto_English")
        .join("\u{91cd}\u{97f3}\u{30c6}\u{30c8}");
    let card = session
        .singers()
        .into_iter()
        .find(|s| s.kind.is_voicebank())
        .expect("the bank is a singer");
    let row = session
        .voicebanks()
        .into_iter()
        .find(|r| r.id == "KasaneTeto_English")
        .expect("the bank is installed");
    let face = inner.join("teto.bmp").display().to_string();
    assert_eq!(card.image.as_deref(), Some(face.as_str()));
    assert_eq!(row.portrait.as_deref(), Some(face.as_str()));
    assert_eq!(card.name, "\u{91cd}\u{97f3}\u{30c6}\u{30c8}");
    assert_eq!(row.name, card.name);
    assert_eq!(card.folder.as_deref(), Some("KasaneTeto_English"));
}
