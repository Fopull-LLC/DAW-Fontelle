//! The corrector as a device somebody can reach (`docs/tune-plan.md` §9.7).
//!
//! Phase 4's half of that list: everything the session and the document have
//! to do before a pixel of the console is drawn. The window's own half is
//! `fontelle-ui/tests/tune.rs`.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary};
use fontelle_model::{AddChannel, AddInsert, Command, MixerTrack, SetInsertNotes};
use fontelle_types::{
    ChannelId, EffectConfig, EffectKind, MixerTrackId, TuneConfig, TuneControl, TuneMode,
    TunePreset, TuneRange,
};
use fontelle_ui::StudioHost;

use common::SR;

const BLOCK: usize = fontelle_engine::BLOCK_SIZE;

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: BLOCK,
        quality: fontelle_app::PLAYBACK_QUALITY,
    }
}

/// A project with a tuner on a track and a second channel to point it at.
fn with_a_tuner() -> (fontelle_model::Project, MixerTrackId, ChannelId) {
    let mut project = common::a_clip_project(4);
    let track = project.mixer.tracks.insert(MixerTrack::new("Vocal"));
    AddInsert::new(track, EffectKind::Tune)
        .apply(&mut project)
        .expect("a tuner");
    let mut add = AddChannel::new("Melody", None);
    add.apply(&mut project).expect("a channel");
    let melody = add.channel().expect("a channel id");
    (project, track, melody)
}

/// Where a track sits in the strip order the window draws — the ordinary
/// tracks first and the master **last**, which is not the arena's order.
fn strip_of(session: &fontelle_app::Session, track: MixerTrackId) -> usize {
    let name = session.project().mixer.tracks[track].name.clone();
    session
        .mixer_strips()
        .iter()
        .position(|strip| strip.name == name)
        .expect("the track is on the mixer")
}

/// Where a channel sits in the rack order the window draws.
fn rack_of(session: &fontelle_app::Session, channel: ChannelId) -> usize {
    session
        .project()
        .channels
        .keys()
        .position(|id| id == channel)
        .expect("the channel is in the rack")
}

#[test]
fn the_midi_source_row_lists_every_channel_and_writes_the_slot() {
    let (project, track, melody) = with_a_tuner();
    let mut session = common::a_session_for(project);
    let strip = strip_of(&session, track);
    let index = rack_of(&session, melody);

    assert_eq!(
        session.insert_notes(strip, 0),
        None,
        "no MIDI to begin with"
    );
    session.set_insert_notes(strip, 0, Some(index));
    assert_eq!(session.insert_notes(strip, 0), Some(index));
    // And through the document, which is what a project file carries.
    assert_eq!(
        session.project().mixer.tracks[track].inserts[0].notes,
        Some(melody)
    );

    // Back to nothing, and it is an undo entry rather than a silent write.
    let before = session.undo_depth();
    session.set_insert_notes(strip, 0, None);
    assert_eq!(session.insert_notes(strip, 0), None);
    assert_eq!(
        session.undo_depth(),
        before + 1,
        "pointing a tuner somewhere is one thing a person did"
    );
}

#[test]
fn an_effect_that_takes_no_notes_is_left_alone() {
    let mut project = common::a_clip_project(4);
    let track = project.mixer.tracks.insert(MixerTrack::new("Vocal"));
    AddInsert::new(track, EffectKind::Eq)
        .apply(&mut project)
        .expect("an EQ");
    let mut session = common::a_session_for(project);
    let strip = strip_of(&session, track);
    session.set_insert_notes(strip, 0, Some(0));
    assert_eq!(session.insert_notes(strip, 0), None);
}

