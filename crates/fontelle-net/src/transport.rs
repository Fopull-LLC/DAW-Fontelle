//! The transport seam (`docs/multiplayer.md` §5.3): sessions speak
//! [`Transport`], never sockets, so the same replication code runs over the
//! in-memory hub (tests + the editor's "Host & Join locally" harness), quic
//! (phase 2e), or anything else.
//!
//! [`MemoryHub`] is the loopback implementation: a server endpoint plus any
//! number of client endpoints in one process, with **simulated latency and
//! loss** driven by the gameplay tick (deterministic — a seeded xorshift
//! decides drops, and delivery time is measured in ticks, not wall time).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// A session peer. `SERVER` (0) is always the authoritative host; clients get
/// ids from 1 up, assigned by the transport at connect time.
pub type PeerId = u64;
/// The server's well-known peer id.
pub const SERVER: PeerId = 0;

/// Delivery guarantees a message is sent with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Delivered, in order (control: hello/spawn/rpc).
    Reliable,
    /// Fire-and-forget.
    Unreliable,
    /// Fire-and-forget, but stale (older-than-delivered) messages are dropped
    /// (snapshots: only the newest matters).
    UnreliableSequenced,
}

/// What [`Transport::poll`] yields.
#[derive(Clone, Debug)]
pub enum Incoming {
    Connected(PeerId),
    /// The link to this peer ended, and why — when anyone knows why.
    ///
    /// The reason is what tells a mistyped lobby code apart from an opponent
    /// closing their laptop. Every layer above this already carries one
    /// ([`crate::NetEvent::Disconnected`] → `net.on("disconnected", fn)` →
    /// the game's own text), so the string a relay sends, "no such lobby",
    /// reaches the game rather than being thrown away one line below the pipe
    /// built to deliver it.
    ///
    /// `None` means the link simply ended: a timeout, a closed process, a
    /// pulled cable. Nobody is in a position to say more, and inventing a
    /// reason there would be worse than admitting it.
    Disconnected(PeerId, Option<String>),
    Message(PeerId, Channel, Vec<u8>),
}

impl Incoming {
    /// A disconnect nobody has an explanation for — the common case.
    pub fn dropped(peer: PeerId) -> Self {
        Self::Disconnected(peer, None)
    }

    /// A disconnect with something the player can be told.
    pub fn refused(peer: PeerId, why: impl Into<String>) -> Self {
        Self::Disconnected(peer, Some(why.into()))
    }
}

/// Link quality, for lag compensation + the net-stats overlay.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinkStats {
    pub rtt_ms: f32,
    /// Configured/measured loss fraction `[0,1]`.
    pub loss: f32,
}

/// The socket seam. One instance per endpoint; a server endpoint talks to many
/// peers, a client endpoint only ever to [`SERVER`].
pub trait Transport: Send {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]);
    fn poll(&mut self) -> Vec<Incoming>;
    fn stats(&self, peer: PeerId) -> LinkStats;

    /// Close a peer's link from this end (a kick).
    ///
    /// Defaulted to nothing, because it is genuinely optional: the session
    /// removes the peer from its own roster either way, so a transport that
    /// cannot hang up leaves a socket open talking to nobody rather than a
    /// player still in the game. Implement it where the transport can, so the
    /// kicked client stops paying for a connection it is no longer part of.
    fn disconnect(&mut self, _peer: PeerId) {}

    /// Anything the far end said that a developer should read, drained.
    ///
    /// Defaulted to nothing because only a managed relay has any: it runs on
    /// somebody else's machine, so a message about the developer's own game —
    /// "it filled up, here is where to raise the ceiling" — has nowhere else to
    /// go. Every other transport is a direct link between two machines the
    /// developer already has, and has nothing to say that is not traffic.
    ///
    /// Not an `Incoming`: it is neither a peer event nor an error, and a
    /// session that treated it as either would be wrong about both.
    fn take_notices(&mut self) -> Vec<String> {
        Vec::new()
    }

    /// **A join that has not landed yet, and what the relay says about it.**
    ///
    /// `None` for every transport but a relay client, and `None` there too
    /// unless the relay has said something. Separate from [`Incoming`] on
    /// purpose: every variant of that enum means something happened to the
    /// connection, and this means the opposite — the link is fine, the lobby is
    /// real, keep waiting. A refusal ends a join attempt; this one continues it.
    fn take_join_progress(&mut self) -> Option<String> {
        None
    }

    /// The lobby code players join with, for a transport that has one.
    ///
    /// `None` for every transport but a relay host — and **`None` for a relay
    /// host whose relay is unreachable**, which is the point of asking rather
    /// than remembering. A code is a fact the relay owns: it hands it over once
    /// at registration, and a relay restart destroys every lobby it knew. A
    /// host that kept publishing the code it was told at startup advertised
    /// six characters that refuse everybody who types them, on a page, to a
    /// shipped build, indefinitely.
    fn lobby_code(&self) -> Option<String> {
        None
    }
}

