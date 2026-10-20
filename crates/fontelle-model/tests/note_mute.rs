//! A note can be muted: kept in the clip, not played (`docs/roll-tools-plan.md`
//! §4.3). A saved song without the flag reads every note as playing, and an
//! unmuted note is saved exactly as it was before the flag existed.

use fontelle_model::{Command, Note, SetNoteMuted};
use fontelle_types::PPQN;

fn a_note(key: u8) -> Note {
    Note {
        start: 0,
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
        muted: false,
    }
}

#[test]
fn an_unmuted_note_is_saved_as_it_always_was() {
    let json = serde_json::to_string(&a_note(60)).unwrap();
    assert!(!json.contains("muted"), "{json}");
    let muted = Note {
        muted: true,
        ..a_note(60)
    };
    let json = serde_json::to_string(&muted).unwrap();
    assert!(json.contains("\"muted\":true"), "{json}");
    let back: Note = serde_json::from_str(&json).unwrap();
    assert!(back.muted);
}

#[test]
fn a_note_saved_before_mute_existed_plays() {
    let old = serde_json::to_string(&a_note(60)).unwrap();
    let back: Note = serde_json::from_str(&old).unwrap();
    assert!(!back.muted);
}

#[test]
fn muting_is_one_command_and_undoes() {
    let mut project = fontelle_model::Project::new("mute");
    let lane = project.lanes.insert(fontelle_model::Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        soloed: false,
        order: 0,
    });
    let channel = project.channels.insert(fontelle_model::Channel {
        preset: None,
        instrument: None,
        name: "ch".into(),
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
        vocal: None,
        vocal_style: Default::default(),
    });
    let mut notes = fontelle_model::Arena::default();
    let a = notes.insert(a_note(60));
    let b = notes.insert(a_note(64));
    let clip = project.clips.insert(fontelle_model::Clip {
        name: None,
        lane,
        start: 0,
        length: PPQN * 4,
        source: fontelle_model::ClipSource::Notes(fontelle_model::NoteData {
            channel,
            notes,
            vocal: None,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    let muted = |p: &fontelle_model::Project| -> Vec<bool> {
        let fontelle_model::ClipSource::Notes(data) = &p.clips[clip].source else {
            unreachable!()
        };
        vec![data.notes[a].muted, data.notes[b].muted]
    };
    let mut command = SetNoteMuted::new(clip, vec![a, b], vec![true, false]);
    command.apply(&mut project).unwrap();
    assert_eq!(muted(&project), vec![true, false]);
    command.invert().apply(&mut project).unwrap();
    assert_eq!(muted(&project), vec![false, false]);
}
