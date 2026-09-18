//! Live MIDI, end to end, with no MIDI device: raw bytes into a `MidiRouter`,
//! through the lock-free queue the audio thread drains, into the graph, out as
//! audio.
//!
//! The only piece this cannot cover is `midir` itself handing over the bytes.
//! Everything from the bytes onward — the decode, the mapping, the queue, the
//! stamping, the routing to a node, the audition path that lets a stopped
//! transport make sound — is the same code the device path runs.

mod common;

use std::sync::Arc;

use fontelle_app::SampleLibrary;
use fontelle_core::{
    FilterSlot, Layer, LoopMode, ModMatrix, Patch, PlaybackConfig, SampleBuffer, Source,
    VoiceConfig,
};
use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};
use fontelle_engine::{
    BLOCK_SIZE, CompiledGraph, IdleGate, LiveEventSource, Transport, TransportReader,
    TransportState, live_event_channel,
};
use fontelle_midi::{DeviceMapping, MidiRouter};
use fontelle_types::{CompiledTimeline, EventSink, NodeId};

use common::SR;

const NOTE_ON: u8 = 0x90;
const NOTE_OFF: u8 = 0x80;

fn patch(library: &mut SampleLibrary) -> Patch {
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
        ..Default::default()
    };
    let env = EnvelopeConfig {
        delay_s: 0.0,
        attack_s: 0.0,
        hold_s: 0.0,
        decay_s: 0.0,
        sustain_level: 1.0,
        // Short, so "the note stopped" is observable within a few blocks.
        release_s: 0.005,
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
        ..Default::default()
    }
}

fn song_and_graph() -> (CompiledGraph, CompiledTimeline, NodeId) {
    let mut library = SampleLibrary::new();
    let patch = patch(&mut library);
    graph_playing(patch, library)
}

/// [`song_and_graph`] over a patch of the caller's own — for the wheels,
/// where what a controller does is the patch's decision and so a patch that
/// routes one is the only way to hear it.
fn graph_playing(
    patch: Patch,
    library: SampleLibrary,
) -> (CompiledGraph, CompiledTimeline, NodeId) {
    let (project, realised, timeline) =
        common::demo_rig(&patch, &library, fontelle_app::PLAYBACK_QUALITY);
    // Where a live note is sent: the first channel's node, which is what
    // `--play-sf2 --midi-in` picks in the absence of any focus to follow.
    let node = project
        .channels
        .keys()
        .next()
        .and_then(|c| realised.channel_nodes.get(&c).copied())
        .expect("the demo project has one channel");
    (realised.graph, timeline, node)
}

/// The audio callback, minus the sound card: drain, decide whether a stopped
/// transport still has to run, render, measure.
struct Callback {
    reader: TransportReader,
    gate: IdleGate,
    source: LiveEventSource,
    graph: CompiledGraph,
    timeline: CompiledTimeline,
    /// The first sample of the block just rendered. A voice reading through a
    /// looped sample lands on a different phase every block, so this changes
    /// while a note sustains and repeats exactly if the voice is being
    /// restarted — which the block's *peak* cannot tell you, since a block
    /// long enough to contain a whole cycle peaks at the same value either
    /// way.
    first_sample: f32,
}

impl Callback {
    /// Renders one block and returns its peak.
    fn block(&mut self, transport: &Transport) -> f32 {
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
            self.first_sample = 0.0;
            return 0.0;
        }

        self.graph
            .process_block_with_live(step.events, live, step.snapshot, step.range.clone());
        self.first_sample = self.graph.buffer_pool.buffer_mut(0)[0];
        let peak = (0..2)
            .map(|bus| {
                self.graph.buffer_pool.buffer_mut(bus)[..step.frames]
                    .iter()
                    .fold(0.0f32, |m, s| m.max(s.abs()))
            })
            .fold(0.0f32, f32::max);
        self.gate.observe(peak);
        peak
    }

    /// Renders until the output goes quiet or `limit` blocks pass, returning
    /// how many blocks made sound.
    fn blocks_until_silent(&mut self, transport: &Transport, limit: usize) -> usize {
        let mut sounding = 0;
        for _ in 0..limit {
            if self.block(transport) > 1e-4 {
                sounding += 1;
            } else {
                break;
            }
        }
        sounding
    }
}

