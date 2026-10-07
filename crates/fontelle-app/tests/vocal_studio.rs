//! The vocal track's second half in the studio: the Fontelle Singer's
//! voicebanks (installed by a drop, listed in the picker, their terms
//! agreed to before they sing), Find singers (an index, Get it watching
//! Downloads), expression and lanes as undoable edits, and an ABI 2
//! engine's own window editing the part — each of its batches one undo
//! step.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fontelle_app::Session;
use fontelle_model::{LaneCurve, LaneKey, NoteVoice, VocalVoice};
use fontelle_sing::{
    Backend, Capabilities, Lane, Phrase, Quality, Session as SingSession, SingError, Sung,
    VoiceInfo,
};
use fontelle_types::{InstrumentKind, NoteId, PPQN};
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::{DocumentHost, SingerKind, StudioHost, VocalRenderState};
use fontelle_vocal::{BuiltinBackend, SingerKind as Kind};

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
        "fontelle-vocal-studio-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// Both test engines, copied once: copying over a loaded library rewrites
/// running code.
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

/// A built-in runtime for the test: every folder with an `oto.ini` under
/// the voicebanks folder is a voice, with its readme as terms.
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
            lanes: vec![Lane::Dynamics, Lane::Breathiness],
            vibrato: true,
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

fn session(name: &str) -> (Session, PathBuf) {
    let dir = scratch(name);
    let project = common::a_clip_project(4);
    let bundle = dir.join("Song.fontelle");
    fontelle_app::save_project(&project, &bundle).unwrap();
    let mut session = common::a_session_in(project, Some(bundle));
    session.set_singer_folders(vec![singer_folder()]);
    session.set_voicebanks(dir.join("voicebanks"), builtins());
    session.set_headless_singer_editors(true);
    (session, dir)
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
            lyric: String::new(),
            voice: None,
        },
    })[0]
}

fn pump_until(session: &mut Session, what: &str, done: impl Fn(&mut Session) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(session) {
        assert!(Instant::now() < deadline, "never: {what}");
        session.pump();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A Shift-JIS UTAU bank, zipped the way a Japanese Windows does it.
fn teto_zip(dir: &Path) -> PathBuf {
    let sjis = |t: &str| encoding_rs::SHIFT_JIS.encode(t).0.into_owned();
    let mut out = Vec::new();
    let mut central = Vec::new();
    let files: Vec<(Vec<u8>, Vec<u8>)> = vec![
        (
            sjis("テト/oto.ini"),
            b"a.wav=- a,20,100,-200,50,30\n".to_vec(),
        ),
        (sjis("テト/character.txt"), sjis("name=重音テト\n")),
        (sjis("テト/readme.txt"), sjis("利用規約：自由に使えます")),
    ];
    for (name, data) in &files {
        let offset = out.len() as u32;
        let crc = {
            let mut c = 0xFFFF_FFFFu32;
            for &b in data {
                c ^= u32::from(b);
                for _ in 0..8 {
                    c = if c & 1 != 0 {
                        (c >> 1) ^ 0xEDB8_8320
                    } else {
                        c >> 1
                    };
                }
            }
            !c
        };
        let header = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes());
            }
            h.extend_from_slice(&20u16.to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            h.extend_from_slice(&[0, 0, 0, 0]);
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(data.len() as u32).to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes());
            if central {
                h.extend_from_slice(&[0; 6]);
                h.extend_from_slice(&0u32.to_le_bytes());
                h.extend_from_slice(&offset.to_le_bytes());
            }
            h.extend_from_slice(name);
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend_from_slice(data);
        central.extend(header(0x0201_4b50, true));
    }
    let at = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    let path = dir.join("teto.zip");
    std::fs::write(&path, out).unwrap();
    path
}

// ------------------------------------------------------------ voicebanks

#[test]
fn a_dropped_bank_is_installed_and_sings_after_its_terms_are_agreed() {
    let (mut session, dir) = session("install");
    assert!(session.voicebanks().is_empty());
    let said = session.install_voicebank(&teto_zip(&dir)).unwrap();
    assert!(said.contains("重音テト"), "{said}");
    let banks = session.voicebanks();
    assert_eq!(banks.len(), 1);
    assert_eq!(banks[0].kind, "UTAU");
    assert!(!banks[0].terms_agreed);
    // In the picker, first, as the Fontelle Singer's.
    let singers = session.singers();
    assert_eq!(singers[0].kind, SingerKind::Utau);
    assert_eq!(singers[0].name, "重音テト");
    assert!(!singers[0].terms_agreed);
    assert!(singers[0].terms.as_deref().unwrap().contains("利用規約"));
    assert!(
        singers
            .iter()
            .any(|s| s.kind == SingerKind::Engine && s.has_editor)
    );
    // A new vocal track gets the free voicebank.
    session.add_channel_of(InstrumentKind::Vocal).unwrap();
    let index = vocal_index(&session);
    let voice = session.vocal_voice(index).unwrap();
    assert_eq!(voice.engine, "fontelle-utau");
    session.agree_singer_terms(&voice.engine, &voice.singer);
    assert!(session.singers()[0].terms_agreed);
    // And it sings.
    draw(&mut session, 0, 60);
    pump_until(&mut session, "the bank sang", |s| {
        s.vocal_view()
            .is_some_and(|v| v.phrases.iter().all(|p| p.state == VocalRenderState::Final))
    });
    let view = session.vocal_view().unwrap();
    assert_eq!(view.backend, "fontelle-utau");
    assert!(view.capabilities.lanes.contains(&Lane::Dynamics));
    assert!(
        view.editor.is_none(),
        "a voicebank has no window of its own"
    );
    // Removed again.
    let id = banks[0].id.clone();
    session.uninstall_voicebank(&id).unwrap();
    assert!(session.voicebanks().is_empty());
}

