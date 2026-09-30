//! The notes being recorded, shown while they are being recorded.
//!
//! > *"recording notes also doesnt show you the notes as youre recording them
//! > which would be nice and for it like audio to show you it making the clip
//! > as youre recording it so you can be sure it is indeed recording it."*
//!
//! An audio take draws as a band that grows with the playhead
//! (`TimelineChrome::recording`). A note take is the same idea in the roll: what
//! the capture has caught so far, as notes, with whatever is still held down
//! drawn out to the playhead — and none of it in the document, because the
//! clip is not written until the transport stops. This is the document's half:
//! that the session reads its own capture back as notes on demand, without
//! spending the take.

mod common;

use fontelle_app::Session;
use fontelle_engine::{LiveEventSource, live_capture_channel, live_event_channel};
use fontelle_types::{EventPayload, EventSink, NodeId, PPQN, TimedEvent};
use fontelle_ui::document::{NotePreview, StudioHost};

use common::SR;

/// A session wired to a live event source that is recording, and the source
/// to play into.
fn recording_session() -> (Session, LiveEventSource, Box<dyn EventSink>) {
    let project = common::a_project_with_a_clip(8, 120.0, SR);
    let (mut source, mut ports) = live_event_channel(1, 64);
    let (writer, reader) = live_capture_channel(1_024);
    source.arm_capture(writer);
    let port: Box<dyn EventSink> = Box::new(ports.claim().expect("a port"));
    let session = common::a_session_for(project).with_capture(reader);
    (session, source, port)
}

fn play(
    source: &mut LiveEventSource,
    port: &mut dyn EventSink,
    sample: i64,
    payload: EventPayload,
) {
    port.send(TimedEvent {
        sample: 0,
        target: NodeId::default(),
        payload,
    });
    // What the audio thread does once a block while recording: stamps the
    // event with the transport's position and mirrors it into the capture.
    source.drain(sample, true);
}

fn on(key: u8) -> EventPayload {
    EventPayload::NoteOn {
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        voice_context: 0,
    }
}

fn off(key: u8) -> EventPayload {
    EventPayload::NoteOff {
        key,
        voice_context: 0,
    }
}

/// One beat at 120, in samples.
const BEAT: i64 = SR as i64 / 2;

#[test]
fn a_played_note_shows_up_as_a_note_while_the_take_is_still_going() {
    let (mut session, mut source, mut port) = recording_session();
    play(&mut source, port.as_mut(), BEAT, on(60));
    play(&mut source, port.as_mut(), BEAT * 2, off(60));
    session.pump();

    let notes = session.recording_notes(BEAT * 3);
    assert_eq!(
        notes,
        vec![NotePreview {
            start: PPQN,
            length: PPQN,
            key: 60,
        }]
    );
}

#[test]
fn a_key_still_held_is_drawn_out_to_the_playhead_and_grows_with_it() {
    let (mut session, mut source, mut port) = recording_session();
    play(&mut source, port.as_mut(), BEAT * 2, on(64));
    session.pump();

    let now = session.recording_notes(BEAT * 3);
    assert_eq!(now.len(), 1);
    assert_eq!(now[0].start, PPQN * 2);
    assert_eq!(now[0].length, PPQN, "held for a beat so far");

    let later = session.recording_notes(BEAT * 4);
    assert_eq!(later[0].length, PPQN * 2, "still held, a beat later");
}

#[test]
fn reading_the_take_back_does_not_spend_it() {
    // Showing the notes must not be the thing that keeps them: the clip is
    // written when the transport stops, and it has to have all of them.
    let (mut session, mut source, mut port) = recording_session();
    play(&mut source, port.as_mut(), BEAT, on(60));
    play(&mut source, port.as_mut(), BEAT * 2, off(60));
    play(&mut source, port.as_mut(), BEAT * 2, on(67));
    session.pump();
    for _ in 0..3 {
        assert_eq!(session.recording_notes(BEAT * 3).len(), 2);
    }
    assert_eq!(session.keep_take(BEAT * 4), 2, "both notes were kept");
}

#[test]
fn a_session_with_no_capture_has_nothing_being_recorded() {
    let session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    assert!(session.recording_notes(BEAT * 3).is_empty());
}