fn rig() -> (Callback, MidiRouter, Box<dyn EventSink>, Transport) {
    rig_over(song_and_graph())
}

fn rig_over(
    built: (CompiledGraph, CompiledTimeline, NodeId),
) -> (Callback, MidiRouter, Box<dyn EventSink>, Transport) {
    let (graph, timeline, node) = built;
    let (source, mut ports) = live_event_channel(4, 64);
    let port = ports.claim().expect("a free port");
    (
        Callback {
            reader: TransportReader::new(),
            gate: IdleGate::new(),
            source,
            graph,
            timeline,
            first_sample: 0.0,
        },
        MidiRouter::new(node, u32::MAX, DeviceMapping::default()),
        Box::new(port),
        Transport::new(),
    )
}

#[test]
fn a_key_pressed_with_the_transport_stopped_makes_a_sound() {
    // The whole point of live input, and the one behaviour that a literal
    // reading of TDD §6.3 ("when stopped, the graph is not processed") would
    // have made impossible.
    let (mut callback, mut router, mut sink, transport) = rig();

    assert_eq!(
        callback.block(&transport),
        0.0,
        "nothing has been played yet, and a stopped transport is silent"
    );

    router.handle(&[NOTE_ON, 60, 100], sink.as_mut());
    assert!(
        callback.block(&transport) > 0.01,
        "the key is down and the transport is stopped — it should still sound"
    );
}

#[test]
fn a_note_keeps_sounding_in_the_blocks_after_the_one_its_event_arrived_in() {
    // The event lands in a single block and the note lasts thousands. A
    // stopped transport that only ran the graph on blocks carrying an event
    // would turn a held key into a 2.7 ms click.
    let (mut callback, mut router, mut sink, transport) = rig();
    router.handle(&[NOTE_ON, 60, 100], sink.as_mut());

    for block in 0..50 {
        assert!(
            callback.block(&transport) > 0.01,
            "the key is still down at block {block}"
        );
    }
}

#[test]
fn releasing_the_key_lets_the_graph_go_back_to_idle() {
    // And the other half: idle CPU has to come back down, or "stopped" costs
    // as much as playing forever after the first note.
    let (mut callback, mut router, mut sink, transport) = rig();
    router.handle(&[NOTE_ON, 60, 100], sink.as_mut());
    callback.block(&transport);

    router.handle(&[NOTE_OFF, 60, 0], sink.as_mut());
    let sounding = callback.blocks_until_silent(&transport, 200);
    assert!(
        sounding > 0,
        "the release tail rings out rather than cutting"
    );
    assert!(
        sounding < 200,
        "the graph never went quiet: it would now run forever"
    );
    assert_eq!(
        callback.block(&transport),
        0.0,
        "back to the idle path, running nothing"
    );
}

#[test]
fn one_note_on_is_delivered_to_one_block_and_not_re_read_by_the_next() {
    // The queue is drained once per callback, so an event is consumed by the
    // block it arrives in. An implementation that re-read the same events
    // every block would restart the note 375 times a second, which sounds
    // like a buzz rather than a note.
    let (mut callback, mut router, mut sink, transport) = rig();
    router.handle(&[NOTE_ON, 60, 100], sink.as_mut());

    callback.block(&transport);
    let first = callback.first_sample;
    callback.block(&transport);
    let second = callback.first_sample;
    callback.block(&transport);
    let third = callback.first_sample;

    // The source is a 100-sample cycle read at rate 1.0 against 128-frame
    // blocks, so a sustaining voice starts each block 28 samples further
    // through it. A voice restarted by a re-read event would begin every
    // block at phase 0 and produce the same first sample every time.
    assert_ne!(
        first, second,
        "the voice restarted from the top instead of continuing"
    );
    assert_ne!(second, third, "the voice restarted from the top");
}

#[test]
fn playing_along_with_the_song_sums_with_it_rather_than_replacing_it() {
    let (mut callback, mut router, mut sink, transport) = rig();
    transport.play();

    let sequenced = callback.block(&transport);
    assert!(
        sequenced > 0.01,
        "the demo phrase starts on the first block"
    );

    router.handle(&[NOTE_ON, 67, 127], sink.as_mut());
    let together = callback.block(&transport);
    assert!(
        together > sequenced,
        "the live note adds to the arrangement ({together} against {sequenced})"
    );
}

