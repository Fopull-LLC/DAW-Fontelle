//! A knob is heard **while it turns** (`docs/flopsynth-plan.md` §2.3).
//!
//! > *"if that's the recommended fix let's go for it"* — Ty, 2026-09-06
//!
//! Before this, `Session::set_instrument_param` on a patch parameter called
//! `store_patch`, which called `rebuild_graph` — a new graph, a new `Sampler`
//! and a fresh voice pool, on **every mouse move**. Tolerable for a
//! soundfont's release knob; unacceptable for a wavetable position swept by
//! hand under a held chord, where every move cut every sounding note.
//!
//! So a parameter edit now writes the document quietly and sends a
//! `ParamValue` onto the live wire, and only a *structural* edit rebuilds.
//! What this file measures is both halves: the note survives, and the sound
//! actually changes.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{
    BLOCK_SIZE, CompiledGraph, IdleGate, LiveEventSource, Transport, TransportReader,
    TransportState, graph_channel, live_event_channel, timeline_channel,
};
use fontelle_types::{CompiledTimeline, InstrumentKind, ParamAddress};
use fontelle_ui::document::StudioHost;

use common::SR;

/// The audio callback minus the sound card, as `tests/audition.rs` runs it —
/// the same shape `AudioDevice::start_output_stream` does, idle gate included,
/// because the gate is what decides whether a stopped transport keeps making
/// the sound somebody is holding.
struct Callback {
    reader: TransportReader,
    gate: IdleGate,
    source: LiveEventSource,
    graphs: fontelle_engine::GraphSource,
    timeline: CompiledTimeline,
    /// How many times a *new graph* has actually been swapped in — the
    /// observable half of "a knob drag does not rebuild".
    swaps: usize,
}

impl Callback {
    /// One block. Returns the left bus, so a caller can measure what it
    /// sounds like rather than only how loud it is.
    fn block(&mut self, transport: &Transport) -> Vec<f32> {
        let live = self.source.drain(
            self.reader.position(),
            transport.state() == TransportState::Recording,
        );
        let awake = self.gate.is_awake(live.len());
        if self.graphs.take_update() {
            self.swaps += 1;
        }
        let graph: &mut CompiledGraph = self.graphs.current();
        let step = self
            .reader
            .next_step(transport, &self.timeline, BLOCK_SIZE, BLOCK_SIZE, awake);
        if step.reset {
            graph.reset_sequenced();
        }
        if !step.process {
            self.gate.observe(0.0);
            return vec![0.0; BLOCK_SIZE];
        }
        graph.process_block_with_live(step.events, live, step.snapshot, step.range.clone());
        let out = graph.buffer_pool.buffer_mut(0)[..step.frames].to_vec();
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        self.gate.observe(peak);
        out
    }
}

fn rig() -> (Session, Callback, Transport) {
    let library = SampleLibrary::new();
    let patch = fontelle_core::flopsynth::flopsynth_init();
    let project = common::demo_with(&patch, &library);
    let clip = Session::first_clip(&project).expect("the demo project has a clip");
    let (realised, timeline) =
        common::realise_at(&project, &library, fontelle_app::PLAYBACK_QUALITY);

    let (timeline_publisher, _timeline_source) = timeline_channel(timeline.clone());
    let (graph_publisher, graph_source) = graph_channel(realised.graph);
    let (live_source, mut ports) = live_event_channel(4, 256);
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
    .with_voice_meters(realised.voice_meters)
    .with_scope_taps(realised.scope_taps)
    .with_param_nodes(realised.param_nodes)
    .with_audition(Box::new(port))
    .with_settings_path(std::env::temp_dir().join(format!(
        "fontelle-flopsynth-live-{}.json",
        std::process::id()
    )));

    (
        session,
        Callback {
            reader: TransportReader::new(),
            gate: IdleGate::new(),
            source: live_source,
            graphs: graph_source,
            timeline,
            swaps: 0,
        },
        Transport::new(),
    )
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// How bright a block is, as the mean absolute first difference.
///
/// Fine for comparing **one wave at two filter settings**, which is what the
/// cutoff sweep does. Useless for comparing two different waves: a sine spends
/// its whole cycle at a steep slope and a saw spends most of its cycle at a
/// gentle one, so a sine reads "brighter" than a saw by this measure and by no
/// other. See [`harmonic_energy`], which is what tells two waves apart.
fn brightness(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / samples.len() as f32
}

/// A Hann-windowed DFT at `hz`, in linear magnitude — what actually says
/// whether a partial is there.
fn energy_at(samples: &[f32], hz: f32) -> f32 {
    let n = samples.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n).cos();
        let phase = std::f32::consts::TAU * hz * i as f32 / SR as f32;
        re += sample * window * phase.cos();
        im -= sample * window * phase.sin();
    }
    (re * re + im * im).sqrt() / n
}

