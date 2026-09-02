//! The EQ's analyser, end to end: notes in a clip, through the graph, out as a
//! spectrum the window can draw.
//!
//! *"currently theres no eq monitor graph drawn to view the frequency spectrum
//! and make edits based off it and see in realtime."*
//!
//! The three pieces have tests of their own — the transform in
//! `fontelle-dsp/tests/spectrum.rs`, the audio thread's ring in
//! `fontelle-engine/tests/spectrum.rs`, the picture in
//! `fontelle-ui/tests/inserts.rs`. This is the one that would have caught the
//! thing none of them can: a chain that is wired up correctly at every joint
//! and connected to nothing at one end.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session, blank_project};
use fontelle_engine::{BLOCK_SIZE, Transport, TransportReader, graph_channel, timeline_channel};
use fontelle_model::{AddNotes, Command, Note};
use fontelle_types::{CompiledTimeline, EffectKind, PPQN};
use fontelle_ui::canvas::{SPECTRUM_BANDS, SPECTRUM_BOTTOM_DB, spectrum_band_hz};
use fontelle_ui::document::StudioHost;

use common::SR;

fn a_note(start: i64, length: i64, key: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity: 127,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
    }
}

/// A studio playing a held middle C on the built-in synth, with an EQ on the
/// master — and the RT ends of both channels, so a test can run blocks through
/// the graph exactly as the device callback does.
fn playing_studio() -> (
    Session,
    fontelle_engine::GraphSource,
    fontelle_engine::TimelineSource,
    usize,
    usize,
) {
    let mut project = blank_project(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    // Four bars of one note: long enough to fill the analyser's window several
    // times over.
    AddNotes::new(clip, vec![a_note(0, PPQN * 16, 60)])
        .apply(&mut project)
        .expect("the clip takes a note");

    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, timeline_source) = timeline_channel(CompiledTimeline::empty());
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let library = SampleLibrary::new();
    let realised =
        fontelle_app::realise(&project, &library, options).expect("a blank project must realise");
    let (graphs, _source) = graph_channel(fontelle_engine::CompiledGraph {
        schedule: Vec::new(),
        buffer_pool: fontelle_engine::BufferPool::with_capacity(2, BLOCK_SIZE),
    });
    let taps = realised.spectrum_taps.clone();
    let mut session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options,
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_spectrum_taps(taps);

    // The EQ goes on the master, which is where the analyser is wanted: it is
    // the mix you are shaping. Adding it rebuilds the graph, so the graph the
    // test runs is taken *after* this.
    let strip = session.mixer_strips().len() - 1;
    session.add_insert(strip, EffectKind::Eq);
    let slot = session.mixer_strips()[strip].inserts.len() - 1;

    // The graph the rebuild published, picked up the way the audio thread
    // picks one up.
    let mut graph_source = _source;
    assert!(
        graph_source.take_update(),
        "adding the EQ publishes a graph with the analyser's tap on it"
    );
    (session, graph_source, timeline_source, strip, slot)
}

/// Renders `blocks` blocks of the song, the way the device callback does.
fn play(
    graphs: &mut fontelle_engine::GraphSource,
    timelines: &mut fontelle_engine::TimelineSource,
    blocks: usize,
) {
    let transport = Transport::new();
    transport.play();
    let mut reader = TransportReader::new();
    for _ in 0..blocks {
        let timeline = timelines.current().clone();
        let step = reader.next_step(&transport, &timeline, BLOCK_SIZE, BLOCK_SIZE, false);
        if step.process {
            graphs
                .current()
                .process_block(step.events, step.snapshot, step.range.clone());
        }
    }
}

#[test]
fn a_playing_song_reaches_the_eqs_analyser() {
    let (mut session, mut graphs, mut timelines, strip, slot) = playing_studio();
    assert!(
        session.spectrum(strip, slot).is_empty(),
        "nothing has played yet, so there is nothing to draw"
    );

    play(&mut graphs, &mut timelines, 12);

    let bands = session.spectrum(strip, slot);
    assert_eq!(bands.len(), SPECTRUM_BANDS);
    let peak = bands.iter().cloned().fold(f32::MIN, f32::max);
    assert!(
        peak > SPECTRUM_BOTTOM_DB + 40.0,
        "a note at full velocity has to read well clear of the floor, got {peak} dB"
    );
}

/// And it reads it in the right place: middle C is 261.6 Hz, so the loudest
/// band is the one covering it or its neighbour.
///
/// *Or its neighbour*, and that is the transform's resolution rather than
/// slack in the test: a 2048-point window at 48 kHz is 23 Hz a bin, and
/// 261.6 Hz falls between two of them, so the note's energy is genuinely
/// shared by the pair. A tighter assertion would be asserting an accuracy the
/// picture does not have.
#[test]
fn the_loudest_band_is_the_note_that_is_playing() {
    let (mut session, mut graphs, mut timelines, strip, slot) = playing_studio();
    play(&mut graphs, &mut timelines, 12);

    let bands = session.spectrum(strip, slot);
    let loudest = bands
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(band, _)| band)
        .expect("a band is loudest");
    let (low, high) = spectrum_band_hz(loudest);
    let bin = fontelle_dsp::bin_width_hz(SR as f32);
    assert!(
        low - bin <= 261.63 && 261.63 <= high + bin,
        "middle C should be the loudest band or its neighbour; got the one \
         covering {low}..{high} Hz"
    );
}

/// An insert nobody has is not a spectrum, and asking for one is not an error:
/// the window asks every frame, and a slot that has gone must draw nothing
/// rather than panic.
#[test]
fn asking_about_an_insert_that_is_not_there_draws_nothing() {
    let (mut session, _graphs, _timelines, strip, _slot) = playing_studio();
    assert!(session.spectrum(strip, 9).is_empty());
    assert!(session.spectrum(99, 0).is_empty());
}
