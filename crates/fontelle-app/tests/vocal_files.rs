//! Vocal song files in the studio, as producers coming from UTAU, OpenUtau,
//! Vocaloid and notation programs bring them: dropped on the window (a
//! vocal track of their own, the song's tempo kept or the file's taken when
//! the song is empty) or onto a vocal row (into that track), one undo step
//! each; a vocal track written back out as MIDI with lyrics or USTX; a
//! vocal track frozen to audio and unfrozen (one undo each way); and a
//! selection rendered to a clip on the row below.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontelle_app::Session;
use fontelle_model::{ClipSource, LaneCurve, Project, TempoSegment};
use fontelle_sing::{
    Backend, Capabilities, Lane, Phrase, Quality, Session as SingSession, SingError, Sung,
    VoiceInfo,
};
use fontelle_types::{ChannelId, InstrumentKind, PPQN};
use fontelle_ui::document::{DocumentHost, JobPoll, StudioHost, VocalFileFormat};
use fontelle_vocal::{BuiltinBackend, SingerKind};
use fontelle_vocal_files::{SongPart, VocalSong, midi, ustx, vocal_note};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-vocal-files-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// A built-in runtime that hums every phrase at a constant level — enough to
/// see where a render sounds.
struct Hummer;

impl Backend for Hummer {
    fn name(&self) -> &str {
        "Hummer"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        vec![VoiceInfo {
            id: "hum".into(),
            name: "Hum".into(),
            languages: vec!["ja".into()],
            lowest: 36,
            highest: 84,
            research_only: false,
            terms: None,
            image: None,
        }]
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
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
            samples: vec![0.25; (end * 8_000.0) as usize + 1],
            ..Sung::default()
        })
    }
}

fn hummer(_: &Path) -> Arc<dyn Backend> {
    Arc::new(Hummer)
}

/// A session on a saved, empty song (one empty clip), singing with the
/// hummer.
fn session(name: &str) -> (Session, PathBuf) {
    let dir = scratch(name);
    let project = common::a_clip_project(4);
    let bundle = dir.join("Song.fontelle");
    fontelle_app::save_project(&project, &bundle).unwrap();
    let mut session = common::a_session_in(project, Some(bundle));
    session.set_singer_folders(Vec::new());
    session.set_voicebanks(
        dir.join("voicebanks"),
        vec![BuiltinBackend {
            key: "fontelle-utau",
            kind: SingerKind::Utau,
            make: hummer,
        }],
    );
    (session, dir)
}

/// A two-note song at `bpm`, written as `.ustx`.
fn ustx_file(dir: &Path, bpm: f64) -> PathBuf {
    let mut song = VocalSong::new("Imported");
    song.tempo = vec![TempoSegment { start_tick: 0, bpm }];
    song.beats_per_bar = 3;
    let mut part = SongPart::new("Lead");
    part.notes = vec![
        vocal_note(PPQN * 3, PPQN, 60, "sa"),
        vocal_note(PPQN * 4, PPQN * 2, 62, "ku"),
    ];
    part.vocal.lanes.insert(
        Lane::Dynamics,
        LaneCurve::from_points(vec![(PPQN * 3, -6.0), (PPQN * 5, 0.0)]),
    );
    song.parts.push(part);
    let path = dir.join("song.ustx");
    std::fs::write(&path, ustx::write(&song)).unwrap();
    path
}

fn vocal_channels(project: &Project) -> Vec<ChannelId> {
    project
        .channels
        .iter()
        .filter(|(_, c)| c.instrument == Some(InstrumentKind::Vocal))
        .map(|(id, _)| id)
        .collect()
}

