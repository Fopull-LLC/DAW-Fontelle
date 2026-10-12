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
    let mut session = common::a_session_in(common::a_clip_project(4), None);
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
    let panel = session.singer_panel().unwrap().message.unwrap_or_default();
    assert!(panel.contains("Find singers"), "{panel}");
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
    let panel = session.singer_panel().unwrap().message.unwrap_or_default();
    assert!(panel.contains("KasaneTeto_tougou_110401"), "{panel}");
    assert!(!panel.contains("utau/"), "{panel}");
    assert!(panel.contains("Find singers"), "{panel}");
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
