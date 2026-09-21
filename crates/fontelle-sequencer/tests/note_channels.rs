//! One clip, several instruments: each note plays the channel it names.
//!
//! The model half is `fontelle-model/tests/note_channels.rs`. This is what
//! the compiler makes of it: a note that names a channel goes to **that**
//! channel's node, a note that names none goes to the clip's, and the rack's
//! mute and solo are read per note rather than per clip — a muted bass
//! channel silences the bass notes in a shared clip and nothing else.

use std::collections::HashMap;

use fontelle_model::{Arena, Clip, ClipSource, Note, NoteData, Project, TempoMap};
use fontelle_types::{ChannelId, EventPayload, NodeId, PPQN, Tick};
use slotmap::KeyData;

const SR: f64 = 48_000.0;
const BPM: f64 = 120.0;

fn a_note(start: Tick, key: u8, channel: Option<ChannelId>) -> Note {
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
        channel,
    }
}

fn a_channel(name: &str) -> fontelle_model::Channel {
    fontelle_model::Channel {
        preset: None,
        instrument: None,
        name: name.into(),
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
    }
}

struct Rig {
    project: Project,
    drums: ChannelId,
    bass: ChannelId,
    drums_node: NodeId,
    bass_node: NodeId,
}

/// A project with two channels and one clip **on the drums** holding a
/// drum note (on the clip's own channel), a bass note (naming the bass), and
/// a second drum note that names the drums explicitly.
fn rig() -> Rig {
    let mut project = Project::new("shared");
    project.tempo_map = TempoMap::new(BPM, SR);
    let drums = project.channels.insert(a_channel("drums"));
    let bass = project.channels.insert(a_channel("bass"));
    let lane = project.lanes.insert(fontelle_model::Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    let mut notes = Arena::default();
    notes.insert(a_note(0, 36, None));
    notes.insert(a_note(PPQN, 40, Some(bass)));
    notes.insert(a_note(PPQN * 2, 38, Some(drums)));
    project.clips.insert(Clip {
        lane,
        start: 0,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData {
            channel: drums,
            notes,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    Rig {
        project,
        drums,
        bass,
        drums_node: NodeId::from(KeyData::from_ffi(1)),
        bass_node: NodeId::from(KeyData::from_ffi(2)),
    }
}

impl Rig {
    fn compile(&self) -> fontelle_types::CompiledTimeline {
        let mut nodes: HashMap<ChannelId, NodeId> = HashMap::new();
        nodes.insert(self.drums, self.drums_node);
        nodes.insert(self.bass, self.bass_node);
        fontelle_sequencer::compile(&self.project, &nodes, &HashMap::new())
    }

    /// The keys started on `node`, in time order.
    fn keys_on(&self, node: NodeId) -> Vec<u8> {
        let mut ons: Vec<(i64, u8)> = self
            .compile()
            .events
            .iter()
            .filter(|e| e.target == node)
            .filter_map(|e| match e.payload {
                EventPayload::NoteOn { key, .. } => Some((e.sample, key)),
                _ => None,
            })
            .collect();
        ons.sort();
        ons.into_iter().map(|(_, key)| key).collect()
    }
}

#[test]
fn each_note_plays_the_channel_it_names_and_the_rest_play_the_clips() {
    let r = rig();
    assert_eq!(r.keys_on(r.drums_node), vec![36, 38]);
    assert_eq!(r.keys_on(r.bass_node), vec![40]);
}

#[test]
fn muting_one_channel_silences_its_notes_in_a_shared_clip_and_no_others() {
    let mut r = rig();
    r.project.channels[r.bass].muted = true;
    assert_eq!(r.keys_on(r.drums_node), vec![36, 38]);
    assert!(r.keys_on(r.bass_node).is_empty());

    // And the other way: a muted home channel keeps the bass playing.
    let mut r = rig();
    r.project.channels[r.drums].muted = true;
    assert!(r.keys_on(r.drums_node).is_empty());
    assert_eq!(r.keys_on(r.bass_node), vec![40]);
}

#[test]
fn soloing_one_channel_keeps_only_its_notes_of_a_shared_clip() {
    let mut r = rig();
    r.project.channels[r.bass].soloed = true;
    assert!(r.keys_on(r.drums_node).is_empty());
    assert_eq!(r.keys_on(r.bass_node), vec![40]);
}

#[test]
fn a_note_naming_a_channel_with_no_node_yet_is_left_out_rather_than_moved() {
    // The document can name a channel before the graph has caught up with
    // it. Its notes are not played on the clip's channel instead — a bass
    // line on the drums for one frame is worse than a bass line late.
    let r = rig();
    let mut nodes: HashMap<ChannelId, NodeId> = HashMap::new();
    nodes.insert(r.drums, r.drums_node);
    let timeline = fontelle_sequencer::compile(&r.project, &nodes, &HashMap::new());
    let keys: Vec<u8> = timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteOn { key, .. } => Some(key),
            _ => None,
        })
        .collect();
    assert!(keys.contains(&36) && keys.contains(&38));
    assert!(!keys.contains(&40));
    assert!(timeline.events.iter().all(|e| e.target == r.drums_node));
}

#[test]
fn a_note_naming_a_channel_that_has_been_deleted_is_silent() {
    let mut r = rig();
    let gone = r.bass;
    r.project.channels.remove(gone);
    // The node map may lag the document by a frame and still name it.
    assert_eq!(r.keys_on(r.drums_node), vec![36, 38]);
    assert!(r.keys_on(r.bass_node).is_empty());
}
