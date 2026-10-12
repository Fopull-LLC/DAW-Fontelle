//! What a silent vocal track says, and that it says what to do: a first
//! Fontelle Singer track with no voicebank installed, and a song opened on
//! a computer without its singer. Found walking the singer as a new user:
//! the first said "No singer engine is installed — put one in <a folder>"
//! (advice for paid engines, not for the free voicebanks), and the second
//! named the bank by its internal id, sent every phrase to fail one by
//! one, and pointed nowhere.

mod common;

use std::path::Path;
use std::sync::Arc;

use fontelle_app::Session;
use fontelle_model::{ClipSource, VocalVoice};
use fontelle_sing::{
    Backend, Capabilities, Phrase, Quality, Session as SingSession, SingError, Sung, VoiceInfo,
};
use fontelle_types::{InstrumentKind, PPQN};
use fontelle_ui::document::{DocumentHost, StudioHost, VocalRenderState};
use fontelle_vocal::{BuiltinBackend, SingerKind};

fn scratch(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-singer-messages-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// A built-in runtime with the banks it is made with.
struct Runtime(Vec<VoiceInfo>);

impl Backend for Runtime {
    fn name(&self) -> &str {
        "Runtime"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        self.0.clone()
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn SingSession>, String> {
        Ok(Box::new(Silent))
    }
}

struct Silent;

impl SingSession for Silent {
    fn sing(
        &mut self,
        phrase: &Phrase,
        _quality: Quality,
        _seed: u64,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        Err(SingError::Failed(format!(
            "no UTAU voicebank \"{}\"",
            phrase.voice
        )))
    }
}

fn no_banks(_: &Path) -> Arc<dyn Backend> {
    Arc::new(Runtime(Vec::new()))
}

fn one_bank(_: &Path) -> Arc<dyn Backend> {
    Arc::new(Runtime(vec![VoiceInfo {
        id: "utau/Another".into(),
        name: "Another".into(),
        languages: vec!["ja".into()],
        lowest: 48,
        highest: 72,
        research_only: false,
        terms: None,
        image: None,
    }]))
}

fn studio(name: &str, make: fn(&Path) -> Arc<dyn Backend>) -> Session {
    let dir = scratch(name);
    // A saved song: its takes are kept in it, not in the cache every test
    // shares (a take sung by an earlier run would be found there).
    let project = common::a_clip_project(4);
    let bundle = dir.join("Song.fontelle");
    fontelle_app::save_project(&project, &bundle).unwrap();
    let mut session = common::a_session_in(project, Some(bundle));
    session.set_singer_folders(vec![dir.join("no-engines")]);
    session.set_voicebanks(
        dir.join("voicebanks"),
        vec![BuiltinBackend {
            key: "fontelle-utau",
            kind: SingerKind::Utau,
            make,
        }],
    );
    session
}

fn vocal_index(session: &Session) -> usize {
    (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .unwrap()
}

#[test]
fn a_first_fontelle_singer_track_with_no_voicebank_says_where_to_find_one() {
    let mut session = studio("first", no_banks);
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    session.select_channel(index);
    let message = session.vocal_view().unwrap().message.unwrap_or_default();
    assert!(message.contains("Find singers"), "{message}");
    assert!(!message.contains("engine"), "{message}");
    let view = session.singer_panel().unwrap();
    let panel = view.message.clone().unwrap_or_default();
    assert!(panel.contains("Find singers"), "{panel}");
    assert!(view.find_first, "the stage offers Find singers");
}

#[test]
fn a_song_whose_voicebank_is_not_here_names_it_and_where_to_get_it() {
    let mut session = studio("missing", one_bank);
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    session.select_channel(index);
    // The song came from a computer with Teto's bank.
    session.set_singer(
        index,
        Some(VocalVoice::new(
            "fontelle-utau",
            "utau/KasaneTeto_tougou_110401",
            "ja",
        )),
    );
    let note = fontelle_vocal_files::vocal_note(0, PPQN, 60, "さ");
    session.edit(fontelle_ui::canvas::RollEdit::Add { note });
    for _ in 0..60 {
        session.pump();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let view = session.vocal_view().unwrap();
    let message = view.message.clone().unwrap_or_default();
    assert!(message.contains("KasaneTeto_tougou_110401"), "{message}");
    assert!(!message.contains("utau/"), "not the internal id: {message}");
    assert!(message.contains("Find singers"), "{message}");
    // Nothing was sent to fail: the phrase waits for its singer.
    assert!(
        view.phrases
            .iter()
            .all(|p| p.state != VocalRenderState::Preview),
        "{:?}",
        view.phrases
    );
    assert!(!message.contains("could not be sung"), "{message}");
    // The stage names the bank, offers Find singers, and the controls are
    // kept for it — not "the Fontelle Singer doesn't use vibrato".
    let view = session.singer_panel().unwrap();
    assert_eq!(view.missing.as_deref(), Some("KasaneTeto_tougou_110401"));
    assert!(view.find_first);
    let panel = view.message.clone().unwrap_or_default();
    assert!(!panel.contains("utau/"), "{panel}");
    assert!(panel.contains("Find singers"), "{panel}");
    assert!(
        view.greyed
            .iter()
            .all(|(_, why)| !why.contains("doesn't use")),
        "{:?}",
        view.greyed
    );
    // The part is all still there.
    let notes = session
        .project()
        .clips
        .values()
        .filter_map(|c| match &c.source {
            ClipSource::Notes(d) => Some(d.notes.len()),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(notes, 1);
}

/// A DiffSinger bank whose vocoder is not installed: it fails, saying so.
struct Needy(std::path::PathBuf);

impl Backend for Needy {
    fn name(&self) -> &str {
        "Needy"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        one_bank(&self.0).voices()
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn SingSession>, String> {
        Ok(Box::new(NeedySession(self.0.clone())))
    }
}

struct NeedySession(std::path::PathBuf);

impl SingSession for NeedySession {
    fn sing(
        &mut self,
        phrase: &Phrase,
        _quality: Quality,
        _seed: u64,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        if !self.0.join("Dependencies").join("voc").is_dir() {
            return Err(SingError::Failed(
                "the vocoder \"voc\" this voicebank needs is not installed".into(),
            ));
        }
        let end = phrase.notes.last().map_or(0.0, |n| n.end());
        Ok(Sung {
            sample_rate: 8_000,
            samples: vec![0.1; (end * 8_000.0) as usize + 1],
            ..Sung::default()
        })
    }
}

fn pump_for(session: &mut Session, ms: u64) {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < until {
        session.pump();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// The singer panel says why a voice is silent, as the roll does — the
/// missing vocoder was said under the notes only — and installing what
/// was missing makes the track sing without an edit: a phrase that failed
/// was never tried again until something about it changed.
#[test]
fn a_missing_vocoder_is_said_on_the_panel_and_installing_it_sings() {
    let mut session = studio("vocoder", |dir| Arc::new(Needy(dir.to_path_buf())));
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    session.select_channel(index);
    let note = fontelle_vocal_files::vocal_note(0, PPQN, 60, "a");
    session.edit(fontelle_ui::canvas::RollEdit::Add { note });
    pump_for(&mut session, 400);
    let view = session.vocal_view().unwrap();
    assert_eq!(view.phrases[0].state, VocalRenderState::Failed);
    let panel = session.singer_panel().unwrap().message.unwrap_or_default();
    assert!(panel.contains("vocoder"), "the panel says why: {panel:?}");

    // The vocoder, installed as its .oudep's folder would be.
    let dep = scratch("vocoder-dep").join("voc");
    std::fs::create_dir_all(&dep).unwrap();
    std::fs::write(dep.join("vocoder.yaml"), "name: voc\n").unwrap();
    std::fs::write(dep.join("oudep.yaml"), "id: voc\nname: voc\n").unwrap();
    session.install_voicebank(&dep).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        session.pump();
        let state = session.vocal_view().unwrap().phrases[0].state;
        if state == VocalRenderState::Final {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "still {state:?} after the vocoder was installed"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn strict_and_open(_: &Path) -> Arc<dyn Backend> {
    let voice = |id: &str, terms: Option<&str>| VoiceInfo {
        id: id.into(),
        name: id.trim_start_matches("utau/").into(),
        languages: vec!["ja".into()],
        lowest: 48,
        highest: 72,
        research_only: false,
        terms: terms.map(str::to_string),
        image: None,
    };
    Arc::new(Runtime(vec![
        voice(
            "utau/Strict",
            Some("Ask the author before publishing anything."),
        ),
        voice("utau/Open", None),
    ]))
}

/// A new track does not start singing a bank whose terms were never read:
/// it takes one whose terms were agreed (or that has none) when there is
/// one. Terms are asked before a bank first sings (`docs/vocal-plan.md`
/// §2.2) — through the picker; a new track went round it.
#[test]
fn a_new_track_starts_with_a_bank_whose_terms_are_settled() {
    let mut session = studio("terms", strict_and_open);
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    assert_eq!(
        session.vocal_voice(index).map(|v| v.singer),
        Some("utau/Open".to_string())
    );
    session.agree_singer_terms("fontelle-utau", "utau/Strict");
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    assert_eq!(
        session.vocal_voice(index).map(|v| v.singer),
        Some("utau/Strict".to_string()),
        "agreed: the first again"
    );
}

/// A singer that cannot sing a phrase starting on "x".
struct Picky;

impl Backend for Picky {
    fn name(&self) -> &str {
        "Picky"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        one_bank(Path::new("")).voices()
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn SingSession>, String> {
        Ok(Box::new(PickySession))
    }
}

struct PickySession;

impl SingSession for PickySession {
    fn sing(
        &mut self,
        phrase: &Phrase,
        _quality: Quality,
        _seed: u64,
        _cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        if phrase.notes[0].lyric == "x" {
            return Err(SingError::Failed("\u{201c}x\u{201d} is not a sound".into()));
        }
        let end = phrase.notes.last().map_or(0.0, |n| n.end());
        Ok(Sung {
            sample_rate: 8_000,
            samples: vec![0.1; (end * 8_000.0) as usize + 1],
            ..Sung::default()
        })
    }
}

/// A freeze of a track nothing on which could be sung is refused with
/// why — it froze to silence and said "Froze" — and a freeze or a render
/// with some phrases unsung says how many are silent in it.
#[test]
fn a_freeze_says_what_could_not_be_sung() {
    let mut session = studio("freeze-unsung", |dir| Arc::new(Needy(dir.to_path_buf())));
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    session.select_channel(index);
    let note = fontelle_vocal_files::vocal_note(0, PPQN, 60, "a");
    session.edit(fontelle_ui::canvas::RollEdit::Add { note });
    let refused = session.freeze_vocal_now(index).unwrap_err();
    assert!(refused.contains("vocoder"), "{refused}");
    assert!(!session.vocal_frozen(index));

    let mut session = studio("freeze-some", |_| Arc::new(Picky));
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    session.select_channel(index);
    // A drawn note sings the language's syllable: the word is typed after.
    for (beat, lyric) in [(0, "la"), (8, "x")] {
        let note = fontelle_vocal_files::vocal_note(PPQN * beat, PPQN, 60, lyric);
        let id = session.edit(fontelle_ui::canvas::RollEdit::Add { note })[0];
        session.set_lyrics(&[id], vec![lyric.to_string()]).unwrap();
    }
    let said = session.freeze_vocal_now(index).unwrap();
    assert!(said.contains("1 phrase could not be sung"), "{said}");
    assert!(said.contains("is not a sound"), "{said}");
}

/// A song exported with phrases that could not be sung says so: a mix sent
/// off with a silent line in it is one to know about first.
#[test]
fn an_export_says_which_phrases_are_silent_in_it() {
    let mut session = studio("export-unsung", |_| Arc::new(Picky));
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    session.select_channel(index);
    for (beat, lyric) in [(0, "la"), (8, "x")] {
        let note = fontelle_vocal_files::vocal_note(PPQN * beat, PPQN, 60, lyric);
        let id = session.edit(fontelle_ui::canvas::RollEdit::Add { note })[0];
        session.set_lyrics(&[id], vec![lyric.to_string()]).unwrap();
    }
    let said = session.export_wav().unwrap();
    assert!(said.contains("1 vocal phrase could not be sung"), "{said}");
    assert!(said.contains("is not a sound"), "{said}");
}