/// `(clip start, note starts on the song, lyrics)` of `channel`'s clips.
fn sung(project: &Project, channel: ChannelId) -> Vec<(i64, Vec<i64>, Vec<String>)> {
    let mut out = Vec::new();
    for clip in project.clips.values() {
        let ClipSource::Notes(data) = &clip.source else {
            continue;
        };
        let mut notes: Vec<_> = data
            .notes
            .values()
            .filter(|n| n.channel_or(data.channel) == channel)
            .collect();
        if notes.is_empty() {
            continue;
        }
        notes.sort_by_key(|n| n.start);
        out.push((
            clip.start,
            notes.iter().map(|n| clip.start + n.start).collect(),
            notes.iter().map(|n| n.lyric.clone()).collect(),
        ));
    }
    out.sort();
    out
}

fn index_of(session: &Session, channel: ChannelId) -> usize {
    let name = session.project().channels[channel].name.clone();
    session
        .channels()
        .iter()
        .position(|c| c.name == name)
        .expect("the channel is in the rack")
}

#[test]
fn a_file_dropped_on_an_empty_song_brings_its_tempo_in_one_undo() {
    let (mut session, dir) = session("empty");
    let file = ustx_file(&dir, 90.0);
    let said = session.drop_file(&file).unwrap();
    assert!(said.contains("Lead"), "{said}");
    assert!(
        said.contains("90"),
        "the toast says the tempo came too: {said}"
    );
    let project = session.project();
    let vocal = vocal_channels(project);
    assert_eq!(vocal.len(), 1, "a vocal track of its own");
    assert_eq!(project.channels[vocal[0]].name, "Lead");
    assert!(
        project.channels[vocal[0]].vocal.is_some(),
        "it arrives singing"
    );
    assert_eq!(project.tempo_map.tempo_at(0), 90.0);
    assert_eq!(project.beats_per_bar, 3);
    // On the beats the file put them on, in a clip starting at their bar.
    assert_eq!(
        sung(project, vocal[0]),
        vec![(
            PPQN * 3,
            vec![PPQN * 3, PPQN * 4],
            vec!["sa".to_string(), "ku".to_string()]
        )]
    );
    session.undo();
    let project = session.project();
    assert!(vocal_channels(project).is_empty());
    assert_eq!(project.tempo_map.tempo_at(0), 120.0);
    assert_eq!(project.beats_per_bar, 4);
}

#[test]
fn a_file_dropped_on_a_song_keeps_the_songs_tempo_and_places_by_beats() {
    let (mut session, dir) = session("busy");
    // Something in the song already.
    session.edit(fontelle_ui::canvas::RollEdit::Add {
        note: vocal_note(0, PPQN, 48, ""),
    });
    let file = ustx_file(&dir, 90.0);
    let said = session.drop_file(&file).unwrap();
    assert!(
        said.contains("120"),
        "the toast says the song's tempo was kept: {said}"
    );
    let project = session.project();
    assert_eq!(project.tempo_map.tempo_at(0), 120.0);
    assert_eq!(project.beats_per_bar, 4);
    let vocal = vocal_channels(project);
    assert_eq!(sung(project, vocal[0])[0].1, vec![PPQN * 3, PPQN * 4]);
}

#[test]
fn a_file_dropped_on_a_vocal_row_goes_into_that_track() {
    let (mut session, dir) = session("onto");
    session.drop_file(&ustx_file(&dir, 120.0)).unwrap();
    let channel = vocal_channels(session.project())[0];
    let row = session
        .project()
        .lane_ids()
        .iter()
        .position(|lane| session.project().lanes[*lane].name == "Lead")
        .unwrap();
    let rows = session.project().lanes.len();
    // Dropped again at bar 9 of that row.
    let at = session.project().tempo_map.tick_to_sample(PPQN * 8 * 3);
    session
        .drop_file_on(&ustx_file(&dir, 120.0), at, Some(row))
        .unwrap();
    let project = session.project();
    assert_eq!(vocal_channels(project), vec![channel], "no second track");
    assert_eq!(project.lanes.len(), rows, "no new row");
    let clips = sung(project, channel);
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[1].1, vec![PPQN * 24 + PPQN * 3, PPQN * 24 + PPQN * 4]);
    session.undo();
    assert_eq!(sung(session.project(), channel).len(), 1);
}

