//! Singers in a shared song (`docs/collab-plan.md` §18, F93).
//!
//! Ty: *"collaborating doesn't share singer files currently like it does
//! with sound fonts and audio so you cannot sync singers you don't share.
//! please add this."*
//!
//! A Fontelle Singer bank whose terms let it be passed on is sent — after
//! the friend has read the terms and agreed, and installed and agreed here
//! before it sings. One whose terms do not (or nobody knows) is not sent:
//! the friend is told which bank and where to get it. Another engine's
//! singer cannot be sent: the friend is told which engine and which singer.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use fontelle_app::Session;
use fontelle_app::collab::{CollabOptions, JoinAnswer};
use fontelle_model::VocalVoice;
use fontelle_net::MemoryHub;
use fontelle_sing::{
    Backend, Capabilities, Lane, Phrase, Quality, Session as SingSession, SingError, Sung,
    VoiceInfo,
};
use fontelle_types::{InstrumentKind, PersistentId};
use fontelle_ui::document::{DocumentHost, StudioHost};
use fontelle_vocal::{BuiltinBackend, SingerKind as Kind};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-collab-singers-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// The Fontelle Singer's runtime for the test: each installed bank a voice.
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
                terms: b.terms.clone(),
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

const TERMS: &str = "Teto may be used and shared freely, credited.";

/// An UTAU bank called Teto, with its terms beside it.
fn install_teto(voicebanks: &Path) {
    let bank = voicebanks.join("Teto");
    std::fs::create_dir_all(&bank).unwrap();
    std::fs::write(bank.join("oto.ini"), b"a.wav=- a,20,100,-200,50,30\n").unwrap();
    std::fs::write(bank.join("a.wav"), vec![1u8; 4096]).unwrap();
    std::fs::write(bank.join("character.txt"), "name=Teto\r\n").unwrap();
    std::fs::write(bank.join("terms.txt"), TERMS).unwrap();
}

/// A catalogue that knows Teto, saying whether she may be passed on.
fn catalogue(dir: &Path, redistribute: &str) -> PathBuf {
    let path = dir.join("singers-index.json");
    let index = serde_json::json!({
        "format": 1,
        "singers": [{
            "id": "teto",
            "name": "Teto",
            "kind": "utau",
            "installs_as": ["Teto"],
            "page": "https://example.org/teto",
            "terms": { "summary": "Free to use.", "redistribute": redistribute }
        }]
    });
    std::fs::write(&path, index.to_string()).unwrap();
    path
}

fn options(name: &str) -> CollabOptions {
    let mut options = CollabOptions::new(name, PersistentId::derived(name));
    options.strict = true;
    options.idle_break = Duration::ZERO;
    options.hello_again = Duration::ZERO;
    options
}

fn studio(dir: &Path, who: &str, index: &Path, with_teto: bool) -> Session {
    let mut session = common::a_session_for(common::a_clip_project(4));
    session.set_singer_folders(Vec::new());
    let voicebanks = dir.join(format!("{who}-voicebanks"));
    std::fs::create_dir_all(&voicebanks).unwrap();
    if with_teto {
        install_teto(&voicebanks);
    }
    session.set_voicebanks(voicebanks, builtins());
    session.set_catalogue_for_tests(Some(index.to_path_buf()), None);
    std::fs::create_dir_all(dir.join(who)).unwrap();
    session.set_projects_dir(Some(dir.join(who)));
    session
}

struct Pair {
    hub: MemoryHub,
    now: u64,
    host: Session,
    joiner: Session,
    dir: PathBuf,
}

impl Drop for Pair {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

impl Pair {
    /// Alice sings with `voice` and shares; Bob joins.
    fn new(name: &str, redistribute: &str, voice: VocalVoice) -> Pair {
        let dir = scratch(name);
        let index = catalogue(&dir, redistribute);
        let mut host = studio(&dir, "alice", &index, true);
        host.save_as("Song").unwrap();
        host.add_channel_of(InstrumentKind::Vocal).unwrap();
        let vocal = (0..host.channels().len())
            .find(|i| host.channel_kind(*i) == Some(InstrumentKind::Vocal))
            .unwrap();
        host.set_singer(vocal, Some(voice));
        host.save().unwrap();
        let joiner = studio(&dir, "bob", &index, false);
        let hub = MemoryHub::new();
        hub.set_conditions(2, 0.0);
        let mut pair = Pair {
            hub,
            now: 0,
            host,
            joiner,
            dir,
        };
        pair.host
            .share(Box::new(pair.hub.server_endpoint()), options("Alice"))
            .unwrap();
        pair.joiner
            .join(Box::new(pair.hub.connect()), options("Bob"))
            .unwrap();
        pair.settle();
        pair.joiner.answer_join(JoinAnswer::Copy).unwrap();
        pair.settle();
        assert!(pair.joiner.collab_live());
        pair
    }

