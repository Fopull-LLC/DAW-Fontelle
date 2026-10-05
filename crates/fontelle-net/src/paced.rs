//! Sending no faster than the relay allows (`docs/collab-plan.md` §8.4, F36).
//!
//! The relay gives one connection a byte budget a second — 512 KiB on any
//! relay, 2 MiB for Fontelle's key on Floptle Cloud (hub card 0264) — and a
//! message that goes over it is dropped, a message lost even on the reliable
//! channel; three seconds running over and the connection is closed. A shared
//! song's samples would go over the moment a joiner asked for them, so the
//! **sender** holds back: [`Paced`] keeps what it is handed and lets it go no
//! faster than its rate, well under the relay's budget so the relay never has
//! to act. What is queued goes out as the transport is polled, which a
//! session does once a tick.
//!
//! **Two rules, both kept.** No second carries more than the rate — the
//! relay's own measure. And what goes out goes in small helpings
//! ([`pace_burst`]), refilled as time passes, rather than a whole second's
//! budget the moment it is free: a second's worth handed to the network at
//! once reached the relay inside one of *its* seconds together with the
//! start of the next helping whenever the two drifted by a few milliseconds,
//! and went over. Spread out, a relay second sees at most the rate, one
//! helping, and whatever a delay on the way bunched up — the margin each
//! relay's pace leaves.
//!
//! A host's connection carries everything it sends to every joiner, so the
//! pace is per connection, not per peer: three friends copying a song at once
//! share one budget, which is the arithmetic §8.4 writes down.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::transport::{Channel, Incoming, LinkStats, PeerId, Transport};

/// Fontelle's pace through a relay that allows 512 KiB a second per
/// connection — any relay but Floptle Cloud: three quarters of it. (It was
/// 480 KiB, sent a second at a time; with a helping and a whole frame on top
/// of the pace that left nothing for a delay on the way.)
pub const PACE: u64 = 384 * 1024;

/// Fontelle's pace through Floptle Cloud, which grants Fontelle's key 2 MiB a
/// second per connection (hub card 0264, relay 0.107 and later): thirteen
/// sixteenths of the grant, three and a half times what the pace was before
/// the grant.
pub const CLOUD_PACE: u64 = 1664 * 1024;

/// What goes out at once at `rate`, give or take one message: a tenth of a
/// second's worth, which is how often the window looks while a session is
/// open — so the pace is kept up, and nothing much bigger is ever handed over
/// in one go.
pub const fn pace_burst(rate: u64) -> u64 {
    rate / 10
}

/// A clock the pace is measured against — the real one, or a test's.
pub type Clock = Box<dyn Fn() -> Duration + Send>;

/// A [`Transport`] that sends no more than `rate` bytes in any second, in
/// helpings of at most [`pace_burst`].
pub struct Paced<T> {
    inner: T,
    rate: u64,
    clock: Clock,
    waiting: VecDeque<(PeerId, Channel, Vec<u8>)>,
    /// What went out in the last second, and when.
    recent: VecDeque<(Duration, u64)>,
    /// What may go now, refilled at `rate` up to a helping, and when it was
    /// last refilled. Below nought after a message bigger than a helping.
    allowance: f64,
    refilled: Duration,
}

impl<T: Transport> Paced<T> {
    pub fn new(inner: T, rate: u64) -> Self {
        let start = Instant::now();
        Self::with_clock(inner, rate, Box::new(move || start.elapsed()))
    }

    pub fn with_clock(inner: T, rate: u64, clock: Clock) -> Self {
        let now = clock();
        Self {
            inner,
            rate,
            clock,
            waiting: VecDeque::new(),
            recent: VecDeque::new(),
            allowance: pace_burst(rate) as f64,
            refilled: now,
        }
    }

    /// The transport under the pace.
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Sends what fits in this second's budget and this helping, oldest
    /// first.
    fn flush(&mut self) {
        let now = (self.clock)();
        let since = now.saturating_sub(self.refilled).as_secs_f64();
        self.allowance =
            (self.allowance + since * self.rate as f64).min(pace_burst(self.rate) as f64);
        self.refilled = now;
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
            // A message bigger than a whole second's budget, or a whole
            // helping, goes on its own, or it would never go at all; framing
            // keeps that from happening.
            if spent + size > self.rate && spent > 0 {
                break;
            }
            // Nothing left of this helping: wait for the next. A message
            // may take the helping below nought — the next waits the longer
            // — so the pace is kept whatever the size of what is sent.
            if self.allowance <= 0.0 {
                break;
            }
            let (peer, channel, bytes) = self.waiting.pop_front().expect("just looked");
            self.inner.send(peer, channel, &bytes);
            self.recent.push_back((now, size));
            self.allowance -= size as f64;
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
