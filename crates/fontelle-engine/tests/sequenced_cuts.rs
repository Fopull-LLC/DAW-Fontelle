//! A note the song started, whose note-off an edit took away.
//!
//! > *"editing notes on plugins like dexxed in the piano roll sometimes
//! > triggers a bug where a note gets stuck playing infinitely ... pressing
//! > play to resume playback makes the infinite sound come back."*
//!
//! A new timeline published while playing only moved the cursor. A note
//! already sounding whose note-off the edit moved before the playhead,
//! deleted, or re-pitched never heard one; one dragged later past the
//! playhead was started a second time, and its one note-off ended one of the
//! two voices. Dexed lets go of one voice a note-off. `SequencedNotes` keeps
//! what the song has started and, at a swap, ends now whatever the new song
//! will not end itself.

use fontelle_engine::SequencedNotes;
use fontelle_types::{CompiledTimeline, EventPayload, NodeId, TimedEvent};
use slotmap::KeyData;

fn node(n: u64) -> NodeId {
    NodeId::from(KeyData::from_ffi(n | (1 << 32)))
}

fn on(sample: i64, target: NodeId, key: u8, clip: u32) -> TimedEvent {
    TimedEvent {
        sample,
        target,
        payload: EventPayload::NoteOn {
            key,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: clip,
        },
    }
}

fn off(sample: i64, target: NodeId, key: u8, clip: u32) -> TimedEvent {
    TimedEvent {
        sample,
        target,
        payload: EventPayload::NoteOff {
            key,
            voice_context: clip,
        },
    }
}

fn timeline(mut events: Vec<TimedEvent>) -> CompiledTimeline {
    events.sort_by_key(|e| e.sample);
    CompiledTimeline {
        events,
        ..Default::default()
    }
}

/// The note-offs a swap to `new` at `at` cuts, as (target, key, clip).
fn cuts(notes: &SequencedNotes) -> Vec<(NodeId, u8, u32, i64)> {
    notes
        .cuts()
        .iter()
        .map(|event| match event.payload {
            EventPayload::NoteOff { key, voice_context } => {
                (event.target, key, voice_context, event.sample)
            }
            ref other => panic!("a cut is a note-off, not {other:?}"),
        })
        .collect()
}

/// One note, key 60 on clip 0, from 0 to 48 000, heard up to 10 000.
fn sounding(target: NodeId) -> SequencedNotes {
    let mut notes = SequencedNotes::with_capacity(64);
    let old = timeline(vec![on(0, target, 60, 0), off(48_000, target, 60, 0)]);
    notes.heard(&old.events[..1]);
    notes
}

#[test]
fn a_note_whose_end_is_still_ahead_is_left_to_ring() {
    let a = node(1);
    let mut notes = sounding(a);
    // An edit somewhere else: the same note, and another after it.
    let new = timeline(vec![
        on(0, a, 60, 0),
        off(48_000, a, 60, 0),
        on(60_000, a, 64, 0),
        off(70_000, a, 64, 0),
    ]);
    notes.retarget(&new, 10_000);
    assert!(cuts(&notes).is_empty(), "{:?}", cuts(&notes));
    // And made longer, the same.
    let longer = timeline(vec![on(0, a, 60, 0), off(96_000, a, 60, 0)]);
    notes.retarget(&longer, 10_000);
    assert!(cuts(&notes).is_empty(), "{:?}", cuts(&notes));
}

#[test]
fn a_note_made_shorter_than_the_playhead_is_ended_now() {
    let a = node(1);
    let mut notes = sounding(a);
    let shorter = timeline(vec![on(0, a, 60, 0), off(5_000, a, 60, 0)]);
    notes.retarget(&shorter, 10_000);
    assert_eq!(cuts(&notes), vec![(a, 60, 0, 10_000)]);
}

#[test]
fn a_note_deleted_or_moved_to_another_key_is_ended_now() {
    let a = node(1);
    let mut notes = sounding(a);
    notes.retarget(&timeline(Vec::new()), 10_000);
    assert_eq!(cuts(&notes), vec![(a, 60, 0, 10_000)]);

    let mut notes = sounding(a);
    let moved = timeline(vec![on(0, a, 62, 0), off(48_000, a, 62, 0)]);
    notes.retarget(&moved, 10_000);
    assert_eq!(cuts(&notes), vec![(a, 60, 0, 10_000)]);
}

/// The case that leaves a voice nobody can reach: dragged later past the
/// playhead, its note-on is ahead again and its one note-off would end only
/// the second voice.
#[test]
fn a_note_dragged_past_the_playhead_is_ended_before_it_starts_again() {
    let a = node(1);
    let mut notes = sounding(a);
    let later = timeline(vec![on(20_000, a, 60, 0), off(68_000, a, 60, 0)]);
    notes.retarget(&later, 10_000);
    assert_eq!(cuts(&notes), vec![(a, 60, 0, 10_000)]);
}

/// Two notes on one key overlapping in the score are two voices, and need
/// two note-offs ahead to be left alone.
#[test]
fn two_voices_on_one_key_are_counted() {
    let a = node(1);
    let mut notes = SequencedNotes::with_capacity(64);
    let old = timeline(vec![
        on(0, a, 60, 0),
        on(1_000, a, 60, 0),
        off(20_000, a, 60, 0),
        off(30_000, a, 60, 0),
    ]);
    notes.heard(&old.events[..2]);
    notes.retarget(&old, 10_000);
    assert!(cuts(&notes).is_empty(), "{:?}", cuts(&notes));

    // One of them deleted: one note-off ahead for two voices.
    let one = timeline(vec![on(0, a, 60, 0), off(20_000, a, 60, 0)]);
    notes.retarget(&one, 10_000);
    assert_eq!(cuts(&notes), vec![(a, 60, 0, 10_000)]);
}

#[test]
fn only_the_note_on_its_own_instrument_and_clip_is_matched() {
    let (a, b) = (node(1), node(2));
    let mut notes = sounding(a);
    // Key 60 still ends ahead — but on another instrument, and on another
    // clip of this one.
    let elsewhere = timeline(vec![
        on(0, b, 60, 0),
        off(48_000, b, 60, 0),
        on(0, a, 60, 7),
        off(48_000, a, 60, 7),
    ]);
    notes.retarget(&elsewhere, 10_000);
    assert_eq!(cuts(&notes), vec![(a, 60, 0, 10_000)]);
}

/// What was cut is no longer sounding, and is said once: the next block's
/// events go out without it, and a second swap does not cut it again.
#[test]
fn a_cut_is_delivered_once_and_forgotten() {
    let a = node(1);
    let mut notes = sounding(a);
    notes.retarget(&timeline(Vec::new()), 10_000);
    assert_eq!(cuts(&notes).len(), 1);
    notes.delivered();
    assert!(cuts(&notes).is_empty());
    notes.retarget(&timeline(Vec::new()), 12_000);
    assert!(cuts(&notes).is_empty(), "{:?}", cuts(&notes));
}

/// A note-off the song played, and a reset, both end what is held; a note
/// heard ending is not cut later.
#[test]
fn notes_heard_ending_and_a_reset_leave_nothing_to_cut() {
    let a = node(1);
    let mut notes = sounding(a);
    notes.heard(&[off(48_000, a, 60, 0)]);
    notes.retarget(&timeline(Vec::new()), 50_000);
    assert!(cuts(&notes).is_empty(), "{:?}", cuts(&notes));

    let mut notes = sounding(a);
    notes.clear();
    notes.retarget(&timeline(Vec::new()), 10_000);
    assert!(cuts(&notes).is_empty(), "{:?}", cuts(&notes));
}