#[test]
fn the_notes_are_in_the_open_clips_own_ticks() {
    // A clip that starts two beats into the song: a note played on the third
    // beat is one beat into *it*.
    let (mut session, mut source, mut port) = recording_session();
    let clip = Session::first_clip(session.project()).expect("a clip");
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Move {
        ids: vec![clip],
        tick_delta: PPQN * 2,
        lane_delta: 0,
    });
    play(&mut source, port.as_mut(), BEAT * 3, on(60));
    play(&mut source, port.as_mut(), BEAT * 4, off(60));
    session.pump();
    let notes = session.recording_notes(BEAT * 5);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].start, PPQN);
}

// ------------------------------------------------- where a take lands ---
//
// > *"Recording is so weird. One minute is deletes the notes after the other
// > it keeps them, along with that it won't let me record onto another
// > track"*
//
// A take was written into whichever clip happened to be open, counted from
// that clip's start. Played before the clip, its notes were dropped; played
// after it, they were kept where they could not sound; played with no clip
// open, they were refused — and every one of those said "recorded N notes".
// A take lands in the clip it was played over, and where there is none it
// is given one, on the row it was played beside.

fn session_capturing(
    project: fontelle_model::Project,
) -> (Session, LiveEventSource, Box<dyn EventSink>) {
    let (mut source, mut ports) = live_event_channel(1, 64);
    let (writer, reader) = live_capture_channel(1_024);
    source.arm_capture(writer);
    let port: Box<dyn EventSink> = Box::new(ports.claim().expect("a port"));
    let session = common::a_session_for(project).with_capture(reader);
    (session, source, port)
}

/// Every note clip in the project, as (start, length, lane index, the
/// starts of its notes).
fn note_clips(session: &Session) -> Vec<(i64, i64, usize, Vec<i64>)> {
    let project = session.project();
    let lanes = project.lane_ids();
    let mut clips: Vec<_> = project
        .clips
        .iter()
        .filter_map(|(id, clip)| {
            let source = project.clip_source(id)?;
            let fontelle_model::ClipSource::Notes(data) = source.as_ref() else {
                return None;
            };
            let mut starts: Vec<i64> = data.notes.values().map(|n| n.start).collect();
            starts.sort();
            let lane = lanes.iter().position(|l| *l == clip.lane)?;
            Some((clip.start, clip.length, lane, starts))
        })
        .collect();
    clips.sort();
    clips
}

const BAR: i64 = PPQN * 4;

#[test]
fn a_take_with_no_clip_open_gets_a_clip_of_its_own() {
    let (mut session, mut source, mut port) =
        session_capturing(fontelle_app::blank_project(8, 120.0, SR));
    // The second beat of the second bar.
    play(&mut source, port.as_mut(), BEAT * 5, on(60));
    play(&mut source, port.as_mut(), BEAT * 6, off(60));
    assert_eq!(session.keep_take(BEAT * 8), 1);
    assert_eq!(
        note_clips(&session),
        vec![(BAR, BAR, 0, vec![PPQN])],
        "a bar's clip where it was played, with the note a beat into it"
    );
    assert_eq!(session.take_message(), None, "nothing was refused");
}

#[test]
fn a_take_played_before_the_open_clip_is_kept_not_dropped() {
    let (mut session, mut source, mut port) =
        session_capturing(common::a_project_with_a_clip(1, 120.0, SR));
    let clip = Session::first_clip(session.project()).expect("a clip");
    session.arrange(fontelle_ui::canvas::ArrangeEdit::Move {
        ids: vec![clip],
        tick_delta: BAR * 2,
        lane_delta: 0,
    });
    play(&mut source, port.as_mut(), BEAT, on(60));
    play(&mut source, port.as_mut(), BEAT * 2, off(60));
    assert_eq!(session.keep_take(BEAT * 3), 1);
    assert_eq!(
        note_clips(&session),
        vec![(0, BAR, 0, vec![PPQN]), (BAR * 2, BAR, 0, vec![])],
        "its own clip in the first bar; the open one is left alone"
    );
}

