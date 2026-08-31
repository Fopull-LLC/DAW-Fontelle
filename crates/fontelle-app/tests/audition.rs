//! Clicking a note has to make a sound, and it has to make the *right* sound.
//!
//! Reported from using the window: *"I can't click on notes in the piano roll
//! to hear them — I just hear a short flicker of static."*
//!
//! Two separate things, and this file pins down the second: the audition path
//! from `Session` (which is what the window calls) all the way through the real
//! graph and the real idle gate, with a stopped transport. `tests/live_midi.rs`
//! covers the same engine path from a MIDI device; this covers it from the
//! mouse, which is the half the window actually uses and the half that had no
//! test at all.

mod common;

use std::sync::Arc;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_engine::{
    BLOCK_SIZE, CompiledGraph, IdleGate, LiveEventSource, Transport, TransportReader,
    TransportState, graph_channel, live_event_channel, timeline_channel,
};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::StudioHost;

use common::SR;

/// A patch that sustains for ever, so "did the note keep sounding" is a
/// question about the audition path rather than about an envelope.
fn sustaining(library: &mut SampleLibrary) -> Patch {
    let cycle = 100;
    let data: Vec<f32> = (0..cycle)
        .map(|i| (i as f32 / cycle as f32 * std::f32::consts::TAU).sin())
        .collect();
    let asset = library.insert_synthetic(
        "cycle",
        SampleBuffer {
            data: Arc::from(data),
            sample_rate: SR,
        },
    );
    let disabled = FilterSlot {
        mode: SvfMode::Lowpass,
        cutoff_hz: 20_000.0,
        resonance: 0.0,
        enabled: false,
    };
    let env = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        release_s: 0.005,
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
                loop_mode: LoopMode::Forward,
                loop_start: 0.0,
                loop_end: cycle as f64,
                end_offset: cycle as f64,
                interpolation: Some(Interpolation::Normal),
                ..PlaybackConfig::default()
            },
            gain_db: 0.0,
            pan: 0.0,
        }],
        filters: [disabled, disabled],
        envelopes: vec![env, env],
        lfos: Vec::new(),
        mod_matrix: ModMatrix::default(),
        voice_config: VoiceConfig::default(),
    }
}

/// The audio callback, minus the sound card — the same shape
/// `AudioDevice::start_output_stream` runs, including the idle gate, because
/// the gate is exactly what decides whether a stopped transport keeps making
/// the sound somebody is holding.
struct Callback {
    reader: TransportReader,
    gate: IdleGate,
    source: LiveEventSource,
    graphs: fontelle_engine::GraphSource,
    timeline: CompiledTimeline,
}

impl Callback {
    fn block(&mut self, transport: &Transport) -> f32 {
        let live = self.source.drain(
            self.reader.position(),
            transport.state() == TransportState::Recording,
        );
        let awake = self.gate.is_awake(live.len());
        self.graphs.take_update();
        let graph: &mut CompiledGraph = self.graphs.current();
        let step = self
            .reader
            .next_step(transport, &self.timeline, BLOCK_SIZE, BLOCK_SIZE, awake);
        if step.reset {
            graph.reset_sequenced();
        }
        if !step.process {
            self.gate.observe(0.0);
            return 0.0;
        }
        graph.process_block_with_live(step.events, live, step.snapshot, step.range.clone());
        let peak = (0..2)
            .map(|bus| {
                graph.buffer_pool.buffer_mut(bus)[..step.frames]
                    .iter()
                    .fold(0.0f32, |m, s| m.max(s.abs()))
            })
            .fold(0.0f32, f32::max);
        self.gate.observe(peak);
        peak
    }
}

