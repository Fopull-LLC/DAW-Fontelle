//! Automation clips as the arrangement makes and plays them (TDD §12.4,
//! §6.3), and the two transport modes.
//!
//! The reports these answer, from using the window:
//!
//! - *"it creates a new automation clip in my arrangement just flat on the
//!   value that its currently at basically with the clip extending the
//!   current length of the song or time selection"*
//! - *"even the tempo section for example which i currently cannot turn into
//!   an automation clip"*
//! - *"time looping selections... right click and drag on the time bar"*
//! - *"switch from a song mode to a clip mode that... will only play solo
//!   that clips content"*
//!
//! The canvas half of each is in `fontelle-ui`'s tests; this is the document
//! and the transport, driven through the same `StudioHost` the window uses.

mod common;

use std::sync::Arc;

use fontelle_app::{RealiseOptions, SampleLibrary, Session};
use fontelle_engine::{BLOCK_SIZE, Transport, graph_channel, timeline_channel};
use fontelle_model::{AddNotes, Command, Note};
use fontelle_types::{CompiledTimeline, EventPayload, PPQN, ParamTarget, Tick};
use fontelle_ui::canvas::ArrangeEdit;
use fontelle_ui::document::{ClipKind, DocumentHost, PlayMode, StudioHost};

use common::SR;

const BAR: Tick = PPQN * 4;

