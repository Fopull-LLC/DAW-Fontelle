//! The mixer and the tempo, driven through the traits the window drives them
//! through.
//!
//! Both are the same shape and it is worth naming once: a control the user
//! *drags* has to move the sound while it is moving, and has to leave one undo
//! entry behind when it stops. Those two requirements pull in opposite
//! directions, because the sound comes from a `CompiledGraph` that costs a
//! patch deserialisation per channel to rebuild and the undo entry comes from
//! a `Command` against the document.
//!
//! The answer here is that a fader writes **both** — the command, for undo and
//! for the file, and a set of atomics the running graph reads, for the sound
//! between now and the next rebuild. This file is what pins that they cannot
//! drift apart.

mod common;

use fontelle_app::{RealiseOptions, SampleLibrary, Session, blank_project};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::{AddMixerTrack, Command};
use fontelle_types::CompiledTimeline;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

/// A session over a blank project: one channel, and the master, which is the
/// only mixer track a project has until somebody makes one.
fn session() -> Session {
    session_with(0)
}

/// The same, with `tracks` mixer tracks built deliberately on top of the
/// master — which is the only way a strip comes into existence now.
fn session_with(tracks: usize) -> Session {
    let mut project = blank_project(8, 120.0, SR);
    for n in 1..=tracks {
        AddMixerTrack::new(format!("Track {n}"))
            .apply(&mut project)
            .expect("a mixer track must be addable");
    }
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let library = SampleLibrary::new();
    let realised =
        fontelle_app::realise(&project, &library, options).expect("a blank project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    Session::new(
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
}

// ------------------------------------------------------------- the strips ---

#[test]
fn a_blank_project_shows_nothing_but_its_master() {
    // A channel no longer brings a strip with it. Twenty instruments used to
    // mean twenty strips nobody asked for; a mixer track is a destination
    // somebody builds — see `fontelle_model::Channel::mixer_track`.
    let session = session();
    let strips = session.mixer_strips();

    assert_eq!(strips.len(), 1, "just the master");
    assert!(strips[0].is_master);
    assert_eq!(
        session.channels().len(),
        1,
        "and the project does have a channel — it plays through the master"
    );
}

#[test]
fn a_track_made_deliberately_shows_up_before_the_master() {
    let mut session = session();
    session.add_mixer_track();

    let strips = session.mixer_strips();
    assert_eq!(strips.len(), 2);
    assert!(!strips[0].is_master, "the new track comes first");
    assert!(
        strips[1].is_master,
        "and the master is last, which is where the panel pins it"
    );
    assert_eq!(strips[0].gain_db, 0.0, "a new track is at unity");
    assert_eq!(strips[0].pan, 0.0, "and centred");
    assert!(!strips[0].name.is_empty(), "and it has a name to click");
}

// ---------------------------------------------------------- moving a fader ---

#[test]
fn setting_a_level_reaches_the_document_and_can_be_undone() {
    let mut session = session();

    session.set_track_gain_db(0, -6.0);
    assert_eq!(session.mixer_strips()[0].gain_db, -6.0);
    assert!(session.is_dirty(), "a level is part of the piece");

    session.end_gesture();
    session.undo();
    assert_eq!(
        session.mixer_strips()[0].gain_db,
        0.0,
        "undo puts the fader back where it was"
    );
}

#[test]
fn a_whole_drag_is_one_undo_entry_and_not_sixty() {
    // The reason `SetNumber::merge_with` exists. Without it, undoing a fader
    // move means pressing Ctrl+Z once per frame the mouse was down for.
    let mut session = session();
    for step in 1..=30 {
        session.set_track_gain_db(0, -(step as f32) / 5.0);
    }
    session.end_gesture();

    session.undo();
    assert_eq!(
        session.mixer_strips()[0].gain_db,
        0.0,
        "one undo reaches back past the whole gesture"
    );
}

#[test]
fn two_drags_with_the_mouse_lifted_between_them_are_two_entries() {
    let mut session = session();
    session.set_track_gain_db(0, -3.0);
    session.end_gesture();
    session.set_track_gain_db(0, -9.0);
    session.end_gesture();

    session.undo();
    assert_eq!(session.mixer_strips()[0].gain_db, -3.0);
    session.undo();
    assert_eq!(session.mixer_strips()[0].gain_db, 0.0);
}

#[test]
fn a_fader_move_is_heard_before_the_mouse_is_let_go() {
    // The point of the atomics. A document that is the source of truth
    // (INVARIANT 9) still has to be *audible* while somebody is moving a
    // control on it, and rebuilding the graph per frame would reload every
    // channel's patch sixty times a second.
    let mut session = session_with(1);
    let controls = session.track_controls();
    assert_eq!(controls.len(), 2, "the new track and the master");

    session.set_track_gain_db(0, -12.0);
    assert_eq!(
        controls[0].gain_db(),
        -12.0,
        "the running graph sees the new level immediately"
    );
    assert!(
        std::sync::Arc::ptr_eq(&controls[0], &session.track_controls()[0]),
        "and it is the same graph — a fader move does not rebuild one"
    );
}

#[test]
fn panning_a_track_moves_it_in_the_document_and_in_the_running_graph() {
    let mut session = session();
    let controls = session.track_controls();

    session.set_track_pan(0, -0.5);
    assert_eq!(session.mixer_strips()[0].pan, -0.5);
    assert_eq!(controls[0].pan(), -0.5);

    session.end_gesture();
    session.undo();
    assert_eq!(session.mixer_strips()[0].pan, 0.0);
    assert_eq!(controls[0].pan(), 0.0, "and the undo is heard too");
}

// ------------------------------------------------------------ mute and solo ---

#[test]
fn muting_a_track_from_the_mixer_silences_it_without_rebuilding_the_graph() {
    let mut session = session();
    let controls = session.track_controls();
    assert!(!controls[0].mute());

    session.toggle_track_mute(0);
    assert!(session.mixer_strips()[0].mute, "the document knows");
    assert!(controls[0].mute(), "and so does the graph that is playing");
    assert!(
        std::sync::Arc::ptr_eq(&controls[0], &session.track_controls()[0]),
        "a mute is a value, not a new graph — it used to reload every patch"
    );
}

#[test]
fn soloing_one_track_silences_the_others_that_are_not_feeding_it() {
    // Two tracks, so there is something for the solo to exclude.
    let mut session = session_with(2);
    let controls = session.track_controls();
    assert_eq!(controls.len(), 3, "two channel tracks and the master");

    session.toggle_track_solo(0);
    assert!(session.mixer_strips()[0].solo);
    assert!(!controls[0].mute(), "the soloed track itself stays audible");
    assert!(
        controls[1].mute(),
        "the other one is silenced by the solo, live"
    );
    assert!(
        !controls[2].mute(),
        "and the master stays open or nothing reaches the speakers"
    );
    assert!(session.mixer_strips()[2].is_master, "which is strip 2 here");

    session.toggle_track_solo(0);
    assert!(
        !controls[1].mute(),
        "unsoloing opens everything back up again"
    );
}

#[test]
fn the_racks_mute_and_the_mixers_mute_are_now_different_switches() {
    // They used to be one, because a channel owned a track. They cannot be
    // any more, and the reason is the whole point of the routing change: a
    // channel plays through the **master** by default, so a rack switch that
    // reached for the mixer track would silence the entire song.
    //
    // The rack's is the channel's, and it is a *sequencer* mute — the
    // compiler drops the channel's clips. The mixer's is the track's fader.
    let mut session = session();
    session.toggle_mute(0);

    assert!(
        session.channels()[0].muted,
        "the rack's switch is the channel's"
    );
    assert!(
        !session.mixer_strips()[0].mute,
        "and it leaves the master — which this channel plays through — alone"
    );
}

#[test]
fn a_muted_channel_is_dropped_by_the_compiler_rather_than_by_a_fader() {
    // What "a sequencer mute" means, checked rather than asserted: the
    // channel's notes stop reaching the timeline at all.
    let mut session = session();
    session.edit(fontelle_ui::canvas::RollEdit::Add {
        note: fontelle_model::Note {
            start: 0,
            length: fontelle_types::PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
    assert!(
        !session.compiled().events.is_empty(),
        "the note is on the timeline to start with"
    );

    session.toggle_mute(0);
    assert!(
        session.compiled().events.is_empty(),
        "and a muted channel puts nothing on it"
    );

    session.toggle_mute(0);
    assert!(!session.compiled().events.is_empty(), "and back again");
}

// ---------------------------------------------------------------- metering ---

#[test]
fn a_strips_meter_reads_nothing_while_nothing_is_playing() {
    let mut session = session();
    let peaks = session.mixer_peaks();
    assert_eq!(peaks.len(), session.mixer_strips().len(), "one per strip");
    assert!(
        peaks.iter().all(|[l, r]| *l == 0.0 && *r == 0.0),
        "a silent project meters silence"
    );
}

// ------------------------------------------------------------- the tempo ---

#[test]
fn the_tempo_is_readable_and_settable_and_undoable() {
    let mut session = session();
    assert_eq!(session.tempo(), 120.0, "what the project was built at");

    session.set_tempo(128.0);
    assert_eq!(session.tempo(), 128.0);
    assert!(session.is_dirty());

    session.end_gesture();
    session.undo();
    assert_eq!(session.tempo(), 120.0);
}

#[test]
fn changing_the_tempo_changes_how_long_a_tick_lasts() {
    // The proof that the tempo reached the `TempoMap` and not just a field:
    // everything that converts between ticks and samples goes through it, and
    // a note's audition length is the cheapest of them to ask.
    let mut session = session();
    let at_120 = session.seconds_per_tick();

    session.set_tempo(240.0);
    let at_240 = session.seconds_per_tick();
    assert!(
        (at_120 / at_240 - 2.0).abs() < 1e-3,
        "twice the tempo is half the tick: {at_120} against {at_240}"
    );
}

#[test]
fn a_whole_tempo_drag_is_one_undo_entry() {
    let mut session = session();
    for step in 0..40 {
        session.set_tempo(120.0 + step as f64 * 0.25);
    }
    session.end_gesture();
    session.undo();
    assert_eq!(session.tempo(), 120.0);
}

#[test]
fn the_time_signature_is_settable_and_the_grid_follows_it() {
    let mut session = session();
    assert_eq!(
        session.beats_per_bar(),
        4,
        "4/4 until somebody says otherwise"
    );

    session.set_beats_per_bar(3);
    assert_eq!(session.beats_per_bar(), 3);
    assert!(session.is_dirty());

    session.end_gesture();
    session.undo();
    assert_eq!(session.beats_per_bar(), 4);
}