/// How much of a note is in its **overtones** rather than its fundamental —
/// the thing that actually distinguishes a saw from a sine.
fn harmonic_energy(samples: &[f32], f0: f32) -> f32 {
    let fundamental = energy_at(samples, f0).max(1e-9);
    let overtones: f32 = (2..=8).map(|h| energy_at(samples, f0 * h as f32)).sum();
    overtones / fundamental
}

/// The one the plan names. A note is held, the cutoff is moved through
/// `Session::set_instrument_param` **every block** for sixty-four blocks, and
/// the note has to still be sounding at the end of it — and the cutoff has to
/// have done something.
#[test]
fn a_knob_drag_does_not_cut_a_held_note() {
    let (mut session, mut callback, transport) = rig();
    // Something with harmonics to filter, and the filter part way down so
    // there is room to move in both directions.
    session.set_instrument_param(&ParamAddress::new("patch/filter[0]/cutoff"), 0.6);
    // The edits above are structural only in that they are the first; let the
    // graph settle before anything is measured.
    for _ in 0..4 {
        callback.block(&transport);
    }

    session.audition_on(60, 100, 0);
    let mut sounding = Vec::new();
    for _ in 0..8 {
        sounding.push(peak(&callback.block(&transport)));
    }
    assert!(
        sounding.iter().fold(0.0f32, |a, b| a.max(*b)) > 0.01,
        "the note has to be sounding before the drag starts: {sounding:?}"
    );

    let swaps_before = callback.swaps;
    let mut floors = 0usize;
    let mut early = Vec::new();
    let mut late = Vec::new();
    for step in 0..64 {
        // A hand sweeping the cutoff open, one move per block — which is
        // faster than a hand and exactly the rate that used to rebuild the
        // graph sixty-four times.
        //
        // From 630 Hz to 18 kHz. The cutoff's taper is logarithmic, so the
        // bottom quarter of the dial is under a middle C's own fundamental —
        // starting there would measure the filter doing its job rather than
        // the note surviving.
        let t = 0.5 + 0.45 * (step as f32 / 63.0);
        session.set_instrument_param(&ParamAddress::new("patch/filter[0]/cutoff"), t);
        let block = callback.block(&transport);
        if peak(&block) < 0.002 {
            floors += 1;
        }
        if step < 8 {
            early.push(brightness(&block));
        }
        if step >= 56 {
            late.push(brightness(&block));
        }
    }

    assert_eq!(
        floors, 0,
        "the note fell to the floor during the drag, which is what a rebuild \
         does: {floors} of 64 blocks were silent"
    );
    assert_eq!(
        callback.swaps,
        swaps_before,
        "a parameter edit must not publish a new graph; {} were published \
         across the drag",
        callback.swaps - swaps_before
    );

    let opened: f32 = late.iter().sum::<f32>() / late.len() as f32;
    let closed: f32 = early.iter().sum::<f32>() / early.len() as f32;
    assert!(
        opened > closed * 1.3,
        "the cutoff has to actually be heard moving: {closed} closed against \
         {opened} open"
    );
}

/// The other half of §2.3's rule: a **structural** edit rebuilds, because the
/// node's wavetable set is resolved in `prepare` and a table it was not built
/// for would render silence.
#[test]
fn choosing_a_table_rebuilds_and_a_knob_does_not() {
    let (mut session, mut callback, transport) = rig();
    for _ in 0..4 {
        callback.block(&transport);
    }

    let before = callback.swaps;
    session.set_instrument_param(&ParamAddress::new("patch/layer[0]/synth/position"), 0.7);
    callback.block(&transport);
    assert_eq!(
        callback.swaps, before,
        "a position knob is a parameter and must not rebuild"
    );

    session.set_instrument_param(&ParamAddress::new("patch/layer[0]/synth/table"), 0.4);
    callback.block(&transport);
    assert!(
        callback.swaps > before,
        "choosing a table has to rebuild, or the layer plays a table the node \
         never resolved"
    );
}

/// And the table it lands on is one you can hear: a rebuild that resolved the
/// wrong set, or none, would be silence rather than a different sound.
#[test]
fn a_table_chosen_through_the_panel_is_audible() {
    let (mut session, mut callback, transport) = rig();
    for _ in 0..4 {
        callback.block(&transport);
    }

    let sound_of = |session: &mut Session, callback: &mut Callback, table: f32| {
        session.set_instrument_param(&ParamAddress::new("patch/layer[0]/synth/table"), table);
        session.audition_on(60, 110, 0);
        let mut out = Vec::new();
        for _ in 0..16 {
            out.extend(callback.block(&transport));
        }
        session.audition_off(60);
        for _ in 0..24 {
            callback.block(&transport);
        }
        out
    };

    // Sine is the first table and Saw the third of forty.
    let sine = sound_of(&mut session, &mut callback, 0.0);
    let saw = sound_of(&mut session, &mut callback, 2.0 / 39.0);
    assert!(peak(&sine) > 0.01, "a sine has to sound: {}", peak(&sine));
    assert!(peak(&saw) > 0.01, "a saw has to sound: {}", peak(&saw));
    // Middle C. A saw has every harmonic; a sine has none.
    let f0 = 261.626;
    assert!(
        harmonic_energy(&saw, f0) > harmonic_energy(&sine, f0) * 5.0,
        "a saw has harmonics a sine does not: {} against {}",
        harmonic_energy(&saw, f0),
        harmonic_energy(&sine, f0)
    );
}