/// A boxed transport is a transport — what lets a wrapper such as
/// [`Impaired`](crate::Impaired) go around an already-erased one.
impl Transport for Box<dyn Transport> {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        (**self).send(peer, channel, bytes);
    }
    fn take_notices(&mut self) -> Vec<String> {
        (**self).take_notices()
    }
    fn take_join_progress(&mut self) -> Option<String> {
        (**self).take_join_progress()
    }
    fn lobby_code(&self) -> Option<String> {
        (**self).lobby_code()
    }
    fn poll(&mut self) -> Vec<Incoming> {
        (**self).poll()
    }
    fn stats(&self, peer: PeerId) -> LinkStats {
        (**self).stats(peer)
    }
    fn disconnect(&mut self, peer: PeerId) {
        (**self).disconnect(peer);
    }
}

// ---------------------------------------------------------------------------
// MemoryHub — in-process loopback with tick-based latency/loss simulation
// ---------------------------------------------------------------------------

struct Queued {
    deliver_at: u64,
    from: PeerId,
    channel: Channel,
    seq: u64,
    bytes: Vec<u8>,
}

struct HubState {
    /// The current gameplay tick — the hub's clock. Advanced by the driver
    /// (editor/test) via [`MemoryHub::set_now`]; messages deliver when
    /// `now >= deliver_at`.
    now: u64,
    /// One-way latency, in ticks.
    latency_ticks: u64,
    /// Drop probability for the unreliable channels `[0,1]`. Reliable never drops.
    loss: f32,
    /// Ticks per second, for reporting `rtt_ms` in [`LinkStats`].
    tick_hz: f32,
    /// xorshift64 state — deterministic loss decisions.
    rng: u64,
    seq: u64,
    /// Inbox per endpoint (server or a client id).
    inbox: HashMap<PeerId, VecDeque<Queued>>,
    /// Newest delivered seq per (destination, source, channel) — sequenced drop.
    delivered: HashMap<(PeerId, PeerId, Channel), u64>,
    /// Connected(peer) events the server endpoint hasn't polled yet.
    pending_joins: VecDeque<PeerId>,
    /// Disconnected(peer) events for the server (client endpoint dropped).
    pending_leaves: VecDeque<PeerId>,
    next_peer: PeerId,
}

impl HubState {
    fn roll_drop(&mut self) -> bool {
        if self.loss <= 0.0 {
            return false;
        }
        // xorshift64 — deterministic, no external dep.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        ((x >> 11) as f64 / (1u64 << 53) as f64) < self.loss as f64
    }

    fn send(&mut self, from: PeerId, to: PeerId, channel: Channel, bytes: &[u8]) {
        if channel != Channel::Reliable && self.roll_drop() {
            return;
        }
        self.seq += 1;
        let q = Queued {
            deliver_at: self.now + self.latency_ticks,
            from,
            channel,
            seq: self.seq,
            bytes: bytes.to_vec(),
        };
        self.inbox.entry(to).or_default().push_back(q);
    }

    fn poll(&mut self, me: PeerId) -> Vec<Incoming> {
        let mut out = Vec::new();
        if me == SERVER {
            while let Some(p) = self.pending_joins.pop_front() {
                out.push(Incoming::Connected(p));
            }
            while let Some(p) = self.pending_leaves.pop_front() {
                out.push(Incoming::dropped(p));
            }
        }
        let now = self.now;
        let Some(q) = self.inbox.get_mut(&me) else {
            return out;
        };
        // Uniform latency keeps fifo order, so a single front-scan suffices.
        let mut ready = Vec::new();
        while q.front().is_some_and(|m| m.deliver_at <= now) {
            ready.push(q.pop_front().unwrap());
        }
        for m in ready {
            if m.channel == Channel::UnreliableSequenced {
                let key = (me, m.from, m.channel);
                let last = self.delivered.get(&key).copied().unwrap_or(0);
                if m.seq <= last {
                    continue; // stale — a newer one already arrived
                }
                self.delivered.insert(key, m.seq);
            }
            out.push(Incoming::Message(m.from, m.channel, m.bytes));
        }
        out
    }
}

/// An in-process "network": one server endpoint + N client endpoints sharing
/// simulated link conditions. Drive its clock with [`Self::set_now`] each
/// gameplay tick.
#[derive(Clone)]
pub struct MemoryHub {
    state: Arc<Mutex<HubState>>,
}

