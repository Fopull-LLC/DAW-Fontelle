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
pub fn compile(
    project: &Project,
    channel_nodes: &HashMap<ChannelId, NodeId>,
    param_nodes: &HashMap<fontelle_types::ParamAddress, NodeId>,
) -> CompiledTimeline {
    let mut events = Vec::new();

    // The channel rack's two switches, resolved once for the whole pass. They
    // are *sequencer* mutes — the same reading a lane's has — and they have to
    // be: a channel plays through the master by default now, so a switch that
    // reached for its mixer track would silence the song. See
    // `Channel::muted`.
    //
    // `None` means nothing is soloed and every channel plays, which is not the
    // same as "the set of all channels": that distinction is what stops a solo
    // being sticky after the soloed channel is deleted.
    let soloed: Option<Vec<ChannelId>> = {
        let soloed: Vec<ChannelId> = project
            .channels
            .iter()
            .filter(|(_, channel)| channel.soloed)
            .map(|(id, _)| id)
            .collect();
        (!soloed.is_empty()).then_some(soloed)
    };
    let audible = |id: ChannelId| {
        project.channels.get(id).is_some_and(|channel| {
            !channel.muted && soloed.as_ref().is_none_or(|set| set.contains(&id))
        })
    };

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

        if !audible(note_data.channel) {
            continue; // muted in the rack, or another channel is soloed
        }

        let Some(&node_id) = channel_nodes.get(&note_data.channel) else {
            continue; // channel not wired into the compiled graph yet
        };

        let voice_context = voice_context_for_clip(clip_index as u32);

        // A **looped** clip is one set of notes played again every
        // `loop_length` until the clip runs out — as against a copy, which is
        // several clips with notes of their own. `repeats()` is 1 for a clip
        // that does not loop, so the plain case walks this loop once and comes
        // out exactly where it used to.
        let period = clip.loop_length.filter(|p| *p > 0);
        for repeat in 0..clip.repeats() {
            let offset = clip.repeat_start(repeat);

            for note in note_data.notes.values() {
                // Only the notes inside one period belong to a repeat. A note
                // written past the period is content the loop does not
                // contain — it is what the clip would play if it were not
                // looping, and playing it on every pass would be a
                // second, invisible loop.
                if period.is_some_and(|p| note.start >= p) {
                    continue;
                }
                let start = note.start + offset;
                // A repeat that begins after the clip ends does not sound.
                // **Only looped clips are bounded this way**: a plain clip's
                // length has never limited its notes, and changing that is a
                // separate decision from this one (see PROGRESS.md).
                if period.is_some() && start >= clip.length {
                    continue;
                }
                let on_tick = clip.start + start;
                let mut off_tick = on_tick + note.length;
                if period.is_some() {
                    // A loop that rings past its own end is a loop whose last
                    // pass sounds different from the others.
                    off_tick = off_tick.min(clip.start + clip.length);
                }

                // A **slide note** starts no voice and ends none: it bends
                // whatever is already sounding on this channel to its pitch,
                // over its own length. See `fontelle_model::Note::slide`, and
                // `fontelle_core::Sampler::slide` for what happens when
                // nothing is sounding (nothing).
                if note.slide {
                    let on = project.tempo_map.tick_to_sample(on_tick);
                    let off = project.tempo_map.tick_to_sample(off_tick);
                    events.push(TimedEvent {
                        sample: on,
                        target: node_id,
                        payload: EventPayload::NoteSlide {
                            key: note.key,
                            glide_samples: (off - on).max(0) as u32,
                            voice_context,
                        },
                    });
                    continue;
                }

                events.push(TimedEvent {
                    sample: project.tempo_map.tick_to_sample(on_tick),
                    target: node_id,
                    payload: EventPayload::NoteOn {
                        key: note.key,
                        velocity: note.velocity,
                        // All five of §16.5's per-note properties. They ride
                        // on the note-on rather than being parameters of the
                        // channel's node because that is what *per note*
                        // means: two notes sounding together on one
                        // instrument can differ in every one of them.
                        pan: note.pan,
                        fine_pitch: note.fine_pitch,
                        release: note.release,
                        mod_x: note.mod_x,
                        mod_y: note.mod_y,
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
    }

    compile_automation(project, param_nodes, &mut events);

    // Sorted once, at the end, over notes and automation together: the RT side
    // walks this list forwards and never sorts, so a sweep interleaved out of
    // order would be applied backwards.
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

/// How often an automated parameter is re-stated, in samples.
///
/// The control rate, and it is a compromise with a reason on each side. Per
/// sample is a hundred thousand events a second per lane for a smoothness
/// nobody can hear. Per block is what the engine would like and what the
/// sequencer cannot do — it has no block size, and the same timeline is played
/// at whatever size the device asks for.
///
/// So: a fixed interval shorter than any block the engine runs (§5.1's
/// `BLOCK_SIZE` is 512), which means a node applying the last value it saw
/// gets one per block whatever the device is doing. Five milliseconds at
/// 48 kHz, which is finer than a hand moves.
pub const AUTOMATION_INTERVAL: fontelle_types::Sample = 256;

/// How much a value has to move before it is worth another event.
///
/// A curve that is not going anywhere costs one event rather than one per
/// interval — which is the difference between a five-minute song with four
/// flat lanes carrying four events and one carrying a hundred thousand.
const AUTOMATION_EPSILON: f64 = 1e-4;

/// Turns every automation clip into `ParamValue` events at the node that owns
/// its target (TDD §12).
///
/// **The address is resolved here, off the audio thread.** `param_nodes` comes
/// from the realisation step, which is the only place that knows both the
/// document's parameter addresses and the graph's node ids — the same reason
/// `channel_nodes` is a parameter of this function (§4.1). Matching a string
/// per event per block on the RT side is work it should never be doing.
///
/// A target no node owns emits nothing: a project whose automation names a
/// track that has since been deleted plays, rather than panicking or filling
/// the timeline with events nobody reads.
fn compile_automation(
    project: &Project,
    param_nodes: &HashMap<fontelle_types::ParamAddress, NodeId>,
    events: &mut Vec<TimedEvent>,
) {
    for target in fontelle_model::automated_targets(project) {
        let Some(node) = param_nodes.get(&target).copied() else {
            continue;
        };
        // The span this target is automated over: from the first clip aimed at
        // it to the last one's end. Outside it there is nothing to say — before,
        // the parameter is the knob's; after, it holds what it was last told.
        let Some((from, to)) = automated_span(project, &target) else {
            continue;
        };

        let mut last: Option<f64> = None;
        let mut tick = from;
        while tick <= to {
            if let Some(value) = fontelle_model::automation_at(project, &target, tick)
                && last.is_none_or(|previous| (value - previous).abs() > AUTOMATION_EPSILON)
            {
                events.push(TimedEvent {
                    sample: project.tempo_map.tick_to_sample(tick),
                    target: node,
                    payload: EventPayload::ParamValue {
                        target: target.clone(),
                        value,
                    },
                });
                last = Some(value);
            }
            // Stepped in *samples* rather than ticks, so the rate is the same
            // through a tempo change — a sweep must not get coarser because
            // the song slowed down.
            let next = project.tempo_map.tick_to_sample(tick) + AUTOMATION_INTERVAL;
            let stepped = project.tempo_map.sample_to_tick(next).max(tick + 1);
            if stepped > to && tick < to {
                tick = to; // one last event exactly at the end
            } else {
                tick = stepped;
            }
        }
    }
}

/// The first tick any clip aims at `target`, and the last tick any of them
/// ends at.
fn automated_span(
    project: &Project,
    target: &fontelle_types::ParamAddress,
) -> Option<(fontelle_types::Tick, fontelle_types::Tick)> {
    let mut span: Option<(fontelle_types::Tick, fontelle_types::Tick)> = None;
    for (_, clip) in project.clips.iter() {
        if clip.muted {
            continue;
        }
        let fontelle_model::ClipSource::Automation(data) = &clip.source else {
            continue;
        };
        if data.target != *target {
            continue;
        }
        let (start, end) = (clip.start, clip.start + clip.length);
        span = Some(match span {
            Some((from, to)) => (from.min(start), to.max(end)),
            None => (start, end),
        });
    }
    span
}

#[cfg(test)]
mod tests {
    use fontelle_model::Arena;
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
            mixer_track: None,
            patch_data: None,
            pan: 0.0,
            muted: false,
            soloed: false,
        });

        let lane_id = project.lanes.insert(Lane {
            name: "lane".into(),
            height: 32.0,
            color: [0, 0, 0, 255],
            muted: false,
            locked: false,
        });

        let mut notes = Arena::default();
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
            slide: false,
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
            loop_length: None,
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

        let timeline = compile(&project, &channel_nodes, &Default::default());

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

        let timeline = compile(&project, &channel_nodes, &Default::default());

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

        let timeline = compile(&project, &channel_nodes, &Default::default());
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

        let timeline = compile(&project, &channel_nodes, &Default::default());
        assert!(timeline.events.is_empty());
    }

    #[test]
    fn a_channel_missing_from_the_node_map_produces_no_events() {
        let (project, _channel_id, _node_id) = project_with_one_note();
        let channel_nodes = HashMap::new(); // deliberately empty

        let timeline = compile(&project, &channel_nodes, &Default::default());
        assert!(timeline.events.is_empty());
    }

    /// §16.5's per-note pan, which the roll's property lane has been able to
    /// draw and edit since the lane existed — and which stopped dead here.
    /// The document stored it, the file round-tripped it, and this function
    /// built a `NoteOn` without it, so panning a note was a picture of a
    /// change rather than a change.
    #[test]
    fn a_notes_pan_reaches_the_compiled_note_on() {
        let (mut project, channel_id, node_id) = project_with_one_note();
        for clip in project.clips.values_mut() {
            if let ClipSource::Notes(data) = &mut clip.source {
                for note in data.notes.values_mut() {
                    note.pan = -100;
                }
            }
        }
        let mut channel_nodes = HashMap::new();
        channel_nodes.insert(channel_id, node_id);

        let timeline = compile(&project, &channel_nodes, &Default::default());

        match timeline.events[0].payload {
            EventPayload::NoteOn { pan, .. } => assert_eq!(
                pan, -100,
                "the note's own pan has to be on the wire — nothing downstream \
                 can see the document to go and fetch it"
            ),
            ref other => panic!("expected NoteOn, got {other:?}"),
        }
    }
}
