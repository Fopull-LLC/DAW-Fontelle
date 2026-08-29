use std::collections::HashMap;

use fontelle_model::{ClipSource, Project};
use fontelle_types::{ChannelId, CompiledTimeline, EventPayload, NodeId, TimedEvent};

use crate::collision::voice_context_for_clip;

/// Turns the whole document — clips, resolved prefab instances, lane mutes,
/// automation, the tempo map — into a flat, sample-timestamped event list
/// (TDD §11.1). Prefab resolution, override merging, and tempo conversion all
/// happen here, on the model thread, ahead of time: playback cost is therefore
/// independent of how deeply prefabs are nested (INVARIANT 3).
///
/// **M0 scope:** only `ClipSource::Notes` clips are read — automation clips
/// (M4) and audio clips (M6) are silently skipped. `clip.prefab_link` is
/// ignored; only `clip.source` itself is compiled, so prefab resolution
/// (TDD §10.5) isn't wired in yet — that lands with M3's real timeline UI.
/// See `PROGRESS.md`.
///
/// `channel_nodes` maps each document `ChannelId` to the engine-side `NodeId`
/// its compiled `SamplerNode` was assigned when the audio graph was built.
/// This crate depends on `fontelle-types` + `fontelle-model` only (TDD §4.1)
/// — it has no way to discover that mapping itself, so whoever builds the
/// graph (currently `fontelle-app`; a real "build graph from project" step
/// belongs to the engine eventually) passes it in. A channel missing from the
/// map produces no events for its notes rather than panicking — the document
/// can reference a channel before the graph has caught up with it.
pub fn compile(project: &Project, channel_nodes: &HashMap<ChannelId, NodeId>) -> CompiledTimeline {
    let mut events = Vec::new();

    for (clip_index, (_clip_id, clip)) in project.clips.iter().enumerate() {
        if clip.muted {
            continue;
        }
        if project.lanes.get(clip.lane).is_some_and(|lane| lane.muted) {
            continue;
        }

        let ClipSource::Notes(note_data) = &clip.source else {
            continue; // automation (M4) / audio (M6) clips: not this pass
        };

        let Some(&node_id) = channel_nodes.get(&note_data.channel) else {
            continue; // channel not wired into the compiled graph yet
        };

        let voice_context = voice_context_for_clip(clip_index as u32);

        for note in note_data.notes.values() {
            let on_tick = clip.start + note.start;
            let off_tick = on_tick + note.length;

            events.push(TimedEvent {
                sample: project.tempo_map.tick_to_sample(on_tick),
                target: node_id,
                payload: EventPayload::NoteOn {
                    key: note.key,
                    velocity: note.velocity,
                    voice_context,
                },
            });
            events.push(TimedEvent {
                sample: project.tempo_map.tick_to_sample(off_tick),
                target: node_id,
                payload: EventPayload::NoteOff {
                    key: note.key,
                    voice_context,
                },
            });
        }
    }

    events.sort_by_key(|e| e.sample);

    CompiledTimeline {
        events,
        // Bar-granularity seek index needs the tempo map's time-signature
        // track, which doesn't exist yet (see TempoMap's scope cut). Not
        // required for correctness — `events_for_block` walks the sorted
        // `events` Vec directly — only for the O(1)-seek optimisation TDD
        // §11.1 describes. M3 work.
        index: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use fontelle_model::{Clip, ClipSource, Lane, Note, NoteData, Project, TempoMap};
    use fontelle_types::EventPayload;
    use slotmap::SlotMap;

    use super::*;

    const SR: f64 = 48_000.0;
    const BPM: f64 = 120.0;

    fn project_with_one_note() -> (Project, ChannelId, NodeId) {
        let mut project = Project::new("test");
        project.tempo_map = TempoMap::new(BPM, SR);

        let channel_id = project.channels.insert(fontelle_model::Channel {
            name: "ch".into(),
            color: [0, 0, 0, 255],
            mixer_track: Default::default(),
            patch_data: None,
        });

        let lane_id = project.lanes.insert(Lane {
            name: "lane".into(),
            height: 32.0,
            color: [0, 0, 0, 255],
            muted: false,
            locked: false,
        });

        let mut notes = SlotMap::default();
        notes.insert(Note {
            start: 0,
            length: fontelle_types::PPQN, // one quarter note
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
        });

        project.clips.insert(Clip {
            lane: lane_id,
            start: 0,
            length: fontelle_types::PPQN,
            source: ClipSource::Notes(NoteData {
                channel: channel_id,
                notes,
            }),
            prefab_link: None,
            color: None,
            muted: false,
        });

        let mut node_ids: SlotMap<NodeId, ChannelId> = SlotMap::default();
        let node_id = node_ids.insert(channel_id);

        (project, channel_id, node_id)
    }

    #[test]
    fn one_note_compiles_to_a_note_on_and_note_off_targeting_its_channels_node() {
        let (project, channel_id, node_id) = project_with_one_note();
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes);

        assert_eq!(timeline.events.len(), 2, "one NoteOn and one NoteOff");

        let note_on = &timeline.events[0];
        assert_eq!(note_on.sample, 0);
        assert_eq!(note_on.target, node_id);
        match note_on.payload {
            EventPayload::NoteOn { key, velocity, .. } => {
                assert_eq!(key, 60);
                assert_eq!(velocity, 100);
            }
            ref other => panic!("expected NoteOn, got {other:?}"),
        }

        let note_off = &timeline.events[1];
        assert_eq!(
            note_off.sample, 24_000,
            "a quarter note at 120bpm/48kHz is 24000 samples long"
        );
        assert_eq!(note_off.target, node_id);
        assert!(matches!(
            note_off.payload,
            EventPayload::NoteOff { key: 60, .. }
        ));
    }

    #[test]
    fn events_are_sorted_by_sample() {
        let (project, channel_id, node_id) = project_with_one_note();
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes);

        let mut sorted = timeline.events.clone();
        sorted.sort_by_key(|e| e.sample);
        assert_eq!(
            timeline.events.iter().map(|e| e.sample).collect::<Vec<_>>(),
            sorted.iter().map(|e| e.sample).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_muted_clip_produces_no_events() {
        let (mut project, channel_id, node_id) = project_with_one_note();
        for clip in project.clips.values_mut() {
            clip.muted = true;
        }
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes);
        assert!(timeline.events.is_empty());
    }

    #[test]
    fn a_muted_lane_produces_no_events() {
        let (mut project, channel_id, node_id) = project_with_one_note();
        for lane in project.lanes.values_mut() {
            lane.muted = true;
        }
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes);
        assert!(timeline.events.is_empty());
    }

    #[test]
    fn a_channel_missing_from_the_node_map_produces_no_events() {
        let (project, _channel_id, _node_id) = project_with_one_note();
        let channel_nodes = HashMap::new(); // deliberately empty

        let timeline = compile(&project, &channel_nodes);
        assert!(timeline.events.is_empty());
    }
}