fn a_note(start: Tick, key: u8) -> Note {
    Note {
        start,
        length: PPQN,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

/// A studio over the built-in synth with one note in its first clip, and the
/// transport the window would drive.
fn studio() -> (Session, Arc<Transport>) {
    let mut project = common::a_project_with_a_clip(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    AddNotes::new(clip, vec![a_note(0, 60)])
        .apply(&mut project)
        .expect("the clip takes a note");

    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timelines) = timeline_channel(CompiledTimeline::empty());
    let options = RealiseOptions {
        sample_rate: SR,
        block_size: BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    };
    let library = SampleLibrary::new();
    let realised =
        fontelle_app::realise(&project, &library, options).expect("a blank project must realise");
    let (graphs, _source) = graph_channel(realised.graph);
    let transport = Arc::new(Transport::new());
    let session = Session::new(
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
    .with_transport(Arc::clone(&transport));
    (session, transport)
}

fn master_gain(session: &Session) -> fontelle_types::ParamAddress {
    let master = session
        .mixer_track_id(session.mixer_strips().len() - 1)
        .expect("the last strip is the master");
    ParamTarget::TrackGain(master).address()
}

fn automation_clips(session: &Session) -> Vec<fontelle_ui::document::ClipInfo> {
    session
        .clips()
        .into_iter()
        .filter(|clip| clip.kind == ClipKind::Automation)
        .collect()
}

// ------------------------------------------------------ making a clip ---

#[test]
fn a_new_automation_clip_spans_the_song_and_lies_flat_at_the_value() {
    let (mut session, _) = studio();
    let address = master_gain(&session);
    session.set_track_gain_db(session.mixer_strips().len() - 1, -12.0);
    session.end_gesture();

    session.create_automation(&address, "Master \u{2014} gain", BAR * 3);
    let clips = automation_clips(&session);
    assert_eq!(clips.len(), 1);
    let clip = &clips[0];
    assert_eq!(
        clip.start, 0,
        "from the front of the song, wherever the playhead was"
    );
    assert_eq!(clip.length, session.song_length(), "to the end of it");
    assert_eq!(clip.curve.len(), 2, "a flat segment");
    assert_eq!(clip.curve[0].tick, 0);
    assert_eq!(clip.curve[1].tick, clip.length);
    // -12 dB on a -60..+6 fader.
    let expected = (-12.0f64 + 60.0) / 66.0;
    for point in &clip.curve {
        assert!(
            (point.value - expected).abs() < 0.01,
            "flat at the fader's value: {} against {expected}",
            point.value
        );
    }
    assert!(clip.open, "and it is the clip the arrangement highlights");
}

#[test]
fn a_new_automation_clip_spans_the_time_selection_when_there_is_one() {
    let (mut session, _) = studio();
    let address = master_gain(&session);
    session.set_loop_range(Some((BAR, BAR * 3)));

    session.create_automation(&address, "Master \u{2014} gain", 0);
    let clips = automation_clips(&session);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].start, BAR);
    assert_eq!(clips[0].length, BAR * 2);
}

#[test]
fn a_second_request_for_the_same_control_reuses_its_clip() {
    let (mut session, _) = studio();
    let address = master_gain(&session);
    session.create_automation(&address, "Master \u{2014} gain", 0);
    session.create_automation(&address, "Master \u{2014} gain", BAR);
    assert_eq!(automation_clips(&session).len(), 1);
}

// ------------------------------------------------------- the tempo ---

#[test]
fn the_tempo_can_become_a_clip_and_it_starts_at_the_boxs_tempo() {
    let (mut session, _) = studio();
    session.set_tempo(96.0);
    session.end_gesture();
    let tempo = ParamTarget::Tempo.address();

    session.create_automation(&tempo, "Tempo", 0);
    let clips = automation_clips(&session);
    assert_eq!(clips.len(), 1, "the tempo box makes a lane like any knob");
    let expected = fontelle_types::normalised_tempo(96.0);
    assert!((clips[0].curve[0].value - expected).abs() < 1e-6);
    assert!(session.is_automated(&tempo));
    // And the song still plays at 96: a flat lane changes nothing.
    let table = session.compiled().tempo;
    assert!(
        table.iter().all(|span| (span.bpm - 96.0).abs() < 1e-3),
        "{table:?}"
    );
}

#[test]
fn drawing_on_the_tempo_lane_bends_the_song() {
    let (mut session, _) = studio();
    let tempo = ParamTarget::Tempo.address();
    session.create_automation(&tempo, "Tempo", 0);
    let clip = automation_clips(&session)[0].clone();
    let last = clip.curve[1].id;

    // Pull the end of the lane up to the top: a ramp to the fastest tempo.
    session.arrange(ArrangeEdit::MovePoints {
        clip: clip.id,
        ids: vec![last],
        tick_delta: 0,
        value_delta: 1.0,
    });
    session.end_gesture();

    let table = session.compiled().tempo;
    assert!(table.len() > 4, "a ramp is many segments: {table:?}");
    let (first, last_bpm) = (table[0].bpm, table[table.len() - 1].bpm);
    assert!(
        (first - 120.0).abs() < 1e-3,
        "starts where the box is: {first}"
    );
    assert!(
        (last_bpm - fontelle_types::TEMPO_MAX_BPM as f32).abs() < 1.0,
        "ends at the top of the lane: {last_bpm}"
    );
    // The window's conversions follow the lane too: bar 8 arrives sooner
    // than it would at a flat 120.
    let straight = fontelle_model::TempoMap::new(120.0, SR as f64).tick_to_sample(BAR * 8);
    assert!(session.sample_of_song_tick(BAR * 8) < straight);
}

// ------------------------------------------------- the time selection ---

#[test]
fn the_time_selection_reaches_the_transport_in_ticks_and_samples() {
    let (mut session, transport) = studio();
    assert_eq!(session.loop_range(), None);
    assert!(!transport.is_looping());

    session.set_loop_range(Some((BAR, BAR * 3)));
    assert_eq!(session.loop_range(), Some((BAR, BAR * 3)));
    assert_eq!(transport.loop_range_tick(), (BAR, BAR * 3));
    assert_eq!(
        transport.loop_range_sample(),
        (
            session.sample_of_song_tick(BAR),
            session.sample_of_song_tick(BAR * 3)
        )
    );
    assert!(transport.is_looping(), "a selection is something you loop");

    session.set_loop_range(None);
    assert_eq!(session.loop_range(), None);
    assert!(!transport.is_looping(), "and clearing it stops the loop");
}

#[test]
fn the_selection_is_saved_with_the_song_and_undoable() {
    let (mut session, _) = studio();
    session.set_loop_range(Some((0, BAR)));
    assert!(session.is_dirty());
    session.undo();
    assert_eq!(session.loop_range(), None);
    session.redo();
    assert_eq!(session.loop_range(), Some((0, BAR)));
}

#[test]
fn a_tempo_change_moves_the_loops_samples_but_not_its_bars() {
    let (mut session, transport) = studio();
    session.set_loop_range(Some((BAR, BAR * 2)));
    let before = transport.loop_range_sample();
    session.set_tempo(60.0);
    session.end_gesture();
    assert_eq!(transport.loop_range_tick(), (BAR, BAR * 2));
    let after = transport.loop_range_sample();
    assert_eq!(after.0, before.0 * 2, "half the tempo, twice the samples");
    assert_eq!(after.1, before.1 * 2);
}

// -------------------------------------------------- song and clip mode ---

fn notes_on(timeline: &CompiledTimeline) -> Vec<u8> {
    timeline
        .events
        .iter()
        .filter_map(|e| match e.payload {
            EventPayload::NoteOn { key, .. } => Some(key),
            _ => None,
        })
        .collect()
}

#[test]
fn clip_mode_plays_only_the_clip_being_edited_round_and_round() {
    let (mut session, transport) = studio();
    // A second channel with a clip and a note of its own, so there is
    // something to leave out. Drawn, because a channel no longer comes with
    // a clip (a clip holds several instruments now — see
    // `multi_instrument_clips.rs`).
    session.add_channel().expect("a channel can be added");
    let made = session.arrange(fontelle_ui::canvas::ArrangeEdit::Add {
        lane: 0,
        start: BAR * 8,
    });
    let second = session
        .clips()
        .into_iter()
        .find(|clip| clip.id == made.clips[0])
        .expect("the drawn clip");
    session.open_clip(second.id);
    session.edit(fontelle_ui::canvas::RollEdit::Add {
        note: a_note(0, 72),
    });
    session.end_gesture();
    assert_eq!(session.play_mode(), PlayMode::Song);
    let mut whole = notes_on(&session.compiled());
    whole.sort();
    assert_eq!(whole, vec![60, 72], "song mode plays everything");

    session.set_play_mode(PlayMode::Clip);
    assert_eq!(session.play_mode(), PlayMode::Clip);
    assert_eq!(
        notes_on(&session.compiled()),
        vec![72],
        "clip mode plays the open clip alone"
    );
    assert!(transport.is_looping(), "and loops it");
    assert_eq!(
        transport.loop_range_tick(),
        (second.start, second.start + second.length),
        "over the clip's own bars"
    );
    assert_eq!(
        session.focused_clip_span(),
        Some((second.start, second.start + second.length))
    );

    session.set_play_mode(PlayMode::Song);
    let mut whole = notes_on(&session.compiled());
    whole.sort();
    assert_eq!(whole, vec![60, 72]);
    assert!(
        !transport.is_looping(),
        "back to the song, and no selection to loop"
    );
}

#[test]
fn leaving_clip_mode_restores_the_time_selection() {
    let (mut session, transport) = studio();
    session.set_loop_range(Some((BAR, BAR * 2)));
    session.set_play_mode(PlayMode::Clip);
    assert_eq!(
        transport.loop_range_tick(),
        (0, BAR * 8),
        "the clip's bars while in clip mode"
    );
    session.set_play_mode(PlayMode::Song);
    assert_eq!(transport.loop_range_tick(), (BAR, BAR * 2));
    assert!(transport.is_looping());
}

#[test]
fn opening_another_clip_in_clip_mode_follows_it() {
    let (mut session, transport) = studio();
    session.add_channel().expect("a channel can be added");
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Add {
        lane: 0,
        start: BAR * 8,
    });
    let clips = session.clips();
    session.set_play_mode(PlayMode::Clip);
    session.open_clip(clips[1].id);
    assert_eq!(
        transport.loop_range_tick(),
        (clips[1].start, clips[1].start + clips[1].length)
    );
}

