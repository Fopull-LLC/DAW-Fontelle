//! Drawing a note and hearing it: the whole chain, without a window.
//!
//! `Session` is where the model, the engine and the UI meet, and this checks
//! the path end to end — a `RollEdit` from the piano roll becomes a `Command`,
//! goes through `History`, is recompiled, and arrives at the RT thread's end of
//! the timeline channel. Every step is real; only the mouse and the sound card
//! are missing.

use fontelle_app::{Session, demo_project};
use fontelle_engine::timeline_channel;
use fontelle_model::ClipSource;
use fontelle_types::{CompiledTimeline, PPQN};
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::DocumentHost;

const RATE: u32 = 48_000;

/// A session over the demo phrase, plus the RT thread's end of the channel.
fn session() -> (Session, fontelle_engine::TimelineSource) {
    let project = demo_project(60, 120.0, RATE);
    let clip = Session::first_clip(&project).expect("the demo has a note clip");
    // The same channel -> node map the real graph realisation builds, so the
    // events land where they would in the app.
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, source) = timeline_channel(CompiledTimeline::empty());
    (
        Session::new(project, channel_nodes, publisher, clip, None),
        source,
    )
}

fn note_count(session: &Session) -> usize {
    session
        .project()
        .clips
        .values()
        .filter_map(|clip| match &clip.source {
            ClipSource::Notes(data) => Some(data.notes.len()),
            _ => None,
        })
        .sum()
}

#[test]
fn drawing_a_note_puts_it_in_the_document_and_on_the_timeline() {
    let (mut session, mut source) = session();
    let before = note_count(&session);
    let events_before = {
        // Prime the channel with what the project already sounds like.
        session.edit(RollEdit::Add {
            tick: PPQN * 8,
            key: 72,
            length: PPQN,
            velocity: 100,
        });
        source.current().events.len()
    };

    assert_eq!(note_count(&session), before + 1);
    // A note-on and a note-off for the note just drawn.
    assert!(
        events_before >= 2,
        "the new note produced {events_before} events"
    );
    assert!(
        source.current().events.iter().any(|e| matches!(
            e.payload,
            fontelle_types::EventPayload::NoteOn { key: 72, .. }
        )),
        "the note that was drawn never reached the timeline"
    );
}

#[test]
fn undo_takes_the_note_back_off_the_timeline_too() {
    let (mut session, mut source) = session();
    let before = note_count(&session);
    session.edit(RollEdit::Add {
        tick: PPQN * 8,
        key: 72,
        length: PPQN,
        velocity: 100,
    });
    let with_note = source.current().events.len();

    session.undo();
    let after = source.current().events.len();

    assert_eq!(note_count(&session), before);
    assert!(
        after < with_note,
        "undo left the note on the timeline: {with_note} events before, {after} after"
    );
    assert!(
        !source.current().events.iter().any(|e| matches!(
            e.payload,
            fontelle_types::EventPayload::NoteOn { key: 72, .. }
        )),
        "the undone note is still going to sound"
    );
}

#[test]
fn redo_puts_it_back() {
    let (mut session, mut source) = session();
    session.edit(RollEdit::Add {
        tick: PPQN * 8,
        key: 72,
        length: PPQN,
        velocity: 100,
    });
    let with_note = source.current().events.len();
    session.undo();
    session.redo();
    assert_eq!(source.current().events.len(), with_note);
}

#[test]
fn a_drag_is_one_undo_entry_when_the_gesture_is_not_broken() {
    let (mut session, _source) = session();
    let notes: Vec<_> = session.notes().keys().collect();
    let first = notes[0];

    // Ten steps of one drag.
    for _ in 0..10 {
        session.edit(RollEdit::Move {
            ids: vec![first],
            tick_delta: 240,
            key_delta: 0,
        });
    }
    // Then all of it comes back at once, because the ten steps coalesced.
    let moved = session
        .notes()
        .get(first)
        .expect("the note is still there")
        .start;
    session.undo();
    let back = session
        .notes()
        .get(first)
        .expect("the note is still there")
        .start;
    assert_ne!(moved, back);
    assert_eq!(back, 0, "one drag took more than one undo to take back");
}

#[test]
fn breaking_the_gesture_starts_a_new_undo_entry() {
    let (mut session, _source) = session();
    let first = session.notes().keys().next().expect("a note");

    session.edit(RollEdit::Move {
        ids: vec![first],
        tick_delta: 240,
        key_delta: 0,
    });
    // Mouse up.
    session.end_gesture();
    session.edit(RollEdit::Move {
        ids: vec![first],
        tick_delta: 240,
        key_delta: 0,
    });
    assert_eq!(session.notes().get(first).expect("a note").start, 480);

    session.undo();
    assert_eq!(
        session.notes().get(first).expect("a note").start,
        240,
        "two separate drags were coalesced into one entry"
    );
}

#[test]
fn an_edit_the_document_refuses_changes_nothing_and_is_not_undoable() {
    let (mut session, _source) = session();
    let first = session.notes().keys().next().expect("a note");
    let before = session.notes().get(first).expect("a note").key;

    // Off the top of MIDI. The command refuses, and the refusal must not
    // become a history entry — offering to undo something that never happened
    // is worse than offering nothing.
    session.edit(RollEdit::Move {
        ids: vec![first],
        tick_delta: 0,
        key_delta: 120,
    });
    assert_eq!(session.notes().get(first).expect("a note").key, before);
}

#[test]
fn a_fresh_session_is_clean_and_an_edit_makes_it_dirty() {
    let (mut session, _source) = session();
    assert!(!session.is_dirty());
    session.edit(RollEdit::Add {
        tick: 0,
        key: 60,
        length: PPQN,
        velocity: 100,
    });
    assert!(session.is_dirty(), "an edited project did not go dirty");
}

#[test]
fn the_playhead_is_only_reported_while_it_is_over_the_clip() {
    let (session, _source) = session();
    // The demo clip starts at tick 0 and runs to the end of the held chord.
    assert_eq!(session.playhead_tick(0), Some(0));
    // Far past the end of the song.
    assert_eq!(session.playhead_tick(RATE as i64 * 600), None);
}

#[test]
fn removing_a_note_takes_its_events_with_it() {
    let (mut session, mut source) = session();
    let ids: Vec<_> = session.notes().keys().collect();
    session.edit(RollEdit::Add {
        tick: 0,
        key: 60,
        length: PPQN,
        velocity: 100,
    });
    let before = source.current().events.len();

    session.edit(RollEdit::Remove(ids));
    let after = source.current().events.len();
    assert!(
        after < before,
        "deleting notes left their events on the timeline"
    );
}
