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

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
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
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
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
            channel: None,
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

// ------------------------------------------- selecting, routing, building ---
//
// Reported from using the window, in one breath:
//
// > *"i'm able to make new mixer tracks only by selecting it from the dropdown
// > when changing a channel's routed track ... also i am not able to click on
// > any of these to select them right now and there's also no routing wiring
// > yet to route tracks to other tracks (tracks should all start just wiring
// > into master by default)."*
//
// Three things, and they are one thing: the mixer was a read-out of tracks
// made elsewhere. The panel's half is `fontelle-ui/tests/track_options.rs`;
// this is the document's.

#[test]
fn the_mixer_has_a_selection_and_it_starts_somewhere_real() {
    let session = session_with(2);
    let selected = session.selected_mixer_track();
    assert!(
        selected < session.mixer_strips().len(),
        "the selection points at a strip that is not there"
    );
}

#[test]
fn selecting_a_strip_sticks() {
    let mut session = session_with(3);
    for strip in 0..session.mixer_strips().len() {
        session.select_mixer_track(strip);
        assert_eq!(session.selected_mixer_track(), strip);
    }
}

#[test]
fn selecting_a_strip_that_is_not_there_leaves_the_selection_alone() {
    // A panel and a document disagree for a frame every time a track is
    // deleted, and a selection that followed the panel off the end would be an
    // index nothing else could use.
    let mut session = session_with(1);
    session.select_mixer_track(1);
    let kept = session.selected_mixer_track();
    session.select_mixer_track(99);
    assert_eq!(session.selected_mixer_track(), kept);
}

#[test]
fn the_selection_survives_the_track_list_shrinking() {
    let mut session = session_with(3);
    session.select_mixer_track(2);
    session.remove_mixer_track(2);
    assert!(
        session.selected_mixer_track() < session.mixer_strips().len(),
        "the selection was left pointing past the end of the mixer"
    );
}

#[test]
fn adding_a_track_from_the_mixer_selects_it() {
    // *"then the plus button moves to the next empty space so you can just add
    // as many new tracks as you want"* — and the one you just made is the one
    // you are about to put an effect on, so the options column follows it.
    let mut session = session_with(1);
    let before = session.mixer_strips().len();
    session.add_mixer_track();
    let strips = session.mixer_strips();
    assert_eq!(strips.len(), before + 1);
    assert_eq!(
        session.selected_mixer_track(),
        strips.len() - 2,
        "the new track is the last one before the master, and it is selected"
    );
    assert!(!strips[session.selected_mixer_track()].is_master);
}

// ------------------------------------------------------------- the routing ---

#[test]
fn every_track_starts_routed_to_the_master() {
    let session = session_with(3);
    for strip in 0..session.mixer_strips().len() - 1 {
        assert_eq!(
            session.track_output(strip),
            None,
            "track {strip} does not start on the master"
        );
    }
}

#[test]
fn a_track_can_be_routed_into_another_and_back() {
    let mut session = session_with(2);
    session.set_track_output(0, Some(1));
    assert_eq!(session.track_output(0), Some(1));

    session.set_track_output(0, None);
    assert_eq!(session.track_output(0), None, "back out to the master");
}

#[test]
fn routing_a_track_is_undoable() {
    let mut session = session_with(2);
    session.set_track_output(0, Some(1));
    session.undo();
    assert_eq!(
        session.track_output(0),
        None,
        "a routing decision is an edit like any other"
    );
    session.redo();
    assert_eq!(session.track_output(0), Some(1));
}

#[test]
fn a_routing_that_would_close_a_loop_is_refused_and_says_so() {
    // The command's own test is `fontelle-model/tests/routing.rs`. What is
    // here is that the refusal reaches the user: a menu row that silently does
    // nothing is worse than one that is not offered.
    let mut session = session_with(2);
    session.set_track_output(0, Some(1));
    session.take_message();

    session.set_track_output(1, Some(0));
    assert_eq!(
        session.track_output(1),
        None,
        "the loop was allowed into the document"
    );
    assert!(
        session.take_message().is_some(),
        "a refused routing said nothing at all"
    );
}

#[test]
fn a_route_reaches_the_running_graph() {
    // A routing that only changed the document would be a mix that sounds the
    // same until the file is reopened. Solo is what proves it arrived: a solo
    // silences *"the others that are not feeding it"*, so a bus carrying the
    // soloed track has to stay open — and whether it carries it is exactly the
    // routing this test just set.
    let mut session = session_with(2);
    session.set_track_output(0, Some(1));

    session.toggle_track_solo(0);
    let controls = session.track_controls();
    assert!(!controls[0].mute(), "the soloed track itself stays audible");
    assert!(
        !controls[1].mute(),
        "the bus the soloed track now feeds was muted, so nothing reaches the \
         master"
    );
    assert!(!controls[2].mute(), "and the master stays open");
}