#[test]
fn a_midi_file_with_lyrics_comes_in_sung() {
    let (mut session, dir) = session("midi");
    let mut song = VocalSong::new("Karaoke");
    let mut part = SongPart::new("Melody");
    part.notes = vec![
        vocal_note(0, PPQN, 64, "twin-"),
        vocal_note(PPQN, PPQN, 64, "kle"),
    ];
    song.parts.push(part);
    let file = dir.join("song.mid");
    std::fs::write(&file, midi::write(&song)).unwrap();
    session.drop_file(&file).unwrap();
    let project = session.project();
    let vocal = vocal_channels(project);
    assert_eq!(vocal.len(), 1);
    assert_eq!(
        sung(project, vocal[0])[0].2,
        vec!["twin-".to_string(), "kle".to_string()]
    );
}

#[test]
fn a_vocal_track_exports_as_midi_with_lyrics_and_as_ustx() {
    let (mut session, dir) = session("export");
    session.drop_file(&ustx_file(&dir, 100.0)).unwrap();
    let channel = vocal_channels(session.project())[0];
    let index = index_of(&session, channel);
    let mid = dir.join("out.mid");
    session
        .export_vocal_to(index, VocalFileFormat::Midi, &mid)
        .unwrap();
    let back = midi::read(&std::fs::read(&mid).unwrap(), "out").unwrap();
    let lyrics: Vec<&str> = back.parts[0]
        .notes
        .iter()
        .map(|n| n.lyric.as_str())
        .collect();
    assert_eq!(lyrics, vec!["sa", "ku"]);
    assert_eq!(back.parts[0].notes[0].start, PPQN * 3);
    assert!((back.tempo[0].bpm - 100.0).abs() < 1e-6);

    let out = dir.join("out.ustx");
    session
        .export_vocal_to(index, VocalFileFormat::Ustx, &out)
        .unwrap();
    let back = ustx::read(&std::fs::read_to_string(&out).unwrap(), "out").unwrap();
    let part = &back.parts[0];
    assert_eq!(part.notes.len(), 2);
    assert_eq!(part.notes[1].start, PPQN * 4);
    // The lane came along, on the song's beats.
    let dynamics = part.vocal.lanes.get(&Lane::Dynamics).expect("dynamics");
    assert!((dynamics.at(PPQN * 3, 0.0) + 6.0).abs() < 0.1);
}

/// The frames of a mono-summed WAV from `path`, and its rate.
fn wav(path: &Path) -> (Vec<f32>, u32) {
    let bytes = std::fs::read(path).unwrap();
    let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    let data = bytes
        .windows(4)
        .position(|w| w == b"data")
        .expect("a data chunk")
        + 8;
    let samples: Vec<f32> = bytes[data..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|f| {
            let l = i16::from_le_bytes([f[0], f[1]]) as f32 / 32768.0;
            let r = i16::from_le_bytes([f[2], f[3]]) as f32 / 32768.0;
            (l + r) / 2.0
        })
        .collect();
    (samples, rate)
}

fn audio_clips(project: &Project) -> Vec<(i64, fontelle_types::AudioClipData)> {
    project
        .clips
        .values()
        .filter_map(|c| match &c.source {
            ClipSource::Audio(data) => Some((c.start, data.clone())),
            _ => None,
        })
        .collect()
}

fn frozen_song(name: &str) -> (Session, PathBuf, ChannelId) {
    let (mut session, dir) = session(name);
    session.drop_file(&ustx_file(&dir, 120.0)).unwrap();
    let channel = vocal_channels(session.project())[0];
    (session, dir, channel)
}

