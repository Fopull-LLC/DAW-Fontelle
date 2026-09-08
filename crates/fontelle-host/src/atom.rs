//! The wire an LV2 editor and its plugin exchange **atoms** on.
//!
//! A control port is a float, and a float cannot say *"load this file"*. An
//! LSP sampler's whole state is a file it was handed; it is handed one as a
//! `patch:Set` atom written by its editor, and it answers with an atom of its
//! own saying what it actually loaded. So a host that carries floats and drops
//! atoms opens a sampler's editor onto a sampler that can never be given a
//! sample — which is exactly the shape the gap had.
//!
//! # Why it looks like this
//!
//! One end is the **main thread** (the editor, in [`crate::lv2_ui`]) and the
//! other is the **audio thread** (the plugin, in [`crate::lv2`]). So:
//!
//! - **Nothing is allocated or freed by either end.** Every byte is taken once
//!   in [`AtomPipe::new`]; a message is copied into a slot that already exists
//!   and copied out of it again. A `VecDeque<Vec<u8>>` would have been shorter
//!   and would free a `Vec` on the audio thread, which INVARIANT 1 forbids.
//! - **The lock is never waited on by the audio thread.** It *tries*; a block
//!   that loses does nothing and tries again next time. That is the same trade
//!   [`crate::ProcessorBay`] makes, and the same reasoning: a contended try is
//!   a compare-exchange, and a blocking wait is not.
//! - **A full pipe drops the oldest message rather than the newest.** These
//!   are commands — "load this", "the file is that" — and the newest is the
//!   one that is still true.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Whether `FONTELLE_ATOM_TRACE` is set: a line on stderr for every atom an
/// editor writes and every processor the bay recalls, with the counts that
/// say whether the audio thread is listening. The one way to tell *"the
/// editor is talking"* from *"the plugin is hearing it"* in the studio —
/// which is how the idle gate was found sleeping under an open editor.
pub fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("FONTELLE_ATOM_TRACE").is_some())
}

/// The largest atom one message may carry, in bytes of body.
///
/// A `patch:Set` naming a file is a few hundred bytes: an object header, a
/// property URID and a path. Two kilobytes is that several times over, and
/// small enough that a pipe of them is measured in tens of kilobytes.
pub const MAX_ATOM_BYTES: usize = 2048;

/// How many messages may be in flight before the oldest is dropped.
///
/// An editor sends one when somebody clicks something; a plugin answers one
/// per change. Sixteen is a burst nobody produces by hand.
const SLOTS: usize = 16;

/// One message, in a slot that already exists.
struct Slot {
    /// The atom's type, as the **plugin's own** URID map named it.
    urid: u32,
    len: usize,
    bytes: [u8; MAX_ATOM_BYTES],
}

struct Ring {
    slots: Box<[Slot]>,
    head: usize,
    len: usize,
}

/// A one-way pipe of atoms between the editor and the audio thread.
pub struct AtomPipe {
    ring: Mutex<Ring>,
    /// How many messages have gone in since the pipe was made.
    ///
    /// Not bookkeeping: it is the one number that separates *"the editor is
    /// open and talking"* from *"the editor is open and its words are going
    /// nowhere"*, and those look identical from outside. Counted on push, so
    /// a message dropped for want of room is not counted as carried.
    carried: AtomicU64,
    /// How many messages the other end has taken out. `carried - taken` is
    /// what is waiting, or was dropped for want of room — the difference
    /// between *"sent"* and *"heard"*.
    taken: AtomicU64,
}

impl AtomPipe {
    pub fn new() -> Self {
        Self {
            ring: Mutex::new(Ring {
                slots: (0..SLOTS)
                    .map(|_| Slot {
                        urid: 0,
                        len: 0,
                        bytes: [0; MAX_ATOM_BYTES],
                    })
                    .collect(),
                head: 0,
                len: 0,
            }),
            carried: AtomicU64::new(0),
            taken: AtomicU64::new(0),
        }
    }

    /// How many messages this pipe has carried. See the field.
    pub fn carried(&self) -> u64 {
        self.carried.load(Ordering::Relaxed)
    }

    /// How many the other end has taken out. See the field.
    pub fn taken(&self) -> u64 {
        self.taken.load(Ordering::Relaxed)
    }

