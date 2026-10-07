//! The vocal track in the document: a channel whose instrument is a voice,
//! notes that carry lyrics, the commands that change them, and the format
//! that keeps them.
//!
//! The singing itself is somebody else's (`fontelle-singer-abi`); the song
//! only says *who* sings, *in which language*, and *what words*. Everything
//! here has to survive a save, an undo and a trip across the wire, and a
//! song written before any of it existed has to open exactly as it was.

use fontelle_model::lyrics::{default_syllable, spread};
use fontelle_model::{
    Arena, Channel, Clip, ClipSource, Command, Edit, Lane, Note, NoteData, PROJECT_FORMAT_VERSION,
    Project, SetLyrics, SetSinger, TempoMap, VocalVoice, load_project, save_project,
};
use fontelle_types::{ChannelId, ClipId, InstrumentKind, NoteId, PPQN};

fn a_note(start: i64, key: u8, lyric: &str) -> Note {
    Note {
        start,
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
        lyric: lyric.to_string(),
        voice: None,
    }
}

struct Fixture {
    project: Project,
    channel: ChannelId,
    clip: ClipId,
    notes: Vec<NoteId>,
}

fn fixture(vocal: bool) -> Fixture {
    let mut project = Project::new("vocal");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let channel = project.channels.insert(Channel {
        preset: None,
        instrument: vocal.then_some(InstrumentKind::Vocal),
        name: "Voice".into(),
        color: [0; 4],
        mixer_track: None,
        patch_data: None,
        plugin: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        ab: Default::default(),
        gain_db: 0.0,
        vocal: vocal.then(|| VocalVoice::new("Some Engine", "alto", "en")),
    });
    let lane = project.lanes.insert(Lane {
        name: "Lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        soloed: false,
        order: 0,
    });
    let mut notes = Arena::default();
    let words = if vocal {
        ["hel-", "lo", "world"]
    } else {
        ["", "", ""]
    };
    let ids = vec![
        notes.insert(a_note(0, 60, words[0])),
        notes.insert(a_note(PPQN, 62, words[1])),
        notes.insert(a_note(PPQN * 2, 64, words[2])),
    ];
    let clip = project.clips.insert(Clip {
        name: None,
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData {
            channel,
            notes,
            vocal: None,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    Fixture {
        project,
        channel,
        clip,
        notes: ids,
    }
}

fn lyrics(project: &Project, clip: ClipId, ids: &[NoteId]) -> Vec<String> {
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("a note clip");
    };
    ids.iter().map(|id| data.notes[*id].lyric.clone()).collect()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fontelle-vocal-model-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

// ------------------------------------------------------------------ format

#[test]
fn a_vocal_song_saves_and_opens_with_its_singer_and_words() {
    let f = fixture(true);
    let dir = scratch("roundtrip");
    save_project(&f.project, &dir).unwrap();
    let back = load_project(&dir).unwrap();
    let channel = &back.channels[f.channel];
    assert_eq!(channel.instrument, Some(InstrumentKind::Vocal));
    assert_eq!(
        channel.vocal,
        Some(VocalVoice::new("Some Engine", "alto", "en"))
    );
    assert_eq!(lyrics(&back, f.clip, &f.notes), vec!["hel-", "lo", "world"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A song with a voice in it is stamped with the format that has voices, so
/// a build from before them refuses it by version ("upgrade Fontelle")
/// instead of failing on whichever field it met first.
#[test]
fn a_vocal_song_is_stamped_with_the_format_that_has_voices() {
    assert_eq!(PROJECT_FORMAT_VERSION, 3);
    let f = fixture(true);
    assert_eq!(f.project.format_needed(), 2);
    let dir = scratch("stamp");
    save_project(&f.project, &dir).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("project.json")).unwrap()).unwrap();
    assert_eq!(json["meta"]["format_version"], serde_json::json!(2));
    let _ = std::fs::remove_dir_all(&dir);
}

/// ...and a song without one keeps the format every earlier build reads, so
/// trying this build costs nobody the songs they made with it.
#[test]
fn a_song_without_a_voice_stays_in_the_format_older_builds_open() {
    let f = fixture(false);
    assert_eq!(f.project.format_needed(), 1);
    let dir = scratch("plain");
    save_project(&f.project, &dir).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["meta"]["format_version"], serde_json::json!(1));
    // Nothing new is written for what is not there.
    assert!(
        !text.contains("\"lyric\":"),
        "an empty lyric is not written"
    );
    assert!(!text.contains("\"vocal\":"), "no voice is not written");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_format_1_song_opens_unchanged() {
    let f = fixture(false);
    let dir = scratch("v1");
    save_project(&f.project, &dir).unwrap();
    // As a format-1 build wrote it: no lyric on any note, no voice on any
    // channel.
    let path = dir.join("project.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    json["meta"]["format_version"] = serde_json::json!(1);
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let back = load_project(&dir).unwrap();
    assert_eq!(back.channels[f.channel].vocal, None);
    assert_eq!(back.channels[f.channel].instrument, None);
    assert_eq!(lyrics(&back, f.clip, &f.notes), vec!["", "", ""]);
    assert_eq!(back.meta.format_version, PROJECT_FORMAT_VERSION);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- commands

#[test]
fn set_lyrics_writes_a_line_in_one_undo_step() {
    let mut f = fixture(true);
    let mut set = SetLyrics::new(
        f.clip,
        f.notes.clone(),
        vec!["a-".into(), "ma-".into(), "zing".into()],
    );
    set.apply(&mut f.project).unwrap();
    assert_eq!(
        lyrics(&f.project, f.clip, &f.notes),
        vec!["a-", "ma-", "zing"]
    );
    let mut undo = set.invert();
    undo.apply(&mut f.project).unwrap();
    assert_eq!(
        lyrics(&f.project, f.clip, &f.notes),
        vec!["hel-", "lo", "world"]
    );
    // Redo puts back the same line, not whatever is there the second time.
    set.apply(&mut f.project).unwrap();
    assert_eq!(
        lyrics(&f.project, f.clip, &f.notes),
        vec!["a-", "ma-", "zing"]
    );
    assert!(set.label().contains("lyric"), "{}", set.label());
}

#[test]
fn set_lyrics_that_does_not_line_up_changes_nothing() {
    let mut f = fixture(true);
    let mut set = SetLyrics::new(f.clip, f.notes.clone(), vec!["la".into()]);
    assert!(set.apply(&mut f.project).is_err());
    assert_eq!(
        lyrics(&f.project, f.clip, &f.notes),
        vec!["hel-", "lo", "world"]
    );
}

#[test]
fn set_lyrics_crosses_the_wire_as_itself() {
    let mut f = fixture(true);
    let mut set = SetLyrics::new(f.clip, vec![f.notes[1]], vec!["ら".into()]);
    set.apply(&mut f.project).unwrap();
    let bytes = set.to_edit().to_bytes();
    let edit = Edit::from_bytes(&bytes).unwrap();
    assert_eq!(edit.tag(), "SetLyrics");
    let mut there = fixture(true);
    edit.into_command().apply(&mut there.project).unwrap();
    assert_eq!(lyrics(&there.project, there.clip, &there.notes)[1], "ら");
}

#[test]
fn set_singer_chooses_a_voice_and_undo_takes_it_back() {
    let mut f = fixture(false);
    let mut set = SetSinger::new(
        f.channel,
        Some(VocalVoice::new("Some Engine", "soprano", "ja")),
    );
    set.apply(&mut f.project).unwrap();
    assert_eq!(
        f.project.channels[f.channel].vocal,
        Some(VocalVoice::new("Some Engine", "soprano", "ja"))
    );
    set.invert().apply(&mut f.project).unwrap();
    assert_eq!(f.project.channels[f.channel].vocal, None);
    let bytes = set.to_edit().to_bytes();
    assert_eq!(Edit::from_bytes(&bytes).unwrap().tag(), "SetSinger");
}

// ------------------------------------------------------------------ lyrics

#[test]
fn a_line_spreads_a_word_per_note() {
    let s = spread("hello my world", 3);
    assert_eq!(s.lyrics, vec!["hello", "my", "world"]);
    assert_eq!(s.left_over, 0);
}

#[test]
fn hyphenated_words_spread_a_syllable_per_note() {
    let s = spread("a-ma-zing  grace", 4);
    assert_eq!(s.lyrics, vec!["a-", "ma-", "zing", "grace"]);
    let s = spread("hel- lo", 2);
    assert_eq!(s.lyrics, vec!["hel-", "lo"]);
}

#[test]
fn a_slur_is_a_note_of_its_own() {
    let s = spread("oh - - yeah", 4);
    assert_eq!(s.lyrics, vec!["oh", "-", "-", "yeah"]);
}

#[test]
fn bracketed_phonemes_stay_one_lyric() {
    let s = spread("[h ə l oʊ] there", 2);
    assert_eq!(s.lyrics, vec!["[h ə l oʊ]", "there"]);
}

#[test]
fn kana_spread_a_mora_per_note() {
    let s = spread("さくら きょう", 5);
    assert_eq!(s.lyrics, vec!["さ", "く", "ら", "きょ", "う"]);
}

#[test]
fn chinese_characters_spread_a_character_per_note_with_their_readings() {
    let s = spread("我爱你 银行[hang]", 5);
    assert_eq!(s.lyrics, vec!["我", "爱", "你", "银", "行[hang]"]);
    assert_eq!(s.left_over, 0);
    // Punctuation stays with the character before it.
    let s = spread("你好，世界", 4);
    assert_eq!(s.lyrics, vec!["你", "好，", "世", "界"]);
}

#[test]
fn hangul_spreads_a_syllable_per_note() {
    let s = spread("사랑해 음악", 5);
    assert_eq!(s.lyrics, vec!["사", "랑", "해", "음", "악"]);
}

#[test]
fn fewer_words_than_notes_fills_the_first_notes_and_more_are_counted() {
    let s = spread("one two", 4);
    assert_eq!(s.lyrics, vec!["one", "two"]);
    assert_eq!(s.left_over, 0);
    let s = spread("one two three", 2);
    assert_eq!(s.lyrics, vec!["one", "two"]);
    assert_eq!(s.left_over, 1);
}

#[test]
fn each_language_has_a_syllable_a_new_note_sings() {
    assert_eq!(default_syllable("en"), "la");
    assert_eq!(default_syllable("en-US"), "la");
    assert_eq!(default_syllable("ja"), "ら");
    assert_eq!(default_syllable("ko"), "라");
    assert_eq!(default_syllable("zh"), "la");
    assert_eq!(default_syllable("xx"), "la");
}