#[test]
fn freezing_renders_the_track_and_swaps_in_its_audio_one_undo_each_way() {
    let (mut session, dir, channel) = frozen_song("freeze");
    let index = index_of(&session, channel);
    assert!(!session.vocal_frozen(index));
    let said = session.freeze_vocal_now(index).unwrap();
    assert!(said.contains("Froze"), "{said}");
    assert!(session.vocal_frozen(index));
    assert!(session.project().is_frozen(channel));
    // The render is in the project's renders/ folder, routed to the
    // track's own strip, and sounds where the notes are.
    let clips = audio_clips(session.project());
    assert_eq!(clips.len(), 1);
    let (start, data) = &clips[0];
    assert_eq!(
        data.mixer_track,
        session.project().channels[channel].mixer_track
    );
    let file = dir.join("Song.fontelle").join(&data.asset.path);
    assert!(
        file.starts_with(dir.join("Song.fontelle").join("renders")),
        "{file:?}"
    );
    let (samples, rate) = wav(&file);
    let tempo = &session.project().tempo_map;
    let clip_seconds = tempo.tick_to_seconds(*start);
    let inside = ((tempo.tick_to_seconds(PPQN * 4) - clip_seconds) * f64::from(rate)) as usize;
    assert!(samples[inside].abs() > 0.05, "sung: {}", samples[inside]);
    // It starts a breath before the first note, in silence.
    assert!(*start < PPQN * 3);
    assert!(
        samples[..32].iter().all(|s| s.abs() < 1e-3),
        "silent before the first note"
    );
    // The part is all still there.
    assert_eq!(sung(session.project(), channel)[0].2.len(), 2);

    session.undo();
    assert!(!session.vocal_frozen(index));
    assert!(audio_clips(session.project()).is_empty());
    session.redo();
    assert!(session.vocal_frozen(index));

    session.unfreeze_vocal(index).unwrap();
    assert!(!session.vocal_frozen(index));
    assert!(audio_clips(session.project()).is_empty());
    session.undo();
    assert!(
        session.vocal_frozen(index),
        "undoing the unfreeze freezes it again"
    );
}

