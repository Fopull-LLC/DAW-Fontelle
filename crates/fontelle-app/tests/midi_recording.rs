//! Recording what you play (TDD §14.7).
//!
//! The whole path with no MIDI device and no sound card: bytes into a
//! `MidiRouter`, through the lock-free queue the audio thread drains, mirrored
//! into the capture ring while the transport is recording, turned into a note
//! clip by a command, and played back through the graph. Only `midir` handing
//! over the bytes is missing, and that has its own hardware test.

mod common;

use std::sync::Arc;

use fontelle_app::{RealiseOptions, SampleLibrary, realise, render_offline};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_engine::{
    BLOCK_SIZE, CompiledGraph, IdleGate, LiveEventSource, Transport, TransportReader,
    TransportState, live_capture_channel, live_event_channel,
};
use fontelle_midi::{DeviceMapping, MidiRouter};
use fontelle_model::{
    AddClip, Clip, ClipSource, Command, Lane, NoteData, Project, notes_from_capture,
};
use fontelle_types::{CompiledTimeline, EventSink, PPQN, TimedEvent};

use common::SR;

const NOTE_ON: u8 = 0x90;
const NOTE_OFF: u8 = 0x80;

/// A flat, sustaining instrument, so a rendered level says something about
/// which notes are sounding and nothing about an envelope.
fn flat_patch(library: &mut SampleLibrary) -> Patch {
    let asset = library.insert_synthetic(
        "recording",
        SampleBuffer {
            data: Arc::from(vec![0.5f32; 200_000]),
            sample_rate: SR,
        },
    );
    let disabled = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
    };
    let instant = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.001,
        curve: EnvelopeCurve::Linear,
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
                end_offset: 200_000.0,
                ..PlaybackConfig::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [disabled, disabled],
        envelopes: vec![instant, instant],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
    }
}

/// A project with one channel holding `patch`, and an empty lane to record
/// onto — the record-armed state, minus the arm button.
fn armed_project(library: &SampleLibrary, patch: &Patch) -> (Project, fontelle_types::LaneId) {
    let mut project = Project::new("take");
    project.tempo_map = fontelle_model::TempoMap::new(120.0, SR as f64);
    let mut add = fontelle_model::AddChannel::new("Keys", None);
    add.apply(&mut project).unwrap();
    let channel = add.channel().unwrap();
    fontelle_app::set_channel_patch(&mut project, channel, patch, library).unwrap();
    let lane = project.lanes.insert(Lane {
        name: "Take 1".into(),
        height: 32.0,
        color: [0; 4],
        muted: false,
        locked: false,
        order: 0,
    });
    (project, lane)
}

/// The audio callback, minus the sound card.
struct Callback {
    reader: TransportReader,
    gate: IdleGate,
    source: LiveEventSource,
    graph: CompiledGraph,
    timeline: CompiledTimeline,
}

impl Callback {
    fn block(&mut self, transport: &Transport) {
        let live = self.source.drain(
            self.reader.position(),
            transport.state() == TransportState::Recording,
        );
        let awake = self.gate.is_awake(live.len());
        let step = self
            .reader
            .next_step(transport, &self.timeline, BLOCK_SIZE, BLOCK_SIZE, awake);
        if step.reset {
            self.graph.reset_sequenced();
        }
        if !step.process {
            self.gate.observe(0.0);
            return;
        }
        self.graph
            .process_block_with_live(step.events, live, step.snapshot, step.range.clone());
        let peak = (0..2)
            .map(|bus| {
                self.graph.buffer_pool.buffer_mut(bus)[..step.frames]
                    .iter()
                    .fold(0.0f32, |m, s| m.max(s.abs()))
            })
            .fold(0.0f32, f32::max);
        self.gate.observe(peak);
    }
}

struct Take {
    project: Project,
    library: SampleLibrary,
    lane: fontelle_types::LaneId,
    channel: fontelle_types::ChannelId,
    captured: Vec<TimedEvent>,
    /// Where the transport was when recording stopped.
    end_sample: i64,
}