// ------------------------------------------------ editing points inline ---

#[test]
fn point_edits_are_arrangement_edits_and_hand_back_what_they_made() {
    let (mut session, _) = studio();
    let address = master_gain(&session);
    session.create_automation(&address, "Master \u{2014} gain", 0);
    let clip = automation_clips(&session)[0].clone();

    let made = session.arrange(ArrangeEdit::AddPoint {
        clip: clip.id,
        tick: BAR,
        value: 0.25,
    });
    assert!(made.clips.is_empty());
    assert_eq!(
        made.points.len(),
        1,
        "the id of the point, for the drag that follows"
    );
    let id = made.points[0];
    let after = automation_clips(&session)[0].clone();
    assert_eq!(after.curve.len(), 3);
    let point = after
        .curve
        .iter()
        .find(|p| p.id == id)
        .expect("the new point is on the curve");
    assert_eq!(point.tick, BAR);
    assert!((point.value - 0.25).abs() < 1e-9);
    assert!(
        after.curve.windows(2).all(|w| w[0].tick <= w[1].tick),
        "the curve is handed over in time order"
    );

    session.arrange(ArrangeEdit::MovePoints {
        clip: clip.id,
        ids: vec![id],
        tick_delta: PPQN,
        value_delta: 0.5,
    });
    session.end_gesture();
    let moved = *automation_clips(&session)[0]
        .curve
        .iter()
        .find(|p| p.id == id)
        .unwrap();
    assert_eq!(moved.tick, BAR + PPQN);
    assert!((moved.value - 0.75).abs() < 1e-9);

    session.arrange(ArrangeEdit::SetPointCurve {
        clip: clip.id,
        ids: vec![id],
        curve: fontelle_model::CurveShape::SCurve,
    });
    let shaped = *automation_clips(&session)[0]
        .curve
        .iter()
        .find(|p| p.id == id)
        .unwrap();
    assert_eq!(shaped.curve, fontelle_model::CurveShape::SCurve);

    session.arrange(ArrangeEdit::RemovePoints {
        clip: clip.id,
        ids: vec![id],
    });
    assert_eq!(automation_clips(&session)[0].curve.len(), 2);
    session.undo();
    assert_eq!(
        automation_clips(&session)[0].curve.len(),
        3,
        "an undo puts it back"
    );
}

