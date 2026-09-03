//! What an audio clip compiles to (TDD §11.1 for §15's content).
//!
//! A note clip becomes **events**: a moment to start a voice and a moment to
//! stop it. An audio clip cannot, because it is not a moment — it is a
//! continuous stream that has to be at one place on the song and nowhere else.
//! So it compiles to an [`AudioPlacement`]: a node to play it, a range in
//! samples, and the clip's own properties along for the ride.
//!
//! The conversions are the point. Everything in the document is in **ticks**
//! and everything the audio thread does is in **samples**, and this pass
//! already owns every tick-to-sample conversion in the project — which is
//! exactly why the placement is built here rather than anywhere that would
//! have to work the tempo out a second time.

use std::collections::HashMap;

use fontelle_model::{Clip, ClipSource, Project, TempoMap};
use fontelle_sequencer::NodeMaps;
use fontelle_types::{
    AssetKind, AssetRef, AudioClipData, MixerTrackId, NodeId, PPQN, Tick,
};
use slotmap::KeyData;

const SR: f64 = 48_000.0;
const BPM: f64 = 120.0;
/// 120 BPM at 48 kHz is exactly 25 samples per tick.
const SAMPLES_PER_TICK: i64 = 25;

fn an_asset() -> AssetRef {
    AssetRef {
        id: fontelle_types::AssetId::default(),
        path: "take.wav".into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    }
}

fn node(n: u64) -> NodeId {
    NodeId::from(KeyData::from_ffi(n))
}

struct Rig {
    project: Project,
    track: MixerTrackId,
    player: NodeId,
}

fn rig() -> Rig {
    let mut project = Project::new("audio");
    project.tempo_map = TempoMap::new(BPM, SR);
    let track = project.mixer.tracks.insert(fontelle_model::MixerTrack::new("Mic"));
    Rig {
        project,
        track,
        player: node(7),
    }
}

impl Rig {
    /// Puts an audio clip at `start`, `length` long, on a fresh lane.
    fn place(&mut self, start: Tick, length: Tick, mut data: AudioClipData) -> fontelle_types::ClipId {
        data.mixer_track = Some(self.track);
        let lane = self.project.lanes.insert(fontelle_model::Lane {
            name: "row".into(),
            height: 32.0,
            color: [0; 4],
            muted: false,
            locked: false,
            order: 0,
        });
        self.project.clips.insert(Clip {
            lane,
            start,
            length,
            source: ClipSource::Audio(data),
            prefab_link: None,
            color: None,
            muted: false,
            loop_length: None,
        })
    }

    fn compile(&self) -> fontelle_types::CompiledTimeline {
        let audio: HashMap<Option<MixerTrackId>, NodeId> =
            [(Some(self.track), self.player)].into_iter().collect();
        fontelle_sequencer::compile_with(
            &self.project,
            &NodeMaps {
                audio: &audio,
                ..Default::default()
            },
            fontelle_sequencer::CompileScope::Song,
        )
    }
}

#[test]
fn an_audio_clip_becomes_a_placement_at_the_sample_the_tick_maps_to() {
    let mut r = rig();
    r.place(PPQN * 4, PPQN * 8, AudioClipData::whole(an_asset(), 100_000, 48_000));
    let timeline = r.compile();

    assert_eq!(timeline.audio.len(), 1);
    let placed = &timeline.audio[0];
    assert_eq!(placed.range.start, PPQN * 4 * SAMPLES_PER_TICK);
    assert_eq!(placed.range.end, PPQN * 12 * SAMPLES_PER_TICK);
    assert_eq!(placed.target, r.player);
    assert_eq!(placed.repeat, 0);
}

#[test]
fn an_audio_clip_produces_no_events_at_all() {
    // It is not a note and it must not look like one to anything downstream.
    let mut r = rig();
    r.place(0, PPQN * 4, AudioClipData::whole(an_asset(), 100_000, 48_000));
    assert!(r.compile().events.is_empty());
}

#[test]
fn a_muted_clip_is_not_placed() {
    let mut r = rig();
    let id = r.place(0, PPQN * 4, AudioClipData::whole(an_asset(), 1000, 48_000));
    r.project.clips[id].muted = true;
    assert!(r.compile().audio.is_empty());
}