/// Plays `performance` — (block index, MIDI bytes) — into a recording
/// transport, and returns what the capture caught.
fn record(performance: &[(usize, [u8; 3])], blocks: usize) -> Take {
    let mut library = SampleLibrary::new();
    let patch = flat_patch(&mut library);
    let (project, lane) = armed_project(&library, &patch);
    let channel = project.channels.keys().next().unwrap();

    let realised = realise(
        &project,
        &library,
        RealiseOptions {
            sample_rate: SR,
            block_size: BLOCK_SIZE,
            quality: Interpolation::Draft,
        },
    )
    .unwrap();
    let node = realised.channel_nodes[&channel];

    let (mut source, mut ports) = live_event_channel(2, 64);
    let (writer, mut capture) = live_capture_channel(1_024);
    source.arm_capture(writer);
    let mut port: Box<dyn EventSink> = Box::new(ports.claim().unwrap());
    let mut router = MidiRouter::new(node, u32::MAX, DeviceMapping::default());

    let mut callback = Callback {
        reader: TransportReader::new(),
        gate: IdleGate::new(),
        source,
        graph: realised.graph,
        timeline: fontelle_sequencer::compile(
            &project,
            &realised.channel_nodes,
            &Default::default(),
        ),
    };

    let transport = Transport::new();
    transport.set_state(TransportState::Recording);
    for block in 0..blocks {
        for (at, bytes) in performance {
            if *at == block {
                router.handle(bytes, port.as_mut());
            }
        }
        callback.block(&transport);
    }
    transport.stop();

    let mut captured = Vec::new();
    capture.drain_into(&mut captured);
    assert_eq!(capture.dropped(), 0, "the ring must not have overflowed");

    Take {
        project,
        library,
        lane,
        channel,
        captured,
        end_sample: transport.position_sample(),
    }
}