#[test]
fn freezing_waits_for_the_final_takes_with_progress_like_an_export() {
    let (mut session, _dir, channel) = frozen_song("freeze-job");
    let index = index_of(&session, channel);
    let started = session.freeze_vocal(index).unwrap();
    assert!(started.contains("Freezing"), "{started}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let finished = loop {
        assert!(
            std::time::Instant::now() < deadline,
            "the freeze never ended"
        );
        match session.poll_job() {
            JobPoll::Finished(result) => break result,
            JobPoll::Running(_) | JobPoll::Idle => {}
        }
        session.pump();
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    finished.unwrap();
    assert!(session.vocal_frozen(index));
}

#[test]
fn a_selection_renders_to_a_clip_on_the_row_below() {
    let (mut session, _dir, channel) = frozen_song("render-sel");
    let row = session
        .project()
        .lane_ids()
        .iter()
        .position(|lane| session.project().lanes[*lane].name == "Lead")
        .unwrap();
    // The imported clip is open in the roll; select its second note.
    let second = session
        .notes()
        .iter()
        .max_by_key(|(_, n)| n.start)
        .map(|(id, _)| id)
        .unwrap();
    session.set_vocal_selection(&[second]);
    let rows = session.project().lanes.len();
    let said = session.render_vocal_selection_now().unwrap();
    assert!(said.contains("Rendered"), "{said}");
    let project = session.project();
    let below = project.lane_ids()[row + 1];
    assert!(project.lanes[below].name.contains("(rendered)"));
    let clips: Vec<_> = project
        .clips
        .values()
        .filter(|c| c.lane == below && matches!(c.source, ClipSource::Audio(_)))
        .collect();
    assert_eq!(clips.len(), 1);
    // It starts near the selected note (a consonant's lead before it), not
    // at the first.
    assert!(clips[0].start > PPQN * 3 + PPQN / 2, "{}", clips[0].start);
    assert!(!project.is_frozen(channel), "rendering is not freezing");
    session.undo();
    assert_eq!(
        session.project().lanes.len(),
        rows,
        "one undo takes the row back"
    );
}

#[test]
fn not_a_vocal_file_is_left_to_the_other_imports() {
    let (mut session, dir) = session("plain-midi");
    // A MIDI file with no words is the ordinary MIDI import.
    let mut song = VocalSong::new("Plain");
    let mut part = SongPart::new("Piano");
    part.notes = vec![vocal_note(0, PPQN, 60, "")];
    song.parts.push(part);
    let file = dir.join("plain.mid");
    std::fs::write(&file, midi::write(&song)).unwrap();
    session.drop_file(&file).unwrap();
    assert!(vocal_channels(session.project()).is_empty());
}

/// For a look at the studio after an import: with `FONTELLE_VOCAL_SCENE`
/// set to a folder, a UST with a drawn bend, vibrato and dynamics is
/// imported and the song saved there as `Imported.fontelle`, to `--open`
/// in the real window.
#[test]
fn an_imported_ust_saved_for_a_look() {
    let (mut session, dir) = session("scene");
    let ust = "[#SETTING]\nTempo=110\nProjectName=Sakura\nMode2=True\n\
        [#0000]\nLength=480\nLyric=R\nNoteNum=60\n\
        [#0001]\nLength=480\nLyric=\u{3055}\nNoteNum=69\nIntensity=80\n\
        [#0002]\nLength=480\nLyric=\u{304f}\nNoteNum=69\nPBS=-40;0\nPBW=80\n\
        [#0003]\nLength=960\nLyric=\u{3089}\nNoteNum=71\nPBS=-60;-20\nPBW=60,90\nPBY=4,0\nVBR=60,180,35,25,25,0,0,0\nIntensity=120\n\
        [#0004]\nLength=480\nLyric=\u{3055}\nNoteNum=69\nPBS=-40;20\nPBW=80\n\
        [#0005]\nLength=480\nLyric=\u{304f}\nNoteNum=71\n\
        [#0006]\nLength=960\nLyric=\u{3089}\nNoteNum=72\nVBR=70,160,45,20,30,0,0,0\n\
        [#TRACKEND]\n";
    let file = dir.join("sakura.ust");
    std::fs::write(&file, encoding_rs::SHIFT_JIS.encode(ust).0).unwrap();
    let said = session.drop_file(&file).unwrap();
    assert!(said.contains("Sakura"), "{said}");
    let vocal = vocal_channels(session.project());
    assert_eq!(sung(session.project(), vocal[0])[0].2.len(), 6);
    if let Ok(out) = std::env::var("FONTELLE_VOCAL_SCENE") {
        let bundle = Path::new(&out).join("Imported.fontelle");
        fontelle_app::save_project(session.project(), &bundle).unwrap();
        eprintln!("saved {}", bundle.display());
    }
}

/// A vocal track added to a song whose open clip is another instrument's:
/// its notes are written into that clip (a clip holds several instruments'
/// notes), and they are sung. Rendering them, freezing the track and
/// dropping a file on that row must find them there too — found walking
/// the singer from a new song: "the clip in the piano roll is not a vocal
/// clip".
#[test]
fn a_vocal_line_written_into_another_instruments_clip_renders_and_freezes() {
    let (mut session, _dir) = session("shared-clip");
    session.add_singer_channel(None).unwrap();
    let index = (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .unwrap();
    session.select_channel(index);
    let channel = vocal_channels(session.project())[0];
    let open = session
        .project()
        .clips
        .values()
        .find_map(|c| match &c.source {
            ClipSource::Notes(d) => Some(d.channel),
            _ => None,
        })
        .unwrap();
    assert_ne!(open, channel, "the open clip is the other instrument's");
    for beat in [0, 1] {
        session.edit(fontelle_ui::canvas::RollEdit::Add {
            note: vocal_note(PPQN * beat, PPQN, 60, "la"),
        });
    }
    assert_eq!(sung(session.project(), channel)[0].2.len(), 2, "sung");
    let rows = session.project().lanes.len();
    let said = session.render_vocal_selection_now().unwrap();
    assert!(said.contains("Rendered"), "{said}");
    let clips = audio_clips(session.project());
    assert_eq!(clips.len(), 1);
    assert_eq!(
        clips[0].1.mixer_track,
        session.project().channels[channel].mixer_track,
        "routed to the vocal track's strip"
    );
    assert_eq!(session.project().lanes.len(), rows + 1, "a row below");
    session.undo();

    session.freeze_vocal_now(index).unwrap();
    let project = session.project();
    let frozen = project
        .clips
        .values()
        .find(|c| matches!(c.source, ClipSource::Audio(_)))
        .unwrap();
    let row = project.lane_ids().iter().position(|l| *l == frozen.lane);
    assert_eq!(row, Some(1), "right under the row the notes are on");
    session.undo();

    // A file dropped on that row goes into the vocal track.
    let dir = scratch("shared-clip-file");
    session
        .drop_file_on(&ustx_file(&dir, 120.0), PPQN * 16, Some(0))
        .unwrap();
    assert_eq!(
        vocal_channels(session.project()),
        vec![channel],
        "no second track"
    );
}

/// What the listener below has been asked to sing.
static HEARD: std::sync::Mutex<Vec<Phrase>> = std::sync::Mutex::new(Vec::new());

struct Listener;

impl Backend for Listener {
    fn name(&self) -> &str {
        "Listener"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        Hummer.voices()
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn SingSession>, String> {
        Ok(Box::new(Listening))
    }
}

struct Listening;

impl SingSession for Listening {
    fn sing(
        &mut self,
        phrase: &Phrase,
        quality: Quality,
        seed: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        HEARD.lock().unwrap().push(phrase.clone());
        Hum.sing(phrase, quality, seed, cancelled)
    }
}

/// A singer chosen is warmed while the first notes are being written: its
/// reference phrase is sung in the background (a DiffSinger bank took 5 to
/// 14 s to load before its first phrase was heard).
#[test]
fn a_chosen_singer_is_warmed_before_any_note_is_written() {
    let dir = scratch("warm");
    let project = common::a_clip_project(4);
    let mut session = common::a_session_in(project, None);
    session.set_singer_folders(Vec::new());
    session.set_voicebanks(
        dir.join("voicebanks"),
        vec![BuiltinBackend {
            key: "fontelle-utau",
            kind: SingerKind::Utau,
            make: |_| Arc::new(Listener),
        }],
    );
    session.add_singer_channel(None).unwrap();
    let reference = fontelle_vocal::reference_phrase(&Hummer.voices()[0]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !HEARD.lock().unwrap().contains(&reference) {
        assert!(
            std::time::Instant::now() < deadline,
            "the singer was never warmed"
        );
        session.pump();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Two banks: the first is anybody's default.
struct Duo;

impl Backend for Duo {
    fn name(&self) -> &str {
        "Duo"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        let voice = |id: &str, name: &str, language: &str| VoiceInfo {
            id: id.into(),
            name: name.into(),
            languages: vec![language.into()],
            lowest: 36,
            highest: 84,
            research_only: false,
            terms: None,
            image: None,
        };
        vec![
            voice("utau/Plain", "Plain", "ja"),
            voice("utau/Joule_EN_CVVC_beta", "Joule EN CVVC \u{3b2}", "en"),
        ]
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn SingSession>, String> {
        Ok(Box::new(Hum))
    }
}

/// A file names the bank it was written for (OpenUtau's `singer:`, UTAU's
/// `VoiceDir`): when that bank is installed it sings the imported track,
/// in its own language — the demo song that ships with Joule's bank came
/// in sung by whichever bank was first. When it is not, the toast says so.
#[test]
fn an_imported_file_is_sung_by_the_bank_it_names() {
    let dir = scratch("named");
    let mut session = common::a_session_in(common::a_clip_project(4), None);
    session.set_singer_folders(Vec::new());
    session.set_voicebanks(
        dir.join("voicebanks"),
        vec![BuiltinBackend {
            key: "fontelle-utau",
            kind: SingerKind::Utau,
            make: |_| Arc::new(Duo),
        }],
    );
    let mut song = VocalSong::new("Daydream");
    let mut part = SongPart::new("Lead");
    part.singer = Some("Joule EN CVVC beta".into());
    part.notes = vec![vocal_note(0, PPQN, 60, "day")];
    song.parts.push(part);
    let path = dir.join("daydream.ustx");
    std::fs::write(&path, ustx::write(&song)).unwrap();
    let said = session.drop_file(&path).unwrap();
    let channel = vocal_channels(session.project())[0];
    assert_eq!(
        session.project().channels[channel].vocal,
        Some(fontelle_model::VocalVoice::new(
            "fontelle-utau",
            "utau/Joule_EN_CVVC_beta",
            "en"
        ))
    );
    assert!(said.contains("Joule EN CVVC \u{3b2}"), "{said}");

    // A bank that is not here: the first one sings, and the toast says so.
    song.parts[0].singer = Some("Nobody Here".into());
    let path = dir.join("other.ustx");
    std::fs::write(&path, ustx::write(&song)).unwrap();
    let said = session.drop_file(&path).unwrap();
    assert!(said.contains("Nobody Here"), "{said}");
    assert!(said.contains("not installed"), "{said}");
}

/// A click's edit is heard soon: walked with real banks, a UTAU phrase was
/// sung in about 25 ms and heard 225 ms after the edit — the rest was the
/// wait for edits to settle. The best of five edits on an instant singer
/// is heard well inside a fifth of a second.
#[test]
fn an_edit_is_heard_soon_after_it_is_made() {
    let (mut session, _dir) = session("soon");
    session.add_singer_channel(None).unwrap();
    let index = (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .unwrap();
    session.select_channel(index);
    let id = session.edit(fontelle_ui::canvas::RollEdit::Add {
        note: vocal_note(0, PPQN, 60, "la"),
    })[0];
    let sung = |s: &Session| {
        s.vocal_view().is_some_and(|v| {
            !v.phrases.is_empty()
                && v.phrases
                    .iter()
                    .all(|p| p.state == fontelle_ui::document::VocalRenderState::Final)
        })
    };
    let settle = |session: &mut Session| {
        let started = std::time::Instant::now();
        while !sung(session) {
            assert!(started.elapsed() < std::time::Duration::from_secs(10));
            session.pump();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        started.elapsed()
    };
    settle(&mut session);
    let mut best = std::time::Duration::MAX;
    for _ in 0..5 {
        session.edit(fontelle_ui::canvas::RollEdit::Move {
            ids: vec![id],
            tick_delta: 0,
            key_delta: 1,
        });
        session.end_gesture();
        best = best.min(settle(&mut session));
    }
    assert!(
        best < std::time::Duration::from_millis(170),
        "best {best:?}"
    );
}

/// Everything a reopen test's singer was asked to sing.
static REOPEN_HEARD: std::sync::Mutex<Vec<Phrase>> = std::sync::Mutex::new(Vec::new());
/// How many sessions (a voicebank's models, for a DiffSinger bank) it made.
static REOPEN_SESSIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Recorder;

impl Backend for Recorder {
    fn name(&self) -> &str {
        "Hummer"
    }
    fn version(&self) -> String {
        "1".into()
    }
    fn voices(&self) -> Vec<VoiceInfo> {
        Hummer.voices()
    }
    fn capabilities(&self, _voice: &str) -> Capabilities {
        Capabilities::default()
    }
    fn session(&self) -> Result<Box<dyn SingSession>, String> {
        REOPEN_SESSIONS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Box::new(Recording))
    }
}

struct Recording;

impl SingSession for Recording {
    fn sing(
        &mut self,
        phrase: &Phrase,
        quality: Quality,
        seed: u64,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Sung, SingError> {
        REOPEN_HEARD.lock().unwrap().push(phrase.clone());
        Hum.sing(phrase, quality, seed, cancelled)
    }
}

fn recording_session(project: Project, bundle: &Path) -> Session {
    let mut session = common::a_session_in(project, Some(bundle.to_path_buf()));
    session.set_singer_folders(Vec::new());
    session.set_voicebanks(
        bundle.with_file_name("voicebanks"),
        vec![BuiltinBackend {
            key: "fontelle-utau",
            kind: SingerKind::Utau,
            make: |_| Arc::new(Recorder),
        }],
    );
    session
}

fn all_final(session: &Session) -> bool {
    session.vocal_view().is_some_and(|v| {
        !v.phrases.is_empty()
            && v.phrases
                .iter()
                .all(|p| p.state == fontelle_ui::document::VocalRenderState::Final)
    })
}

fn pump_while(session: &mut Session, what: &str, until: impl Fn(&Session) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !until(session) {
        assert!(std::time::Instant::now() < deadline, "{what}: never");
        session.pump();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// A song reopened with every take on disk plays from the disk and loads
/// no singer: warming a DiffSinger bank loaded hundreds of megabytes of
/// models that nothing needed until an edit. The first edit warms it.
#[test]
fn a_reopened_song_sung_already_loads_no_singer_until_an_edit() {
    let dir = scratch("reopen-cold");
    let bundle = dir.join("Song.fontelle");
    fontelle_app::save_project(&common::a_clip_project(4), &bundle).unwrap();
    let mut session = recording_session(common::a_clip_project(4), &bundle);
    session.add_singer_channel(None).unwrap();
    let vocal = (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .unwrap();
    session.select_channel(vocal);
    let mut ids = Vec::new();
    for (beat, key) in [(0, 60), (1, 62), (4, 64)] {
        ids.push(
            session.edit(fontelle_ui::canvas::RollEdit::Add {
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
                    lyric: "a".into(),
                    voice: None,
                },
            })[0],
        );
    }
    session.end_gesture();
    pump_while(&mut session, "sung", all_final);
    DocumentHost::save(&mut session).unwrap();
    drop(session);

    REOPEN_HEARD.lock().unwrap().clear();
    REOPEN_SESSIONS.store(0, std::sync::atomic::Ordering::SeqCst);
    let project = fontelle_app::open_project(&bundle).unwrap().project;
    let mut reopened = recording_session(project, &bundle);
    let vocal = (0..reopened.channels().len())
        .rev()
        .find(|i| reopened.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .unwrap();
    reopened.select_channel(vocal);
    pump_while(&mut reopened, "reopened from the disk", all_final);
    // A while longer, as the window idles: still nothing loaded.
    let idle = std::time::Instant::now();
    while idle.elapsed() < std::time::Duration::from_millis(300) {
        reopened.pump();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        REOPEN_HEARD.lock().unwrap().is_empty(),
        "sung on reopen: {:?}",
        REOPEN_HEARD.lock().unwrap().len()
    );
    assert_eq!(
        REOPEN_SESSIONS.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a session (the models) was opened on reopen"
    );

    // The first edit warms the voice and sings the phrase edited.
    reopened.edit(fontelle_ui::canvas::RollEdit::Move {
        ids: vec![ids[2]],
        tick_delta: 0,
        key_delta: 2,
    });
    reopened.end_gesture();
    pump_while(&mut reopened, "the edit sung", all_final);
    let reference = fontelle_vocal::reference_phrase(&Hummer.voices()[0]);
    pump_while(&mut reopened, "warmed on the edit", |_| {
        REOPEN_HEARD.lock().unwrap().contains(&reference)
    });
    let heard = REOPEN_HEARD.lock().unwrap();
    assert!(
        heard
            .iter()
            .any(|p| p.notes.iter().any(|n| n.pitch == 66.0)),
        "the edited phrase was sung"
    );
}