/// A session with a real instrument on its one channel, and the RT thread's end
/// of every channel that feeds it.
fn rig() -> (Session, Callback, Transport) {
    let mut library = SampleLibrary::new();
    let patch = sustaining(&mut library);
    let project = common::demo_with(&patch, &library);
    let clip = Session::first_clip(&project).expect("the demo project has a clip");
    let (realised, timeline) =
        common::realise_at(&project, &library, fontelle_app::PLAYBACK_QUALITY);

    let (timeline_publisher, _timeline_source) = timeline_channel(timeline.clone());
    let (graph_publisher, graph_source) = graph_channel(realised.graph);
    let (live_source, mut ports) = live_event_channel(4, 64);
    let port = ports.claim().expect("a free port");

    let session = Session::new(
        project,
        library,
        realised.channel_nodes,
        timeline_publisher,
        RealiseOptions {
            sample_rate: SR,
            block_size: BLOCK_SIZE,
            quality: fontelle_app::PLAYBACK_QUALITY,
        },
        clip,
        None,
    )
    .with_graphs(graph_publisher, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_audition(Box::new(port))
    .with_settings_path(
        std::env::temp_dir().join(format!("fontelle-audition-{}.json", std::process::id())),
    );

    (
        session,
        Callback {
            reader: TransportReader::new(),
            gate: IdleGate::new(),
            source: live_source,
            graphs: graph_source,
            timeline,
        },
        Transport::new(),
    )
}

#[test]
fn clicking_a_note_with_the_transport_stopped_sounds_it() {
    let (mut session, mut callback, transport) = rig();
    assert_eq!(
        callback.block(&transport),
        0.0,
        "a stopped transport nobody is playing is silent"
    );

    session.audition_on(60, 100, 0);
    assert!(
        callback.block(&transport) > 1e-3,
        "the click made no sound at all"
    );
}

#[test]
fn a_held_audition_keeps_sounding_rather_than_flickering() {
    // The reported symptom: *"a short flicker of static"* — a note that sounds
    // for one block and is then cut is a click, not a note. The idle gate is
    // what has to keep the graph awake once the event that started the note has
    // gone past.
    let (mut session, mut callback, transport) = rig();
    session.audition_on(60, 100, 0);

    for block in 0..64 {
        let peak = callback.block(&transport);
        assert!(
            peak > 1e-3,
            "block {block} of a held note was silent — the note is being cut short"
        );
    }
}

#[test]
fn releasing_an_audition_lets_it_die_away_and_the_graph_sleep() {
    let (mut session, mut callback, transport) = rig();
    session.audition_on(60, 100, 0);
    for _ in 0..8 {
        assert!(callback.block(&transport) > 1e-3);
    }

    session.audition_off(60);
    // A 5 ms release at 48 kHz is under three blocks; give it ten and then it
    // has to be both silent and asleep, or a stopped studio burns a core.
    let mut silent = 0;
    for _ in 0..10 {
        if callback.block(&transport) <= 1e-4 {
            silent += 1;
        }
    }
    assert!(silent > 0, "the note never stopped");
    assert!(
        !callback.gate.is_awake(0),
        "the graph is still being processed after the note died away"
    );
}

#[test]
fn an_audition_goes_to_the_channel_the_rack_has_selected() {
    // The window auditions on the *selected* channel, so a second instrument
    // must not be the one that speaks.
    let (mut session, mut callback, transport) = rig();
    session.select_channel(0);
    session.audition_on(60, 100, 0);
    assert!(callback.block(&transport) > 1e-3);

    // A channel index nobody has is not a reason to send the note to node
    // zero, which is somebody else's instrument.
    session.select_channel(99);
    assert_eq!(session.selected_channel(), 0, "the selection did not move");
}

#[test]
fn auditions_survive_the_transport_being_started_and_stopped_under_them() {
    // Holding a key while pressing play is ordinary, and a stop must cut what
    // the *song* was playing without cutting what the player is holding
    // (TDD §6.3 against §14).
    let (mut session, mut callback, transport) = rig();
    session.audition_on(72, 100, 0);
    assert!(callback.block(&transport) > 1e-3);

    transport.play();
    for _ in 0..4 {
        callback.block(&transport);
    }
    transport.stop();
    // The stop transition resets the sequenced voices on this block.
    callback.block(&transport);
    assert!(
        callback.block(&transport) > 1e-3,
        "stopping the song cut the note the player was holding"
    );
}

/// **Why the window must release a key before striking it again.**
///
/// Reported from using the window: *"when placing a note sometimes it would
/// play a different note on hold that wouldn't stop until I replayed again."*
///
/// This is that note, in the engine. `Sampler::note_off` releases **one**
/// voice — the first active one matching the key and the voice context — which
/// is correct and deliberate (§11.4's per-clip tagging depends on it). So two
/// note-ons on one key and one note-off leaves a voice sounding with nothing
/// left that will ever address it.
///
/// The window's own bookkeeping is what stops that ever being sent, and it is
/// tested as a state machine in `fontelle-ui`'s `tests/audition_voice.rs`. This
/// test is the other half: it pins down the engine behaviour that makes the
/// rule necessary, so that if voice allocation ever changes, the reason the
/// window is careful is written down where somebody would find it.
#[test]
fn a_doubled_note_on_leaves_a_voice_that_one_note_off_cannot_stop() {
    let (mut session, mut callback, transport) = rig();

    // What the window used to do: sound the same key twice without letting go.
    session.audition_on(60, 100, 0);
    for _ in 0..4 {
        assert!(callback.block(&transport) > 1e-3);
    }
    session.audition_on(60, 100, 0);
    for _ in 0..4 {
        assert!(callback.block(&transport) > 1e-3);
    }

    // And then release it once, the way a single mouse-up does.
    session.audition_off(60);
    let mut loudest: f32 = 0.0;
    for _ in 0..16 {
        loudest = loudest.max(callback.block(&transport));
    }
    assert!(
        loudest > 1e-3,
        "if this ever goes quiet, voice allocation has changed and the window's \
         release-before-retrigger rule can be revisited"
    );
    assert!(
        callback.gate.is_awake(0),
        "the graph is still awake because a voice is still sounding — which is \
         exactly the hung note that was reported"
    );

    // The window's rule, applied: one more note-off per note-on, and it stops.
    session.audition_off(60);
    let mut silent = 0;
    for _ in 0..16 {
        if callback.block(&transport) <= 1e-4 {
            silent += 1;
        }
    }
    assert!(silent > 0, "matched note-offs have to end the note");
}