/// Commits a take to the document, as the stop button will.
fn keep(take: &mut Take) -> fontelle_types::ClipId {
    let source = notes_from_capture(&take.captured, &take.project.tempo_map, 0, take.end_sample);
    let ClipSource::Notes(data) = source else {
        unreachable!()
    };
    let length = data
        .notes
        .values()
        .map(|n| n.start + n.length)
        .max()
        .unwrap_or(0);
    let mut add = AddClip::new(Clip {
        lane: take.lane,
        start: 0,
        length,
        source: ClipSource::Notes(NoteData {
            channel: take.channel,
            notes: data.notes,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    });
    add.apply(&mut take.project).expect("the take must land");
    add.id().unwrap()
}

fn played(take: &Take, clip: fontelle_types::ClipId) -> Vec<(i64, i64, u8, u8)> {
    let ClipSource::Notes(data) = &take.project.clips[clip].source else {
        unreachable!()
    };
    let mut out: Vec<_> = data
        .notes
        .values()
        .map(|n| (n.start, n.length, n.key, n.velocity))
        .collect();
    out.sort();
    out
}

/// Renders the project from the top, so a recorded clip can be heard back.
fn play_back(take: &Take) -> Vec<f32> {
    let mut realised = realise(
        &take.project,
        &take.library,
        RealiseOptions {
            sample_rate: SR,
            block_size: BLOCK_SIZE,
            quality: Interpolation::Draft,
        },
    )
    .unwrap();
    let timeline =
        fontelle_sequencer::compile(&take.project, &realised.channel_nodes, &Default::default());
    render_offline(&timeline, &mut realised.graph, 48_000)
}

#[test]
fn a_key_played_into_a_recording_transport_comes_back_as_a_note() {
    // One block is 128 frames, so block 40 starts at sample 5120 and block 140
    // at 17 920. At 120 bpm / 48 kHz a tick is 25 samples, and the conversion
    // rounds to nearest rather than truncating — 5120 is 204.8 ticks, and
    // truncating every note would drag the whole take early.
    let mut take = record(&[(40, [NOTE_ON, 60, 100]), (140, [NOTE_OFF, 60, 0])], 200);
    assert!(!take.captured.is_empty(), "nothing was captured");

    let clip = keep(&mut take);
    let notes = played(&take, clip);
    assert_eq!(notes.len(), 1, "one key, one note: {notes:?}");
    let (start, length, key, velocity) = notes[0];
    assert_eq!((key, velocity), (60, 100));
    assert_eq!(start, 205, "5120 samples is 204.8 ticks, rounded");
    assert_eq!(length, 512, "17 920 is 716.8 ticks, rounded: 717 - 205");
}

#[test]
fn a_recorded_take_plays_back() {
    // The gate's own words: "record more from a MIDI keyboard". A take that
    // cannot be heard again is a log file.
    let mut take = record(&[(10, [NOTE_ON, 64, 110]), (60, [NOTE_OFF, 64, 0])], 100);
    let clip = keep(&mut take);
    assert_eq!(played(&take, clip).len(), 1);

    let audio = play_back(&take);
    let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(
        peak > 0.1,
        "the recorded note must sound on playback: {peak}"
    );

    // And it starts where it was played rather than at the top of the piece:
    // block 10 is sample 1280, so the first 1000 frames are silent.
    let opening = audio[..2_000].iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert_eq!(opening, 0.0, "the take must keep its timing");
}

#[test]
fn a_chord_records_as_three_notes_that_start_together() {
    let mut take = record(
        &[
            (20, [NOTE_ON, 60, 90]),
            (20, [NOTE_ON, 64, 90]),
            (20, [NOTE_ON, 67, 90]),
            (80, [NOTE_OFF, 60, 0]),
            (80, [NOTE_OFF, 64, 0]),
            (80, [NOTE_OFF, 67, 0]),
        ],
        120,
    );
    let clip = keep(&mut take);
    let notes = played(&take, clip);
    assert_eq!(notes.len(), 3);
    assert!(
        notes.iter().all(|n| n.0 == notes[0].0),
        "a chord is simultaneous: {notes:?}"
    );
    assert_eq!(
        notes.iter().map(|n| n.2).collect::<Vec<_>>(),
        vec![60, 64, 67]
    );
}

#[test]
fn nothing_is_recorded_when_the_transport_is_only_playing() {
    // Playing along without the record button down leaves no take behind.
    let mut library = SampleLibrary::new();
    let patch = flat_patch(&mut library);
    let (project, _lane) = armed_project(&library, &patch);
    let channel = project.channels.keys().next().unwrap();
    let realised = realise(
        &project,
        &library,
        RealiseOptions {
            sample_rate: SR,
            block_size: BLOCK_SIZE,
            quality: Interpolation::Draft,
        },
    )
    .unwrap();
    let node = realised.channel_nodes[&channel];

    let (mut source, mut ports) = live_event_channel(2, 64);
    let (writer, mut capture) = live_capture_channel(64);
    source.arm_capture(writer);
    let mut port: Box<dyn EventSink> = Box::new(ports.claim().unwrap());
    let mut router = MidiRouter::new(node, u32::MAX, DeviceMapping::default());
    let mut callback = Callback {
        reader: TransportReader::new(),
        gate: IdleGate::new(),
        source,
        graph: realised.graph,
        timeline: fontelle_sequencer::compile(
            &project,
            &realised.channel_nodes,
            &Default::default(),
        ),
    };

    let transport = Transport::new();
    transport.play();
    router.handle(&[NOTE_ON, 60, 100], port.as_mut());
    for _ in 0..20 {
        callback.block(&transport);
    }

    let mut captured = Vec::new();
    capture.drain_into(&mut captured);
    assert!(captured.is_empty(), "captured {} events", captured.len());
}

#[test]
fn a_take_survives_being_saved_and_reopened() {
    // The whole point of recording it: `--midi-in --record` writes a project
    // you can `--open` and hear back.
    let mut take = record(&[(10, [NOTE_ON, 62, 100]), (70, [NOTE_OFF, 62, 0])], 100);
    let clip = keep(&mut take);
    let before = played(&take, clip);

    let bundle = std::env::temp_dir().join(format!(
        "fontelle-take-{}-{}.fontelle",
        std::process::id(),
        PPQN
    ));
    std::fs::remove_dir_all(&bundle).ok();
    fontelle_app::save_project(&take.project, &bundle).expect("save");
    let opened = fontelle_app::open_project(&bundle).expect("open");

    let reopened_clip = opened.project.clips.keys().next().unwrap();
    let ClipSource::Notes(data) = &opened.project.clips[reopened_clip].source else {
        unreachable!()
    };
    let mut after: Vec<_> = data
        .notes
        .values()
        .map(|n| (n.start, n.length, n.key, n.velocity))
        .collect();
    after.sort();
    assert_eq!(before, after);
    std::fs::remove_dir_all(&bundle).ok();
}
