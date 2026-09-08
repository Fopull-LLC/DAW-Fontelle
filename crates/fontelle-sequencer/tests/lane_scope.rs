//! Compiling one row of the arrangement and nothing else.
//!
//! What *"the ability to render a track into an audio clip"* needs: a bounce
//! of a track is that track, and a scope that let the rest of the song through
//! would put the whole mix in every render.
//!
//! The same shape as [`CompileScope::Clip`] and for the same reason — a row is
//! the unit being soloed, so a curve on another row is not part of it.

use fontelle_model::{
    AddChannel, AddClip, Arena, Clip, ClipSource, Command, Lane, Note, NoteData, Project, TempoMap,
};
use fontelle_sequencer::{CompileScope, NodeMaps, compile_with};
use fontelle_types::{ChannelId, LaneId, NodeId, PPQN};
use std::collections::HashMap;

fn a_note(start: i64, key: u8) -> Note {
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
        channel: None,
    }
}

fn a_lane(project: &mut Project, name: &str, order: u32) -> LaneId {
    project.lanes.insert(Lane {
        name: name.to_string(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order,
    })
}

/// A project with two rows, each carrying a clip of one note on its own
/// channel, and the ids to address them by.
fn two_rows() -> (
    Project,
    Vec<LaneId>,
    Vec<ChannelId>,
    HashMap<ChannelId, NodeId>,
) {
    let mut project = Project::new("scoped");
    project.tempo_map = TempoMap::new(120.0, 48_000.0);
    let mut lanes = Vec::new();
    let mut channels = Vec::new();
    let mut nodes: HashMap<ChannelId, NodeId> = HashMap::new();
    let mut ids: Arena<NodeId, ()> = Arena::default();
    for (index, key) in [60u8, 72].into_iter().enumerate() {
        let mut add = AddChannel::new(format!("Ch {index}"), None);
        add.apply(&mut project).expect("adds");
        let channel = add.channel().expect("made");
        let lane = a_lane(&mut project, &format!("Lane {index}"), index as u32);
        let mut notes = Arena::default();
        notes.insert(a_note(0, key));
        AddClip::new(Clip {
            lane,
            start: 0,
            length: PPQN * 4,
            source: ClipSource::Notes(NoteData { channel, notes }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        })
        .apply(&mut project)
        .expect("adds");
        nodes.insert(channel, ids.insert(()));
        lanes.push(lane);
        channels.push(channel);
    }
    (project, lanes, channels, nodes)
}

fn keys(timeline: &fontelle_types::CompiledTimeline) -> Vec<u8> {
    let mut out: Vec<u8> = timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            fontelle_types::EventPayload::NoteOn { key, .. } => Some(key),
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn the_song_scope_still_carries_every_row() {
    let (project, _lanes, _channels, nodes) = two_rows();
    let timeline = compile_with(
        &project,
        &NodeMaps {
            channels: &nodes,
            ..Default::default()
        },
        CompileScope::Song,
    );
    assert_eq!(keys(&timeline), vec![60, 72]);
}

#[test]
fn a_lane_scope_carries_that_row_and_no_other() {
    let (project, lanes, _channels, nodes) = two_rows();
    let timeline = compile_with(
        &project,
        &NodeMaps {
            channels: &nodes,
            ..Default::default()
        },
        CompileScope::Lane(lanes[0]),
    );
    assert_eq!(keys(&timeline), vec![60], "the other row came with it");

    let timeline = compile_with(
        &project,
        &NodeMaps {
            channels: &nodes,
            ..Default::default()
        },
        CompileScope::Lane(lanes[1]),
    );
    assert_eq!(keys(&timeline), vec![72]);
}

#[test]
fn a_row_with_nothing_on_it_compiles_to_nothing_rather_than_to_everything() {
    // The failure worth guarding: a scope that fell through to "no filter"
    // would render the whole mix into what is supposed to be one track.
    let (mut project, _lanes, _channels, nodes) = two_rows();
    let empty = a_lane(&mut project, "Empty", 9);
    let timeline = compile_with(
        &project,
        &NodeMaps {
            channels: &nodes,
            ..Default::default()
        },
        CompileScope::Lane(empty),
    );
    assert!(keys(&timeline).is_empty(), "got {:?}", keys(&timeline));
}

#[test]
fn a_muted_row_is_still_muted_when_it_is_the_one_being_rendered() {
    // Rendering a row you have muted should give you silence, not a bounce of
    // something the song does not play.
    let (mut project, lanes, _channels, nodes) = two_rows();
    project.lanes[lanes[0]].muted = true;
    let timeline = compile_with(
        &project,
        &NodeMaps {
            channels: &nodes,
            ..Default::default()
        },
        CompileScope::Lane(lanes[0]),
    );
    assert!(keys(&timeline).is_empty());
}