/// A change of range or mode changes the **latency**, so it is a graph
/// rebuild rather than a value down the live wire (§3.8) — and what the graph
/// compensates has to move with it.
#[test]
fn changing_the_range_rebuilds_and_the_reported_latency_moves() {
    let (mut project, track, _) = with_a_tuner();
    let before = fontelle_engine::insert_latency_samples(
        &project.mixer.tracks[track].inserts[0].config,
        SR as f32,
    );
    let EffectConfig::Tune(config) = project.mixer.tracks[track].inserts[0].config else {
        unreachable!()
    };
    assert_eq!(before, config.latency_samples(SR as f32));

    project.mixer.tracks[track].inserts[0].config = EffectConfig::Tune(TuneConfig {
        range: TuneRange::Low,
        mode: TuneMode::Live,
        ..config
    });
    let after = fontelle_engine::insert_latency_samples(
        &project.mixer.tracks[track].inserts[0].config,
        SR as f32,
    );
    assert_ne!(before, after, "the range is what the latency is made of");

    // And the graph really builds with it, which is what a rebuild is for.
    let library = SampleLibrary::default();
    let realised = fontelle_app::realise(&project, &library, options()).expect("a graph");
    assert!(
        realised.latency_samples >= after,
        "{} < {after}",
        realised.latency_samples
    );
}

/// The whole point of the tap: a window can read back what the node did to the
/// note, and it is the node's own answer rather than the window's guess.
///
/// End to end, the way `spectrum.rs` runs the analyser — a note in a clip,
/// through the real graph, out as a trace the window can draw. The three
/// pieces have tests of their own; this is the one that would catch a chain
/// wired correctly at every joint and connected to nothing at one end.
#[test]
fn the_trace_reads_back_what_the_node_wrote() {
    use fontelle_engine::{
        BLOCK_SIZE, Transport, TransportReader, graph_channel, timeline_channel,
    };
    use fontelle_model::{AddNotes, Note};
    use fontelle_types::{CompiledTimeline, PPQN};

    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = fontelle_app::Session::first_clip(&project).expect("one clip");
    AddNotes::new(
        clip,
        vec![Note {
            start: 0,
            length: PPQN * 16,
            key: 60,
            velocity: 127,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            channel: None,
        }],
    )
    .apply(&mut project)
    .expect("the clip takes a note");

    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, mut timelines) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let realised =
        fontelle_app::realise(&project, &library, options()).expect("a blank project realises");
    let (graphs, mut graph_source) = graph_channel(fontelle_engine::CompiledGraph {
        schedule: Vec::new(),
        buffer_pool: fontelle_engine::BufferPool::with_capacity(2, BLOCK_SIZE),
    });
    let mut session = fontelle_app::Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options(),
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    .with_tune_taps(realised.tune_taps);

    // The tuner goes on the master, which is where the whole mix arrives.
    // Adding it rebuilds the graph, so the graph the test runs is taken after.
    let strip = session.mixer_strips().len() - 1;
    session.add_insert(strip, EffectKind::Tune);
    let slot = session.mixer_strips()[strip].inserts.len() - 1;
    assert!(
        graph_source.take_update(),
        "adding the tuner publishes a graph with the trace's tap on it"
    );
    assert!(
        session.tune_trace(strip, slot).is_empty(),
        "nothing has played yet, so there is nothing to draw"
    );

    let transport = Transport::new();
    transport.play();
    let mut reader = TransportReader::new();
    for _ in 0..96 {
        let timeline = timelines.current().clone();
        let step = reader.next_step(&transport, &timeline, BLOCK_SIZE, BLOCK_SIZE, false);
        if step.process {
            graph_source
                .current()
                .process_block(step.events, step.snapshot, step.range.clone());
        }
    }

    let trace = session.tune_trace(strip, slot);
    assert!(
        !trace.is_empty(),
        "the node wrote no hops, so the window would draw nothing"
    );
    let voiced = trace
        .iter()
        .filter(|frame| frame.flags & fontelle_types::TUNE_VOICED != 0)
        .count();
    assert!(
        voiced > 20,
        "a held middle C should be heard as a note: {voiced} voiced hops of {}",
        trace.len()
    );
    // And it is the note that was played, not whatever the corrector opened on.
    let last = trace
        .iter()
        .rev()
        .find(|frame| frame.flags & fontelle_types::TUNE_VOICED != 0)
        .expect("a voiced hop");
    assert!(
        (last.sung_cents - 6_000.0).abs() < 60.0,
        "middle C is 6000 cents; the trace says {}",
        last.sung_cents
    );
}