#[test]
fn a_take_played_after_the_open_clip_ends_is_kept_where_it_sounds() {
    let (mut session, mut source, mut port) =
        session_capturing(common::a_project_with_a_clip(1, 120.0, SR));
    // The second beat of the *third* bar; the open clip is one bar long.
    play(&mut source, port.as_mut(), BEAT * 9, on(64));
    play(&mut source, port.as_mut(), BEAT * 10, off(64));
    assert_eq!(session.keep_take(BEAT * 12), 1);
    assert_eq!(
        note_clips(&session),
        vec![(0, BAR, 0, vec![]), (BAR * 2, BAR, 0, vec![PPQN])]
    );
}

#[test]
fn a_take_over_the_open_clip_goes_into_it() {
    let (mut session, mut source, mut port) =
        session_capturing(common::a_project_with_a_clip(2, 120.0, SR));
    play(&mut source, port.as_mut(), BEAT * 5, on(60));
    play(&mut source, port.as_mut(), BEAT * 6, off(60));
    assert_eq!(session.keep_take(BEAT * 7), 1);
    assert_eq!(note_clips(&session), vec![(0, BAR * 2, 0, vec![PPQN * 5])]);
}

#[test]
fn a_take_that_runs_past_its_clips_end_says_so() {
    // Starts inside the one-bar clip and goes on into the next: kept in the
    // clip — a clip never grows to fit — and the part past the end, which
    // will not sound, is *said*.
    let (mut session, mut source, mut port) =
        session_capturing(common::a_project_with_a_clip(1, 120.0, SR));
    play(&mut source, port.as_mut(), BEAT, on(60));
    play(&mut source, port.as_mut(), BEAT * 2, off(60));
    play(&mut source, port.as_mut(), BEAT * 5, on(62));
    play(&mut source, port.as_mut(), BEAT * 6, off(62));
    assert_eq!(session.keep_take(BEAT * 7), 2);
    let said = session
        .take_message()
        .expect("the silent note is mentioned");
    assert!(said.contains("past"), "{said}");
}

// ---------------------------------------- onto the selected lane (2026-09-30)
//
// Ty, `docs/ux-routing-and-learning-plan.md` §2: *"new clips, recordings and
// imports land on that lane"* — the lane selected by a click on its header.

#[test]
fn a_take_with_its_own_clip_goes_onto_the_selected_lane() {
    let (mut session, mut source, mut port) =
        session_capturing(fontelle_app::blank_project(8, 120.0, SR));
    session.select_lane(Some(3));
    play(&mut source, port.as_mut(), BEAT * 5, on(60));
    play(&mut source, port.as_mut(), BEAT * 6, off(60));
    assert_eq!(session.keep_take(BEAT * 8), 1);
    assert_eq!(note_clips(&session), vec![(BAR, BAR, 3, vec![PPQN])]);
}

#[test]
fn a_take_where_the_selected_lane_is_taken_gets_a_row_under_it() {
    use fontelle_ui::canvas::ArrangeEdit;
    let (mut session, mut source, mut port) =
        session_capturing(fontelle_app::blank_project(8, 120.0, SR));
    // Something already on the first row in bar five, and another clip open
    // elsewhere, so the take is a clip of its own.
    session.arrange(ArrangeEdit::Add {
        lane: 0,
        start: BAR * 4,
    });
    session.arrange(ArrangeEdit::Add { lane: 5, start: 0 });
    session.select_lane(Some(0));
    let rows = session.project().lanes.len();
    // `play` and `keep_take` count samples: the second beat of bar five.
    play(&mut source, port.as_mut(), BEAT * 17, on(60));
    play(&mut source, port.as_mut(), BEAT * 18, off(60));
    assert_eq!(session.keep_take(BEAT * 20), 1);
    assert_eq!(
        session.project().lanes.len(),
        rows + 1,
        "one row made: {:?}",
        note_clips(&session)
    );
    assert!(
        note_clips(&session)
            .iter()
            .any(|c| c.0 == BAR * 4 && c.2 == 1 && !c.3.is_empty()),
        "on the new row directly under the selected one: {:?}",
        note_clips(&session)
    );
}