#[test]
fn a_track_that_is_no_longer_in_the_path_is_still_silenced_by_a_solo() {
    // The other half of the sentence above, so the test before it is not
    // passing merely because everything stays open.
    let mut session = session_with(3);
    session.set_track_output(0, Some(1));
    session.toggle_track_solo(0);
    let controls = session.track_controls();
    assert!(
        controls[2].mute(),
        "track 2 feeds nothing that is soloed and should be out of the mix"
    );
}

// -------------------------------------------------------- the insert chain ---

#[test]
fn an_effect_can_be_dragged_up_and_down_its_chain() {
    // *"there's no place right now to actually edit the effect stack"* — and
    // order is most of what an effect stack *is*: a compressor before an EQ
    // and after it are two different sounds.
    let mut session = session_with(1);
    session.add_insert(0, fontelle_types::EffectKind::Eq);
    session.add_insert(0, fontelle_types::EffectKind::Compressor);
    let labels = |s: &Session| -> Vec<String> {
        s.mixer_strips()[0]
            .inserts
            .iter()
            .map(|i| i.label.clone())
            .collect()
    };
    let before = labels(&session);
    assert_eq!(before.len(), 2);

    session.move_insert(0, 1, 0);
    let after = labels(&session);
    assert_eq!(
        after,
        vec![before[1].clone(), before[0].clone()],
        "the chain did not reorder"
    );

    session.undo();
    assert_eq!(
        labels(&session),
        before,
        "reordering is an edit like any other"
    );
}

#[test]
fn moving_an_effect_nowhere_changes_nothing() {
    let mut session = session_with(1);
    session.add_insert(0, fontelle_types::EffectKind::Eq);
    session.add_insert(0, fontelle_types::EffectKind::Compressor);
    let before: Vec<String> = session.mixer_strips()[0]
        .inserts
        .iter()
        .map(|i| i.label.clone())
        .collect();

    session.move_insert(0, 1, 1);
    session.move_insert(0, 5, 0);
    session.move_insert(0, 0, 9);

    let after: Vec<String> = session.mixer_strips()[0]
        .inserts
        .iter()
        .map(|i| i.label.clone())
        .collect();
    assert_eq!(after, before, "an out-of-range move rearranged the chain");
}

#[test]
fn an_effect_can_be_taken_off_a_track_from_the_options_column() {
    let mut session = session_with(1);
    session.add_insert(0, fontelle_types::EffectKind::Eq);
    session.add_insert(0, fontelle_types::EffectKind::Compressor);
    assert_eq!(session.mixer_strips()[0].inserts.len(), 2);

    session.remove_insert(0, 0);
    assert_eq!(session.mixer_strips()[0].inserts.len(), 1);

    session.undo();
    assert_eq!(
        session.mixer_strips()[0].inserts.len(),
        2,
        "an effect deleted by accident has to come back with its settings"
    );
}

// -------------------------------------------------------------- the sends ---
//
// TDD §13.2, from the side the window drives. The document's half is
// `fontelle-model/tests/routing.rs` and the graph's is
// `fontelle-app/tests/sends.rs`; what is here is that the panel can reach
// them, and that a send level behaves like a fader — heard while it is moving,
// one undo entry when it stops.

fn a_send_rig() -> (Session, usize, usize) {
    // Two tracks and a master. Track 0 sends to track 1.
    let mut session = session_with(2);
    session.add_send(0, 1);
    (session, 0, 1)
}

#[test]
fn a_track_starts_with_no_sends_and_can_be_given_one() {
    let mut session = session_with(2);
    assert!(session.mixer_strips()[0].sends.is_empty());

    session.add_send(0, 1);
    let sends = session.mixer_strips()[0].sends.clone();
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].target, 1);
    assert_eq!(sends[0].target_name, session.route_names()[1]);
    assert!(
        sends[0].level_db <= -60.0,
        "a new send starts silent, so making one changes nothing"
    );
    assert!(!sends[0].pre_fader);
}

#[test]
fn a_send_is_undoable() {
    let mut session = session_with(2);
    session.add_send(0, 1);
    session.undo();
    assert!(session.mixer_strips()[0].sends.is_empty());
    session.redo();
    assert_eq!(session.mixer_strips()[0].sends.len(), 1);
}

#[test]
fn a_send_into_itself_is_refused_and_says_so() {
    let mut session = session_with(2);
    session.take_message();
    session.add_send(0, 0);
    assert!(session.mixer_strips()[0].sends.is_empty());
    assert!(
        session.take_message().is_some(),
        "a refused send said nothing at all"
    );
}