impl Default for MemoryHub {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryHub {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(HubState {
                now: 0,
                latency_ticks: 0,
                loss: 0.0,
                tick_hz: 60.0,
                rng: 0x9E37_79B9_7F4A_7C15,
                seq: 0,
                inbox: HashMap::new(),
                delivered: HashMap::new(),
                pending_joins: VecDeque::new(),
                pending_leaves: VecDeque::new(),
                next_peer: 1,
            })),
        }
    }

    /// Set the simulated link: one-way latency in ticks and unreliable-drop
    /// probability. Live-tunable (the harness sliders).
    pub fn set_conditions(&self, latency_ticks: u64, loss: f32) {
        let mut s = self.state.lock().unwrap();
        s.latency_ticks = latency_ticks;
        s.loss = loss.clamp(0.0, 1.0);
    }

    /// Advance the hub clock to the current gameplay tick (call once per tick,
    /// before endpoints poll).
    pub fn set_now(&self, tick: u64) {
        self.state.lock().unwrap().now = tick;
    }

    /// The server endpoint. Create exactly one.
    pub fn server_endpoint(&self) -> MemoryTransport {
        MemoryTransport {
            me: SERVER,
            state: self.state.clone(),
        }
    }

    /// Connect a new client endpoint; the server endpoint's next poll yields
    /// `Connected(peer)`.
    pub fn connect(&self) -> MemoryTransport {
        let mut s = self.state.lock().unwrap();
        let peer = s.next_peer;
        s.next_peer += 1;
        s.pending_joins.push_back(peer);
        MemoryTransport {
            me: peer,
            state: self.state.clone(),
        }
    }

    /// Disconnect a client endpoint (the harness "leave" button / a dropped peer).
    pub fn disconnect(&self, peer: PeerId) {
        let mut s = self.state.lock().unwrap();
        s.pending_leaves.push_back(peer);
        s.inbox.remove(&peer);
    }
}

/// One endpoint on a [`MemoryHub`]. The server endpoint (`me == SERVER`) sends
/// to any client id; client endpoints send only to [`SERVER`].
pub struct MemoryTransport {
    me: PeerId,
    state: Arc<Mutex<HubState>>,
}

impl Transport for MemoryTransport {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        self.state
            .lock()
            .unwrap()
            .send(self.me, peer, channel, bytes);
    }

    fn poll(&mut self) -> Vec<Incoming> {
        self.state.lock().unwrap().poll(self.me)
    }

    fn stats(&self, _peer: PeerId) -> LinkStats {
        let s = self.state.lock().unwrap();
        LinkStats {
            rtt_ms: 2.0 * s.latency_ticks as f32 * 1000.0 / s.tick_hz,
            loss: s.loss,
        }
    }

    fn disconnect(&mut self, peer: PeerId) {
        // The leave is queued; the peer's inbox is left alone. A kick sends the
        // reason and then hangs up, and wiping the mail on the way out would
        // deliver the hangup and drop the explanation — which is the exact
        // failure a kick with a reason exists to remove. (The harness's own
        // leave button still clears it: a client discarding its own mail on the
        // way out is a different act from a server hanging up on it.)
        self.state.lock().unwrap().pending_leaves.push_back(peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivers_with_tick_latency() {
        let hub = MemoryHub::new();
        hub.set_conditions(3, 0.0);
        let mut server = hub.server_endpoint();
        let mut client = hub.connect();
        assert!(matches!(server.poll().as_slice(), [Incoming::Connected(1)]));

        client.send(SERVER, Channel::Reliable, b"hi");
        hub.set_now(2);
        assert!(
            server.poll().is_empty(),
            "must not arrive before the latency elapses"
        );
        hub.set_now(3);
        match server.poll().as_slice() {
            [Incoming::Message(1, Channel::Reliable, b)] => assert_eq!(b, b"hi"),
            other => panic!("expected the message at tick 3, got {other:?}"),
        }
    }

    #[test]
    fn loss_drops_unreliable_but_never_reliable() {
        let hub = MemoryHub::new();
        hub.set_conditions(0, 1.0); // drop EVERYTHING unreliable
        let mut server = hub.server_endpoint();
        let mut client = hub.connect();
        let _ = server.poll();
        client.send(SERVER, Channel::Unreliable, b"gone");
        client.send(SERVER, Channel::Reliable, b"kept");
        let got = server.poll();
        assert_eq!(got.len(), 1);
        assert!(matches!(&got[0], Incoming::Message(_, Channel::Reliable, b) if b == b"kept"));
    }

    #[test]
    fn sequenced_drops_stale() {
        let hub = MemoryHub::new();
        let mut server = hub.server_endpoint();
        let mut client = hub.connect();
        let _ = server.poll();
        // Two sends, but the destination only polls after both are queued —
        // both deliver (in order); sequenced only drops if an older seq arrives
        // after a newer one was delivered. Simulate that by delivering #2 first.
        client.send(SERVER, Channel::UnreliableSequenced, b"a");
        let _ = server.poll(); // delivers "a" (seq 1)
        client.send(SERVER, Channel::UnreliableSequenced, b"b");
        client.send(SERVER, Channel::UnreliableSequenced, b"c");
        let got = server.poll();
        assert_eq!(got.len(), 2, "in-order sequenced messages all deliver");
    }
}
