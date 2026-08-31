//! The one note the mouse is holding down (TDD §14.1's live path).
//!
//! Clicking a key, clicking a note, drawing one, dragging one to a new pitch —
//! all of them sound something *now*, outside the timeline, whether or not the
//! transport is rolling. That is one note at a time and two events, which
//! sounds like too little to be worth a module until you notice that both bugs
//! reported against it were bugs in exactly this bookkeeping:
//!
//! - **A hung voice.** [`Sampler::note_off`](fontelle_core) releases *one*
//!   voice — the first active one matching the key and the voice context. The
//!   window only sent a note-off when the key *changed*, so clicking the same
//!   note twice sent two note-ons and, between them, nothing. The second voice
//!   had no note-off coming and sustained until something else in the engine
//!   happened to take it. Hence [`Auditions::start`] releasing the sounding
//!   key **even when it is the same key**: that is the whole fix, and the
//!   property worth holding is that every note-on this type emits is matched
//!   by exactly one note-off.
//! - **A flicker.** The minimum was a flat 180 ms for everything, so clicking
//!   a half note sounded a fifth of a beat of it. A hold is now per-audition:
//!   what you click sounds for as long as it *is*, floored so a thirty-second
//!   is still a note and capped so a whole note is not a drone.
//!
//! None of this needs a window — it is two events and a clock — so it lives
//! here as a state machine that *returns what to send*, and the window is the
//! only thing that knows how to send it. §2.5's rule, and the reason the two
//! bugs above are now regression tests rather than something to re-notice by
//! ear.

use std::time::{Duration, Instant};

/// The shortest an audition sounds for.
///
/// A click is thirty milliseconds, and thirty milliseconds of any sample is a
/// click rather than a note — of a chiptune noise channel it is literally
/// static, which is what this was first reported as.
pub const MIN_AUDITION: Duration = Duration::from_millis(180);

/// And the longest. Clicking a note four bars long should tell you what it is,
/// not commit you to hearing all of it.
pub const MAX_AUDITION: Duration = Duration::from_millis(2500);

/// What the window should put on the live path.
///
/// Values rather than calls, for the reason every other canvas here hands back
/// values: this type may not touch the audio path, and a type that cannot
/// touch it cannot touch it wrongly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditionAction {
    On {
        key: u8,
        velocity: u8,
        /// §16.5's per-note pan, in the range `Note::pan` is stored in. The
        /// live path carries it for the same reason the compiled one does: a
        /// note you click should sound like the note you wrote, and a live
        /// path that centres everything means the only way to hear a pan is
        /// to press play.
        pan: i8,
    },
    Off {
        key: u8,
    },
}

/// The note that is sounding, and when it may stop.
#[derive(Debug, Clone, Copy)]
struct Voice {
    key: u8,
    started: Instant,
    /// The shortest this one may sound for — see [`Auditions::start`].
    hold: Duration,
}

/// The live-path note the pointer is holding, as a state machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Auditions {
    sounding: Option<Voice>,
    /// When the sounding note is due to be released. `None` while it is being
    /// held, which is what makes the hold a floor and not a length.
    until: Option<Instant>,
}

impl Auditions {
    /// Sounds `key`, releasing whatever was sounding first.
    ///
    /// **Whatever was sounding, including this same key.** See the module
    /// docs: a note-on that is not preceded by a note-off for the voice
    /// already on that key is a voice nobody can address again.
    ///
    /// `hold` is how long this note should sound at minimum once released —
    /// the length of the thing that was clicked — and is clamped into
    /// [`MIN_AUDITION`]..=[`MAX_AUDITION`] here rather than at the call site,
    /// because there are four call sites and one of them would forget.
    pub fn start(
        &mut self,
        key: u8,
        velocity: u8,
        pan: i8,
        hold: Duration,
        now: Instant,
    ) -> Vec<AuditionAction> {
        let mut actions = Vec::with_capacity(2);
        if let Some(previous) = self.sounding.take() {
            actions.push(AuditionAction::Off { key: previous.key });
        }
        // The old note's release is cancelled with the old note: left
        // scheduled, it lands *after* the new note-on and cuts it off, which
        // is the other half of "just a flicker".
        self.until = None;
        self.sounding = Some(Voice {
            key,
            started: now,
            hold: hold.clamp(MIN_AUDITION, MAX_AUDITION),
        });
        actions.push(AuditionAction::On { key, velocity, pan });
        actions
    }

    /// The button came up. The note is *scheduled* to stop rather than
    /// stopped: a note released after twenty milliseconds still has to have
    /// been a note.
    pub fn release(&mut self, now: Instant) {
        let Some(voice) = self.sounding else {
            return;
        };
        self.until = Some(now.max(voice.started + voice.hold));
    }

    /// Releases a scheduled note once it is due. Called on every pass of the
    /// window's loop, and never returns the same release twice.
    pub fn settle(&mut self, now: Instant) -> Option<AuditionAction> {
        let due = self.until?;
        if now < due {
            return None;
        }
        self.until = None;
        self.sounding
            .take()
            .map(|voice| AuditionAction::Off { key: voice.key })
    }

    /// Stop now, whatever is sounding and whatever it was promised.
    ///
    /// What the window calls when the compositor takes the pointer away
    /// mid-gesture, and on the way out: an alt-tab must not leave a note
    /// ringing in a window nobody is looking at.
    pub fn silence(&mut self) -> Option<AuditionAction> {
        self.until = None;
        self.sounding
            .take()
            .map(|voice| AuditionAction::Off { key: voice.key })
    }

    /// When [`settle`](Self::settle) next has something to do. The window has
    /// to *wake up* for this, or a loop that goes back to sleep the instant
    /// the mouse comes up leaves the note sounding until something else
    /// happens to the window.
    pub fn due(&self) -> Option<Instant> {
        self.until
    }

    /// The key that is sounding, if one is.
    pub fn key(&self) -> Option<u8> {
        self.sounding.map(|voice| voice.key)
    }
}
