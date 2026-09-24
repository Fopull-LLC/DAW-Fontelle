//! The count-in: the playhead stands still on the marker while the click
//! counts, and then the song rolls **from there**.
//!
//! > *"when recording audio the count in isnt always the same like it needs
//! > to instead of playing 4 bars before or whatever just put the playhead on
//! > the same spot frozen and count in, then play it from there instead of
//! > trying to do some weird calculations because thats causing it so when
//! > you are recording past the first section that all of your recordings
//! > will be offset by like a bar."*
//!
//! It used to be a pre-roll: the window moved the **marker** a bar back and
//! started the tape when the playhead crossed where it had been. That moved
//! the marker for good (the next take counted in from a bar earlier, and
//! landed a bar early), clamped at the song's start (a take from bar 1 had no
//! count-in at all), and started the tape at whatever frame the window
//! happened to look. Now the transport counts itself:
//! `Transport::set_count_in(frames)`, and the reader holds the playhead for
//! exactly that many frames — running the graph, so monitoring and the click
//! are heard, but playing none of the song — and then rolls.

use std::sync::Arc;

use fontelle_engine::{
    AudioNode, BLOCK_SIZE, Metronome, MetronomeNode, PrepareContext, ProcessContext, Transport,
    TransportReader, TransportState,
};
use fontelle_types::{CompiledTimeline, EventPayload, TimedEvent};

fn note_at(sample: i64) -> TimedEvent {
    TimedEvent {
        sample,
        target: Default::default(),
        payload: EventPayload::NoteOn {
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
        },
    }
}

fn timeline(events: Vec<TimedEvent>) -> CompiledTimeline {
    CompiledTimeline {
        events,
        ..Default::default()
    }
}

/// What one step of the reader was: how many frames, where the playhead was
/// before it, how many events it carried, and the state its nodes were told.
#[derive(Debug)]
struct Seen {
    frames: usize,
    playhead: i64,
    events: usize,
    state: TransportState,
    range: std::ops::Range<i64>,
}

fn run(
    reader: &mut TransportReader,
    transport: &Transport,
    timeline: &CompiledTimeline,
    frames: usize,
) -> Vec<Seen> {
    let mut seen = Vec::new();
    let mut left = frames;
    while left > 0 {
        let step = reader.next_step(transport, timeline, left.min(BLOCK_SIZE), BLOCK_SIZE, false);
        seen.push(Seen {
            frames: step.frames,
            // Where the nodes are told the playhead is.
            playhead: step.snapshot.position_sample,
            events: step.events.len(),
            state: step.snapshot.state,
            range: step.range.clone(),
        });
        left -= step.frames.min(left);
    }
    seen
}

const MARKER: i64 = 96_000;
/// Four beats at 120 bpm and 48 kHz, and **not** a whole number of blocks —
/// a count-in that rounded to the block would be a take a few milliseconds
/// off, every time.
const COUNT: i64 = 4 * 24_000 + 77;

#[test]
fn the_playhead_holds_on_the_marker_for_exactly_the_count_then_rolls_from_it() {
    let transport = Transport::new();
    let tl = timeline(vec![]);
    let mut reader = TransportReader::new();
    transport.seek(MARKER);
    transport.set_count_in(COUNT);
    transport.set_state(TransportState::Recording);

    let seen = run(
        &mut reader,
        &transport,
        &tl,
        COUNT as usize + 4 * BLOCK_SIZE,
    );
    let counted: i64 = seen
        .iter()
        .filter(|s| s.state == TransportState::CountingIn)
        .map(|s| s.frames as i64)
        .sum();
    assert_eq!(counted, COUNT, "the count-in was not exactly its length");
    for s in seen
        .iter()
        .filter(|s| s.state == TransportState::CountingIn)
    {
        assert_eq!(s.playhead, MARKER, "the playhead moved during the count-in");
    }
    // And then it rolls from the marker, not from a bar before it.
    let first = seen
        .iter()
        .find(|s| s.state != TransportState::CountingIn)
        .expect("it rolled");
    assert_eq!(first.range.start, MARKER);
    assert_eq!(first.state, TransportState::Recording);
    assert!(reader.position() > MARKER);
    assert!(!transport.is_counting_in());
}