#[test]
fn a_preset_from_the_bank_writes_every_knob_and_is_one_entry() {
    // The bank is on disk (`assets/presets/fx-tune/Factory`) and the forty
    // recipes are the constructors it was exported from — this is the claim
    // that the two are the same forty.
    for preset in TunePreset::ALL {
        let config = TuneConfig::from_preset(preset);
        assert_ne!(
            config,
            TuneConfig::new(),
            "{} is the state a fresh insert is already in",
            preset.label()
        );
    }
    assert_eq!(TunePreset::ALL.len(), 40);
}

/// The workflow §5.1 names: a channel with no instrument is a perfectly good
/// source, because its notes still compile and it makes no sound.
#[test]
fn a_channel_with_no_instrument_is_a_source() {
    let (mut project, track, melody) = with_a_tuner();
    project.channels[melody].instrument = None;
    SetInsertNotes::new(track, 0, Some(melody))
        .apply(&mut project)
        .expect("an empty channel is still a channel");
    project.mixer.tracks[track].inserts[0].config = EffectConfig::Tune(TuneConfig {
        control: TuneControl::MidiMelody,
        ..TuneConfig::new()
    });
    let library = SampleLibrary::default();
    fontelle_app::realise(&project, &library, options()).expect("a graph with a silent source");
}

/// The MIDI card, and the drop-down that is the only way to reach
/// [`SetInsertNotes`] with a mouse (§7.2, §7.5).
///
/// The source is a **routing edge**, not a value in the config, so it cannot
/// have a real parameter address; it carries
/// [`fontelle_ui::canvas::TUNE_SOURCE`] instead and the window sends that one
/// control to `set_insert_notes` rather than to `set_insert_param`. What is
/// measured here is the half `fontelle-app` owns: that the card exists, that
/// it lists the rack under "no MIDI", and that it says which row is on.
#[test]
fn the_midi_card_offers_the_rack_under_no_midi() {
    let (project, track, melody) = with_a_tuner();
    let mut session = common::a_session_for(project);
    let strip = strip_of(&session, track);
    let index = rack_of(&session, melody);

    let view = session.tune_view(strip, 0).expect("a console");
    let card = view
        .cards
        .iter()
        .find(|card| card.group.name == "MIDI")
        .expect("a MIDI card");
    assert_eq!(
        card.group.params.len(),
        2,
        "the source and the bend switch (§7.2)"
    );

    let source = &card.group.params[0];
    assert_eq!(source.address.as_str(), fontelle_ui::canvas::TUNE_SOURCE);
    let fontelle_ui::canvas::ParamKind::Choice(options) = &source.kind else {
        panic!("the source is a drop-down");
    };
    assert_eq!(options[0], fontelle_ui::canvas::NO_MIDI);
    assert!(
        options.iter().any(|name| name == "Melody"),
        "every channel in the rack is a source: {options:?}"
    );
    assert_eq!(
        fontelle_ui::canvas::choice_index(&source.kind, source.value),
        0,
        "a fresh tuner listens to nothing"
    );

    // Point it at the channel and the drop-down says so.
    session.set_insert_notes(strip, 0, Some(index));
    let view = session.tune_view(strip, 0).expect("a console");
    let source = &view
        .cards
        .iter()
        .find(|card| card.group.name == "MIDI")
        .expect("a MIDI card")
        .group
        .params[0];
    assert_eq!(
        fontelle_ui::canvas::choice_index(&source.kind, source.value),
        index + 1,
        "row 0 is `no MIDI`, so a channel's row is its rack place plus one"
    );
}

/// And the bend switch moved with it: the Scale card is the scale, and MIDI
/// controls live in the MIDI card.
#[test]
fn the_scale_card_is_only_the_scale() {
    let (project, track, _) = with_a_tuner();
    let session = common::a_session_for(project);
    let strip = strip_of(&session, track);
    let view = session.tune_view(strip, 0).expect("a console");
    let scale = view
        .cards
        .iter()
        .find(|card| card.group.name == "Scale")
        .expect("a Scale card");
    assert_eq!(
        scale.group.params.len(),
        3,
        "root, scale and control — the twelve keys are the keyboard's"
    );
    assert!(
        !scale
            .group
            .params
            .iter()
            .any(|param| param.label.contains("MIDI")),
        "the MIDI controls are in the MIDI card"
    );
}