#[test]
fn get_it_installs_the_bank_when_it_lands_in_downloads() {
    let (mut session, dir) = session("getit");
    let downloads = dir.join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let index = dir.join("index.json");
    std::fs::write(
        &index,
        r#"{"format":1,"singers":[{"id":"teto","name":"Teto","kind":"utau","languages":["ja"],
            "terms":{"summary":"Free.","url":"https://example.org/t"},
            "page":"https://example.org/teto"}]}"#,
    )
    .unwrap();
    session.set_catalogue_for_tests(Some(index), Some(downloads.clone()));
    let rows = session.catalogue();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].direct);
    let said = session.catalogue_get("teto").unwrap();
    assert!(said.contains("installs when it lands"), "{said}");
    assert!(session.catalogue()[0].waiting);
    std::fs::copy(teto_zip(&dir), downloads.join("teto-1.0.zip")).unwrap();
    pump_until(&mut session, "the download was installed", |s| {
        !s.voicebanks().is_empty()
    });
    assert!(!session.catalogue()[0].waiting);
}

#[test]
fn the_shipped_catalogue_is_offered_with_no_index_of_ones_own() {
    let (mut session, dir) = session("shipped");
    session.set_catalogue_for_tests(Some(dir.join("none.json")), None);
    let rows = session.catalogue();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r.terms_url.starts_with("https://")));
}

// ------------------------------------------------- expression and lanes

#[test]
fn lanes_and_note_voices_are_edits_that_undo_and_reach_the_singer() {
    let (mut session, dir) = session("lanes");
    session.install_voicebank(&teto_zip(&dir)).unwrap();
    session.add_channel_of(InstrumentKind::Vocal).unwrap();
    let a = draw(&mut session, 0, 60);
    let mut breath = LaneCurve::default();
    for end in [PPQN / 2, PPQN] {
        breath.write(0, end, 20, |_| 0.5, 0.0);
        session.set_vocal_lane(LaneKey::Standard(Lane::Breathiness), Some(breath.clone()));
    }
    session.end_vocal_gesture();
    let voice = NoteVoice {
        vibrato: Some(fontelle_sing::Vibrato::default()),
        ..NoteVoice::default()
    };
    session
        .set_note_voices(&[a], vec![Some(voice.clone())], "Vibrato")
        .unwrap();
    session.end_vocal_gesture();
    let view = session.vocal_view().unwrap();
    assert_eq!(view.part.lanes.get(&Lane::Breathiness), Some(&breath));
    assert_eq!(session.notes()[a].voice.as_deref(), Some(&voice));
    // The pitch line the bend is drawn over follows the vibrato.
    assert!(!view.pitch_line.is_empty());
    session.undo();
    assert!(session.notes()[a].voice.is_none());
    session.undo();
    assert!(
        session.vocal_view().unwrap().part.lanes.is_empty(),
        "a stroke was one step"
    );
    let _ = BTreeMap::<String, f32>::new();
}

// ------------------------------------------------ an engine's own window

fn engine_session(name: &str) -> (Session, NoteId) {
    let (mut session, _) = session(name);
    session.add_channel_of(InstrumentKind::Vocal).unwrap();
    let index = vocal_index(&session);
    session.set_singer(
        index,
        Some(VocalVoice::new(
            fontelle_testsinger::ENGINE2_NAME,
            fontelle_testsinger::ALTO_ID,
            "en",
        )),
    );
    let id = draw(&mut session, 0, 60);
    (session, id)
}

