//! UTAU's `.ust`: notes, lyrics, tempo, velocity, intensity, flags, the
//! Mode2 pitch bend (PBS/PBW/PBY/PBM) and vibrato (VBR).
//!
//! The fixture is written here, by hand, in the format UTAU saves.

mod common;

use common::{assert_near, lane};
use fontelle_sing::{GlideShape, Lane};
use fontelle_vocal_files::{Format, ust};

/// 120 bpm: a tick (960 a quarter) is 0.52083 ms.
const MS: f64 = 960.0 * 2.0 / 1000.0;

fn ticks(ms: f64) -> i64 {
    (ms * MS).round() as i64
}

const SONG: &str = "[#VERSION]
UST Version1.2
[#SETTING]
Tempo=120.00
Tracks=1
ProjectName=Test Song
VoiceDir=%VOICE%uta
Mode2=True
Flags=B60
[#0000]
Length=480
Lyric=R
NoteNum=60
[#0001]
Length=480
Lyric=\u{3042}
NoteNum=60
Intensity=100
Modulation=30
Velocity=100
PBS=-40;0
PBW=80
PBY=0
[#0002]
Length=960
Lyric=\u{3044}
NoteNum=62
Intensity=50
Velocity=150
Flags=g-10
PBS=-50;-20
PBW=50,100
PBY=10,0
PBM=,s
VBR=50,200,40,20,30,0,0,0
[#0003]
Length=480
Lyric=\u{3046}
NoteNum=64
Tempo=60.00
[#TRACKEND]
";

#[test]
fn notes_lyrics_and_tempo_land_on_the_grid() {
    let song = ust::read(SONG.as_bytes(), "file").unwrap();
    assert_eq!(song.name, "Test Song");
    assert_eq!(song.parts.len(), 1);
    let part = &song.parts[0];
    // The rest is not a note; UTAU's 480 a quarter is 960 here.
    let notes: Vec<(i64, i64, u8, &str)> = part
        .notes
        .iter()
        .map(|n| (n.start, n.length, n.key, n.lyric.as_str()))
        .collect();
    assert_eq!(
        notes,
        vec![
            (960, 960, 60, "\u{3042}"),
            (1920, 1920, 62, "\u{3044}"),
            (3840, 960, 64, "\u{3046}"),
        ]
    );
    // A note's Tempo= changes the tempo where that note starts.
    let tempo: Vec<(i64, f64)> = song.tempo.iter().map(|s| (s.start_tick, s.bpm)).collect();
    assert_eq!(tempo, vec![(0, 120.0), (3840, 60.0)]);
}

#[test]
fn velocity_is_consonant_speed_and_intensity_is_dynamics() {
    let song = ust::read(SONG.as_bytes(), "file").unwrap();
    let part = &song.parts[0];
    assert_eq!(part.notes[0].velocity, 100);
    // UTAU's 150 is a consonant 2^0.5 as quick: 100 + 27 * 0.5.
    assert_eq!(part.notes[1].velocity, 114);
    // Intensity 50 is half the amplitude.
    assert_near(
        "dyn in the quiet note",
        lane(part, Lane::Dynamics, 2600),
        -6.02,
        0.05,
    );
    assert_near(
        "dyn in the first note",
        lane(part, Lane::Dynamics, 1300),
        0.0,
        0.01,
    );
}

#[test]
fn flags_become_gender_and_breathiness() {
    let song = ust::read(SONG.as_bytes(), "file").unwrap();
    let part = &song.parts[0];
    // B60 for the whole song (50 is the voice's own); g-10 on the second note
    // is a little younger/brighter.
    assert_near("breath", lane(part, Lane::Breathiness, 1300), 0.2, 0.01);
    assert_near("breath", lane(part, Lane::Breathiness, 4000), 0.2, 0.01);
    assert_near("gender", lane(part, Lane::Gender, 2600), 60.0, 0.5);
    assert_near("gender elsewhere", lane(part, Lane::Gender, 1300), 0.0, 0.5);
}

#[test]
fn vbr_is_the_notes_vibrato() {
    let song = ust::read(SONG.as_bytes(), "file").unwrap();
    let v = song.parts[0].notes[1]
        .voice
        .as_ref()
        .and_then(|v| v.vibrato)
        .expect("vibrato");
    assert_near("depth", v.depth, 40.0, 1e-3);
    assert_near("rate", v.rate, 5.0, 1e-3);
    assert_near("delay", v.delay, 0.5, 1e-3);
    assert_near("fade in", v.fade_in, 0.2, 1e-3);
    assert_near("fade out", v.fade_out, 0.3, 1e-3);
    assert!(
        song.parts[0].notes[0]
            .voice
            .as_ref()
            .and_then(|v| v.vibrato)
            .is_none()
    );
}

#[test]
fn the_mode2_pitch_bend_is_the_pitch_lane_over_the_notes() {
    let song = ust::read(SONG.as_bytes(), "file").unwrap();
    let part = &song.parts[0];
    let second = 1920;
    // PBS=-50;-20 starts 50 ms early at the first note's pitch (2 semitones
    // under this one), rises (io) to +1 semitone at the note's start, then
    // falls (s: straight) to the note over 100 ms.
    assert_near(
        "at the note's start",
        lane(part, Lane::PitchBend, second),
        100.0,
        3.0,
    );
    assert_near(
        "50 ms in, halfway down the straight line",
        lane(part, Lane::PitchBend, second + ticks(50.0)),
        50.0,
        3.0,
    );
    // 25 ms before, over the first note (60): halfway up the curve, 6150
    // cents, which is 150 over the note sounding there.
    assert_near(
        "the lead-in, over the note before",
        lane(part, Lane::PitchBend, second - ticks(25.0)),
        150.0,
        4.0,
    );
    assert_near(
        "held at the note after",
        lane(part, Lane::PitchBend, second + ticks(300.0)),
        0.0,
        0.5,
    );
    assert_near(
        "the first note, flat",
        lane(part, Lane::PitchBend, 1200),
        0.0,
        0.5,
    );
    // The drawn curve is the glide: the backend adds none of its own.
    let glide = part.notes[1]
        .voice
        .as_ref()
        .and_then(|v| v.portamento)
        .expect("a glide of none");
    assert_eq!(glide.length, 0.0);
    assert_eq!(glide.shape, GlideShape::Linear);
}

#[test]
fn what_has_no_place_is_named() {
    let song = ust::read(SONG.as_bytes(), "file").unwrap();
    assert!(
        song.unmapped.iter().any(|u| u.contains("Modulation")),
        "{:?}",
        song.unmapped
    );
}

#[test]
fn a_shift_jis_ust_reads_its_lyrics() {
    let sjis = encoding_rs::SHIFT_JIS.encode(SONG).0.into_owned();
    assert!(std::str::from_utf8(&sjis).is_err(), "really Shift-JIS");
    let song = ust::read(&sjis, "file").unwrap();
    assert_eq!(song.parts[0].notes[0].lyric, "\u{3042}");
    assert_eq!(song.parts[0].notes[2].lyric, "\u{3046}");
}

#[test]
fn vcv_context_is_left_to_the_phonemizer() {
    // "a か" is UTAU's VCV alias for か after an a; the lyric is the か.
    let song = ust::read(
        "[#SETTING]\nTempo=120\n[#0000]\nLength=480\nLyric=- \u{304b}\nNoteNum=60\n[#0001]\nLength=480\nLyric=a \u{304b}\nNoteNum=60\n[#TRACKEND]\n"
            .as_bytes(),
        "x",
    )
    .unwrap();
    let lyrics: Vec<&str> = song.parts[0]
        .notes
        .iter()
        .map(|n| n.lyric.as_str())
        .collect();
    assert_eq!(lyrics, vec!["\u{304b}", "\u{304b}"]);
}

#[test]
fn a_mode1_pitchbend_list_is_read_too() {
    // No PBS: the old list of cents every 5 UTAU ticks from PBStart ms.
    let song = ust::read(
        "[#SETTING]\nTempo=120\n[#0000]\nLength=480\nLyric=a\nNoteNum=60\nPBType=5\nPBStart=0\nPitchBend=0,20,40,40,40\n[#TRACKEND]\n"
            .as_bytes(),
        "x",
    )
    .unwrap();
    let part = &song.parts[0];
    assert_near("third value", lane(part, Lane::PitchBend, 20), 40.0, 1.0);
    assert_near("first", lane(part, Lane::PitchBend, 0), 0.0, 1.0);
}

#[test]
fn the_extension_says_ust() {
    assert_eq!(
        Format::of_path(std::path::Path::new("a/b.UST")),
        Some(Format::Ust)
    );
}

/// The bank a UST was written for is its `VoiceDir`'s last folder —
/// `%VOICE%uta`, or a whole path on the writer's computer — so the
/// importing studio can choose that bank when it has it.
#[test]
fn the_voice_folder_names_the_singer() {
    let song = ust::read(SONG.as_bytes(), "file").unwrap();
    assert_eq!(song.parts[0].singer.as_deref(), Some("uta"));
    let windows = SONG.replace(
        "VoiceDir=%VOICE%uta",
        "VoiceDir=C:\\Program Files (x86)\\UTAU\\voice\\重音テト音声ライブラリー\\",
    );
    let song = ust::read(windows.as_bytes(), "file").unwrap();
    assert_eq!(
        song.parts[0].singer.as_deref(),
        Some("重音テト音声ライブラリー")
    );
}
