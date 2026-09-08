//! The M0 gate (FONTELLE_TDD.md §22), assembled end to end minus the one
//! step that genuinely needs real hardware (device out — see
//! `fontelle-engine`'s `manual_audio_output` test for that): a `Project`
//! with one channel/lane/clip/note, compiled by `fontelle_sequencer::compile`
//! into a `CompiledTimeline`, fed block-by-block through
//! `CompiledTimeline::events_for_block` into a real `CompiledGraph` wrapping
//! a real `fontelle_core::Sampler` — exactly the sequence
//! `fontelle-app::play_sf2` drives against a real device. This is what
//! proves "triggered by a note from a clip on the timeline" is real, in a
//! form CI can run without speakers.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, SampleStore,
    Sampler, Source, VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_engine::{BufferPool, CompiledGraph, SamplerNode, ScheduledNode};
use fontelle_model::Arena;
use fontelle_model::{
    Channel, Clip, ClipSource, Lane, MixerTrack, Note, NoteData, Project, TempoMap,
};
use fontelle_types::{ChannelId, NodeId};
use slotmap::SlotMap;

const SR: f32 = 48_000.0;
const BLOCK: usize = 128;

fn synthetic_patch(store: &mut SampleStore) -> Patch {
    let asset = store.insert(SampleBuffer {
        data: Arc::from(vec![1.0; 10_000]),
        sample_rate: SR as u32,
    });
    let disabled_filter = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
        ..Default::default()
    };
    let instant = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.01,
        curve: EnvelopeCurve::Linear,
        ..Default::default()
    };
    Patch {
        layers: vec![Layer {
            source: Source::Sample { file: asset },
            key_range: (0, 127),
            vel_range: (0, 127),
            root_key: 60,
            fine_tune_cents: 0.0,
            playback: PlaybackConfig {
                loop_mode: LoopMode::Off,
                interpolation: Some(Interpolation::Draft),
                end_offset: 10_000.0,
                ..PlaybackConfig::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [disabled_filter, disabled_filter],
        envelopes: vec![instant, instant],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
        ..Default::default()
    }
}

/// The same fixture as `synthetic_patch`, registered in a library so the patch
/// survives the trip through the document that `realise` puts it through.
fn synthetic_patch_in(library: &mut fontelle_app::SampleLibrary) -> Patch {
    let mut store = SampleStore::new();
    let mut patch = synthetic_patch(&mut store);
    let asset = library.insert_synthetic(
        "flat",
        SampleBuffer {
            data: Arc::from(vec![1.0; 10_000]),
            sample_rate: SR as u32,
        },
    );
    patch.layers[0].source = Source::Sample { file: asset };
    patch
}

fn rms(buf: &[f32]) -> f32 {
    (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
}

#[test]
fn a_note_on_a_clip_on_a_timeline_reaches_the_sampler_through_the_compiled_graph() {
    // --- Document: one channel, one lane, one clip, one note. ---
    let mut project = Project::new("vertical slice");
    project.tempo_map = TempoMap::new(120.0, SR as f64);

    let track = project.mixer.tracks.insert(MixerTrack::new("ch"));
    let channel_id = project.channels.insert(Channel {
        preset: None,
        instrument: None,
        name: "ch".into(),
        color: [0, 0, 0, 255],
        mixer_track: Some(track),
        patch_data: None,
        plugin: None,
        pan: 0.0,
        muted: false,
        soloed: false,
        named_keys: false,
        gain_db: 0.0,
    });
    let lane_id = project.lanes.insert(Lane {
        name: "lane".into(),
        height: 32.0,
        color: [0, 0, 0, 255],
        muted: false,
        locked: false,
        order: 0,
    });

    let note_length_ticks = fontelle_types::PPQN; // one quarter note = 24000 samples @ 120bpm/48kHz
    let mut notes = Arena::default();
    notes.insert(Note {
        start: 0,
        length: note_length_ticks,
        key: 60,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    });
    project.clips.insert(Clip {
        lane: lane_id,
        start: 0,
        length: note_length_ticks,
        source: ClipSource::Notes(NoteData {
            channel: channel_id,
            notes,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });

    // --- Engine-side node identity + compiled graph, built the same way
    //     fontelle-app builds them for real playback. ---
    let mut node_ids: SlotMap<NodeId, ChannelId> = SlotMap::default();
    let node_id = node_ids.insert(channel_id);
    let channel_nodes: HashMap<ChannelId, NodeId> = HashMap::from([(channel_id, node_id)]);

    let timeline = fontelle_sequencer::compile(&project, &channel_nodes, &Default::default());
    assert_eq!(
        timeline.events.len(),
        2,
        "expected one NoteOn and one NoteOff"
    );
    assert_eq!(timeline.events[0].sample, 0);
    assert_eq!(timeline.events[1].sample, 24_000);

    let mut store = SampleStore::new();
    let patch = synthetic_patch(&mut store);
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&fontelle_core::PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let node = SamplerNode::new(sampler, Arc::new(store));

    let mut graph = CompiledGraph {
        schedule: vec![ScheduledNode {
            id: node_id,
            node: Box::new(node),
            input_buffers: Vec::new(),
            output_buffers: vec![0],
        }],
        buffer_pool: BufferPool::with_capacity(1, BLOCK),
    };
    graph.prepare(SR, BLOCK as u32);

    // --- Drive the graph exactly like `AudioDevice`'s real callback does:
    //     block-by-block, slicing the compiled timeline by sample range. ---
    let transport = fontelle_engine::TransportSnapshot {
        state: fontelle_engine::TransportState::Playing,
        position_sample: 0,
        bpm: fontelle_types::DEFAULT_BPM,
    };
    let mut cursor = 0usize;
    let mut sample = 0i64;
    let mut heard_sound = false;

    // Run past the note's length so both the NoteOn and NoteOff are consumed.
    while sample < 24_000 + BLOCK as i64 {
        let range = sample..sample + BLOCK as i64;
        let events = timeline.events_for_block(&mut cursor, range.clone());
        graph.process_block(events, transport, range);
        if rms(graph.buffer_pool.buffer_mut(0)) > 0.5 {
            heard_sound = true;
        }
        sample += BLOCK as i64;
    }

    assert!(
        heard_sound,
        "a note from a clip on the timeline must produce real sampler output"
    );
    assert_eq!(
        cursor,
        timeline.events.len(),
        "both timeline events must have been consumed by the end of playback"
    );
}

/// TDD §22's M0 gate, end to end, minus only the physical device: "audio
/// callback → compiled graph → one sampler voice reading a real SF2 zone →
/// mixer track → device out, triggered by a note from a clip on the timeline,
/// at 128 frames / 48 kHz, with the zero-allocation assertion active."
///
/// This drives the *same* library functions `fontelle-app --play-sf2` uses,
/// so it isn't a parallel re-implementation that could drift from what
/// actually ships. The SF2 zone is stood in for by a synthetic patch —
/// real-file import has its own exact-value tests in `fontelle-assets`, and
/// binding this test to a licensed binary fixture would make it
/// unrunnable in CI.
#[test]
fn the_full_m0_chain_renders_the_demo_song_through_a_mixer_track() {
    let mut library = fontelle_app::SampleLibrary::new();
    let patch = synthetic_patch_in(&mut library);
    let (project, mut realised, timeline) =
        common::demo_rig(&patch, &library, fontelle_app::PLAYBACK_QUALITY);
    assert!(
        !timeline.events.is_empty(),
        "the demo song must compile to real events"
    );
    let graph = &mut realised.graph;

    let transport = fontelle_engine::TransportSnapshot {
        state: fontelle_engine::TransportState::Playing,
        position_sample: 0,
        bpm: fontelle_types::DEFAULT_BPM,
    };
    let total = fontelle_app::project_duration_samples(&project, fontelle_types::PPQN);
    let mut cursor = 0usize;
    let mut sample = 0i64;
    let mut peak_left: f32 = 0.0;
    let mut peak_right: f32 = 0.0;

    while sample < total {
        let range = sample..sample + BLOCK as i64;
        let events = timeline.events_for_block(&mut cursor, range.clone());
        graph.process_block(events, transport, range);

        peak_left = peak_left.max(rms(graph.buffer_pool.buffer_mut(0)));
        peak_right = peak_right.max(rms(graph.buffer_pool.buffer_mut(1)));
        sample += BLOCK as i64;
    }

    assert!(
        peak_left > 0.1,
        "the demo song must produce audible output, got peak rms {peak_left}"
    );
    assert!(
        (peak_left - peak_right).abs() < 1e-4,
        "a centred mixer track must deliver both channels equally: {peak_left} vs {peak_right}"
    );
    assert_eq!(
        cursor,
        timeline.events.len(),
        "playback must consume every event in the timeline"
    );
}
