//! Publishing a new timeline to a stream that is already running.
//!
//! This is what "loop and edit while playing" in the first-usable gate rests
//! on: the model thread recompiles, hands the result over, and the RT thread
//! picks it up at a block boundary without a lock and without allocating.

use fontelle_engine::{BLOCK_SIZE, Transport, TransportReader, TransportState, timeline_channel};
use fontelle_types::{CompiledTimeline, EventPayload, TimedEvent};

fn note_at(sample: i64, key: u8) -> TimedEvent {
    TimedEvent {
        sample,
        target: Default::default(),
        payload: EventPayload::NoteOn {
            key,
            velocity: 100,
            voice_context: 0,
        },
    }
}

fn timeline(events: Vec<TimedEvent>) -> CompiledTimeline {
    let mut timeline = CompiledTimeline {
        events,
        index: Vec::new(),
    };
    timeline.events.sort_by_key(|e| e.sample);
    timeline
}

#[test]
fn the_reader_sees_the_timeline_it_was_given_until_a_new_one_is_published() {
    let (mut publisher, mut source) = timeline_channel(timeline(vec![note_at(0, 60)]));

    assert_eq!(source.current().events.len(), 1);
    assert!(matches!(
        source.current().events[0].payload,
        EventPayload::NoteOn { key: 60, .. }
    ));

    publisher.publish(timeline(vec![note_at(0, 60), note_at(4800, 64)]));
    // The RT side does not see it until it looks — which is the point: it
    // looks at a block boundary, never mid-block.
    assert_eq!(source.current().events.len(), 2);
}

#[test]
fn only_the_newest_timeline_survives_a_burst_of_edits() {
    let (mut publisher, mut source) = timeline_channel(CompiledTimeline::empty());
    // Dragging a note produces one of these per mouse-move. The RT thread must
    // never work through a backlog; it takes the latest and the rest are
    // dropped on the writer's thread.
    for count in 1..=50 {
        publisher.publish(timeline((0..count).map(|i| note_at(i * 480, 60)).collect()));
    }
    assert_eq!(source.current().events.len(), 50);
}

#[test]
fn the_source_says_whether_looking_would_find_anything_new() {
    let (mut publisher, mut source) = timeline_channel(CompiledTimeline::empty());
    assert!(!source.has_update());

    publisher.publish(timeline(vec![note_at(0, 60)]));
    assert!(source.has_update());

    source.current();
    // Having taken it, there is nothing new until the next publish. This is
    // what stops the RT side repositioning its event cursor every block.
    assert!(!source.has_update());
}

#[test]
fn a_published_timeline_is_dropped_on_the_thread_that_published_it() {
    // INVARIANT 1: deallocating on the RT thread is forbidden, and a
    // `CompiledTimeline` is a `Vec`. The triple buffer's writer overwrites the
    // slot it owns, so the old events are freed here, on the model thread —
    // never inside the callback. The RT side only ever swaps an index.
    //
    // Asserted structurally: `publish` takes the timeline by value and the
    // source hands out `&CompiledTimeline` only, so there is no path by which
    // the RT side could come to own one and drop it.
    let (mut publisher, mut source) = timeline_channel(CompiledTimeline::empty());
    let big = timeline((0..10_000).map(|i| note_at(i, 60)).collect());
    publisher.publish(big);
    assert_eq!(source.current().events.len(), 10_000);
    publisher.publish(CompiledTimeline::empty());
    assert_eq!(source.current().events.len(), 0);
}

#[test]
fn a_new_timeline_repositions_the_event_cursor_rather_than_replaying_the_old_one() {
    // The bug this exists to prevent: the reader's cursor is an index into a
    // `Vec` that no longer exists. Carried across a swap it either replays
    // events already played or skips ones that have not been.
    let first = timeline((0..8).map(|i| note_at(i * 4800, 60)).collect());
    let (mut publisher, mut source) = timeline_channel(first);

    let transport = Transport::new();
    transport.set_state(TransportState::Playing);
    let mut reader = TransportReader::new();

    // Play four blocks' worth so the cursor is somewhere in the middle.
    for _ in 0..4 {
        let step = reader.next_step(&transport, source.current(), BLOCK_SIZE, BLOCK_SIZE, false);
        let _ = step.events;
    }
    let played_to = reader.position();
    assert!(played_to > 0);

    // Now the user edits: a completely different timeline, half the length.
    publisher.publish(timeline((0..4).map(|i| note_at(i * 4800, 72)).collect()));
    let updated = source.current();
    reader.retarget(updated);

    // The very next block must deliver events from the *new* timeline at the
    // playhead, and nothing from before it.
    let step = reader.next_step(&transport, updated, BLOCK_SIZE, BLOCK_SIZE, false);
    for event in step.events {
        assert!(
            event.sample >= played_to,
            "replayed an event at {} with the playhead at {played_to}",
            event.sample
        );
    }
}