#[test]
fn a_count_in_at_the_top_of_the_song_is_as_long_as_anywhere_else() {
    let transport = Transport::new();
    let tl = timeline(vec![]);
    let mut reader = TransportReader::new();
    transport.seek(0);
    transport.set_count_in(COUNT);
    transport.set_state(TransportState::Recording);
    let seen = run(&mut reader, &transport, &tl, COUNT as usize + BLOCK_SIZE);
    let counted: i64 = seen
        .iter()
        .filter(|s| s.state == TransportState::CountingIn)
        .map(|s| s.frames as i64)
        .sum();
    assert_eq!(counted, COUNT, "a count-in from bar one was cut short");
}

#[test]
fn nothing_of_the_song_plays_during_the_count_in_and_the_marker_note_plays_after() {
    let transport = Transport::new();
    // A note before the marker (which a pre-roll would have played) and one
    // on it.
    let tl = timeline(vec![note_at(MARKER - 1000), note_at(MARKER)]);
    let mut reader = TransportReader::new();
    transport.seek(MARKER);
    transport.set_count_in(COUNT);
    transport.set_state(TransportState::Playing);
    let seen = run(
        &mut reader,
        &transport,
        &tl,
        COUNT as usize + 2 * BLOCK_SIZE,
    );
    let during: usize = seen
        .iter()
        .filter(|s| s.state == TransportState::CountingIn)
        .map(|s| s.events)
        .sum();
    assert_eq!(during, 0, "the song played during the count-in");
    let after: usize = seen
        .iter()
        .filter(|s| s.state != TransportState::CountingIn)
        .map(|s| s.events)
        .sum();
    assert_eq!(after, 1, "the note on the marker did not play after it");
}

#[test]
fn stopping_during_a_count_in_forgets_it() {
    let transport = Transport::new();
    let tl = timeline(vec![]);
    let mut reader = TransportReader::new();
    transport.seek(MARKER);
    transport.set_count_in(COUNT);
    transport.set_state(TransportState::Recording);
    run(&mut reader, &transport, &tl, BLOCK_SIZE);
    transport.stop();
    run(&mut reader, &transport, &tl, BLOCK_SIZE);
    assert!(
        !transport.is_counting_in(),
        "a stop left the count-in armed"
    );
    transport.set_state(TransportState::Playing);
    let seen = run(&mut reader, &transport, &tl, BLOCK_SIZE);
    assert_ne!(
        seen[0].state,
        TransportState::CountingIn,
        "the next play counted in again"
    );
}

/// Peaks of the click over a count-in of four beats ending at the marker, in
/// blocks, with the metronome switched **off**.
#[test]
fn the_count_in_clicks_its_beats_even_with_the_metronome_off() {
    const BEAT: i64 = 24_000;
    let metronome = Arc::new(Metronome::new());
    metronome.set_beat(BEAT as u32, 4);
    assert!(!metronome.is_on());
    let mut node = MetronomeNode::new(Arc::clone(&metronome));
    node.prepare(&PrepareContext {
        sample_rate: 48_000.0,
        max_block_size: BLOCK_SIZE as u32,
    });

    let transport = Transport::new();
    let tl = timeline(vec![]);
    let mut reader = TransportReader::new();
    transport.seek(MARKER);
    transport.set_count_in(4 * BEAT);
    transport.set_state(TransportState::Recording);

    let mut clicks: Vec<(i64, f32)> = Vec::new();
    let mut counted = 0i64;
    while transport.is_counting_in() || counted == 0 {
        let step = reader.next_step(&transport, &tl, BLOCK_SIZE, BLOCK_SIZE, false);
        if step.snapshot.state != TransportState::CountingIn {
            break;
        }
        let mut buffer = vec![0.0f32; step.frames];
        {
            let mut outputs: Vec<&mut [f32]> = vec![&mut buffer];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &[],
                live_events: &[],
                audio: &[],
                node: Default::default(),
                transport: step.snapshot,
                sample_range: step.range.clone(),
            };
            node.process(&mut ctx);
        }
        for (i, v) in buffer.iter().enumerate() {
            if v.abs() > 0.05
                && clicks
                    .last()
                    .is_none_or(|(at, _)| counted + i as i64 - at > 12_000)
            {
                clicks.push((counted + i as i64, v.abs()));
            }
        }
        counted += step.frames as i64;
    }
    let starts: Vec<i64> = clicks.iter().map(|(at, _)| *at).collect();
    assert_eq!(
        starts.len(),
        4,
        "a four-beat count-in clicked {} times at {starts:?}",
        starts.len()
    );
    for (beat, at) in starts.iter().enumerate() {
        assert!(
            (at - beat as i64 * BEAT).abs() < 64,
            "beat {beat} clicked at {at}, not {}",
            beat as i64 * BEAT
        );
    }
}
