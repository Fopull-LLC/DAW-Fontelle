//! **The one choke point** a session's bytes pass on their way to the wire
//! and back: every message a `Collab` sends or receives — the song's pieces,
//! edits, chat, whereabouts on the unreliable channel, a file's chunks — goes
//! through the [`Sealed`] around its transport, and nowhere else.
//!
//! On a connection that came in with a session password the frames are
//! sealed with ChaCha20-Poly1305 under that connection's own key, one each
//! way ([`super::identity::PasswordKeys::link`]). The nonce is the channel
//! and a counter that only goes up, so it never repeats under one key, and
//! the receiver takes only a counter higher than the last it took on that
//! channel — a frame sent again, or one carried over from another channel,
//! is dropped. So is a frame that does not open, and a frame in the clear on
//! a sealed connection unless it is part of the handshake (which a
//! connection starting again speaks in the clear). Dropped, never a panic:
//! a relay that mangles or invents frames can stop a session, not crash one.
//!
//! A connection without a password is passed through as it always was.

use std::collections::BTreeMap;

use fontelle_model::wire::{Msg, SEALED};
use fontelle_net::{Channel, Incoming, PeerId, Transport};
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};

use super::identity::LinkKeys;

/// Which end of the session this is: it decides which of a connection's two
/// keys seals and which opens, and which messages may still come in the
/// clear on a sealed connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Host,
    Joiner,
}

/// A frame's header: [`SEALED`], the channel, the counter.
const HEADER: usize = 2 + 1 + 8;

struct Link {
    seal: LessSafeKey,
    open: LessSafeKey,
    /// The last counter sent, and taken, on each channel.
    sent: [u64; 3],
    taken: [u64; 3],
}

/// A transport whose connections may be sealed — see the module doc.
pub struct Sealed {
    inner: Box<dyn Transport>,
    end: End,
    links: BTreeMap<PeerId, Link>,
    sealed_out: usize,
    opened_in: usize,
    dropped_in: usize,
}

fn channel_tag(channel: Channel) -> u8 {
    match channel {
        Channel::Reliable => 0,
        Channel::Unreliable => 1,
        Channel::UnreliableSequenced => 2,
    }
}

fn nonce_for(tag: u8, counter: u64) -> Nonce {
    let mut nonce = [0u8; 12];
    nonce[0] = tag;
    nonce[4..].copy_from_slice(&counter.to_be_bytes());
    Nonce::assume_unique_for_key(nonce)
}

fn key(bytes: &[u8; 32]) -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, bytes).expect("32 bytes is the key"))
}

impl Sealed {
    pub fn new(inner: Box<dyn Transport>, end: End) -> Sealed {
        Sealed {
            inner,
            end,
            links: BTreeMap::new(),
            sealed_out: 0,
            opened_in: 0,
            dropped_in: 0,
        }
    }

    /// Seals everything to and from `peer` from now on, under `keys`. A
    /// connection armed again (a fresh challenge) starts its counters over
    /// under its fresh keys.
    pub fn arm(&mut self, peer: PeerId, keys: &LinkKeys) {
        let (seal, open) = match self.end {
            End::Host => (&keys.host_to_joiner, &keys.joiner_to_host),
            End::Joiner => (&keys.joiner_to_host, &keys.host_to_joiner),
        };
        self.links.insert(
            peer,
            Link {
                seal: key(seal),
                open: key(open),
                sent: [0; 3],
                taken: [0; 3],
            },
        );
    }

    /// `peer`'s connection is in the clear again: it has gone, or it is
    /// starting over.
    pub fn disarm(&mut self, peer: PeerId) {
        self.links.remove(&peer);
    }

    /// Lets go of the link underneath, for `closed` — an ended session's.
    pub fn let_go(&mut self, closed: Box<dyn Transport>) {
        self.inner = closed;
        self.links.clear();
    }

