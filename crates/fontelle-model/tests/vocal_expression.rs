//! A vocal part's expression in the document: what each note does beyond
//! its lyric (phonemes, vibrato, the glide in, a backend's own options), the
//! lanes drawn under the roll (pitch bend, dynamics, ... and each backend's
//! own), and each backend's settings and state — the commands that change
//! them, one undo step a gesture, across the wire, and the format that keeps
//! them while older songs open as they were.

use std::collections::BTreeMap;

use fontelle_model::{
    Arena, Channel, Clip, ClipSource, Command, Edit, History, Lane, LaneCurve, LaneKey, Note,
    NoteData, NoteVoice, PROJECT_FORMAT_VERSION, Project, SetNoteVoices, SetSinger, SetVocalLane,
    SetVocalSettings, SetVocalState, TempoMap, VocalPart, VocalVoice, consonant_speed, lane_spec,
    load_project, save_project, velocity_for_speed,
};
use fontelle_sing::{GlideShape, Lane as Curve, PhonemeEdit, Portamento, Vibrato};
use fontelle_types::{ChannelId, ClipId, InstrumentKind, NoteId, PPQN};

const NEUTRAL: f32 = 0.0;

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
        muted: false,
    }
}

struct Fixture {
    project: Project,
    channel: ChannelId,
    clip: ClipId,
    notes: Vec<NoteId>,
}

