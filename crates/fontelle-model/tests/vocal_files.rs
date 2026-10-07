//! A vocal song file brought into the document (`ImportVocal`): one vocal
//! track per part, or onto the vocal track that is selected, with the file's
//! tempo when asked — each one undo step. And a vocal track **frozen**
//! (`FreezeVocal` / `UnfreezeVocal`): its render on a row of its own, the
//! part kept and silent until it is unfrozen — one undo step each way.

use fontelle_model::{
    Channel, Clip, ClipSource, Command, Edit, FreezeVocal, History, ImportVocal, Lane, LaneCurve,
    Note, NoteData, Project, TempoSegment, UnfreezeVocal, VocalClipImport, VocalLanding, VocalPart,
    VocalVoice, load_project, save_project,
};
use fontelle_sing::Lane as Sung;
use fontelle_types::{AssetKind, AssetRef, AudioClipData, ChannelId, InstrumentKind, PPQN};

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

fn a_part(name: &str, start: i64) -> VocalClipImport {
    let mut vocal = VocalPart::default();
    vocal.lanes.insert(
        Sung::Dynamics,
        LaneCurve::from_points(vec![(0, -3.0), (PPQN, 0.0)]),
    );
    VocalClipImport {
        name: name.to_string(),
        start,
        length: PPQN * 4,
        notes: vec![a_note(0, 60, "la"), a_note(PPQN, 62, "li")],
        vocal: Some(Box::new(vocal)),
    }
}

fn a_song() -> Project {
    let mut project = Project::new("files");
    project.lanes.insert(Lane {
        name: "Existing".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        soloed: false,
        order: 0,
    });
    project
}

fn vocal_channel(project: &mut Project) -> ChannelId {
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
    })
}

fn voice() -> VocalVoice {
    VocalVoice::new("fontelle-utau", "Teto", "ja")
}

/// The notes clips on `channel`, as (start, length, lyrics).
fn clips_on(project: &Project, channel: ChannelId) -> Vec<(i64, i64, Vec<String>)> {
    let mut out: Vec<_> = project
        .clips
        .values()
        .filter_map(|clip| match &clip.source {
            ClipSource::Notes(data) if data.channel == channel => {
                let mut notes: Vec<&Note> = data.notes.values().collect();
                notes.sort_by_key(|n| n.start);
                Some((
                    clip.start,
                    clip.length,
                    notes.iter().map(|n| n.lyric.clone()).collect(),
                ))
            }
            _ => None,
        })
        .collect();
    out.sort();
    out
}

#[test]
fn each_part_is_a_vocal_track_of_its_own_in_one_undo() {
    let mut project = a_song();
    let before = project.channels.len();
    let mut history = History::new();
    let import = ImportVocal::new(
        "song.ustx",
        vec![a_part("Lead", PPQN * 4), a_part("Harmony", 0)],
        VocalLanding::NewTracks {
            at: Some(0),
            voice: Some(voice()),
        },
    );
    history.apply(Box::new(import), &mut project).unwrap();
    assert_eq!(project.channels.len(), before + 2);
    let lead = project
        .channels
        .iter()
        .find(|(_, c)| c.name == "Lead")
        .map(|(id, _)| id)
        .expect("a track named for the part");
    let channel = &project.channels[lead];
    assert_eq!(channel.instrument, Some(InstrumentKind::Vocal));
    assert_eq!(channel.vocal, Some(voice()));
    assert_eq!(
        clips_on(&project, lead),
        vec![(PPQN * 4, PPQN * 4, vec!["la".to_string(), "li".to_string()])]
    );
    // The part's lanes came with it.
    let clip = project
        .clips
        .values()
        .find(|c| matches!(&c.source, ClipSource::Notes(d) if d.channel == lead))
        .unwrap();
    let ClipSource::Notes(data) = &clip.source else {
        unreachable!()
    };
    assert!(
        data.vocal
            .as_ref()
            .unwrap()
            .lanes
            .contains_key(&Sung::Dynamics)
    );
    // The rows went where they were asked: the top, in the file's order.
    let rows: Vec<String> = project
        .lane_ids()
        .iter()
        .map(|id| project.lanes[*id].name.clone())
        .collect();
    assert_eq!(rows, vec!["Lead", "Harmony", "Existing"]);

    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.channels.len(), before);
    assert_eq!(project.lanes.len(), 1);
    assert!(project.clips.is_empty());
    // A redo puts it back under the same ids.
    history.redo(&mut project).unwrap().unwrap();
    assert_eq!(clips_on(&project, lead).len(), 1);
}

