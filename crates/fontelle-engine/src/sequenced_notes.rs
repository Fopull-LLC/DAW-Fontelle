//! The notes the song has started and not yet ended, so a timeline swapped in
//! while playing can end the ones the new song will not.
//!
//! > *"editing notes on plugins like dexxed in the piano roll sometimes
//! > triggers a bug where a note gets stuck playing infinitely"*
//!
//! A swap only moved the cursor. A sounding note whose note-off an edit took
//! away (shortened before the playhead, deleted, moved to another key, muted)
//! never heard one, and one dragged later past the playhead was started a
//! second time with one note-off for the two voices. Dexed lets go of one
//! voice a note-off, so the other rang for good — through a stop, because the
//! host's own record had already seen its key end.
//!
//! Notes are matched by instrument, key and clip (`voice_context`): the
//! sequencer writes one context a clip, so a note on the same key in another
//! clip is not taken for this one.

use fontelle_types::{CompiledTimeline, EventPayload, NodeId, Sample, TimedEvent};

#[derive(Clone, Copy)]
struct Held {
    target: NodeId,
    key: u8,
    clip: u32,
    /// Started and not ended — two notes on one key overlapping are two.
    count: u16,
    /// Note-offs for it ahead of the playhead in the new timeline, before
    /// its next note-on. Scratch for [`SequencedNotes::retarget`].
    ahead: u16,
    open: bool,
}

/// **RT.** See the module note. Fixed capacity, given off the audio thread:
/// nothing here allocates once made. A note past the capacity is not
/// tracked, which is the behaviour before this existed.
pub struct SequencedNotes {
    held: Vec<Held>,
    cuts: Vec<TimedEvent>,
}

impl SequencedNotes {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            held: Vec::with_capacity(capacity),
            cuts: Vec::with_capacity(capacity),
        }
    }

    /// The song's events for a block just played.
    pub fn heard(&mut self, events: &[TimedEvent]) {
        for event in events {
            match event.payload {
                EventPayload::NoteOn {
                    key, voice_context, ..
                } => match self.find(event.target, key, voice_context) {
                    Some(index) => {
                        let held = &mut self.held[index];
                        held.count = held.count.saturating_add(1);
                    }
                    None if self.held.len() < self.held.capacity() => self.held.push(Held {
                        target: event.target,
                        key,
                        clip: voice_context,
                        count: 1,
                        ahead: 0,
                        open: false,
                    }),
                    None => {}
                },
                EventPayload::NoteOff { key, voice_context } => {
                    if let Some(index) = self.find(event.target, key, voice_context) {
                        let held = &mut self.held[index];
                        held.count -= 1;
                        if held.count == 0 {
                            self.held.swap_remove(index);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// A reset: the graph ended everything the song was playing.
    pub fn clear(&mut self) {
        self.held.clear();
        self.cuts.clear();
    }

    /// `timeline` replaces the one playing, at `at`: every sounding note it
    /// has no note-off for ahead (before that note's next start) is cut at
    /// `at`, as many times as it is sounding beyond the note-offs there are.
    ///
    /// One pass from the playhead, stopping once every note is answered; a
    /// note the new song ends nowhere walks it to the end.
    pub fn retarget(&mut self, timeline: &CompiledTimeline, at: Sample) {
        if self.held.is_empty() {
            return;
        }
        for held in &mut self.held {
            held.ahead = 0;
            held.open = true;
        }
        let mut open = self.held.len();
        for event in &timeline.events[timeline.cursor_at(at)..] {
            if open == 0 {
                break;
            }
            let (key, clip, starts) = match event.payload {
                EventPayload::NoteOn {
                    key, voice_context, ..
                } => (key, voice_context, true),
                EventPayload::NoteOff { key, voice_context } => (key, voice_context, false),
                _ => continue,
            };
            let Some(held) = self.held.iter_mut().find(|held| {
                held.open && held.target == event.target && held.key == key && held.clip == clip
            }) else {
                continue;
            };
            if !starts {
                held.ahead += 1;
            }
            if starts || held.ahead >= held.count {
                held.open = false;
                open -= 1;
            }
        }
        let mut index = 0;
        while index < self.held.len() {
            let held = self.held[index];
            for _ in held.ahead..held.count {
                if self.cuts.len() < self.cuts.capacity() {
                    self.cuts.push(TimedEvent {
                        sample: at,
                        target: held.target,
                        payload: EventPayload::NoteOff {
                            key: held.key,
                            voice_context: held.clip,
                        },
                    });
                }
            }
            if held.ahead == 0 {
                self.held.swap_remove(index);
            } else {
                self.held[index].count = held.count.min(held.ahead);
                index += 1;
            }
        }
    }

    /// The note-offs to go out at the top of the next block.
    pub fn cuts(&self) -> &[TimedEvent] {
        &self.cuts
    }

    /// The next block has carried them.
    pub fn delivered(&mut self) {
        self.cuts.clear();
    }

    fn find(&self, target: NodeId, key: u8, clip: u32) -> Option<usize> {
        self.held
            .iter()
            .position(|held| held.target == target && held.key == key && held.clip == clip)
    }
}
