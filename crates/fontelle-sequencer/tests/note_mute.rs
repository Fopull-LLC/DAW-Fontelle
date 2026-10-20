//! A muted note is written but not played (`docs/roll-tools-plan.md` §4.3,
//! FL Studio's note mute): it stays in the clip, hollow, and the timeline
//! never hears it.

use std::collections::HashMap;

use fontelle_model::{Arena, Clip, ClipSource, Note, NoteData, Project, TempoMap};
use fontelle_types::{ChannelId, EventPayload, NodeId, PPQN, Tick};
use slotmap::KeyData;

fn a_note(start: Tick, key: u8, muted: bool) -> Note {
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
        lyric: String::new(),
        voice: None,
        muted,
    }
}

fn compile(notes: Vec<Note>) -> fontelle_types::CompiledTimeline {
    let mut project = Project::new("properties");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
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
    let lane = project.lanes.insert(fontelle_model::Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        soloed: false,
        order: 0,
    });
    let mut arena = Arena::default();
    for note in notes {
        arena.insert(note);
    }
    project.clips.insert(Clip {
        name: None,
        lane,
        start: 0,
        length: PPQN * 16,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: arena,
            vocal: None,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    let node = NodeId::from(KeyData::from_ffi(1));
    let map: HashMap<ChannelId, NodeId> = [(channel, node)].into_iter().collect();
    fontelle_sequencer::compile(&project, &map, &Default::default())
}

fn keys_played(timeline: &fontelle_types::CompiledTimeline) -> Vec<u8> {
    timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteOn { key, .. } => Some(key),
            _ => None,
        })
        .collect()
}

#[test]
fn a_muted_note_is_not_played_and_its_neighbours_are() {
    let timeline = compile(vec![
        a_note(0, 60, false),
        a_note(PPQN, 62, true),
        a_note(PPQN * 2, 64, false),
    ]);
    let mut keys = keys_played(&timeline);
    keys.sort();
    assert_eq!(keys, vec![60, 64]);
}