#[test]
fn the_engines_window_opens_from_the_track_and_its_batch_is_one_undo() {
    let (mut session, id) = engine_session("window");
    let view = session.vocal_view().unwrap();
    assert_eq!(
        view.editor.as_deref(),
        Some(fontelle_testsinger::ENGINE2_NAME)
    );
    // Its own lane, setting and option are offered; gender is not honoured.
    assert_eq!(
        view.capabilities.extra_lanes[0].id,
        fontelle_testsinger::GROWL_LANE
    );
    assert!(!view.capabilities.lanes.contains(&Lane::Gender));
    session.open_singer_editor().unwrap();
    assert!(session.singer_editor_open());
    assert!(session.vocal_view().unwrap().editor_open);
    session
        .set_lyrics(&[id], vec![fontelle_testsinger::POKE_LYRIC.into()])
        .unwrap();
    pump_until(&mut session, "the window's edit landed", |s| {
        s.notes()[id].lyric == "poked"
    });
    let view = session.vocal_view().unwrap();
    let backend = fontelle_testsinger::ENGINE2_NAME;
    assert_eq!(
        view.part.settings[backend][fontelle_testsinger::BRIGHTNESS_SETTING],
        0.5
    );
    assert!(view.part.extra[backend].contains_key(fontelle_testsinger::GROWL_LANE));
    assert!(
        session.notes()[id]
            .voice
            .as_ref()
            .is_some_and(|v| v.portamento.is_some())
    );
    // One Ctrl+Z takes the whole batch back.
    session.undo();
    assert_eq!(session.notes()[id].lyric, fontelle_testsinger::POKE_LYRIC);
    let view = session.vocal_view().unwrap();
    assert!(view.part.settings.is_empty() && view.part.extra.is_empty());
    assert!(session.notes()[id].voice.is_none());
    session.close_singer_editor();
    assert!(!session.singer_editor_open());
}

#[test]
fn the_window_reads_the_take_under_a_note() {
    let (mut session, id) = engine_session("take");
    session.open_singer_editor().unwrap();
    pump_until(&mut session, "sung", |s| {
        s.vocal_view()
            .is_some_and(|v| v.phrases.iter().all(|p| p.state == VocalRenderState::Final))
    });
    session
        .set_lyrics(&[id], vec![fontelle_testsinger::TAKE_LYRIC.into()])
        .unwrap();
    // The edit makes a new phrase; the window reads whichever take is there.
    pump_until(&mut session, "the window read a take", |s| {
        s.notes()[id].lyric.starts_with("take ")
    });
}

#[test]
fn a_batch_naming_a_note_that_is_not_there_changes_nothing() {
    let (mut session, id) = engine_session("refused");
    session.open_singer_editor().unwrap();
    session
        .set_lyrics(&[id], vec![fontelle_testsinger::BAD_LYRIC.into()])
        .unwrap();
    pump_until(&mut session, "the window said it was refused", |s| {
        s.notes()[id].lyric == "refused"
    });
}

// ------------------------------------------------------------- kept takes

#[test]
fn sung_takes_are_kept_in_the_song_and_a_reopened_song_plays_them_at_once() {
    let (mut session, dir) = session("takes");
    session.install_voicebank(&teto_zip(&dir)).unwrap();
    session.add_channel_of(InstrumentKind::Vocal).unwrap();
    let voice = session.vocal_voice(vocal_index(&session)).unwrap();
    session.agree_singer_terms(&voice.engine, &voice.singer);
    draw(&mut session, 0, 60);
    draw(&mut session, 4, 62);
    pump_until(&mut session, "the bank sang", |s| {
        s.vocal_view()
            .is_some_and(|v| v.phrases.iter().all(|p| p.state == VocalRenderState::Final))
    });
    // In the song's own cache folder, beside Analyze Musically's.
    let kept = dir.join("Song.fontelle").join("cache").join("vocal");
    pump_until(&mut session, "the takes were written", |_| {
        std::fs::read_dir(&kept)
            .map(|d| {
                d.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "take"))
                    .count()
            })
            .unwrap_or(0)
            == 2
    });
    // Reopened: the finals come off the disk; nothing is previewed.
    let project = session.project().clone();
    drop(session);
    let mut reopened = common::a_session_in(project, Some(dir.join("Song.fontelle")));
    reopened.set_singer_folders(vec![singer_folder()]);
    reopened.set_voicebanks(dir.join("voicebanks"), builtins());
    reopened.set_headless_singer_editors(true);
    reopened.select_channel(vocal_index(&reopened));
    let mut previewed = false;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        reopened.pump();
        if let Some(v) = reopened.vocal_view() {
            previewed |= v
                .phrases
                .iter()
                .any(|p| p.state == VocalRenderState::Preview);
            if !v.phrases.is_empty() && v.phrases.iter().all(|p| p.state == VocalRenderState::Final)
            {
                break;
            }
        }
        assert!(Instant::now() < deadline, "the kept takes never played");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!previewed, "a kept final is played, not sung again");
}

#[test]
fn an_unsaved_songs_takes_go_to_fontelles_cache_folder() {
    use fontelle_app::settings::Platform;
    let env = |key: &str| (key == "XDG_CACHE_HOME").then(|| "/home/someone/.cache".to_string());
    assert_eq!(
        fontelle_app::vocal_takes_dir(None, &env, Platform::Unix),
        Some(PathBuf::from("/home/someone/.cache/fontelle/vocal"))
    );
    assert_eq!(
        fontelle_app::vocal_takes_dir(
            Some(Path::new("/songs/Song.fontelle")),
            &env,
            Platform::Unix
        ),
        Some(PathBuf::from("/songs/Song.fontelle/cache/vocal"))
    );
}
