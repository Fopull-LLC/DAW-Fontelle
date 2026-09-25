//! Sending no faster than the relay allows (`docs/collab-plan.md` §8.4, F36).
//!
//! The relay gives one connection 512 KiB a second, and closes a connection
//! that goes over three seconds running. A shared song's samples would go
//! over the moment a joiner asked for them, so the **sender** holds back:
//! [`Paced`] keeps what it is handed and lets it go no faster than its rate —
//! 480 KiB/s in Fontelle, under the relay's budget so the relay never has to
//! act. What is queued goes out as the transport is polled, which a session
//! does once a tick.
//!
//! A host's connection carries everything it sends to every joiner, so the
//! pace is per connection, not per peer: three friends copying a song at once
//! share one budget, which is the arithmetic §8.4 writes down.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::transport::{Channel, Incoming, LinkStats, PeerId, Transport};

/// Fontelle's pace: under the relay's 512 KiB/s per connection.
pub const PACE: u64 = 480 * 1024;

/// A clock the pace is measured against — the real one, or a test's.
pub type Clock = Box<dyn Fn() -> Duration + Send>;

/// A [`Transport`] that sends no more than `rate` bytes in any second.
pub struct Paced<T> {
    inner: T,
    rate: u64,
    clock: Clock,
    waiting: VecDeque<(PeerId, Channel, Vec<u8>)>,
    /// What went out in the last second, and when.
    recent: VecDeque<(Duration, u64)>,
}

impl<T: Transport> Paced<T> {
    pub fn new(inner: T, rate: u64) -> Self {
        let start = Instant::now();
        Self::with_clock(inner, rate, Box::new(move || start.elapsed()))
    }

    pub fn with_clock(inner: T, rate: u64, clock: Clock) -> Self {
        Self {
            inner,
            rate,
            clock,
            waiting: VecDeque::new(),
            recent: VecDeque::new(),
        }
    }

    /// Sends what fits in this second's budget, oldest first.
    fn flush(&mut self) {
        let now = (self.clock)();
        while self
            .recent
            .front()
            .is_some_and(|(at, _)| now.saturating_sub(*at) >= Duration::from_secs(1))
        {
            self.recent.pop_front();
        }
        let mut spent: u64 = self.recent.iter().map(|(_, n)| n).sum();
        while let Some((_, _, bytes)) = self.waiting.front() {
            let size = bytes.len() as u64;
            // A message bigger than a whole second's budget goes on its own,
            // or it would never go at all; framing keeps that from happening.
            if spent + size > self.rate && spent > 0 {
                break;
            }
            let (peer, channel, bytes) = self.waiting.pop_front().expect("just looked");
            self.inner.send(peer, channel, &bytes);
            self.recent.push_back((now, size));
            spent += size;
        }
    }

    /// How much is waiting to go.
    pub fn backlog(&self) -> usize {
        self.waiting.iter().map(|(_, _, b)| b.len()).sum()
    }
}

impl<T: Transport> Transport for Paced<T> {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        self.waiting.push_back((peer, channel, bytes.to_vec()));
        self.flush();
    }

    fn poll(&mut self) -> Vec<Incoming> {
        self.flush();
        let incoming = self.inner.poll();
        // A peer that has gone takes what was waiting for it with it.
        for event in &incoming {
            if let Incoming::Disconnected(peer, _) = event {
                self.waiting.retain(|(p, _, _)| p != peer);
            }
        }
        incoming
    }

    fn stats(&self, peer: PeerId) -> LinkStats {
        self.inner.stats(peer)
    }

    fn disconnect(&mut self, peer: PeerId) {
        // What was queued for it goes first — a kick with a reason.
        self.flush();
        self.inner.disconnect(peer);
    }

    fn take_notices(&mut self) -> Vec<String> {
        self.inner.take_notices()
    }

    fn take_join_progress(&mut self) -> Option<String> {
        self.inner.take_join_progress()
    }

    fn set_wake(&mut self, wake: crate::transport::Wake) {
        self.inner.set_wake(wake);
    }

    fn lobby_code(&self) -> Option<String> {
        self.inner.lobby_code()
    }
}