#[test]
fn a_note_the_player_is_holding_survives_the_song_stopping_underneath_it() {
    // Press stop with your hands still on the keys. Every DAW keeps those
    // notes sounding: stop is a statement about the sequencer, not about the
    // person playing. Cutting them leaves the player holding keys in silence
    // until they let go and press again — and the router still has those keys
    // marked down, so the note-off that eventually arrives matches a voice
    // that is already dead.
    let (mut callback, mut router, mut sink, transport) = rig();
    transport.play();
    callback.block(&transport);

    router.handle(&[NOTE_ON, 67, 127], sink.as_mut());
    assert!(callback.block(&transport) > 0.01, "the key is down");

    transport.stop();
    assert!(
        callback.block(&transport) > 0.01,
        "the key is still down, so it must still sound"
    );
    for block in 0..20 {
        assert!(
            callback.block(&transport) > 0.01,
            "still holding it at block {block} after the stop"
        );
    }
}

#[test]
fn a_note_the_player_is_holding_survives_a_seek() {
    // Same rule as stop, and the same reason: jumping the playhead says
    // nothing about the key somebody has their finger on. It is worth its own
    // test because a seek reaches the reset by a different route — a pending
    // request observed at the top of a block, rather than a state transition.
    let (mut callback, mut router, mut sink, transport) = rig();
    transport.play();
    router.handle(&[NOTE_ON, 67, 127], sink.as_mut());
    assert!(callback.block(&transport) > 0.01, "the key is down");

    transport.seek(96_000);
    for block in 0..10 {
        assert!(
            callback.block(&transport) > 0.01,
            "still held across the seek at block {block}"
        );
    }
}

#[test]
fn stopping_still_cuts_the_notes_the_song_was_playing() {
    // The other half, and the reason the stop resets anything at all: a
    // sequenced voice belongs to a moment in the song that is no longer where
    // the playhead is. Only the live ones are spared.
    let (mut callback, _router, _sink, transport) = rig();
    transport.play();
    for _ in 0..8 {
        callback.block(&transport);
    }
    assert!(callback.block(&transport) > 0.01, "the phrase is playing");

    transport.stop();
    let sounding = callback.blocks_until_silent(&transport, 50);
    assert_eq!(
        sounding, 0,
        "nothing the sequencer started may outlive the stop"
    );
}

#[test]
fn a_device_that_disappears_does_not_leave_its_note_sounding() {
    // TDD §14.2. The router is what remembers; this is the proof that what it
    // remembers reaches the audio.
    let (mut callback, mut router, mut sink, transport) = rig();
    router.handle(&[NOTE_ON, 60, 100], sink.as_mut());
    assert!(callback.block(&transport) > 0.01);

    // The cable comes out: `MidiHub` closes the connection and releases.
    router.release_all(sink.as_mut());
    let sounding = callback.blocks_until_silent(&transport, 200);
    assert!(
        sounding < 200,
        "the note outlived the device that was holding it"
    );
}

// ------------------------------------------------- the wheels (2026-09-06)

/// A block of the callback's own output, so a pitch can be counted.
impl Callback {
    /// The level **once the master has caught up**: one block rendered and
    /// thrown away, then the next one measured.
    ///
    /// The master limiter looks ahead two milliseconds — 96 samples of a
    /// 128-sample block — so the block a change lands in still carries most
    /// of a block of what came before it, and its *peak* is that. Measuring
    /// it directly says a wheel took a block longer than it did.
    fn settled(&mut self, transport: &Transport) -> f32 {
        self.block(transport);
        self.block(transport)
    }

    fn crossings(&mut self, transport: &Transport, blocks: usize) -> usize {
        let mut count = 0;
        for _ in 0..blocks {
            self.block(transport);
            let bus = &self.graph.buffer_pool.buffer_mut(0)[..BLOCK_SIZE];
            count += bus
                .windows(2)
                .filter(|pair| (pair[0] >= 0.0) != (pair[1] >= 0.0))
                .count();
        }
        count
    }
}

const BEND: u8 = 0xE0;
const CONTROLLER: u8 = 0xB0;
const PRESSURE: u8 = 0xD0;

