//! Turning a captured take into notes (TDD §14.7).

use std::collections::HashMap;

use fontelle_types::{EventPayload, Sample, Tick, TimedEvent};

use crate::arena::Arena;
use crate::clip::ClipSource;
use crate::note::{Note, NoteData};
use crate::project::TempoMap;

/// The shortest note a take can hold.
///
/// A note-on and its note-off inside one block round to the same tick, and a
/// zero-length note is a note-on and a note-off on the same sample: the
/// sequencer emits both and the sampler cannot sound anything between them, so
/// the fastest possible stab would vanish from the take. One tick is 1/960 of
/// a beat — inaudibly short, and present.
const SHORTEST_NOTE: Tick = 1;

/// Turns the events a recording captured into the notes of a clip.
///
/// `clip_start` is where on the timeline the clip will sit, because a note's
/// start is relative to its clip (TDD §10.4). `end_sample` is where recording
/// stopped, which is where any key still held ends.
///
/// **No quantisation beyond rounding to the tick grid.** Quantise is a
/// piano-roll command (§16.5) applied to a selection the user can see; doing
/// it here would throw away the performance before anyone had a chance to look
/// at it, and there would be nothing to undo back to.
///
/// **Every event is taken.** Splitting a take across several record-armed
/// channels means splitting by `TimedEvent::target`, and there is one live
/// target today — the channel the keyboard is playing.
pub fn notes_from_capture(
    events: &[TimedEvent],
    tempo_map: &TempoMap,
    clip_start: Tick,
    end_sample: Sample,
) -> ClipSource {
    let mut notes: Arena<fontelle_types::NoteId, Note> = Arena::new();
    // What is currently down, by key. Keyed rather than a list because a
    // note-off names only a key, and the newest note-on for that key is the
    // one it ends.
    let mut held: HashMap<u8, (Tick, u8)> = HashMap::new();
    let mut finished: Vec<(Tick, Tick, u8, u8)> = Vec::new();

    let to_tick = |sample: Sample| tempo_map.sample_to_tick(sample) - clip_start;

    let mut close = |held: &mut HashMap<u8, (Tick, u8)>, key: u8, at: Tick| {
        if let Some((start, velocity)) = held.remove(&key) {
            finished.push((start, (at - start).max(SHORTEST_NOTE), key, velocity));
        }
    };

    for event in events {
        let at = to_tick(event.sample);
        match &event.payload {
            // The MIDI decoder already turns a zero-velocity note-on into a
            // note-off, so a take through a real keyboard never carries one.
            // A capture from anywhere else might, and reading it literally
            // starts a silent note that never ends.
            EventPayload::NoteOn { key, velocity, .. } if *velocity == 0 => {
                close(&mut held, *key, at)
            }
            EventPayload::NoteOn { key, velocity, .. } => {
                // A key retriggered before its note-off ends the first note.
                // Two overlapping notes on one key is a shape the piano roll
                // cannot draw and the sampler cannot voice sensibly.
                close(&mut held, *key, at);
                held.insert(*key, (at, *velocity));
            }
            EventPayload::NoteOff { key, .. } => close(&mut held, *key, at),
            _ => {}
        }
    }

    // Whatever is still down when the tape stops ends there. A held chord at
    // the end of a take is a normal way to finish playing; letting it hang
    // forever, or dropping it, are both worse.
    let end = to_tick(end_sample);
    let still_held: Vec<u8> = held.keys().copied().collect();
    for key in still_held {
        close(&mut held, key, end);
    }

    // Sorted so a take's notes land in the arena in the order they were
    // played, which is what makes two recordings of the same performance the
    // same document.
    finished.sort_by_key(|(start, _, key, _)| (*start, *key));
    for (start, length, key, velocity) in finished {
        // A note before the clip starts — a count-in, or a key held over from
        // before the take — has nowhere to go: a negative offset inside a clip
        // is not something the piano roll can show.
        if start < 0 {
            continue;
        }
        notes.insert(Note {
            start,
            length,
            key,
            velocity,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            // A recorded note is an ordinary one: a slide is
            // something you draw, not something a keyboard sends.
            slide: false,
            channel: None,
        });
    }

    ClipSource::Notes(NoteData {
        // The caller fills this in: which channel a take belongs to is the
        // record-arm, not anything the events carry.
        channel: fontelle_types::ChannelId::default(),
        notes,
    })
}