    /// How many frames this end has sealed, opened, and dropped for not
    /// opening (or for coming in the clear where they may not) — for tests.
    #[cfg(test)]
    pub fn sealed_out(&self) -> usize {
        self.sealed_out
    }

    #[cfg(test)]
    pub fn opened_in(&self) -> usize {
        self.opened_in
    }

    #[cfg(test)]
    pub fn dropped_in(&self) -> usize {
        self.dropped_in
    }

    /// What may still come in the clear on a sealed connection: the
    /// handshake, which a connection that starts over speaks before it has
    /// keys again.
    fn clear_allowed(&self, bytes: &[u8]) -> bool {
        matches!(
            (self.end, Msg::from_bytes(bytes)),
            (
                End::Host,
                Ok(Msg::Session { .. } | Msg::Hello { .. } | Msg::Answer { .. })
            ) | (End::Joiner, Ok(Msg::Challenge { .. } | Msg::Refuse { .. }))
        )
    }

    fn open(&mut self, peer: PeerId, frame: &[u8]) -> Option<Vec<u8>> {
        let link = self.links.get_mut(&peer)?;
        if frame.len() < HEADER + CHACHA20_POLY1305.tag_len() {
            return None;
        }
        let tag = frame[2];
        let slot = usize::from(tag);
        if slot >= 3 {
            return None;
        }
        let counter = u64::from_be_bytes(frame[3..HEADER].try_into().ok()?);
        if counter <= link.taken[slot] {
            return None;
        }
        let mut body = frame[HEADER..].to_vec();
        let plain = link
            .open
            .open_in_place(
                nonce_for(tag, counter),
                Aad::from(&frame[..HEADER]),
                &mut body,
            )
            .ok()?
            .len();
        link.taken[slot] = counter;
        body.truncate(plain);
        Some(body)
    }
}

impl Transport for Sealed {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        let Some(link) = self.links.get_mut(&peer) else {
            self.inner.send(peer, channel, bytes);
            return;
        };
        let tag = channel_tag(channel);
        let slot = usize::from(tag);
        link.sent[slot] += 1;
        let counter = link.sent[slot];
        let mut frame = Vec::with_capacity(HEADER + bytes.len() + 16);
        frame.extend_from_slice(&SEALED);
        frame.push(tag);
        frame.extend_from_slice(&counter.to_be_bytes());
        let mut body = bytes.to_vec();
        link.seal
            .seal_in_place_append_tag(nonce_for(tag, counter), Aad::from(&frame[..]), &mut body)
            .expect("a frame this size always seals");
        frame.extend_from_slice(&body);
        self.sealed_out += 1;
        self.inner.send(peer, channel, &frame);
    }

    fn poll(&mut self) -> Vec<Incoming> {
        let mut out = Vec::new();
        for incoming in self.inner.poll() {
            match incoming {
                Incoming::Message(peer, channel, bytes) => {
                    if bytes.starts_with(&SEALED) {
                        match self.open(peer, &bytes) {
                            Some(plain) => {
                                self.opened_in += 1;
                                out.push(Incoming::Message(peer, channel, plain));
                            }
                            None => self.dropped_in += 1,
                        }
                    } else if !self.links.contains_key(&peer) || self.clear_allowed(&bytes) {
                        out.push(Incoming::Message(peer, channel, bytes));
                    } else {
                        self.dropped_in += 1;
                    }
                }
                Incoming::Disconnected(peer, why) => {
                    self.links.remove(&peer);
                    out.push(Incoming::Disconnected(peer, why));
                }
                other => out.push(other),
            }
        }
        out
    }

    fn stats(&self, peer: PeerId) -> fontelle_net::LinkStats {
        self.inner.stats(peer)
    }

    fn disconnect(&mut self, peer: PeerId) {
        self.links.remove(&peer);
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

    fn set_wake(&mut self, wake: fontelle_net::Wake) {
        self.inner.set_wake(wake);
    }

    fn backlog(&self) -> usize {
        self.inner.backlog()
    }
}