/// The pitch wheel, from the bytes a keyboard sends to a note that is
/// already sounding **bending** — the half of the controller work that the
/// hosting pass left open (TDD §7.4). Two semitones up is the default range
/// and about a 12% higher pitch, which counts as more zero crossings.
#[test]
fn a_pitch_bend_from_the_keyboard_bends_a_sounding_note() {
    let (mut callback, mut router, mut sink, transport) = rig();
    router.handle(&[NOTE_ON, 60, 100], sink.as_mut());
    // **Long enough for the count to be a pitch**, which the first draft of
    // this test was not: over eight blocks a whole tone is worth one
    // crossing more than nothing at all, and the test passed before the
    // feature existed. Sixty-four blocks hold about 160 crossings, so two
    // semitones — 12% — is twenty of them.
    let unbent = callback.crossings(&transport, 64);
    assert!(unbent > 100, "the note is sounding: {unbent}");

    // Full up: fourteen bits, low seven first.
    router.handle(&[BEND, 0x7F, 0x7F], sink.as_mut());
    let bent = callback.crossings(&transport, 64);
    assert!(
        bent as f32 > unbent as f32 * 1.08,
        "two semitones up: unbent {unbent}, bent {bent}"
    );

    // Centred again — 8192, which is 0x00 0x40 — and the note goes back to
    // its own pitch rather than staying where it was pushed.
    router.handle(&[BEND, 0x00, 0x40], sink.as_mut());
    let centred = callback.crossings(&transport, 64);
    assert!(
        (centred as f32 / unbent as f32 - 1.0).abs() < 0.02,
        "{centred} vs {unbent}"
    );
}

/// The mod wheel reaches the **matrix**, which is where a patch decides
/// what it does — here, a route that opens the layer's gain. A controller
/// the matrix has no source for changes nothing rather than being invented
/// onto something.
#[test]
fn the_mod_wheel_reaches_a_patch_that_routes_it() {
    let (mut callback, mut router, mut sink, transport) =
        routing(fontelle_core::ModSource::ModWheel);

    router.handle(&[NOTE_ON, 60, 100], sink.as_mut());
    let at_rest = callback.settled(&transport);
    assert!(at_rest < 1e-3, "the route is shut: {at_rest}");

    router.handle(&[CONTROLLER, 1, 127], sink.as_mut());
    let wheel_up = callback.settled(&transport);
    assert!(wheel_up > 0.01, "the wheel opened it: {wheel_up}");

    router.handle(&[CONTROLLER, 1, 0], sink.as_mut());
    assert!(callback.settled(&transport) < 1e-3, "and shut it again");

    // A controller with no source of its own — CC 2 — is not put anywhere.
    router.handle(&[CONTROLLER, 2, 127], sink.as_mut());
    assert!(
        callback.settled(&transport) < 1e-3,
        "CC 2 means nothing here"
    );
}

#[test]
fn aftertouch_reaches_a_patch_that_routes_it() {
    let (mut callback, mut router, mut sink, transport) =
        routing(fontelle_core::ModSource::Aftertouch);

    router.handle(&[NOTE_ON, 60, 100], sink.as_mut());
    assert!(callback.settled(&transport) < 1e-3);
    router.handle(&[PRESSURE, 127], sink.as_mut());
    let leaning = callback.settled(&transport);
    assert!(leaning > 0.01, "aftertouch opened it: {leaning}");
    router.handle(&[PRESSURE, 0], sink.as_mut());
    assert!(callback.settled(&transport) < 1e-3);
}

/// A rig whose patch has **one** route, from `source` to the layer's gain.
///
/// One, deliberately: every route aimed at a destination is summed, so two
/// of these at full depth hold the layer 96 dB down between them however
/// far either is pushed — which is what the first draft of these tests did,
/// and it read as "the wheel does nothing".
fn routing(
    source: fontelle_core::ModSource,
) -> (Callback, MidiRouter, Box<dyn EventSink>, Transport) {
    let mut library = SampleLibrary::new();
    let mut patch = patch(&mut library);
    patch.mod_matrix = ModMatrix {
        routes: vec![route(source)],
    };
    rig_over(graph_playing(patch, library))
}

/// Shut at zero, open at full — see the same shape in
/// `fontelle-core/tests/performance.rs`.
fn route(source: fontelle_core::ModSource) -> fontelle_core::ModRoute {
    fontelle_core::ModRoute {
        source,
        destination: fontelle_core::ModDest::LayerGain(0),
        depth: -1.0,
        curve: fontelle_core::Curve::Linear,
        via: None,
        invert: true,
        bypass: false,
    }
}