#[test]
fn onto_a_vocal_track_it_is_a_clip_on_its_row() {
    let mut project = a_song();
    let channel = vocal_channel(&mut project);
    let row = project.lane_ids()[0];
    let mut history = History::new();
    history
        .apply(
            Box::new(ImportVocal::new(
                "song.ust",
                vec![a_part("Ignored", PPQN * 8)],
                VocalLanding::Onto { channel, lane: row },
            )),
            &mut project,
        )
        .unwrap();
    assert_eq!(project.channels.len(), 1, "no track was made");
    assert_eq!(project.lanes.len(), 1, "no row was made");
    assert_eq!(clips_on(&project, channel)[0].0, PPQN * 8);
    history.undo(&mut project).unwrap().unwrap();
    assert!(clips_on(&project, channel).is_empty());
}

#[test]
fn the_files_tempo_and_metre_come_and_go_with_the_import() {
    let mut project = a_song();
    let mut history = History::new();
    let tempo = vec![
        TempoSegment {
            start_tick: 0,
            bpm: 90.0,
        },
        TempoSegment {
            start_tick: PPQN * 8,
            bpm: 140.0,
        },
    ];
    history
        .apply(
            Box::new(
                ImportVocal::new(
                    "song.ust",
                    vec![a_part("Lead", 0)],
                    VocalLanding::NewTracks {
                        at: None,
                        voice: None,
                    },
                )
                .with_tempo(tempo.clone(), 3),
            ),
            &mut project,
        )
        .unwrap();
    assert_eq!(project.tempo_map.segments(), &tempo[..]);
    assert_eq!(project.beats_per_bar, 3);
    history.undo(&mut project).unwrap().unwrap();
    assert_eq!(project.tempo_map.segments().len(), 1);
    assert_eq!(project.tempo_map.tempo_at(0), 120.0);
    assert_eq!(project.beats_per_bar, 4);
}

#[test]
fn an_import_crosses_the_wire_as_the_same_edit() {
    let mut here = a_song();
    let mut there = here.clone();
    let mut import = ImportVocal::new(
        "song.ustx",
        vec![a_part("Lead", 0)],
        VocalLanding::NewTracks {
            at: None,
            voice: Some(voice()),
        },
    )
    .with_tempo(
        vec![TempoSegment {
            start_tick: 0,
            bpm: 100.0,
        }],
        4,
    );
    import.apply(&mut here).unwrap();
    let edit = Edit::from_bytes(&import.to_edit().to_bytes()).unwrap();
    assert_eq!(edit.tag(), "ImportVocal");
    edit.into_command().apply(&mut there).unwrap();
    assert_eq!(
        serde_json::to_value(&here.clips).unwrap(),
        serde_json::to_value(&there.clips).unwrap()
    );
}

// ------------------------------------------------------------------ freeze

fn a_render() -> AudioClipData {
    AudioClipData::whole(
        AssetRef {
            id: fontelle_types::AssetId::default(),
            path: "renders/Voice (frozen).wav".into(),
            content_hash: 0,
            size: 0,
            kind: AssetKind::Sample,
        },
        96_000,
        48_000,
    )
}