fn fixture() -> Fixture {
    let mut project = Project::new("expression");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let channel = project.channels.insert(Channel {
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
        vocal: Some(VocalVoice::new("fontelle-utau", "utau:Teto", "ja")),
        vocal_style: Default::default(),
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
    let ids = vec![
        notes.insert(a_note(0, 60, "さ")),
        notes.insert(a_note(PPQN, 62, "く")),
        notes.insert(a_note(PPQN * 2, 64, "ら")),
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

fn data(project: &Project, clip: ClipId) -> &NoteData {
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("a note clip");
    };
    data
}

fn voice_of(project: &Project, clip: ClipId, id: NoteId) -> Option<NoteVoice> {
    data(project, clip).notes[id].voice.as_deref().cloned()
}

fn part(project: &Project, clip: ClipId) -> VocalPart {
    data(project, clip)
        .vocal
        .as_deref()
        .cloned()
        .unwrap_or_default()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fontelle-vocal-expression-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn vibrato() -> Vibrato {
    Vibrato {
        depth: 40.0,
        rate: 6.0,
        delay: 0.3,
        fade_in: 0.2,
        fade_out: 0.1,
    }
}

// ------------------------------------------------------------ lane curves

#[test]
fn a_stroke_on_an_empty_lane_is_its_values_and_neutral_either_side() {
    let mut c = LaneCurve::default();
    assert!(c.is_empty());
    assert_eq!(c.at(500, NEUTRAL), NEUTRAL);
    c.write(1000, 2000, 100, |_| 0.5, NEUTRAL);
    assert!((c.at(1500, NEUTRAL) - 0.5).abs() < 1e-6);
    assert!((c.at(1000, NEUTRAL) - 0.5).abs() < 1e-6);
    assert!((c.at(2000, NEUTRAL) - 0.5).abs() < 1e-6);
    // Outside the stroke the lane is what it was: neutral, with a step back
    // to it right beside the stroke rather than a ramp across the song.
    assert_eq!(c.at(0, NEUTRAL), NEUTRAL);
    assert_eq!(c.at(990, NEUTRAL), NEUTRAL);
    assert_eq!(c.at(5000, NEUTRAL), NEUTRAL);
    // A flat stroke is two points and its edges, not eleven.
    assert!(c.points().len() <= 4, "{:?}", c.points());
}

#[test]
fn a_second_stroke_beside_the_first_does_not_bridge_the_gap() {
    let mut c = LaneCurve::default();
    c.write(0, 1000, 50, |_| 1.0, NEUTRAL);
    c.write(3000, 4000, 50, |_| -1.0, NEUTRAL);
    assert_eq!(c.at(2000, NEUTRAL), NEUTRAL, "{:?}", c.points());
    assert!((c.at(500, NEUTRAL) - 1.0).abs() < 1e-6);
    assert!((c.at(3500, NEUTRAL) + 1.0).abs() < 1e-6);
}

#[test]
fn a_stroke_over_part_of_another_keeps_the_rest_of_it() {
    let mut c = LaneCurve::default();
    // A ramp 0 → 1 over 0..1000.
    c.write(0, 1000, 10, |t| t as f32 / 1000.0, NEUTRAL);
    c.write(400, 600, 10, |_| -0.5, NEUTRAL);
    assert!((c.at(200, NEUTRAL) - 0.2).abs() < 0.02);
    assert!((c.at(500, NEUTRAL) + 0.5).abs() < 1e-6);
    assert!((c.at(800, NEUTRAL) - 0.8).abs() < 0.02);
    // The ramp's straight run is simplified to its ends.
    let before = c.points().len();
    assert!(before < 20, "{before} points for two straight runs");
}

#[test]
fn erasing_returns_a_stretch_to_neutral_and_an_all_neutral_lane_is_empty() {
    let mut c = LaneCurve::default();
    c.write(0, 1000, 50, |_| 0.7, NEUTRAL);
    c.erase(200, 400, NEUTRAL);
    assert_eq!(c.at(300, NEUTRAL), NEUTRAL);
    assert!((c.at(100, NEUTRAL) - 0.7).abs() < 1e-6);
    assert!((c.at(800, NEUTRAL) - 0.7).abs() < 1e-6);
    c.erase(-100, 2000, NEUTRAL);
    assert!(c.is_empty(), "{:?}", c.points());
}

#[test]
fn a_lane_with_a_neutral_other_than_zero_keeps_it() {
    // A backend's own lane whose default is 0.5.
    let mut c = LaneCurve::default();
    assert_eq!(c.at(0, 0.5), 0.5);
    c.write(100, 200, 10, |_| 0.9, 0.5);
    assert_eq!(c.at(50, 0.5), 0.5);
    assert_eq!(c.at(300, 0.5), 0.5);
}

#[test]
fn each_standard_lane_has_a_range_that_holds_its_neutral() {
    for lane in Curve::ALL {
        let spec = lane_spec(lane);
        assert!(spec.min < spec.max, "{lane:?}");
        assert!(spec.min <= lane.neutral() && lane.neutral() <= spec.max);
        assert!(!spec.name.is_empty());
    }
    assert_eq!(lane_spec(Curve::PitchBend).unit, "cents");
    assert_eq!(lane_spec(Curve::Dynamics).unit, "dB");
}

// -------------------------------------------------------------- velocity

/// A vocal note's velocity is its consonant speed — OpenUtau's VEL: the
/// note's own velocity of 100 is the voice's own speed, 27 either way
/// doubles or halves it.
#[test]
fn velocity_is_consonant_speed() {
    assert!((consonant_speed(100) - 1.0).abs() < 1e-6);
    assert!((consonant_speed(127) - 2.0).abs() < 1e-3);
    assert!((consonant_speed(73) - 0.5).abs() < 1e-3);
    for v in [1u8, 40, 73, 100, 127] {
        assert_eq!(velocity_for_speed(consonant_speed(v)), v);
    }
    assert_eq!(velocity_for_speed(100.0), 127);
    assert_eq!(velocity_for_speed(0.0), 1);
    assert_eq!(velocity_for_speed(f32::NAN), 100);
}

// ---------------------------------------------------------- note voices

#[test]
fn a_notes_voice_is_set_and_undone_in_one_step() {
    let mut f = fixture();
    let sung = NoteVoice {
        vibrato: Some(vibrato()),
        portamento: Some(Portamento {
            length: 0.12,
            position: 0.5,
            shape: GlideShape::Out,
        }),
        phonemes: Some(vec![PhonemeEdit {
            symbol: "s".into(),
            offset: Some(-0.05),
        }]),
        options: BTreeMap::from([(
            "fontelle-utau".to_string(),
            BTreeMap::from([("append".to_string(), "soft".to_string())]),
        )]),
    };
    let mut set = SetNoteVoices::new(
        f.clip,
        vec![f.notes[0], f.notes[2]],
        vec![Some(sung.clone()), Some(sung.clone())],
        "Add vibrato",
    );
    set.apply(&mut f.project).unwrap();
    assert_eq!(voice_of(&f.project, f.clip, f.notes[0]), Some(sung.clone()));
    assert_eq!(voice_of(&f.project, f.clip, f.notes[1]), None);
    assert_eq!(voice_of(&f.project, f.clip, f.notes[2]), Some(sung.clone()));
    assert_eq!(set.label(), "Add vibrato");
    set.invert().apply(&mut f.project).unwrap();
    assert_eq!(voice_of(&f.project, f.clip, f.notes[0]), None);
    assert_eq!(voice_of(&f.project, f.clip, f.notes[2]), None);
    // An empty voice is stored as none at all.
    let mut empty = SetNoteVoices::new(
        f.clip,
        vec![f.notes[0]],
        vec![Some(NoteVoice::default())],
        "Clear",
    );
    empty.apply(&mut f.project).unwrap();
    assert!(data(&f.project, f.clip).notes[f.notes[0]].voice.is_none());
}

#[test]
fn a_drag_of_a_vibrato_handle_is_one_undo_step() {
    let mut f = fixture();
    let mut history = History::default();
    for depth in [10.0, 20.0, 30.0, 45.0] {
        let v = NoteVoice {
            vibrato: Some(Vibrato { depth, ..vibrato() }),
            ..NoteVoice::default()
        };
        history
            .apply(
                Box::new(SetNoteVoices::new(
                    f.clip,
                    vec![f.notes[1]],
                    vec![Some(v)],
                    "Vibrato depth",
                )),
                &mut f.project,
            )
            .unwrap();
    }
    history.break_gesture();
    assert_eq!(
        voice_of(&f.project, f.clip, f.notes[1])
            .and_then(|v| v.vibrato)
            .map(|v| v.depth),
        Some(45.0)
    );
    history.undo(&mut f.project).unwrap().unwrap();
    assert_eq!(voice_of(&f.project, f.clip, f.notes[1]), None);
    assert_eq!(history.depth(), 0, "four moves were one step");
}

#[test]
fn a_voice_that_is_not_a_number_or_names_no_note_changes_nothing() {
    let mut f = fixture();
    let bad = NoteVoice {
        vibrato: Some(Vibrato {
            depth: f32::NAN,
            ..vibrato()
        }),
        ..NoteVoice::default()
    };
    let mut set = SetNoteVoices::new(f.clip, vec![f.notes[0]], vec![Some(bad)], "x");
    assert!(set.apply(&mut f.project).is_err());
    let mut gone = fixture();
    let stray = gone.notes[0];
    let mut other = fixture();
    let ClipSource::Notes(d) = &mut other.project.clips[other.clip].source else {
        unreachable!()
    };
    d.notes.remove(stray);
    let mut set = SetNoteVoices::new(
        other.clip,
        vec![other.notes[1], stray],
        vec![Some(NoteVoice::default()), Some(NoteVoice::default())],
        "x",
    );
    assert!(set.apply(&mut other.project).is_err());
    let _ = &mut gone;
}

// ------------------------------------------------------------ the part

#[test]
fn a_standard_lane_is_drawn_and_undone() {
    let mut f = fixture();
    let mut curve = LaneCurve::default();
    curve.write(0, PPQN, 40, |_| 0.4, NEUTRAL);
    let mut set = SetVocalLane::new(
        f.clip,
        LaneKey::Standard(Curve::Breathiness),
        Some(curve.clone()),
    );
    set.apply(&mut f.project).unwrap();
    assert_eq!(
        part(&f.project, f.clip).lanes.get(&Curve::Breathiness),
        Some(&curve)
    );
    set.invert().apply(&mut f.project).unwrap();
    assert!(
        data(&f.project, f.clip).vocal.is_none(),
        "an empty part is no part"
    );
}

#[test]
fn a_backends_own_lanes_settings_and_state_are_kept_by_backend() {
    let mut f = fixture();
    let mut curve = LaneCurve::default();
    curve.write(0, PPQN * 2, 40, |_| 0.9, 0.5);
    SetVocalLane::new(
        f.clip,
        LaneKey::Extra {
            backend: "fontelle-diffsinger".into(),
            id: "energy".into(),
        },
        Some(curve.clone()),
    )
    .apply(&mut f.project)
    .unwrap();
    SetVocalSettings::new(
        f.clip,
        "fontelle-diffsinger",
        BTreeMap::from([("steps".to_string(), 20.0)]),
    )
    .apply(&mut f.project)
    .unwrap();
    SetVocalState::new(f.clip, "Some Engine", Some(vec![0, 1, 2, 255]))
        .apply(&mut f.project)
        .unwrap();
    // Switching who sings keeps all of it: the next singer may be the one
    // that uses it again.
    SetSinger::new(
        f.channel,
        Some(VocalVoice::new("Some Engine", "alto", "en")),
    )
    .apply(&mut f.project)
    .unwrap();
    let p = part(&f.project, f.clip);
    assert_eq!(p.extra["fontelle-diffsinger"]["energy"], curve);
    assert_eq!(p.settings["fontelle-diffsinger"]["steps"], 20.0);
    assert_eq!(p.state["Some Engine"], vec![0, 1, 2, 255]);
}

#[test]
fn a_lane_drag_is_one_step_and_settings_refuse_nan() {
    let mut f = fixture();
    let mut history = History::default();
    let mut curve = LaneCurve::default();
    for end in [100, 200, 300] {
        curve.write(0, end, 10, |_| 3.0, NEUTRAL);
        history
            .apply(
                Box::new(SetVocalLane::new(
                    f.clip,
                    LaneKey::Standard(Curve::Dynamics),
                    Some(curve.clone()),
                )),
                &mut f.project,
            )
            .unwrap();
    }
    history.break_gesture();
    history.undo(&mut f.project).unwrap().unwrap();
    assert!(part(&f.project, f.clip).lanes.is_empty());
    assert_eq!(history.depth(), 0);
    let mut nan = SetVocalSettings::new(
        f.clip,
        "x",
        BTreeMap::from([("a".to_string(), f32::INFINITY)]),
    );
    assert!(nan.apply(&mut f.project).is_err());
}

// ------------------------------------------------------------------ wire

#[test]
fn every_vocal_part_command_crosses_the_wire_as_itself() {
    let mut f = fixture();
    let mut curve = LaneCurve::default();
    curve.write(0, 400, 40, |t| t as f32 / 400.0, NEUTRAL);
    let mut commands: Vec<Box<dyn Command>> = vec![
        Box::new(SetNoteVoices::new(
            f.clip,
            vec![f.notes[0]],
            vec![Some(NoteVoice {
                vibrato: Some(vibrato()),
                ..NoteVoice::default()
            })],
            "Vibrato",
        )),
        Box::new(SetVocalLane::new(
            f.clip,
            LaneKey::Standard(Curve::PitchBend),
            Some(curve.clone()),
        )),
        Box::new(SetVocalSettings::new(
            f.clip,
            "E",
            BTreeMap::from([("s".to_string(), 1.5)]),
        )),
        Box::new(SetVocalState::new(f.clip, "E", Some(vec![9; 40]))),
    ];
    let mut there = fixture();
    for command in &mut commands {
        command.apply(&mut f.project).unwrap();
        let edit = Edit::from_bytes(&command.to_edit().to_bytes()).unwrap();
        edit.into_command().apply(&mut there.project).unwrap();
    }
    assert_eq!(part(&there.project, there.clip), part(&f.project, f.clip));
    assert_eq!(
        voice_of(&there.project, there.clip, there.notes[0]),
        voice_of(&f.project, f.clip, f.notes[0])
    );
}

// ---------------------------------------------------------------- format

#[test]
fn a_song_with_expression_is_format_3_and_comes_back_whole() {
    assert_eq!(PROJECT_FORMAT_VERSION, 3);
    let mut f = fixture();
    let mut curve = LaneCurve::default();
    curve.write(0, PPQN, 40, |_| -0.25, NEUTRAL);
    SetVocalLane::new(f.clip, LaneKey::Standard(Curve::Tension), Some(curve))
        .apply(&mut f.project)
        .unwrap();
    SetVocalState::new(f.clip, "E", Some((0..=255).collect()))
        .apply(&mut f.project)
        .unwrap();
    SetNoteVoices::new(
        f.clip,
        vec![f.notes[1]],
        vec![Some(NoteVoice {
            vibrato: Some(vibrato()),
            ..NoteVoice::default()
        })],
        "v",
    )
    .apply(&mut f.project)
    .unwrap();
    assert_eq!(f.project.format_needed(), 3);
    let dir = scratch("v3");
    save_project(&f.project, &dir).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["meta"]["format_version"], serde_json::json!(3));
    let back = load_project(&dir).unwrap();
    assert_eq!(part(&back, f.clip), part(&f.project, f.clip));
    assert_eq!(
        voice_of(&back, f.clip, f.notes[1]),
        voice_of(&f.project, f.clip, f.notes[1])
    );
    // A state blob is written as text, not as a list of 256 numbers.
    assert!(!text.contains("254,255"), "state written as a number list");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_vocal_song_without_expression_stays_format_2() {
    let f = fixture();
    assert_eq!(f.project.format_needed(), 2);
    let dir = scratch("v2");
    save_project(&f.project, &dir).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(!text.contains("\"voice\":"), "no note voice is written");
    assert!(!text.contains("\"vocal\":null"), "no part is written");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_format_2_song_opens_with_no_expression() {
    let f = fixture();
    let dir = scratch("old2");
    save_project(&f.project, &dir).unwrap();
    let path = dir.join("project.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    json["meta"]["format_version"] = serde_json::json!(2);
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    let back = load_project(&dir).unwrap();
    assert_eq!(back.meta.format_version, PROJECT_FORMAT_VERSION);
    assert!(data(&back, f.clip).vocal.is_none());
    assert!(
        data(&back, f.clip)
            .notes
            .values()
            .all(|n| n.voice.is_none())
    );
    let _ = std::fs::remove_dir_all(&dir);
}
