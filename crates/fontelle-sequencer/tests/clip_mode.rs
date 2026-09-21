//! Compiling one clip on its own, and compiling through the automated tempo.
//!
//! **Clip mode.** FL Studio has a pattern mode: the transport plays only the
//! pattern you are editing, round and round, so a part can be heard by itself
//! while it is written. Fontelle's clips are the patterns, so the equivalent
//! is a compile that reads *one clip* — which is what `CompileScope::Clip`
//! is. The transport's loop is the session's business; this is the half that
//! decides what is on the timeline.
//!
//! **The tempo lane.** §12.3: the tempo map is generated from the tempo
//! automation. The compiler is where every tick becomes a sample, so it is
//! the compiler that has to convert through the *automated* map rather than
//! the box's — or a ritardando would be drawn, saved, and inaudible.

use std::collections::HashMap;

use fontelle_model::{
    Arena, AutomationData, AutomationPoint, Clip, ClipSource, CurveShape, Lane, Note, NoteData,
    Project, TempoMap,
};
use fontelle_types::{
    ChannelId, ClipId, EventPayload, NodeId, PPQN, ParamTarget, Tick, normalised_tempo,
};
use slotmap::KeyData;

const SR: f64 = 48_000.0;
const BAR: Tick = PPQN * 4;

fn node(n: u64) -> NodeId {
    NodeId::from(KeyData::from_ffi(n))
}