#[test]
fn a_dragged_point_is_one_undo_entry() {
    let (mut session, _) = studio();
    let address = master_gain(&session);
    session.create_automation(&address, "Master \u{2014} gain", 0);
    let clip = automation_clips(&session)[0].clone();
    let id = clip.curve[0].id;
    for _ in 0..10 {
        session.arrange(ArrangeEdit::MovePoints {
            clip: clip.id,
            ids: vec![id],
            tick_delta: 0,
            value_delta: -0.02,
        });
    }
    session.end_gesture();
    session.undo();
    let back = automation_clips(&session)[0]
        .curve
        .iter()
        .find(|p| p.id == id)
        .unwrap()
        .value;
    assert!(
        (back - clip.curve[0].value).abs() < 1e-9,
        "one undo takes the whole drag back, got {back}"
    );
}

#[test]
fn the_windows_clock_agrees_with_the_timeline_it_compiled() {
    // The playhead is drawn by asking the document what tick a sample is, and
    // the note it should be sitting on was placed by the compiler. If those
    // two run off different tempo maps the playhead is in the wrong bar — and
    // clip mode compiles without the tempo lane (a lane on another row is not
    // part of the clip being soloed), so the window has to leave it out too.
    let (mut session, _) = studio();
    let tempo = ParamTarget::Tempo.address();
    session.create_automation(&tempo, "Tempo", 0);
    let clip = automation_clips(&session)[0].clone();
    session.arrange(ArrangeEdit::MovePoints {
        clip: clip.id,
        ids: vec![clip.curve[1].id],
        tick_delta: 0,
        value_delta: 1.0,
    });
    session.end_gesture();

    for mode in [PlayMode::Song, PlayMode::Clip] {
        session.set_play_mode(mode);
        let first_note = session
            .compiled()
            .events
            .iter()
            .find(|e| matches!(e.payload, EventPayload::NoteOn { .. }))
            .map(|e| e.sample);
        // The note is at the very start, so this is a weak claim there; the
        // sharp one is a tick well into the ramp.
        assert_eq!(first_note, Some(session.sample_of_song_tick(0)), "{mode:?}");
        let deep = BAR * 6;
        assert_eq!(
            session.playhead_song_tick(session.sample_of_song_tick(deep)),
            deep,
            "{mode:?}: the window's clock does not round-trip"
        );
    }

    // And the two modes genuinely differ, or the assertions above would hold
    // for the wrong reason.
    session.set_play_mode(PlayMode::Song);
    let bent = session.sample_of_song_tick(BAR * 6);
    session.set_play_mode(PlayMode::Clip);
    let plain = session.sample_of_song_tick(BAR * 6);
    assert!(
        bent < plain,
        "the tempo lane speeds the song up, so song mode gets there sooner: \
         {bent} against {plain}"
    );
}
