//! A client's end of a relayed session, over any leg to the relay: QUIC on
//! the desktop, a WebSocket from a page.

// Fontelle's own, not in the engine's copy: the engine reads a clock that
// also runs in a browser (`floptle_core::time`); a studio is never one.
use std::time::Instant;

use crate::relay_wire::{JOIN_RETRY_EVERY, RelayMsg, SeqState, channel_tag, tag_channel};
use crate::transport::{Channel, Incoming, LinkStats, PeerId, SERVER, Transport};

/// A client's end of a relayed session: joins by lobby code; the host appears
/// as [`SERVER`], exactly like a direct connection.
pub struct RelayClient {
    inner: Box<dyn Transport>,
    seq: u64,
    dedup: SeqState,
    /// The relay's last word on a join that has not landed yet — see
    /// [`RelayMsg::Starting`]. Drained by [`Transport::take_join_progress`].
    starting: Option<String>,
    /// The code this client is joining, kept so the join can be asked again
    /// while a server wakes.
    code: String,
    /// When to ask again, set only once the relay has said the server is
    /// starting. `None` for an ordinary join, which is answered immediately and
    /// must never be re-sent.
    retry_at: Option<Instant>,
}

impl RelayClient {
    /// Connect to a relay over QUIC and join lobby `code`. Non-blocking: the
    /// session's `Hello` rides the same ordered stream right behind the
    /// `Join`, so the handshake completes as soon as the relay lets us in
    /// ([`Incoming`] carries a `Disconnected` if it refuses).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn join(relay_addr: &str, code: &str) -> Result<Self, String> {
        Ok(Self::join_over(
            Box::new(crate::quic::QuicClient::connect(relay_addr)?),
            code,
        ))
    }

    /// Join lobby `code` over a leg that is already on its way to the relay.
    /// The leg must deliver in order what it is sent on
    /// [`Channel::Reliable`]: the `Join` goes first and everything else
    /// follows it.
    pub fn join_over(mut inner: Box<dyn Transport>, code: &str) -> Self {
        inner.send(
            SERVER,
            Channel::Reliable,
            &RelayMsg::Join {
                code: code.to_uppercase(),
            }
            .encode(),
        );
        Self {
            inner,
            seq: 0,
            dedup: SeqState::default(),
            starting: None,
            code: code.to_uppercase(),
            retry_at: None,
        }
    }
}

impl Transport for RelayClient {
    // Fontelle's own (F48).
    fn set_wake(&mut self, wake: crate::transport::Wake) {
        self.inner.set_wake(wake);
    }

    fn take_notices(&mut self) -> Vec<String> {
        self.inner.take_notices()
    }

    fn take_join_progress(&mut self) -> Option<String> {
        self.starting.take()
    }

    fn send(&mut self, _peer: PeerId, channel: Channel, bytes: &[u8]) {
        let seq = if channel == Channel::UnreliableSequenced {
            self.seq += 1;
            self.seq
        } else {
            0
        };
        let msg = RelayMsg::ToHost {
            channel: channel_tag(channel),
            seq,
            bytes: bytes.to_vec(),
        };
        let leg = if channel == Channel::Reliable {
            Channel::Reliable
        } else {
            Channel::Unreliable
        };
        self.inner.send(SERVER, leg, &msg.encode());
    }

    fn poll(&mut self) -> Vec<Incoming> {
        if let Some(at) = self.retry_at
            && Instant::now() >= at
        {
            self.retry_at = Some(Instant::now() + JOIN_RETRY_EVERY);
            let code = self.code.clone();
            self.inner
                .send(SERVER, Channel::Reliable, &RelayMsg::Join { code }.encode());
        }
        let mut out = Vec::new();
        for inc in self.inner.poll() {
            match inc {
                Incoming::Message(_, _, bytes) => match RelayMsg::decode(&bytes) {
                    Some(RelayMsg::JoinOk) => {
                        // In. Stop asking.
                        self.retry_at = None;
                        out.push(Incoming::Connected(SERVER));
                    }
                    // The relay told us exactly what was wrong — usually that
                    // the code doesn't match a lobby. Carry it: mistyping the
                    // code is the most common thing that will ever go wrong in
                    // an online session, and it must not arrive at the game
                    // indistinguishable from the host closing their laptop.
                    Some(RelayMsg::Refused { reason }) => {
                        // Never, rather than not yet. Stop asking.
                        self.retry_at = None;
                        out.push(Incoming::refused(SERVER, reason));
                    }
                    // ⚠ Deliberately not an `Incoming` — the link is fine and
                    // nobody is disconnected. A refusal ends the attempt; this
                    // says to keep waiting, so it rides the same side channel
                    // `Notice` uses rather than widening a transport enum whose
                    // every variant means something happened to the connection.
                    Some(RelayMsg::Starting { detail }) => {
                        self.starting = Some(detail);
                        // **Ask again shortly.** The relay answered "not yet",
                        // and nothing will tell us when it becomes "yes" — the
                        // waking server hosts a lobby, it does not know anybody
                        // is waiting. So the joiner polls, and the retry only
                        // ever starts after the relay has said the server is
                        // coming, so an ordinary join is never re-sent.
                        self.retry_at = Some(Instant::now() + JOIN_RETRY_EVERY);
                    }
                    Some(RelayMsg::FromHost {
                        channel,
                        seq,
                        bytes,
                    }) if !self.dedup.stale(SERVER, channel, seq) => {
                        out.push(Incoming::Message(SERVER, tag_channel(channel), bytes));
                    }
                    _ => {}
                },
                // Whatever the leg below knew, if anything.
                Incoming::Disconnected(_, why) => out.push(Incoming::Disconnected(SERVER, why)),
                Incoming::Connected(_) => {}
            }
        }
        out
    }

    fn stats(&self, _peer: PeerId) -> LinkStats {
        self.inner.stats(SERVER)
    }
}