fn a_note(start: Tick, key: u8) -> Note {
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

struct Rig {
    project: Project,
    channels: HashMap<ChannelId, NodeId>,
    lane: fontelle_types::LaneId,
}

impl Rig {
    fn new() -> Self {
        let mut project = Project::new("clip mode");
        project.tempo_map = TempoMap::new(120.0, SR);
        let lane = project.lanes.insert(Lane {
            name: "lane".into(),
            height: 32.0,
            color: [0; 4],
            muted: false,
            locked: false,
            order: 0,
        });
        Self {
            project,
            channels: HashMap::new(),
            lane,
        }
    }

    /// A channel with one clip of one note on it, at `start`.
    fn channel_with_clip(&mut self, start: Tick, key: u8, node_number: u64) -> ClipId {
        let channel = self.project.channels.insert(fontelle_model::Channel {
            preset: None,
            instrument: None,
            name: format!("ch{key}"),
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
        self.channels.insert(channel, node(node_number));
        let mut notes = Arena::default();
        notes.insert(a_note(0, key));
        self.project.clips.insert(Clip {
            lane: self.lane,
            start,
            length: BAR,
            source: ClipSource::Notes(NoteData { channel, notes }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        })
    }

    fn tempo_clip(&mut self, start: Tick, length: Tick, from_bpm: f64, to_bpm: f64) -> ClipId {
        let mut points = Arena::default();
        points.insert(AutomationPoint {
            tick: 0,
            value: normalised_tempo(from_bpm),
            curve: CurveShape::Linear,
            tension: 0.0,
        });
        points.insert(AutomationPoint {
            tick: length,
            value: normalised_tempo(to_bpm),
            curve: CurveShape::Linear,
            tension: 0.0,
        });
        self.project.clips.insert(Clip {
            lane: self.lane,
            start,
            length,
            source: ClipSource::Automation(AutomationData {
                target: ParamTarget::Tempo.address(),
                points,
            }),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        })
    }
}

fn keys_on(timeline: &fontelle_types::CompiledTimeline) -> Vec<u8> {
    timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteOn { key, .. } => Some(key),
            _ => None,
        })
        .collect()
}

// ------------------------------------------------------------- clip mode ---

#[test]
fn the_song_scope_is_the_compile_there_has_always_been() {
    let mut rig = Rig::new();
    rig.channel_with_clip(0, 60, 1);
    rig.channel_with_clip(BAR, 64, 2);
    let whole = fontelle_sequencer::compile(&rig.project, &rig.channels, &Default::default());
    let scoped = fontelle_sequencer::compile_scoped(
        &rig.project,
        &rig.channels,
        &Default::default(),
        fontelle_sequencer::CompileScope::Song,
    );
    assert_eq!(whole.events.len(), scoped.events.len());
    assert_eq!(keys_on(&whole), vec![60, 64]);
}

#[test]
fn the_clip_scope_compiles_that_clip_and_nothing_else() {
    let mut rig = Rig::new();
    let first = rig.channel_with_clip(0, 60, 1);
    let second = rig.channel_with_clip(BAR, 64, 2);

    let only_first = fontelle_sequencer::compile_scoped(
        &rig.project,
        &rig.channels,
        &Default::default(),
        fontelle_sequencer::CompileScope::Clip(first),
    );
    assert_eq!(keys_on(&only_first), vec![60]);

    let only_second = fontelle_sequencer::compile_scoped(
        &rig.project,
        &rig.channels,
        &Default::default(),
        fontelle_sequencer::CompileScope::Clip(second),
    );
    assert_eq!(keys_on(&only_second), vec![64]);
    // Where it is in the song, not moved to the front: the loop the session
    // sets is over the clip's own bars, and the roll's playhead is drawn
    // against the clip's own start.
    assert_eq!(
        only_second.events[0].sample,
        rig.project.tempo_map.tick_to_sample(BAR)
    );
}

#[test]
fn a_clip_scope_leaves_other_lanes_automation_out() {
    // A tempo lane elsewhere in the song is not part of the clip being
    // soloed, so the clip plays at the box's tempo.
    let mut rig = Rig::new();
    let clip = rig.channel_with_clip(BAR * 4, 60, 1);
    rig.tempo_clip(0, BAR * 4, 60.0, 60.0);

    let scoped = fontelle_sequencer::compile_scoped(
        &rig.project,
        &rig.channels,
        &Default::default(),
        fontelle_sequencer::CompileScope::Clip(clip),
    );
    assert_eq!(
        scoped.tempo.len(),
        1,
        "one tempo, the box's: {:?}",
        scoped.tempo
    );
    assert!((scoped.tempo[0].1 - 120.0).abs() < 1e-3);
    assert_eq!(
        scoped.events[0].sample,
        TempoMap::new(120.0, SR).tick_to_sample(BAR * 4)
    );
}

#[test]
fn a_clip_scope_naming_no_clip_compiles_to_silence() {
    let mut rig = Rig::new();
    let clip = rig.channel_with_clip(0, 60, 1);
    rig.project.clips.remove(clip);
    let scoped = fontelle_sequencer::compile_scoped(
        &rig.project,
        &rig.channels,
        &Default::default(),
        fontelle_sequencer::CompileScope::Clip(clip),
    );
    assert!(scoped.events.is_empty());
}

// ------------------------------------------------------- the tempo lane ---

#[test]
fn a_tempo_lane_moves_the_notes_after_it() {
    let mut rig = Rig::new();
    rig.channel_with_clip(BAR * 2, 60, 1);
    let plain = fontelle_sequencer::compile(&rig.project, &rig.channels, &Default::default());

    // Two bars at half speed before the note: it arrives a whole two bars
    // later than the box alone would put it.
    rig.tempo_clip(0, BAR * 2, 60.0, 60.0);
    let slowed = fontelle_sequencer::compile(&rig.project, &rig.channels, &Default::default());

    let plain_on = plain.events[0].sample;
    let slowed_on = slowed.events[0].sample;
    assert_eq!(
        slowed_on,
        plain_on * 2,
        "two bars at 60 take as long as four at 120"
    );
}

#[test]
fn the_tempo_table_the_audio_thread_reads_is_the_automated_one() {
    let mut rig = Rig::new();
    rig.channel_with_clip(0, 60, 1);
    rig.tempo_clip(0, BAR * 2, 100.0, 140.0);
    let timeline = fontelle_sequencer::compile(&rig.project, &rig.channels, &Default::default());

    assert!(
        timeline.tempo.len() > 4,
        "a ramp is many segments, got {}",
        timeline.tempo.len()
    );
    assert!(
        timeline.tempo.windows(2).all(|w| w[0].0 < w[1].0),
        "sorted by sample: {:?}",
        timeline.tempo
    );
    assert!((timeline.tempo[0].1 - 100.0).abs() < 1e-3);
    let last = timeline.tempo.last().unwrap().1;
    assert!((last - 140.0).abs() < 1.0, "ends near 140, got {last}");
}

#[test]
fn a_tempo_lane_emits_no_parameter_events() {
    // The tempo has no node to send a value to: it is realised as the map
    // the whole timeline is converted through, and a `ParamValue` for it
    // would be a value nothing reads.
    let mut rig = Rig::new();
    rig.channel_with_clip(0, 60, 1);
    rig.tempo_clip(0, BAR, 100.0, 140.0);
    let timeline = fontelle_sequencer::compile(&rig.project, &rig.channels, &Default::default());
    assert!(
        !timeline
            .events
            .iter()
            .any(|e| matches!(e.payload, EventPayload::ParamValue { .. })),
        "the tempo lane put values on the wire"
    );
}