    fn settle(&mut self) {
        for _ in 0..60 {
            self.now += 1;
            self.hub.set_now(self.now);
            self.host.pump_collab();
            self.joiner.pump_collab();
        }
    }

    fn bobs_banks(&self) -> Vec<String> {
        fontelle_vocal::voicebanks::list(&self.dir.join("bob-voicebanks"))
            .into_iter()
            .map(|b| b.id)
            .collect()
    }
}

fn teto() -> VocalVoice {
    VocalVoice::new("fontelle-utau", "Teto", "ja")
}

/// A bank its author lets be passed on: Bob is shown its terms and asked;
/// after his yes it arrives, is installed, and is agreed — and only then is
/// it his to sing with.
#[test]
fn a_bank_that_may_be_passed_on_is_sent_after_its_terms_are_agreed() {
    let mut pair = Pair::new("sent", "yes", teto());
    let asked = pair
        .joiner
        .session_question()
        .expect("Bob is asked about the singer");
    let words = asked.lines.join(" ");
    assert!(words.contains("Teto"), "{words}");
    assert!(words.contains(TERMS), "the terms are shown: {words}");
    assert!(pair.bobs_banks().is_empty(), "nothing before the yes");

    pair.joiner.answer_session_question(0).unwrap();
    pair.settle();
    assert_eq!(pair.bobs_banks(), vec!["Teto".to_string()]);
    let bank = pair.dir.join("bob-voicebanks").join("Teto");
    assert_eq!(
        std::fs::read(bank.join("a.wav")).unwrap(),
        vec![1u8; 4096],
        "the bank as Alice has it"
    );
    assert!(
        fontelle_vocal::voicebanks::terms_acknowledged(&bank),
        "agreed here before it sings"
    );
    assert!(
        pair.joiner
            .session()
            .missing
            .iter()
            .all(|m| !m.what.contains("Teto")),
        "{:?}",
        pair.joiner.session().missing
    );
}

/// "Not now" sends nothing, and the panel says the singer is missing.
#[test]
fn a_bank_turned_down_is_not_sent_and_is_listed_as_missing() {
    let mut pair = Pair::new("declined", "yes", teto());
    assert!(pair.joiner.session_question().is_some());
    pair.joiner.answer_session_question(1).unwrap();
    pair.settle();
    assert!(pair.bobs_banks().is_empty());
    assert!(pair.joiner.session_question().is_none(), "asked once");
    let missing = pair.joiner.session().missing;
    assert!(
        missing.iter().any(|m| m.what.contains("Teto")),
        "{missing:?}"
    );
}

/// A bank whose terms do not let it be passed on is never sent and never
/// asked about: Bob is told which bank, and where its author offers it.
#[test]
fn a_bank_that_may_not_be_passed_on_is_named_with_where_to_get_it() {
    let mut pair = Pair::new("not-sent", "no", teto());
    pair.settle();
    assert!(pair.joiner.session_question().is_none());
    assert!(pair.bobs_banks().is_empty());
    let missing = pair.joiner.session().missing;
    let teto = missing
        .iter()
        .find(|m| m.what.contains("Teto"))
        .unwrap_or_else(|| panic!("{missing:?}"));
    assert!(teto.detail.contains("https://example.org/teto"), "{teto:?}");
}

/// Another engine's singer cannot be sent: Bob is told which engine and
/// which singer.
#[test]
fn an_engines_singer_is_named_with_its_engine() {
    let mut pair = Pair::new(
        "engine",
        "yes",
        VocalVoice::new("Some Engine", "alto", "en"),
    );
    pair.settle();
    assert!(pair.joiner.session_question().is_none());
    let missing = pair.joiner.session().missing;
    assert!(
        missing
            .iter()
            .any(|m| m.what.contains("alto") && m.detail.contains("Some Engine")),
        "{missing:?}"
    );
}
