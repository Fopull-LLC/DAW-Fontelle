//! Between a file's song and the document: a part placed as a clip (from
//! the bar its first note is in, by beats), and a vocal track read back
//! out as a song to write (its clips' notes and lanes on the song's ticks,
//! a looped clip's every pass, the song's tempo and bar).

mod common;

use common::{assert_near, lane};
use fontelle_model::{
    Arena, Channel, Clip, ClipSource, Lane as Row, LaneCurve, NoteData, Project, TempoMap,
    VocalPart, VocalVoice,
};
use fontelle_sing::Lane;
use fontelle_types::{ChannelId, InstrumentKind, PPQN};
use fontelle_vocal_files::{SongPart, clip_of, song_of_channel, vocal_note};

const BAR: i64 = PPQN * 4;

#[test]
fn a_part_is_a_clip_from_the_bar_of_its_first_note() {
    let mut part = SongPart::new("Lead");
    part.notes = vec![
        vocal_note(BAR + PPQN, PPQN, 60, "a"),
        vocal_note(BAR * 2 + PPQN, PPQN, 62, "b"),
    ];
    part.vocal.lanes.insert(
        Lane::Dynamics,
        LaneCurve::from_points(vec![(BAR + PPQN, -6.0), (BAR * 2, 0.0)]),
    );
    let clip = clip_of(&part, 0, BAR);
    assert_eq!(clip.name, "Lead");
    assert_eq!(clip.start, BAR);
    // To the end of the bar the last note ends in.
    assert_eq!(clip.length, BAR * 2);
    let starts: Vec<i64> = clip.notes.iter().map(|n| n.start).collect();
    assert_eq!(starts, vec![PPQN, BAR + PPQN]);
    let dynamics = &clip.vocal.as_ref().unwrap().lanes[&Lane::Dynamics];
    assert_near(
        "lane moved with the notes",
        dynamics.at(PPQN, 0.0),
        -6.0,
        1e-4,
    );

    // Dropped at bar 9: everything nine bars on, by beats.
    let later = clip_of(&part, BAR * 8, BAR);
    assert_eq!(later.start, BAR * 9);
    assert_eq!(later.notes[0].start, PPQN);
}

fn channel(project: &mut Project) -> ChannelId {
    project.channels.insert(Channel {
        preset: None,
        instrument: Some(InstrumentKind::Vocal),
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
        vocal: Some(VocalVoice::new("fontelle-utau", "Teto", "ja")),
        vocal_style: Default::default(),
    })
}

#[test]
fn a_vocal_track_reads_back_as_a_song() {
    let mut project = Project::new("Back");
    project.tempo_map = TempoMap::new(96.0, 48_000.0);
    project.beats_per_bar = 3;
    let voice = channel(&mut project);
    let row = project.lanes.insert(Row {
        name: "Row".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        soloed: false,
        order: 0,
    });
    let clip = |start: i64, loop_length: Option<i64>, notes: Vec<fontelle_model::Note>| {
        let mut arena = Arena::default();
        for n in notes {
            arena.insert(n);
        }
        let mut part = VocalPart::default();
        part.lanes
            .insert(Lane::Breathiness, LaneCurve::from_points(vec![(0, 0.5)]));
        Clip {
            name: None,
            lane: row,
            start,
            length: loop_length.map_or(BAR, |l| l * 2),
            source: ClipSource::Notes(NoteData {
                channel: voice,
                notes: arena,
                vocal: Some(Box::new(part)),
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length,
        }
    };
    project
        .clips
        .insert(clip(0, None, vec![vocal_note(0, PPQN, 60, "la")]));
    // Looped twice from bar 3.
    project.clips.insert(clip(
        BAR * 2,
        Some(PPQN * 2),
        vec![vocal_note(0, PPQN, 64, "li")],
    ));
    let song = song_of_channel(&project, voice);
    assert_eq!(song.name, "Voice");
    assert_eq!(song.beats_per_bar, 3);
    assert_near("tempo", song.tempo[0].bpm as f32, 96.0, 1e-6);
    let part = &song.parts[0];
    let notes: Vec<(i64, &str)> = part
        .notes
        .iter()
        .map(|n| (n.start, n.lyric.as_str()))
        .collect();
    assert_eq!(
        notes,
        vec![(0, "la"), (BAR * 2, "li"), (BAR * 2 + PPQN * 2, "li")]
    );
    assert_near(
        "lane",
        lane(part, Lane::Breathiness, BAR * 2 + 10),
        0.5,
        1e-4,
    );
}