    /// Puts one message in. `false` when it did not fit or the lock was busy.
    ///
    /// **RT-safe**: it copies into a slot that exists and takes no lock it is
    /// willing to wait for.
    pub fn push(&self, urid: u32, body: &[u8]) -> bool {
        if body.len() > MAX_ATOM_BYTES {
            return false;
        }
        let Ok(mut ring) = self.ring.try_lock() else {
            return false;
        };
        // Full: the oldest goes, because these are commands and the newest is
        // the one still worth acting on.
        if ring.len == SLOTS {
            ring.head = (ring.head + 1) % SLOTS;
            ring.len -= 1;
        }
        let index = (ring.head + ring.len) % SLOTS;
        let slot = &mut ring.slots[index];
        slot.urid = urid;
        slot.len = body.len();
        slot.bytes[..body.len()].copy_from_slice(body);
        ring.len += 1;
        self.carried.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Hands every message waiting to `take`, oldest first, and empties the
    /// pipe. Does nothing at all if the lock is busy.
    ///
    /// **RT-safe** for the same reason [`push`](Self::push) is.
    pub fn drain(&self, mut take: impl FnMut(u32, &[u8])) {
        let Ok(mut ring) = self.ring.try_lock() else {
            return;
        };
        for step in 0..ring.len {
            let index = (ring.head + step) % SLOTS;
            let (urid, len) = (ring.slots[index].urid, ring.slots[index].len);
            take(urid, &ring.slots[index].bytes[..len]);
        }
        self.taken.fetch_add(ring.len as u64, Ordering::Relaxed);
        ring.head = 0;
        ring.len = 0;
    }

    /// Whether anything is waiting. For tests, and for skipping work.
    pub fn is_empty(&self) -> bool {
        self.ring.lock().map(|ring| ring.len == 0).unwrap_or(true)
    }
}

impl Default for AtomPipe {
    fn default() -> Self {
        Self::new()
    }
}

/// Both directions, and the port each one belongs to.
///
/// Made when an LV2 plugin is opened and shared with whatever is rendering it,
/// exactly as [`crate::ParamValues`] is — the editor comes and goes, and the
/// pipes outlive it.
pub struct AtomPipes {
    /// How many blocks the plugin has run since it was opened — the one
    /// number that says whether the audio thread is reaching it at all.
    /// Counted by the processor, read by whoever is wondering.
    pub runs: AtomicU64,
    /// What the editor has written, waiting for the plugin's next block.
    pub to_plugin: AtomPipe,
    /// What the plugin wrote, waiting for the editor's next frame.
    pub to_editor: AtomPipe,
    /// The plugin's atom **input** port, which is where the editor's messages
    /// are addressed. `None` when it has none.
    pub in_port: Option<u32>,
    /// And its atom **output** port, which is where the plugin's come from.
    pub out_port: Option<u32>,
}

impl AtomPipes {
    pub fn new(in_port: Option<u32>, out_port: Option<u32>) -> Self {
        Self {
            runs: AtomicU64::new(0),
            to_plugin: AtomPipe::new(),
            to_editor: AtomPipe::new(),
            in_port,
            out_port,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pipe_counts_what_it_carried_and_not_what_it_refused() {
        let pipe = AtomPipe::new();
        assert_eq!(pipe.carried(), 0);
        pipe.push(0, b"one");
        pipe.push(0, b"two");
        assert!(!pipe.push(0, &vec![0u8; MAX_ATOM_BYTES + 1]));
        assert_eq!(pipe.carried(), 2);
    }

    #[test]
    fn a_message_comes_out_the_way_it_went_in() {
        let pipe = AtomPipe::new();
        assert!(pipe.push(7, b"hello"));
        let mut seen = Vec::new();
        pipe.drain(|urid, body| seen.push((urid, body.to_vec())));
        assert_eq!(seen, vec![(7, b"hello".to_vec())]);
        assert!(pipe.is_empty());
    }

    #[test]
    fn messages_come_out_in_the_order_they_went_in() {
        let pipe = AtomPipe::new();
        for n in 0..5u8 {
            assert!(pipe.push(u32::from(n), &[n]));
        }
        let mut seen = Vec::new();
        pipe.drain(|_, body| seen.push(body[0]));
        assert_eq!(seen, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn a_full_pipe_drops_the_oldest_rather_than_the_newest() {
        let pipe = AtomPipe::new();
        for n in 0..(SLOTS + 3) as u8 {
            pipe.push(0, &[n]);
        }
        let mut seen = Vec::new();
        pipe.drain(|_, body| seen.push(body[0]));
        assert_eq!(seen.len(), SLOTS);
        assert_eq!(*seen.last().unwrap(), (SLOTS + 2) as u8);
        assert_eq!(seen[0], 3, "the first three are what went");
    }

    #[test]
    fn a_message_too_big_for_a_slot_is_refused_rather_than_truncated() {
        let pipe = AtomPipe::new();
        assert!(!pipe.push(0, &vec![0u8; MAX_ATOM_BYTES + 1]));
        assert!(pipe.is_empty());
    }

    #[test]
    fn draining_an_empty_pipe_says_nothing() {
        let pipe = AtomPipe::new();
        let mut seen = 0;
        pipe.drain(|_, _| seen += 1);
        assert_eq!(seen, 0);
    }
}
