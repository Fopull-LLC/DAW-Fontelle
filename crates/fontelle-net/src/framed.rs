//! Messages of any size, in frames the relay lets through
//! (`docs/collab-plan.md` §8.4, F35).
//!
//! The relay caps a reliable frame at 128 KB host to peer and 64 KB peer to
//! host, and a shared song's messages are not all small: a snapshot, a file,
//! an edit that deletes five thousand notes. [`Framed`] goes round any
//! [`Transport`] and splits whatever is bigger than [`FRAME`] into numbered
//! frames, put back together on the other side before anybody sees them.
//!
//! On the reliable channel only — the one a shared session uses — because
//! that channel is ordered, which is what lets a message's frames be joined
//! in the order they came. A frame is one tag byte, then either the whole
//! message, or its number, which frame this is and how many there are.

use std::collections::HashMap;

use crate::transport::{Channel, Incoming, LinkStats, PeerId, Transport};

/// The most one frame carries: under the relay's 64 KB peer-to-host cap with
/// room for its own envelope, and big enough that a 48 KB piece of a file is
/// one frame.
pub const FRAME: usize = 56 * 1024;

/// The biggest message a peer may send, put back together: a bound on what a
/// modified peer can make this side hold in memory.
const MAX_MESSAGE: usize = 512 * 1024 * 1024;

const WHOLE: u8 = 0;
const PART: u8 = 1;

/// Frames a message's parts are kept in until the last arrives.
struct Assembly {
    of: u32,
    next: u32,
    bytes: Vec<u8>,
}

/// A [`Transport`] whose reliable messages may be any size.
pub struct Framed<T> {
    inner: T,
    next_id: u32,
    assembling: HashMap<(PeerId, u32), Assembly>,
}

impl<T: Transport> Framed<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            next_id: 0,
            assembling: HashMap::new(),
        }
    }

    fn take(&mut self, peer: PeerId, frame: &[u8]) -> Option<Vec<u8>> {
        match frame.split_first()? {
            (&WHOLE, rest) => Some(rest.to_vec()),
            (&PART, rest) if rest.len() >= 12 => {
                let word = |at: usize| u32::from_le_bytes(rest[at..at + 4].try_into().unwrap());
                let (id, index, of) = (word(0), word(4), word(8));
                let body = &rest[12..];
                let assembly = self.assembling.entry((peer, id)).or_insert(Assembly {
                    of,
                    next: 0,
                    bytes: Vec::new(),
                });
                // Out of order, or bigger than anybody should send: the
                // message is dropped rather than put together wrong.
                if index != assembly.next
                    || of != assembly.of
                    || assembly.bytes.len() + body.len() > MAX_MESSAGE
                {
                    self.assembling.remove(&(peer, id));
                    return None;
                }
                assembly.bytes.extend_from_slice(body);
                assembly.next += 1;
                if assembly.next == assembly.of {
                    return self.assembling.remove(&(peer, id)).map(|a| a.bytes);
                }
                None
            }
            _ => None,
        }
    }
}

impl<T: Transport> Transport for Framed<T> {
    fn set_wake(&mut self, wake: crate::transport::Wake) {
        self.inner.set_wake(wake);
    }

    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        if channel != Channel::Reliable || bytes.len() < FRAME {
            let mut frame = Vec::with_capacity(bytes.len() + 1);
            frame.push(WHOLE);
            frame.extend_from_slice(bytes);
            self.inner.send(peer, channel, &frame);
            return;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let parts: Vec<&[u8]> = bytes.chunks(FRAME).collect();
        let of = parts.len() as u32;
        for (index, part) in parts.into_iter().enumerate() {
            let mut frame = Vec::with_capacity(part.len() + 13);
            frame.push(PART);
            frame.extend_from_slice(&id.to_le_bytes());
            frame.extend_from_slice(&(index as u32).to_le_bytes());
            frame.extend_from_slice(&of.to_le_bytes());
            frame.extend_from_slice(part);
            self.inner.send(peer, channel, &frame);
        }
    }

    fn poll(&mut self) -> Vec<Incoming> {
        let mut out = Vec::new();
        for incoming in self.inner.poll() {
            match incoming {
                Incoming::Message(peer, channel, frame) => {
                    if let Some(message) = self.take(peer, &frame) {
                        out.push(Incoming::Message(peer, channel, message));
                    }
                }
                Incoming::Disconnected(peer, why) => {
                    self.assembling.retain(|(p, _), _| *p != peer);
                    out.push(Incoming::Disconnected(peer, why));
                }
                other => out.push(other),
            }
        }
        out
    }

    fn stats(&self, peer: PeerId) -> LinkStats {
        self.inner.stats(peer)
    }

    fn disconnect(&mut self, peer: PeerId) {
        self.inner.disconnect(peer);
    }

    fn take_notices(&mut self) -> Vec<String> {
        self.inner.take_notices()
    }

    fn take_join_progress(&mut self) -> Option<String> {
        self.inner.take_join_progress()
    }

    fn lobby_code(&self) -> Option<String> {
        self.inner.lobby_code()
    }
}