#[test]
fn a_clip_on_a_muted_lane_is_not_placed() {
    let mut r = rig();
    let id = r.place(0, PPQN * 4, AudioClipData::whole(an_asset(), 1000, 48_000));
    let lane = r.project.clips[id].lane;
    r.project.lanes[lane].muted = true;
    assert!(r.compile().audio.is_empty());
}

#[test]
fn a_clip_routed_to_a_track_with_no_player_is_left_out_rather_than_panicking() {
    // The document can name a mixer track before the graph has caught up with
    // it — a clip dropped on a track made the same frame.
    let mut r = rig();
    let id = r.place(0, PPQN * 4, AudioClipData::whole(an_asset(), 1000, 48_000));
    let elsewhere = r.project.mixer.tracks.insert(fontelle_model::MixerTrack::new("Other"));
    let ClipSource::Audio(data) = &mut r.project.clips[id].source else {
        unreachable!()
    };
    data.mixer_track = Some(elsewhere);
    assert!(r.compile().audio.is_empty());
}

#[test]
fn a_looped_clip_carries_its_period_in_samples() {
    // Not unrolled into several placements, which is what the note compiler
    // does: a note is a moment and has to be emitted per pass, and a stream is
    // one range that comes round. Unrolling it would also make a sixteen-bar
    // one-bar loop sixteen filter states instead of one.
    let mut r = rig();
    let id = r.place(0, PPQN * 16, AudioClipData::whole(an_asset(), 100_000, 48_000));
    r.project.clips[id].loop_length = Some(PPQN * 4);
    let timeline = r.compile();

    assert_eq!(timeline.audio.len(), 1, "a loop is one placement");
    assert_eq!(timeline.audio[0].repeat, PPQN * 4 * SAMPLES_PER_TICK);
}

#[test]
fn the_placement_carries_the_clips_own_properties() {
    // The player reads them on the audio thread and has no way to ask the
    // document anything (INVARIANT 4), so they travel with the placement.
    let mut r = rig();
    let mut data = AudioClipData::whole(an_asset(), 100_000, 48_000);
    data.gain_db = -4.5;
    data.reverse = true;
    r.place(0, PPQN * 4, data);
    let timeline = r.compile();
    assert_eq!(timeline.audio[0].data.gain_db, -4.5);
    assert!(timeline.audio[0].data.reverse);
}

#[test]
fn clip_mode_leaves_out_every_clip_but_the_one_being_edited() {
    let mut r = rig();
    let first = r.place(0, PPQN * 4, AudioClipData::whole(an_asset(), 1000, 48_000));
    r.place(PPQN * 8, PPQN * 4, AudioClipData::whole(an_asset(), 1000, 48_000));

    let audio: HashMap<Option<MixerTrackId>, NodeId> =
        [(Some(r.track), r.player)].into_iter().collect();
    let timeline = fontelle_sequencer::compile_with(
        &r.project,
        &NodeMaps {
            audio: &audio,
            ..Default::default()
        },
        fontelle_sequencer::CompileScope::Clip(first),
    );
    assert_eq!(timeline.audio.len(), 1);
    assert_eq!(timeline.audio[0].range.start, 0);
}

#[test]
fn a_clip_on_the_master_is_placed_on_whatever_plays_the_master() {
    // `None` is the master, matching `Channel::mixer_track`, and a file dropped
    // on the arrangement before anybody has built a mixer track has to sound.
    let mut r = rig();
    let id = r.place(0, PPQN * 4, AudioClipData::whole(an_asset(), 1000, 48_000));
    let ClipSource::Audio(data) = &mut r.project.clips[id].source else {
        unreachable!()
    };
    data.mixer_track = None;

    let master = node(99);
    let audio: HashMap<Option<MixerTrackId>, NodeId> =
        [(None, master)].into_iter().collect();
    let timeline = fontelle_sequencer::compile_with(
        &r.project,
        &NodeMaps {
            audio: &audio,
            ..Default::default()
        },
        fontelle_sequencer::CompileScope::Song,
    );
    assert_eq!(timeline.audio.len(), 1);
    assert_eq!(timeline.audio[0].target, master);
}
