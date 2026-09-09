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