/// The channel is a Flopsynth channel end to end: the menu puts one on, it
/// arrives playing, and the roll sounds it with nothing configured — gate 1
/// of the plan's §1.
#[test]
fn a_flopsynth_channel_plays_a_note_with_nothing_configured() {
    let (mut session, mut callback, transport) = rig();
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    assert_eq!(session.channel_kind(0), Some(InstrumentKind::Flopsynth));
    for _ in 0..4 {
        callback.block(&transport);
    }
    session.audition_on(64, 100, 0);
    let mut loudest = 0.0f32;
    for _ in 0..16 {
        loudest = loudest.max(peak(&callback.block(&transport)));
    }
    assert!(
        loudest > 0.01,
        "a fresh Flopsynth channel has to make a sound; peaked at {loudest}"
    );
}

// ----------------------------------------------------- the voice meter ---

/// A synthesiser's window says how many voices are sounding
/// (`docs/flopsynth-plan.md` §11, phase 6).
///
/// Not decoration: polyphony is a number a person *sets*, and the only way to
/// know whether 16 is enough for what they are playing is to watch it. It is
/// also the first read-out in this program that comes off the **audio
/// thread's own state** rather than off the document — a count of live voices
/// is not a fact the document has.
#[test]
fn the_window_says_how_many_voices_are_sounding() {
    let (mut session, mut callback, transport) = rig();
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    for _ in 0..4 {
        callback.block(&transport);
    }
    assert_eq!(session.voice_count(), 0, "nothing is playing yet");

    // Four notes, rendered so the graph actually runs — the count comes back
    // off the node, which has to have had a block to fill.
    for key in [60, 64, 67, 71] {
        session.audition_on(key, 100, 0);
    }
    for _ in 0..4 {
        callback.block(&transport);
    }
    assert_eq!(session.voice_count(), 4, "four notes, four voices");

    for key in [60, 64, 67, 71] {
        session.audition_off(key);
    }
    // Long enough for the release to finish and the voices to be reclaimed.
    for _ in 0..400 {
        callback.block(&transport);
    }
    assert_eq!(session.voice_count(), 0, "the voices did not go");
}

/// And the window carries it, so the drawing has something to draw.
#[test]
fn the_flopsynth_view_carries_the_voice_count() {
    let (mut session, mut callback, transport) = rig();
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    for _ in 0..4 {
        callback.block(&transport);
    }
    session.audition_on(60, 100, 0);
    for _ in 0..4 {
        callback.block(&transport);
    }
    let view = session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Synth)
        .expect("a Flopsynth window");
    assert_eq!(view.voices, 1);
}

/// The sky through the canopy hears the instrument: once a note has sounded
/// through the graph, the host hands the window the instrument's bands and
/// waveform — **from the first graph**, not the first rebuild, which is how
/// it was first wired and how the sky stayed dark while a note played.
#[test]
fn the_window_hears_the_instrument_from_the_first_graph() {
    let (mut session, mut callback, transport) = rig();
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    for _ in 0..4 {
        callback.block(&transport);
    }
    // Before a note the graph may not have run at all — the callback's idle
    // gate skips silent blocks — so the answer is nothing or silence, and
    // either is honest.
    if let Some(quiet) = session.instrument_sound() {
        assert!(
            quiet.bands_db.iter().all(|db| *db <= -80.0),
            "silence reads as silence: {:?}",
            &quiet.bands_db[..8]
        );
    }
    session.audition_on(60, 100, 0);
    for _ in 0..8 {
        callback.block(&transport);
    }
    let heard = session
        .instrument_sound()
        .expect("the tap has a note in it");
    assert_eq!(heard.bands_db.len(), fontelle_ui::canvas::SPECTRUM_BANDS);
    let loudest = heard.bands_db.iter().cloned().fold(f32::MIN, f32::max);
    assert!(
        loudest > -40.0,
        "a note at full velocity is loud: {loudest} dB"
    );
    assert!(!heard.wave.is_empty());
    assert!(
        heard.wave.iter().any(|s| s.abs() > 0.01),
        "and the waveform is in it"
    );
}