/// A vocal track with one clip on the first row.
fn sung() -> (Project, ChannelId) {
    let mut project = a_song();
    let channel = vocal_channel(&mut project);
    let row = project.lane_ids()[0];
    let mut notes = fontelle_model::Arena::default();
    notes.insert(a_note(0, 60, "la"));
    project.clips.insert(Clip {
        name: None,
        lane: row,
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
    (project, channel)
}

fn audio_clips(project: &Project) -> usize {
    project
        .clips
        .values()
        .filter(|c| matches!(c.source, ClipSource::Audio(_)))
        .count()
}

#[test]
fn freezing_swaps_in_the_render_and_keeps_the_part() {
    let (mut project, channel) = sung();
    let mut history = History::new();
    assert!(!project.is_frozen(channel));
    history
        .apply(
            Box::new(FreezeVocal::new(
                channel,
                "Voice (frozen)",
                a_render(),
                0,
                PPQN * 4,
                1,
            )),
            &mut project,
        )
        .unwrap();
    assert!(project.is_frozen(channel));
    assert_eq!(audio_clips(&project), 1);
    // On a row of its own, under the part's, named for the track.
    let rows: Vec<String> = project
        .lane_ids()
        .iter()
        .map(|id| project.lanes[*id].name.clone())
        .collect();
    assert_eq!(rows, vec!["Existing", "Voice (frozen)"]);
    // The part is all still there.
    assert_eq!(clips_on(&project, channel).len(), 1);

    history.undo(&mut project).unwrap().unwrap();
    assert!(!project.is_frozen(channel));
    assert_eq!(audio_clips(&project), 0);
    assert_eq!(project.lanes.len(), 1);
    history.redo(&mut project).unwrap().unwrap();
    assert!(project.is_frozen(channel));
}

#[test]
fn unfreezing_takes_the_render_away_in_one_undo() {
    let (mut project, channel) = sung();
    let mut history = History::new();
    history
        .apply(
            Box::new(FreezeVocal::new(
                channel,
                "Voice (frozen)",
                a_render(),
                0,
                PPQN * 4,
                1,
            )),
            &mut project,
        )
        .unwrap();
    history
        .apply(Box::new(UnfreezeVocal::new(channel)), &mut project)
        .unwrap();
    assert!(!project.is_frozen(channel));
    assert_eq!(audio_clips(&project), 0);
    assert_eq!(project.lanes.len(), 1, "the row it made went with it");
    // Undoing the unfreeze is frozen again, render and row.
    history.undo(&mut project).unwrap().unwrap();
    assert!(project.is_frozen(channel));
    assert_eq!(audio_clips(&project), 1);
    assert_eq!(project.lanes.len(), 2);
}

#[test]
fn a_track_frozen_twice_or_unfrozen_when_it_is_not_is_refused() {
    let (mut project, channel) = sung();
    let freeze = || FreezeVocal::new(channel, "V", a_render(), 0, PPQN, 1);
    freeze().apply(&mut project).unwrap();
    assert!(freeze().apply(&mut project).is_err());
    UnfreezeVocal::new(channel).apply(&mut project).unwrap();
    assert!(UnfreezeVocal::new(channel).apply(&mut project).is_err());
}

#[test]
fn a_frozen_song_saves_and_opens_frozen_and_old_songs_open_unfrozen() {
    let (mut project, channel) = sung();
    FreezeVocal::new(channel, "V", a_render(), 0, PPQN, 1)
        .apply(&mut project)
        .unwrap();
    assert_eq!(project.format_needed(), 3);
    let dir = std::env::temp_dir().join(format!("fontelle-frozen-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let bundle = dir.join("Frozen.fontelle");
    save_project(&project, &bundle).unwrap();
    let opened = load_project(&bundle).unwrap();
    assert!(opened.is_frozen(channel));
    // A song with nothing frozen writes no word of it.
    let (plain, _) = sung();
    let json = serde_json::to_string(&plain).unwrap();
    assert!(!json.contains("frozen"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn freezing_crosses_the_wire() {
    let (mut here, channel) = sung();
    let mut there = here.clone();
    let mut freeze = FreezeVocal::new(channel, "V", a_render(), 0, PPQN, 1);
    freeze.apply(&mut here).unwrap();
    let edit = Edit::from_bytes(&freeze.to_edit().to_bytes()).unwrap();
    assert_eq!(edit.tag(), "FreezeVocal");
    edit.into_command().apply(&mut there).unwrap();
    assert!(there.is_frozen(channel));
    let mut unfreeze = UnfreezeVocal::new(channel);
    unfreeze.apply(&mut here).unwrap();
    let edit = Edit::from_bytes(&unfreeze.to_edit().to_bytes()).unwrap();
    assert_eq!(edit.tag(), "UnfreezeVocal");
    edit.into_command().apply(&mut there).unwrap();
    assert!(!there.is_frozen(channel));
}
