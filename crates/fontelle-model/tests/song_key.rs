//! A song's key, and fitting notes to it.
//!
//! > *"a scale tool so you can chose between any note and the mode or
//! > whatever and it will snap all of your notes to that scale"*
//!
//! The key is part of the song — saved with it, undoable, and sent to
//! anybody the song is shared with — so it is a field on the project and a
//! command that sets it. Fitting is a key **per note**, which no command
//! could say before: the transposer moves everything by one amount, and a
//! scale moves each note by its own.

use fontelle_model::{
    Arena, Clip, ClipSource, Command, Compound, Lane, Note, NoteData, Project, SetKey, SetNoteKeys,
    TempoMap,
};
use fontelle_types::{ChannelId, ClipId, KeyScale, NoteId, PPQN};

fn a_note(start: i64, key: u8, velocity: u8) -> Note {
    Note {
        start,
        length: PPQN,
        key,
        velocity,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

struct Fixture {
    project: Project,
    clip: ClipId,
    notes: Vec<NoteId>,
}

/// A clip of three notes at three different velocities, so a change that
/// flattens them is visible as a failure rather than as a coincidence.
fn fixture() -> Fixture {
    let mut project = Project::new("key");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let channel: ChannelId = project.channels.insert(fontelle_model::Channel {
        preset: None,
        instrument: None,
        name: "Part".into(),
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
    });
    let lane = project.lanes.insert(Lane {
        name: "Lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let mut notes = Arena::default();
    let ids = vec![
        notes.insert(a_note(0, 60, 40)),
        notes.insert(a_note(PPQN, 64, 100)),
        notes.insert(a_note(PPQN * 2, 67, 120)),
    ];
    let clip = project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData { channel, notes }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    Fixture {
        project,
        clip,
        notes: ids,
    }
}

fn keys(project: &Project, clip: ClipId, ids: &[NoteId]) -> Vec<u8> {
    let ClipSource::Notes(data) = &project.clips[clip].source else {
        panic!("the fixture's clip holds notes");
    };
    ids.iter().map(|id| data.notes[*id].key).collect()
}

#[test]
fn a_new_song_has_no_key_and_setting_one_is_undoable() {
    let mut f = fixture();
    assert_eq!(f.project.key, None);
    let mut set = SetKey::new(Some(KeyScale::new(9, "natural-minor")));
    set.apply(&mut f.project).unwrap();
    assert_eq!(f.project.key, Some(KeyScale::new(9, "natural-minor")));

    // Changing it and taking the change back lands on the first key, and
    // taking that back lands on none.
    let mut again = SetKey::new(Some(KeyScale::new(2, "dorian")));
    again.apply(&mut f.project).unwrap();
    again.invert().apply(&mut f.project).unwrap();
    assert_eq!(f.project.key, Some(KeyScale::new(9, "natural-minor")));
    set.invert().apply(&mut f.project).unwrap();
    assert_eq!(f.project.key, None);
}

#[test]
fn a_song_saved_before_keys_existed_opens_with_none() {
    let f = fixture();
    let mut json = serde_json::to_value(&f.project).unwrap();
    json.as_object_mut().unwrap().remove("key");
    let back: Project = serde_json::from_value(json).unwrap();
    assert_eq!(back.key, None);

    let mut keyed = fixture().project;
    keyed.key = Some(KeyScale::new(7, "mixolydian"));
    let back: Project = serde_json::from_str(&serde_json::to_string(&keyed).unwrap()).unwrap();
    assert_eq!(back.key, keyed.key);
}

#[test]
fn each_note_gets_its_own_key_and_undo_gives_each_its_own_back() {
    let mut f = fixture();
    let mut fit = SetNoteKeys::new(f.clip, f.notes.clone(), vec![59, 65, 67]);
    fit.apply(&mut f.project).unwrap();
    assert_eq!(keys(&f.project, f.clip, &f.notes), vec![59, 65, 67]);
    fit.invert().apply(&mut f.project).unwrap();
    assert_eq!(keys(&f.project, f.clip, &f.notes), vec![60, 64, 67]);
}

#[test]
fn a_list_that_does_not_line_up_or_goes_off_the_keyboard_changes_nothing() {
    let mut f = fixture();
    assert!(
        SetNoteKeys::new(f.clip, f.notes.clone(), vec![61, 62])
            .apply(&mut f.project)
            .is_err()
    );
    assert!(
        SetNoteKeys::new(f.clip, f.notes.clone(), vec![61, 62, 128])
            .apply(&mut f.project)
            .is_err()
    );
    assert_eq!(keys(&f.project, f.clip, &f.notes), vec![60, 64, 67]);
}

#[test]
fn choosing_a_key_and_fitting_the_notes_is_one_undo() {
    let mut f = fixture();
    let mut both = Compound::new(
        "Scale: C minor",
        vec![
            Box::new(SetKey::new(Some(KeyScale::new(0, "natural-minor")))),
            Box::new(SetNoteKeys::new(f.clip, vec![f.notes[1]], vec![63])),
        ],
    );
    both.apply(&mut f.project).unwrap();
    assert_eq!(keys(&f.project, f.clip, &f.notes), vec![60, 63, 67]);
    both.invert().apply(&mut f.project).unwrap();
    assert_eq!(keys(&f.project, f.clip, &f.notes), vec![60, 64, 67]);
    assert_eq!(f.project.key, None);
}