#[test]
fn a_send_level_reaches_the_running_graph_before_the_mouse_is_let_go() {
    // The same requirement a fader has, and the same reason: `realise`
    // deserialises every channel's patch, and a drag calls this sixty times a
    // second.
    let (mut session, from, _) = a_send_rig();
    let controls = session.send_controls();
    let live = controls
        .get(&(from, 0))
        .cloned()
        .expect("the send has a live control surface");

    session.set_send_level(from, 0, -6.0);
    assert!(
        (live.level_db() + 6.0).abs() < 1e-4,
        "the level did not reach the graph: {}",
        live.level_db()
    );
    assert!(
        std::sync::Arc::ptr_eq(&live, &session.send_controls()[&(from, 0)]),
        "a send level is a value, not a new graph"
    );
}

#[test]
fn a_whole_send_drag_is_one_undo_entry() {
    let (mut session, from, _) = a_send_rig();
    for step in 0..30 {
        session.set_send_level(from, 0, -30.0 + step as f32);
    }
    session.end_gesture();
    session.undo();
    assert!(
        session.mixer_strips()[from].sends[0].level_db <= -60.0,
        "one drag left more than one entry behind: {}",
        session.mixer_strips()[from].sends[0].level_db
    );
}

#[test]
fn a_send_can_be_flipped_pre_fader_and_back() {
    let (mut session, from, _) = a_send_rig();
    session.toggle_send_pre_fader(from, 0);
    assert!(session.mixer_strips()[from].sends[0].pre_fader);
    session.toggle_send_pre_fader(from, 0);
    assert!(!session.mixer_strips()[from].sends[0].pre_fader);

    session.undo();
    assert!(
        session.mixer_strips()[from].sends[0].pre_fader,
        "the tap point is an edit like any other"
    );
}

#[test]
fn a_send_can_be_taken_off_and_comes_back_where_it_was_set() {
    let (mut session, from, _) = a_send_rig();
    session.set_send_level(from, 0, -9.0);
    session.end_gesture();

    session.remove_send(from, 0);
    assert!(session.mixer_strips()[from].sends.is_empty());

    session.undo();
    let sends = session.mixer_strips()[from].sends.clone();
    assert_eq!(sends.len(), 1);
    assert!(
        (sends[0].level_db + 9.0).abs() < 1e-4,
        "a send deleted by accident came back wide open: {}",
        sends[0].level_db
    );
}

#[test]
fn an_out_of_range_send_edit_does_nothing_rather_than_complaining() {
    // A panel and a document disagree for a frame every time something is
    // deleted, and a stale index arriving from a click is ordinary.
    let (mut session, from, _) = a_send_rig();
    session.take_message();
    session.set_send_level(from, 9, 0.0);
    session.toggle_send_pre_fader(from, 9);
    session.remove_send(from, 9);
    session.remove_send(9, 0);
    assert_eq!(session.mixer_strips()[from].sends.len(), 1);
    assert!(
        session.take_message().is_none(),
        "a stale index is not something to tell the user about"
    );
}

#[test]
fn a_solo_silences_a_send_without_rebuilding_the_graph() {
    // A mute and a solo are switches somebody flicks *while listening*, so
    // they are pushed at the graph that is already playing rather than
    // rebuilt into a new one — and a send that kept arriving through a solo
    // would leave a reverb ringing from a part nobody can hear.
    // Three tracks, so there is one for the solo to exclude: 0 sends to 1, and
    // 2 is somewhere else entirely. Soloing the *master* would prove nothing —
    // every track feeds it, so a solo there leaves the whole mix audible,
    // which is right and is why this needs a third track.
    let mut session = session_with(3);
    let from = 0;
    session.add_send(from, 1);
    session.set_send_level(from, 0, 0.0);
    session.end_gesture();
    let live = session.send_controls()[&(from, 0)].clone();
    assert!(!live.mute());

    session.toggle_track_solo(2);
    assert!(
        live.mute(),
        "the send went on arriving from a track the solo had silenced"
    );
    assert!(
        std::sync::Arc::ptr_eq(&live, &session.send_controls()[&(from, 0)]),
        "a solo is a value, not a new graph"
    );

    session.toggle_track_solo(2);
    assert!(!live.mute(), "unsoloing opens the send back up");
}

#[test]
fn muting_a_track_silences_what_it_sends() {
    let (mut session, from, _) = a_send_rig();
    session.set_send_level(from, 0, 0.0);
    session.end_gesture();
    let live = session.send_controls()[&(from, 0)].clone();

    session.toggle_track_mute(from);
    assert!(
        live.mute(),
        "a muted track was still feeding its reverb bus"
    );
}
