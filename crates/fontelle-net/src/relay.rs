//! The rendezvous relay: hosts register and get a lobby code, clients join
//! with the code, and the relay forwards opaque session traffic both ways.
//! Nobody port-forwards; the only reachable address anyone needs is the
//! relay's. This is the open, self-hostable reference implementation; Floptle
//! Cloud runs the managed one.
//!
//! Everything rides the quic transport: an endpoint's leg to the relay is an
//! ordinary [`QuicClient`], the relay itself an ordinary [`QuicServer`].
//! Control and reliable game traffic go on the framed stream, ordered, so a
//! `Join` is always processed before the session's `Hello` that follows it;
//! unreliable game traffic goes as datagrams.
//!
//! Sequenced drops are end-to-end: the sender stamps a `seq` inside the
//! relayed message and the final receiver drops stale ones per
//! `(peer, channel)`. The legs carry unreliable datagrams without per-leg
//! dedup, so interleaved traffic for different peers never false-drops.
//!
//! [`RelayServer`] knows lobbies, peer ids and forwarding, and nothing else:
//! a session over a relay is the same bytes as a direct one.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::quic::{QuicClient, QuicServer, ServerCertificate};
use crate::transport::{Channel, Incoming, LinkStats, PeerId, SERVER, Transport};

/// Wire channel tags inside relay messages.
const CH_RELIABLE: u8 = 0;
const CH_UNRELIABLE: u8 = 1;
const CH_SEQUENCED: u8 = 2;

fn channel_tag(c: Channel) -> u8 {
    match c {
        Channel::Reliable => CH_RELIABLE,
        Channel::Unreliable => CH_UNRELIABLE,
        Channel::UnreliableSequenced => CH_SEQUENCED,
    }
}

fn tag_channel(t: u8) -> Channel {
    match t {
        CH_RELIABLE => Channel::Reliable,
        CH_SEQUENCED => Channel::UnreliableSequenced,
        _ => Channel::Unreliable,
    }
}

/// Everything that crosses a relay leg.
#[derive(Clone, Debug, Serialize, Deserialize)]
enum RelayMsg {
    /// Endpoint → relay: host a new lobby, presenting nothing.
    ///
    /// **Kept as a unit variant on purpose.** A managed relay refuses this with
    /// a sentence telling the developer to connect their project, and that
    /// message is far more use than the silence a re-shaped variant would
    /// produce: postcard indexes variants by declaration order, so widening
    /// `Host` would make every game already in the wild fail to decode here and
    /// hang with nothing said. [`RelayMsg::HostKeyed`] is appended at the end
    /// instead, which old builds never send and new ones only send when the
    /// project actually carries a key.
    Host,
    /// Endpoint → relay: join a lobby by code.
    Join {
        code: String,
    },
    /// Relay → host: your lobby is live.
    Hosted {
        code: String,
    },
    /// Relay → client: you're in.
    JoinOk,
    /// Relay → endpoint: no.
    Refused {
        reason: String,
    },
    /// Relay → host: a client attached / detached (its game peer id).
    PeerJoined {
        peer: u64,
    },
    PeerLeft {
        peer: u64,
    },
    /// Host → relay: deliver to one client. `seq` is the end-to-end
    /// sequenced-drop stamp (0 on non-sequenced channels).
    ToPeer {
        peer: u64,
        channel: u8,
        seq: u64,
        bytes: Vec<u8>,
    },
    /// Relay → host: a client's traffic.
    FromPeer {
        peer: u64,
        channel: u8,
        seq: u64,
        bytes: Vec<u8>,
    },
    /// Client → relay: deliver to the host.
    ToHost {
        channel: u8,
        seq: u64,
        bytes: Vec<u8>,
    },
    /// Relay → client: the host's traffic.
    FromHost {
        channel: u8,
        seq: u64,
        bytes: Vec<u8>,
    },
    /// Endpoint → relay: host a lobby **as a registered game**, presenting the
    /// game key from `project.ron` and the build hash if there is one.
    ///
    /// **Appended last, and that placement is the compatibility story.**
    /// Postcard numbers enum variants by declaration order, so a new relay
    /// decodes every message an old build sends exactly as before, and this one
    /// is simply a variant old relays have never heard of. The other direction
    /// — a new build meeting an old relay — is what
    /// [`RelayHost::host_keyed`]'s fallback is for.
    HostKeyed {
        key: String,
        build: Option<String>,
    },
    /// Relay → host: something the developer should hear, once per episode.
    ///
    /// **Not a refusal and not an error** — the session is fine and nobody was
    /// disconnected. It exists because a managed relay runs on somebody else's
    /// machine, so the only way a developer learns that their game filled up is
    /// if the relay tells their process.
    ///
    /// Appended after `HostKeyed` for the same compatibility reason that one
    /// was: postcard numbers variants by declaration order, so an older host
    /// simply fails to decode this and skips it, exactly as it would any
    /// message it has never heard of.
    Notice {
        text: String,
    },
    /// Endpoint → relay: **this host is a dedicated server, not a player.**
    ///
    /// Sent immediately after the host request, on the same connection. The
    /// relay counts a lobby's occupancy as its clients plus its host, which is
    /// right for a listen host — that person is playing — and wrong for a box
    /// nobody is sitting at: an idle dedicated server reported one concurrent
    /// player, showed "1 in this game right now" on its developer's page, and
    /// consumed one of the account's ceiling forever.
    ///
    /// A separate marker rather than a field on [`RelayMsg::HostKeyed`],
    /// because widening a shipped variant changes its encoding and every host
    /// already in the wild would fail to decode — and rather than a new host
    /// variant, because a dedicated server may host keyless on a self-hosted
    /// relay too. Appended last, for the reason every variant above it says.
    HostIsDedicated,
    /// Relay → client: the lobby exists, and its server is waking up.
    ///
    /// Not a refusal. [`RelayMsg::Refused`] means this will never succeed; a
    /// dedicated server slept to free its machine is "not yet", and a player
    /// holding a good code must not be told their friend's game does not
    /// exist.
    ///
    /// `detail` is words for a human, not a status noun: "about 20 seconds"
    /// renders as a sentence, where "starting" makes every developer invent
    /// one.
    ///
    /// Appended last. A build already in players' hands decodes this variant
    /// to nothing and stays in `connecting`, showing the spinner it already
    /// draws; a new build reads the state and says something better. No
    /// version negotiation, no flag.
    Starting {
        detail: String,
    },
    /// Endpoint → relay: reclaim this lobby code rather than minting one.
    ///
    /// Sent immediately after the host request, on the same connection, the
    /// same shape as [`RelayMsg::HostIsDedicated`]: widening a shipped variant
    /// changes its encoding, and every host in the wild would fail to decode.
    ///
    /// A request, never an instruction. The relay hands the code over only
    /// when its policy says this key owns it, so a host that asks for somebody
    /// else's code is minted a fresh one. The game key is the proof of
    /// ownership and is already validated at registration; the worst a liar
    /// achieves is the code they would have got anyway.
    ///
    /// This is what makes six characters survive a restart, a sleep and a
    /// relay upgrade: a managed server brings its code with it.
    WantCode {
        code: String,
    },
}

impl RelayMsg {
    fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("relay messages always encode")
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}

/// What a relay decides about a request to host.
#[derive(Clone, Debug, PartialEq)]
pub enum HostAdmission {
    /// Allow. `prefix` is prepended to the lobby code — a managed relay puts
    /// its region letter there so a client can map a code back to a relay
    /// without asking anybody. `None` keeps the self-hosted 5-character code.
    Allow { prefix: Option<char> },
    /// Refuse, with a sentence the player reads **verbatim**. It reaches them
    /// through `Refused { reason }` → the F1 menu and `net.on("refused")`, so
    /// it is product copy, not a diagnostic.
    Refuse { reason: String },
    /// **Not yet — ask me again next step.** The relay parks the host and
    /// re-asks until the policy decides or [`HOST_DECISION_DEADLINE`] passes.
    ///
    /// This exists so that a policy which has to consult something slow does
    /// not get to stop the relay to do it. A managed relay answers almost every
    /// host from a snapshot it already holds, but a key minted in the last
    /// thirty seconds is not in that snapshot yet and has to be asked about —
    /// and a relay that blocked for even a second on that would freeze the
    /// traffic of **every other lobby on the box**, which for a game is a
    /// disconnect. So the policy starts its lookup, says `Pending`, and answers
    /// on a later step.
    Pending,
}

/// How long a lobby outlives its host's connection.
///
/// A host that drops for a moment (a NAT rebinding a UDP mapping, a Wi-Fi
/// roam, a burst of loss) must not end everybody's match. Without this the
/// joiner's socket stays open, sending into a lobby that no longer exists,
/// and everything network-owned ages out of its world while the skybox and
/// the HUD stay put. So the lobby is kept, its players are held, and the host
/// reclaims it on reconnect. `RelayHost` retries with backoff starting at one
/// second, so this covers several attempts and far less than a player's
/// patience.
///
/// A grace window, not a lease. Past it the lobby is destroyed and the
/// clients are told: a host that is really gone must not hold a code and a
/// room full of people indefinitely.
pub const HOST_GRACE: Duration = Duration::from_secs(20);

/// Why a lobby ended, for the operator's journal.
///
/// A bare count — "lobbies: 1" — cannot say which lobby died or what killed
/// it, and a teardown mid-match then takes a byte-level diff of two counters
/// to find at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LobbyEnd {
    /// The host's connection went, and it did not come back inside
    /// [`HOST_GRACE`].
    HostGone,
    /// The host closed it or the process ended cleanly.
    HostLeft,
    /// Nobody was in it for [`RelayLimits::idle_lobby`], and the host is not
    /// a dedicated server.
    Idle,
}

impl LobbyEnd {
    pub fn as_str(self) -> &'static str {
        match self {
            LobbyEnd::HostGone => "the host's connection dropped and did not return",
            LobbyEnd::HostLeft => "the host closed it",
            LobbyEnd::Idle => "nobody joined it for half an hour",
        }
    }
}

/// **How often a joiner asks again while a server wakes**.
///
/// A waking server hosts a lobby; it has no idea anybody is queued for it, so
/// nothing pushes the good news and the joiner has to ask. Two seconds is
/// frequent enough that the wait feels like a wait rather than a hang, and
/// sparse enough that a lobby's worth of friends retrying costs the relay
/// nothing next to the traffic it forwards.
pub const JOIN_RETRY_EVERY: Duration = Duration::from_secs(2);

/// **What a host is told by a relay that has just restarted** and has not yet
/// learned which codes are spoken for.
///
/// It is a wait, not a rejection, and the sentence says so — the relay pulls a
/// snapshot every thirty seconds, so trying again shortly works. Product copy:
/// the person reading it is a developer whose server came up during a relay
/// restart, and "not ready" with no advice is the kind of message that
/// generates a support thread.
pub const NOT_READY_YET: &str = "This relay is still starting up. Try again in a few seconds.";

/// How long a host may sit parked on [`HostAdmission::Pending`] before the
/// relay gives up and refuses. Comfortably longer than the cold-path lookup it
/// exists for, and short enough that a player is not left staring at nothing.
pub const HOST_DECISION_DEADLINE: Duration = Duration::from_secs(5);

/// **What one connection, one address and one lobby may do**, on any relay —
/// the open one included. A relay multiplies traffic (one datagram into an
/// eight-player lobby leaves seven times), so it is the cheapest thing on the
/// box to take down, and a leaked game key makes it the cheapest way to fill
/// a developer's player cap with phantom lobbies. None of these is a plan
/// limit; they are the shape of a relay that is being used as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelayLimits {
    /// Lobbies open at once, across every key.
    pub max_lobbies: usize,
    /// Clients in one lobby. A managed policy's player cap applies as well
    /// and is the smaller number; this is the ceiling the open relay has.
    pub max_clients_per_lobby: usize,
    /// Payload bytes one connection may send in one [`Self::window`].
    pub bytes_per_window: u64,
    /// Messages one connection may send in one [`Self::window`].
    pub msgs_per_window: u32,
    /// The accounting window for the two above.
    pub window: Duration,
    /// Over budget this many windows in a row and the connection is closed.
    pub strikes: u32,
    /// Lobbies one address may open per [`Self::rate_window`].
    pub opens_per_address: u32,
    /// Joins one address may make per [`Self::rate_window`].
    pub joins_per_address: u32,
    pub rate_window: Duration,
    /// A lobby whose host is not a dedicated server and that has had nobody
    /// in it for this long is ended.
    pub idle_lobby: Duration,
}

impl Default for RelayLimits {
    fn default() -> Self {
        Self {
            max_lobbies: 4096,
            max_clients_per_lobby: 64,
            bytes_per_window: 512 * 1024,
            msgs_per_window: 4000,
            window: Duration::from_secs(1),
            strikes: 3,
            opens_per_address: 10,
            joins_per_address: 30,
            rate_window: Duration::from_secs(60),
            idle_lobby: Duration::from_secs(30 * 60),
        }
    }
}

/// The largest reliable payload a host may push through the relay to one peer.
///
/// Measured before it was set: the largest reliable message a host sends is a
/// `Spawn` carrying a prefab's RON, and the largest prefab in any shipped game
/// is Solar's `FacHangar` at 28 248 bytes (Forgery's `Survivor` is 18 400).
/// Four times that, rounded to a power of two.
pub const MAX_HOST_RELIABLE: usize = 128 * 1024;
/// The largest reliable payload a client may push through the relay to a host.
///
/// A client's reliable messages are `Hello`, `Input` and `Rpc`, and an RPC's
/// value is capped at 1 KB by the wire's own rule; a voice packet is 400
/// bytes. Sixty-four times the largest of those, which is still a quarter of
/// what the transport's frame decoder would otherwise let a modified client
/// hand a host to decode.
pub const MAX_CLIENT_RELIABLE: usize = 64 * 1024;

/// What a relay decides about a request to join an existing lobby.
#[derive(Clone, Debug, PartialEq)]
pub enum JoinAdmission {
    Allow,
    Refuse {
        reason: String,
    },
    /// **The lobby is real and its server is waking up.** Hold the joiner; do
    /// not refuse.
    ///
    /// ⚠ `Refuse` means *this will never succeed* — that is its whole purpose,
    /// the distinction between "not yet" and "never" that elapsed time cannot
    /// draw. A managed deployment that was slept to free its box is "not yet",
    /// and a code that survives a sleep only to be refused on use is the same
    /// broken experience with better bookkeeping.
    Starting {
        detail: String,
    },
}

/// The relay's admission policy — who may host, who may join, and what the
/// lobby codes look like.
///
/// **`floptle-net` never makes a network call, and this trait is why.** A
/// managed relay decides from a key snapshot it pulls on its own schedule; a
/// self-hosted relay has no policy at all and is byte-identical to the day
/// this file was written. Keeping the decision behind a trait is what lets
/// both of those be true at once, and it is what makes the managed rules
/// testable without a network: the guards below drive a policy that answers
/// from a table.
///
/// The bookkeeping hooks default to doing nothing, so a policy that only cares
/// about admission implements one method.
pub trait RelayPolicy: Send {
    /// A host is asking for a lobby. `key` is `None` when the endpoint sent the
    /// keyless [`RelayMsg::Host`] — which a managed relay must refuse and a
    /// self-hosted one must not care about.
    fn admit_host(&mut self, key: Option<&str>, build: Option<&str>) -> HostAdmission;

    /// A client is joining `code`. This is where a CCU cap bites, and it must
    /// bite here rather than on the host: **a live session is never broken for
    /// a cap**, so the only thing a limit can do is refuse the next arrival.
    fn admit_join(&mut self, _code: &str) -> JoinAdmission {
        JoinAdmission::Allow
    }

    /// **May this key reclaim this code?**
    ///
    /// Answered from the reservations the policy holds. `false` by default, so
    /// a self-hosted relay mints exactly as it always has and a host asking for
    /// a code on one is quietly given a fresh one instead.
    fn claim_code(&mut self, _key: Option<&str>, _code: &str) -> bool {
        false
    }

    /// **Is this code spoken for by somebody who is not here right now?**
    ///
    /// A reserved code belongs to a deployment that may be asleep, restarting,
    /// or between relays — there is no lobby holding it, and minting it for a
    /// stranger would point one memorised code at two different games. The
    /// relay skips it and draws again.
    fn code_is_reserved(&self, _code: &str) -> bool {
        false
    }

    /// The relay's own limits refused or dropped a message — see
    /// [`RelayLimits`]. Counted, so a box being leaned on is visible on the
    /// usage report rather than only in a quieter graph.
    fn dropped_by_limit(&mut self) {}

    /// **May the relay invent a code at all yet?**
    ///
    /// ⚠ `false` until this policy has pulled its first snapshot. A relay that
    /// has just restarted knows no reservations, so every code it mints is one
    /// it cannot know is already promised — and it would hand somebody else's
    /// six characters to a stranger. `true` by default: a self-hosted relay has
    /// no snapshot to wait for and must never be made to wait for one.
    fn may_mint(&self) -> bool {
        true
    }

    /// A lobby opened, under the key that was admitted, from the host's
    /// address. The policy owns the code → key mapping; the relay does not
    /// know what a key means. The address is what lets a developer be told
    /// "your key is hosting from somewhere you are not";
    /// `None` only when the transport cannot say.
    fn lobby_opened(&mut self, _code: &str, _key: Option<&str>, _from: Option<std::net::IpAddr>) {}
    fn lobby_closed(&mut self, _code: &str) {}

    /// **A lobby's host dropped and its grace window has started**.
    /// The lobby is still alive and its players are held.
    fn lobby_host_lost(&mut self, _code: &str) {}

    /// **A lobby's host came back inside its grace window** and reclaimed it,
    /// with its players still attached.
    /// The host is back — possibly from a different address than it left.
    fn lobby_host_returned(&mut self, _code: &str, _from: Option<std::net::IpAddr>) {}

    /// **A lobby ended, and why.**
    ///
    /// Named, with the reason: a bare lobby count cannot say which lobby died
    /// or what killed it, and a mid-match teardown then takes a byte-level
    /// diff of two counters that are supposed to be equal before anybody
    /// notices it happened at all.
    fn lobby_ended(&mut self, _code: &str, _why: LobbyEnd) {}

    /// **Payload that arrived for a lobby that does not exist**.
    ///
    /// These bytes were received and never forwarded. Uncounted, they show up
    /// only as `bytes_in` exceeding `bytes_out`, and a teardown is found by
    /// differencing two numbers that match to the byte in every other bucket.
    /// Counted, so nobody has to notice it that way.
    fn orphaned(&mut self, _code: &str, _bytes: u64) {}
    fn peer_joined(&mut self, _code: &str) {}
    fn peer_left(&mut self, _code: &str) {}

    /// Messages the relay should push to a lobby's **host**, drained each step.
    ///
    /// **The developer never reads the relay's journal.** A managed relay runs
    /// on Fopull's box; `say()` reaches the operator there, which is the wrong
    /// person entirely for "your game filled up". This is the seam that carries
    /// such a message back to the process that opened the lobby, so it lands
    /// where a developer actually looks — the editor console, a dedicated
    /// server's stdout, or their game's own UI.
    ///
    /// `(lobby code, text)`. Drained rather than pushed because the policy has
    /// no idea which connection a code belongs to; the relay does.
    fn take_host_notices(&mut self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// Payload forwarded for a lobby: what arrived, and what went on.
    ///
    /// **The two numbers are not the same number**. A datagram
    /// for a peer that has just left is received and never forwarded, so a
    /// divergence between them is a real signal rather than rounding — and
    /// egress is the half a region is billed for, which is the reason the
    /// control plane wants them apart.
    ///
    /// Defaulted to nothing, so the open self-hosted relay — which meters
    /// nobody and reports to no one — is unaffected by its existence.
    fn forwarded(&mut self, _code: &str, _bytes_in: u64, _bytes_out: u64) {}

    /// This lobby's host is a dedicated server rather than somebody playing.
    ///
    /// Occupancy counts clients plus the host, which is correct for a listen
    /// host and an off-by-one for a box nobody is sitting at.
    /// Defaulted to nothing: a self-hosted relay meters no one and has no use
    /// for the distinction.
    fn host_is_dedicated(&mut self, _code: &str) {}

    /// Called on every [`RelayServer::step`], so a policy can refresh its
    /// snapshot or flush a usage batch without owning a thread of its own.
    fn tick(&mut self) {}
}

/// How long a keyed host waits before trying the keyless message as well, in
/// 5 ms polls — see [`RelayHost::host_keyed`]. Long enough that a managed
/// relay has always answered, short enough that an older self-hosted relay
/// still feels instant.
const OLD_RELAY_FALLBACK_POLLS: usize = 300;

/// Lobby codes: 5 characters from an unambiguous alphabet (no 0/O, 1/I).
///
/// A managed relay prefixes its region letter, making the code six — see
/// [`HostAdmission::Allow`]. The prefix is **not** drawn from the alphabet
/// below; it is an operator-allocated letter and the control plane owns the
/// registry.
fn lobby_code(rng: &mut u64, prefix: Option<char>) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    prefix
        .into_iter()
        .chain((0..5).map(|_| {
            let mut x = *rng;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *rng = x;
            ALPHABET[(x >> 33) as usize % ALPHABET.len()] as char
        }))
        .collect()
}

// ---------------------------------------------------------------------------
// The relay server
// ---------------------------------------------------------------------------

/// What a relay connection currently is.
enum Role {
    Fresh,
    Host { code: String },
    Client { code: String, game_peer: u64 },
}

struct Lobby {
    /// The host's relay connection.
    host: PeerId,
    /// game peer id → the client's relay connection.
    clients: HashMap<u64, PeerId>,
    next_peer: u64,
    /// **When this lobby's host vanished**, if it has.
    ///
    /// `None` is the ordinary case: a host is connected and the lobby is live.
    /// `Some` starts a grace window during which the lobby is kept alive and
    /// its players are held, so a host whose connection blipped can come back
    /// to the match it was already in.
    host_lost_at: Option<Instant>,
    /// Since when the lobby has had no clients — for [`RelayLimits::idle_lobby`].
    ///
    /// ⚠ Restarted when a host reclaims the lobby: the
    /// dedicated marker is per connection, and a server that restarts arrives
    /// as a new connection whose marker has not landed yet — so for one sweep
    /// a reclaimed lobby is a player's, and if this clock still says "empty
    /// since morning" the reaper ends it in the same second the reclaim
    /// restored it. A fresh window from the reclaim gives the marker its
    /// moment, and gives a player's reclaimed lobby what a new one gets.
    empty_since: Instant,
}

/// The relay: step it forever (the `floptle-relay` binary) or from a test
/// thread. One instance serves many lobbies.
pub struct RelayServer {
    transport: QuicServer,
    conns: HashMap<PeerId, Role>,
    lobbies: HashMap<String, Lobby>,
    rng: u64,
    port: u16,
    /// The admission policy, or `None` for the open self-hostable relay —
    /// which is the default, and which behaves exactly as it did before
    /// managed mode existed. Every managed rule lives behind this.
    policy: Option<Box<dyn RelayPolicy>>,
    /// Hosts whose policy said [`HostAdmission::Pending`], with the moment they
    /// asked. Re-asked every step; refused at [`HOST_DECISION_DEADLINE`].
    parked: Vec<ParkedHost>,
    /// Connections that declared themselves dedicated servers
    /// ([`RelayMsg::HostIsDedicated`]). Held per connection rather than per
    /// lobby because the marker can arrive while the host is still parked, so
    /// there is not yet a code to file it under.
    dedicated: HashSet<PeerId>,
    /// Codes hosts have asked to reclaim, until their lobby opens.
    /// Keyed by connection, like `dedicated`, because the
    /// marker can arrive while a keyed host is parked on a policy decision.
    wanted: HashMap<PeerId, String>,
    /// How long a lobby outlives its host's connection. [`HOST_GRACE`] in
    /// production; shortened by tests that would otherwise sleep for it.
    grace: Duration,
    /// See [`RelayLimits`].
    limits: RelayLimits,
    /// Per-connection ingress accounting for the byte and message budgets.
    ingress: HashMap<PeerId, Ingress>,
    /// Per-address lobby-open and join timestamps inside the rate window.
    address_rates: HashMap<std::net::IpAddr, AddressRate>,
    /// Messages the limits refused, cumulative. Not a proxy for anything: each
    /// one is a message a connection sent that the relay chose not to carry.
    limit_drops: u64,
}

/// One connection's spend inside the current window.
struct Ingress {
    window_start: Instant,
    bytes: u64,
    msgs: u32,
    /// Windows in a row that went over budget.
    strikes: u32,
}

#[derive(Default)]
struct AddressRate {
    opens: Vec<Instant>,
    joins: Vec<Instant>,
}

/// A host waiting on a policy that has not decided yet.
struct ParkedHost {
    conn: PeerId,
    key: Option<String>,
    build: Option<String>,
    since: Instant,
}

/// **What a relay asks the kernel for on its UDP socket**, receive and send.
/// 8 MiB: the box has 954 MB and the relay peaks at 25 MB,
/// and the kernel default (212,992 B) overflowed at ~100 CCU while the link
/// sat under 1% used. What the kernel actually grants is on
/// [`RelayServer::socket_buffers`]; the binary prints it, because the ask is
/// silently clamped to `net.core.rmem_max` and only the grant is a fact.
pub const RELAY_SOCKET_BUFFER: usize = 8 << 20;

impl RelayServer {
    /// Bind on `0.0.0.0:port` (0 = ephemeral; see [`Self::port`]) presenting
    /// the dev self-signed certificate.
    pub fn bind(port: u16) -> Result<Self, String> {
        Self::bind_with_certificate(port, &ServerCertificate::self_signed()?)
    }

    /// [`Self::bind`] presenting a certificate a client can verify — the
    /// managed relay's, issued for its region name.
    pub fn bind_with_certificate(port: u16, cert: &ServerCertificate) -> Result<Self, String> {
        Self::with_transport(QuicServer::bind_sized(
            port,
            cert,
            Some(RELAY_SOCKET_BUFFER),
        )?)
    }

    /// The socket buffers the kernel granted against [`RELAY_SOCKET_BUFFER`].
    /// Always `Some` for a relay that came up through [`Self::bind`].
    pub fn socket_buffers(&self) -> Option<crate::quic::SocketBuffers> {
        self.transport.socket_buffers()
    }

    /// Present a renewed certificate from now on. Every lobby stays up: only
    /// handshakes from here take the new chain (see
    /// [`QuicServer::set_certificate`]).
    pub fn set_certificate(&self, cert: &ServerCertificate) -> Result<(), String> {
        self.transport.set_certificate(cert)
    }

    fn with_transport(transport: QuicServer) -> Result<Self, String> {
        let port = transport.local_port();
        let seed = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x5EED)
            | 1;
        Ok(Self {
            transport,
            conns: HashMap::new(),
            lobbies: HashMap::new(),
            rng: seed,
            port,
            policy: None,
            parked: Vec::new(),
            dedicated: HashSet::new(),
            wanted: HashMap::new(),
            grace: HOST_GRACE,
            limits: RelayLimits::default(),
            ingress: HashMap::new(),
            address_rates: HashMap::new(),
            limit_drops: 0,
        })
    }

    /// Replace the limits — for an operator flag, or a test that trips one
    /// without a thousand connections.
    pub fn set_limits(&mut self, limits: RelayLimits) {
        self.limits = limits;
    }

    pub fn limits(&self) -> RelayLimits {
        self.limits
    }

    /// How many messages the limits have refused since the relay started.
    pub fn limit_drops(&self) -> u64 {
        self.limit_drops
    }

    /// Run under an admission policy — Floptle Cloud's managed mode.
    ///
    /// Without this the relay is the open one: no keys, no
    /// control plane, nothing to authorize against. That is not a fallback, it
    /// is the product — a self-hosted relay must keep working exactly as it
    /// does today, and there is a guard that says so.
    pub fn set_policy(&mut self, policy: Box<dyn RelayPolicy>) {
        self.policy = Some(policy);
    }

    /// The actually-bound UDP port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Live lobby count (diagnostics).
    /// Shorten the window a lobby outlives its host, for tests that must not
    /// sleep for [`HOST_GRACE`].
    #[cfg(test)]
    pub(crate) fn set_grace(&mut self, g: Duration) {
        self.grace = g;
    }

    pub fn lobby_count(&self) -> usize {
        self.lobbies.len()
    }

    /// Process everything that arrived; returns how many messages moved.
    /// Drive it in a loop with a short sleep (the reference binary does 1 ms).
    pub fn step(&mut self) -> usize {
        // Before the traffic: a managed policy refreshes its key snapshot and
        // flushes its usage batch from here, so it never needs a thread of its
        // own and can never be halfway through a refresh while a host is being
        // admitted.
        if let Some(p) = self.policy.as_mut() {
            p.tick();
        }
        self.sweep_lost_hosts();
        self.sweep_idle_lobbies();
        // **The policy has words for a developer and no way to reach one.** It
        // knows lobby codes; only the relay knows which connection a code
        // belongs to, so the routing is here.
        let notices = self
            .policy
            .as_mut()
            .map(|p| p.take_host_notices())
            .unwrap_or_default();
        for (code, text) in notices {
            if let Some(host) = self.lobbies.get(&code).map(|l| l.host) {
                self.send(host, Channel::Reliable, &RelayMsg::Notice { text });
            }
        }
        self.retry_parked_hosts();
        let mut moved = 0;
        for inc in self.transport.poll() {
            moved += 1;
            match inc {
                Incoming::Connected(c) => {
                    self.conns.insert(c, Role::Fresh);
                }
                Incoming::Disconnected(c, _) => self.drop_conn(c),
                Incoming::Message(c, ch, bytes) => {
                    // **Budgeted before it is decoded.** A connection over its
                    // byte or message budget for the window has this message
                    // dropped and counted; over it for [`RelayLimits::strikes`]
                    // windows in a row, the connection is closed.
                    if !self.charge_ingress(c, bytes.len() as u64) {
                        continue;
                    }
                    let Some(msg) = RelayMsg::decode(&bytes) else {
                        continue;
                    };
                    self.dispatch(c, ch, msg);
                }
            }
        }
        moved
    }

    fn dispatch(&mut self, from: PeerId, leg_channel: Channel, msg: RelayMsg) {
        match msg {
            // **The marker can arrive before or after the lobby exists**, since
            // a keyed host may be parked while the policy answers. Both orders
            // are handled: remember the connection, and tell the policy as soon
            // as there is a code to name.
            RelayMsg::HostIsDedicated => {
                self.dedicated.insert(from);
                if let Some(Role::Host { code }) = self.conns.get(&from) {
                    let code = code.clone();
                    if let Some(p) = self.policy.as_mut() {
                        p.host_is_dedicated(&code);
                    }
                }
            }
            // **Same both-orders handling as the dedicated marker**: a keyed
            // host may be parked while the policy answers, so this can arrive
            // before or after the lobby exists. Only the before-case can be
            // honoured — a code is chosen once — so a marker that turns up late
            // is dropped rather than renaming a live lobby out from under its
            // players.
            RelayMsg::WantCode { code } => {
                if matches!(self.conns.get(&from), Some(Role::Fresh) | None) {
                    self.wanted.insert(from, code.to_uppercase());
                }
            }
            RelayMsg::Host => self.open_lobby(from, None, None),
            RelayMsg::HostKeyed { key, build } => {
                self.open_lobby(from, Some(&key), build.as_deref())
            }
            RelayMsg::Join { code } => {
                // A connection already in a lobby, in either role, does not
                // join another: the relay would otherwise hold two roles for
                // one socket, and the first lobby would keep a client who is
                // never coming back.
                if !matches!(self.conns.get(&from), Some(Role::Fresh)) {
                    self.refuse_limit(from, "this connection is already in a lobby");
                    return;
                }
                if !self.address_may(from, |r| &mut r.joins, self.limits.joins_per_address) {
                    self.refuse_limit(
                        from,
                        "too many joins from this address in a minute — wait a moment",
                    );
                    return;
                }
                if self
                    .lobbies
                    .get(&code)
                    .is_some_and(|l| l.clients.len() >= self.limits.max_clients_per_lobby)
                {
                    self.refuse_limit(from, "this lobby is full");
                    return;
                }
                // The cap is enforced here and nowhere else: a live session is
                // never broken for a limit, so the only thing a limit can do is
                // turn away the next arrival — with words that name the plan
                // and where to change it.
                if let Some(p) = self.policy.as_mut() {
                    match p.admit_join(&code) {
                        JoinAdmission::Refuse { reason } => {
                            self.send(from, Channel::Reliable, &RelayMsg::Refused { reason });
                            return;
                        }
                        // ⚠ **Held, not refused, and not joined either.** The
                        // server is coming up; the joiner waits and asks again.
                        // Falling through to the `no lobby` refusal below is the
                        // exact bug this card exists to prevent — a good code
                        // reported as a game that does not exist.
                        JoinAdmission::Starting { detail } => {
                            self.send(from, Channel::Reliable, &RelayMsg::Starting { detail });
                            return;
                        }
                        JoinAdmission::Allow => {}
                    }
                }
                let Some(lobby) = self.lobbies.get_mut(&code) else {
                    self.send(
                        from,
                        Channel::Reliable,
                        &RelayMsg::Refused {
                            reason: format!("no lobby {code}"),
                        },
                    );
                    return;
                };
                let peer = lobby.next_peer;
                lobby.next_peer += 1;
                lobby.clients.insert(peer, from);
                lobby.empty_since = Instant::now();
                let host = lobby.host;
                self.conns.insert(
                    from,
                    Role::Client {
                        code: code.clone(),
                        game_peer: peer,
                    },
                );
                if let Some(p) = self.policy.as_mut() {
                    p.peer_joined(&code);
                }
                self.send(from, Channel::Reliable, &RelayMsg::JoinOk);
                self.send(host, Channel::Reliable, &RelayMsg::PeerJoined { peer });
            }
            RelayMsg::ToPeer {
                peer,
                channel,
                seq,
                bytes,
            } => {
                let Some(Role::Host { code }) = self.conns.get(&from) else {
                    return;
                };
                if leg_channel == Channel::Reliable && bytes.len() > MAX_HOST_RELIABLE {
                    self.limit_drops += 1;
                    self.note_limit_drop();
                    return;
                }
                let code = code.clone();
                let n = bytes.len() as u64;
                let target = self
                    .lobbies
                    .get(&code)
                    .and_then(|l| l.clients.get(&peer))
                    .copied();
                // Counted even when it goes nowhere: bytes arriving for a peer
                // who has just left are real traffic this region carried, and
                // the gap between the two numbers is how that shows up.
                let Some(target) = target else {
                    if let Some(p) = self.policy.as_mut() {
                        p.forwarded(&code, n, 0);
                    }
                    return;
                };
                self.send(
                    target,
                    leg_channel,
                    &RelayMsg::FromHost {
                        channel,
                        seq,
                        bytes,
                    },
                );
                if let Some(p) = self.policy.as_mut() {
                    p.forwarded(&code, n, n);
                }
            }
            RelayMsg::ToHost {
                channel,
                seq,
                bytes,
            } => {
                let Some(Role::Client { code, game_peer }) = self.conns.get(&from) else {
                    return;
                };
                // A host has to decode what a client sends; a modified client
                // pushing a megabyte at it is the cheapest way to hurt one.
                if leg_channel == Channel::Reliable && bytes.len() > MAX_CLIENT_RELIABLE {
                    self.limit_drops += 1;
                    self.note_limit_drop();
                    return;
                }
                let (peer, code) = (*game_peer, code.clone());
                let n = bytes.len() as u64;
                let host = self.lobbies.get(&code).map(|l| l.host);
                let Some(host) = host else {
                    // ⚠ Received and forwarded nowhere. Counted as its own
                    // fact, not merely as the gap between two byte totals —
                    // that gap is how a mid-match teardown was eventually
                    // found, and only because every other bucket had matched to
                    // the byte.
                    if let Some(p) = self.policy.as_mut() {
                        p.forwarded(&code, n, 0);
                        p.orphaned(&code, n);
                    }
                    return;
                };
                self.send(
                    host,
                    leg_channel,
                    &RelayMsg::FromPeer {
                        peer,
                        channel,
                        seq,
                        bytes,
                    },
                );
                if let Some(p) = self.policy.as_mut() {
                    p.forwarded(&code, n, n);
                }
            }
            _ => { /* endpoints never send the rest */ }
        }
    }

    /// Re-ask the policy about every host it has not decided on yet, and give
    /// up on the ones that have waited too long.
    ///
    /// The deadline refusal names the control plane rather than the developer's
    /// key: a lookup that never came back is our problem, not theirs, and
    /// telling somebody their key is bad when we simply could not check it is
    /// the kind of wrong answer that costs a support ticket and a customer's
    /// confidence.
    fn retry_parked_hosts(&mut self) {
        if self.parked.is_empty() {
            return;
        }
        for p in std::mem::take(&mut self.parked) {
            if p.since.elapsed() >= HOST_DECISION_DEADLINE {
                self.send(
                    p.conn,
                    Channel::Reliable,
                    &RelayMsg::Refused {
                        reason: "Floptle Cloud could not check this game's key just now. \
                                 Try hosting again in a moment."
                            .into(),
                    },
                );
                continue;
            }
            self.open_lobby(p.conn, p.key.as_deref(), p.build.as_deref());
        }
    }

    /// The shared tail of `Host` and `HostKeyed`: ask the policy, then either
    /// open a lobby or say why not.
    ///
    /// **A refusal is a message, never a silence.** The reason goes back on the
    /// wire and reaches the player verbatim — the whole value of "connect your
    /// project at fopull.com/cloud" is that somebody reads it.
    fn open_lobby(&mut self, from: PeerId, key: Option<&str>, build: Option<&str>) {
        // One live lobby per connection. A second `Host` on a connection that
        // already hosts must not open a second lobby and forget the first,
        // with its players attached to a code nobody serves.
        if let Some(Role::Host { code }) = self.conns.get(&from) {
            let code = code.clone();
            self.refuse_limit(from, &format!("this connection already hosts lobby {code}"));
            return;
        }
        if matches!(self.conns.get(&from), Some(Role::Client { .. })) {
            self.refuse_limit(
                from,
                "this connection is a client in a lobby and cannot host",
            );
            return;
        }
        // Parked hosts have already paid the address rate when they first
        // asked; they are re-asked from `retry_parked_hosts`.
        if self.parked_since(from).is_none()
            && !self.address_may(from, |r| &mut r.opens, self.limits.opens_per_address)
        {
            self.refuse_limit(
                from,
                "too many lobbies opened from this address in a minute — wait a moment",
            );
            return;
        }
        if self.lobbies.len() >= self.limits.max_lobbies {
            self.refuse_limit(
                from,
                "this relay is at its lobby limit — try again in a moment",
            );
            return;
        }
        let prefix = match self.policy.as_mut() {
            Some(p) => match p.admit_host(key, build) {
                HostAdmission::Allow { prefix } => prefix,
                HostAdmission::Refuse { reason } => {
                    self.send(from, Channel::Reliable, &RelayMsg::Refused { reason });
                    return;
                }
                // Undecided: park it and ask again next step, rather than
                // block this one. A parked host keeps its place in the queue
                // from the moment it first asked, so a slow lookup cannot be
                // restarted forever by the retry that is waiting on it.
                HostAdmission::Pending => {
                    let since = self.parked_since(from).unwrap_or_else(Instant::now);
                    self.parked.retain(|p| p.conn != from);
                    self.parked.push(ParkedHost {
                        conn: from,
                        key: key.map(str::to_string),
                        build: build.map(str::to_string),
                        since,
                    });
                    return;
                }
            },
            // No policy: the open relay. A key, if one was presented, is
            // ignored rather than checked — a self-hosted relay has nothing to
            // check it against and is not entitled to an opinion about it.
            None => None,
        };
        // **Reclaim before minting**. A managed server brings
        // the code it already had; the policy decides whether this key owns it.
        // A code somebody is actively hosting is never handed over, however good
        // the claim — that would move live players into a different lobby.
        let wanted = self.wanted.remove(&from);
        // ⚠ **A held lobby is rejoined, not replaced**. This is
        // the case that saves a match: the host blipped, its lobby is inside
        // the grace window with everybody still attached, and it has come back
        // asking for its own code. Re-point the lobby at the new connection and
        // the fight carries on — a new lobby here would strand the very players
        // the grace window was holding.
        if let Some(c) = wanted.clone()
            && self.policy.as_mut().is_some_and(|p| p.claim_code(key, &c))
            && let Some(l) = self.lobbies.get_mut(&c)
            && l.host_lost_at.is_some()
        {
            l.host = from;
            l.host_lost_at = None;
            // ⚠ **The idle clock restarts at the reclaim**.
            // The lobby carried its predecessor's `empty_since`, so a
            // dedicated server that had sat open since morning — which is a
            // dedicated server's job — was reaped by the sweep after the one
            // that restored it, before its new connection's dedicated marker
            // had landed. The new host gets the full window from now.
            l.empty_since = Instant::now();
            let clients: Vec<u64> = l.clients.keys().copied().collect();
            self.conns.insert(from, Role::Host { code: c.clone() });
            let addr = self.transport.remote_addr(from).map(|a| a.ip());
            if let Some(p) = self.policy.as_mut() {
                p.lobby_host_returned(&c, addr);
            }
            // The host is new to these players even though they never left, so
            // it needs the roster it is now responsible for.
            for peer in clients {
                self.send(from, Channel::Reliable, &RelayMsg::PeerJoined { peer });
            }
            self.send(from, Channel::Reliable, &RelayMsg::Hosted { code: c });
            return;
        }
        let claimed = match wanted {
            Some(c) if !self.lobbies.contains_key(&c) => {
                let ok = self.policy.as_mut().is_some_and(|p| p.claim_code(key, &c));
                ok.then_some(c)
            }
            _ => None,
        };
        let code = match claimed {
            Some(c) => c,
            None => {
                // ⚠ **A relay that has not learned its reservations yet must not
                // invent a code.** Every code it drew would be one it cannot
                // know is already promised to a sleeping deployment, and handing
                // those six characters to a stranger is the one failure this
                // whole mechanism exists to prevent. It refuses the way it
                // already refuses without keys — briefly, and with a sentence.
                if self.policy.as_ref().is_some_and(|p| !p.may_mint()) {
                    self.send(
                        from,
                        Channel::Reliable,
                        &RelayMsg::Refused {
                            reason: NOT_READY_YET.into(),
                        },
                    );
                    return;
                }
                loop {
                    let c = lobby_code(&mut self.rng, prefix);
                    let reserved = self.policy.as_ref().is_some_and(|p| p.code_is_reserved(&c));
                    if !reserved && !self.lobbies.contains_key(&c) {
                        break c;
                    }
                }
            }
        };
        self.lobbies.insert(
            code.clone(),
            Lobby {
                host: from,
                clients: HashMap::new(),
                next_peer: 1,
                host_lost_at: None,
                empty_since: Instant::now(),
            },
        );
        self.conns.insert(from, Role::Host { code: code.clone() });
        let addr = self.transport.remote_addr(from).map(|a| a.ip());
        if let Some(p) = self.policy.as_mut() {
            p.lobby_opened(&code, key, addr);
            // The marker may have arrived while this host was parked.
            if self.dedicated.contains(&from) {
                p.host_is_dedicated(&code);
            }
        }
        self.send(from, Channel::Reliable, &RelayMsg::Hosted { code });
    }

    /// When this connection first asked to host, if it is already parked.
    fn parked_since(&self, conn: PeerId) -> Option<Instant> {
        self.parked.iter().find(|p| p.conn == conn).map(|p| p.since)
    }

    /// **End the lobbies whose hosts did not come back**.
    ///
    /// Everything a held lobby was protecting — the code, the players, the
    /// match — is released here, once, with a reason a person can read.
    fn sweep_lost_hosts(&mut self) {
        let dead: Vec<String> = self
            .lobbies
            .iter()
            .filter(|(_, l)| l.host_lost_at.is_some_and(|t| t.elapsed() >= self.grace))
            .map(|(c, _)| c.clone())
            .collect();
        for code in dead {
            self.end_lobby(&code, LobbyEnd::HostGone);
        }
    }

    /// Destroy a lobby and tell everybody still in it why.
    fn end_lobby(&mut self, code: &str, why: LobbyEnd) {
        let Some(lobby) = self.lobbies.remove(code) else {
            return;
        };
        if let Some(p) = self.policy.as_mut() {
            p.lobby_ended(code, why);
            p.lobby_closed(code);
        }
        for (_, conn) in lobby.clients {
            self.send(
                conn,
                Channel::Reliable,
                &RelayMsg::Refused {
                    reason: why.as_str().into(),
                },
            );
        }
    }

    fn drop_conn(&mut self, c: PeerId) {
        // A host that hangs up while we are still deciding takes its question
        // with it — otherwise the retry loop keeps asking about a connection
        // that has gone, and eventually answers into a closed socket.
        self.parked.retain(|p| p.conn != c);
        // Peer ids are handed out per connection, and a reconnecting dedicated
        // server arrives as a new one — so this set must shrink with the
        // connections, or a long-lived relay accumulates one entry per restart.
        self.dedicated.remove(&c);
        match self.conns.remove(&c) {
            Some(Role::Host { code }) => {
                // The lobby is held, not destroyed. A host whose connection
                // blips for a few seconds must not take everybody's match with
                // it, leaving the players sending into a lobby that no longer
                // exists with nothing to tell them. The grace window is swept
                // in `step`; if the host does not come back, it ends there with
                // a reason.
                if let Some(l) = self.lobbies.get_mut(&code) {
                    l.host_lost_at = Some(Instant::now());
                    if let Some(p) = self.policy.as_mut() {
                        p.lobby_host_lost(&code);
                    }
                }
            }
            Some(Role::Client { code, game_peer }) => {
                if let Some(p) = self.policy.as_mut() {
                    p.peer_left(&code);
                }
                if let Some(lobby) = self.lobbies.get_mut(&code) {
                    lobby.clients.remove(&game_peer);
                    if lobby.clients.is_empty() {
                        lobby.empty_since = Instant::now();
                    }
                    let host = lobby.host;
                    self.send(
                        host,
                        Channel::Reliable,
                        &RelayMsg::PeerLeft { peer: game_peer },
                    );
                }
            }
            _ => {}
        }
        self.ingress.remove(&c);
    }

    /// Account one message against its connection's window; `false` means it
    /// is dropped. The strikes are consecutive: a window under budget resets.
    fn charge_ingress(&mut self, c: PeerId, bytes: u64) -> bool {
        let limits = self.limits;
        let now = Instant::now();
        let e = self.ingress.entry(c).or_insert(Ingress {
            window_start: now,
            bytes: 0,
            msgs: 0,
            strikes: 0,
        });
        if now.duration_since(e.window_start) >= limits.window {
            // The window that just closed: over budget counts a strike, under
            // budget clears them.
            if e.bytes > limits.bytes_per_window || e.msgs > limits.msgs_per_window {
                e.strikes += 1;
            } else {
                e.strikes = 0;
            }
            e.window_start = now;
            e.bytes = 0;
            e.msgs = 0;
        }
        e.bytes += bytes;
        e.msgs += 1;
        let over = e.bytes > limits.bytes_per_window || e.msgs > limits.msgs_per_window;
        if !over {
            return true;
        }
        let strikes = e.strikes;
        self.limit_drops += 1;
        self.note_limit_drop();
        // A repeat offender is closed — the strikes it already has plus the
        // window it is over right now.
        if strikes + 1 >= limits.strikes {
            self.transport.disconnect(c);
            self.drop_conn(c);
        }
        false
    }

    /// May this connection's address do one more of `which` inside the rate
    /// window? Records it if so. A connection with no known address is never
    /// rate-limited by address: the transport did not say, and refusing a
    /// player for what the relay could not measure is the wrong default.
    fn address_may(
        &mut self,
        c: PeerId,
        which: impl FnOnce(&mut AddressRate) -> &mut Vec<Instant>,
        per_window: u32,
    ) -> bool {
        let Some(addr) = self.transport.remote_addr(c) else {
            return true;
        };
        let now = Instant::now();
        let window = self.limits.rate_window;
        let rate = self.address_rates.entry(addr.ip()).or_default();
        let stamps = which(rate);
        stamps.retain(|t| now.duration_since(*t) < window);
        if stamps.len() as u32 >= per_window {
            return false;
        }
        stamps.push(now);
        true
    }

    /// A refusal the limits made, on the wire like any other.
    fn refuse_limit(&mut self, to: PeerId, reason: &str) {
        self.limit_drops += 1;
        self.note_limit_drop();
        self.send(
            to,
            Channel::Reliable,
            &RelayMsg::Refused {
                reason: reason.into(),
            },
        );
    }

    fn note_limit_drop(&mut self) {
        if let Some(p) = self.policy.as_mut() {
            p.dropped_by_limit();
        }
    }

    /// **End the lobbies nobody is in** — a host that opened one and never
    /// had a player is not a session, and a leaked key opening thousands of
    /// them is not a developer. Dedicated servers are exempt: an empty server
    /// waiting for players is what a server is for.
    fn sweep_idle_lobbies(&mut self) {
        let idle = self.limits.idle_lobby;
        let dead: Vec<String> = self
            .lobbies
            .iter()
            .filter(|(_, l)| {
                l.clients.is_empty()
                    && l.host_lost_at.is_none()
                    && !self.dedicated.contains(&l.host)
                    && l.empty_since.elapsed() >= idle
            })
            .map(|(c, _)| c.clone())
            .collect();
        for code in dead {
            let host = self.lobbies.get(&code).map(|l| l.host);
            self.end_lobby(&code, LobbyEnd::Idle);
            if let Some(h) = host {
                self.conns.insert(h, Role::Fresh);
                self.send(
                    h,
                    Channel::Reliable,
                    &RelayMsg::Refused {
                        reason: LobbyEnd::Idle.as_str().into(),
                    },
                );
            }
        }
    }

    fn send(&mut self, to: PeerId, channel: Channel, msg: &RelayMsg) {
        // Legs never use per-leg sequenced dedup (see the module docs) —
        // unreliable stays unreliable, sequencing is end-to-end.
        let ch = if channel == Channel::Reliable {
            Channel::Reliable
        } else {
            Channel::Unreliable
        };
        self.transport.send(to, ch, &msg.encode());
    }
}

// ---------------------------------------------------------------------------
// Endpoint transports
// ---------------------------------------------------------------------------

/// End-to-end sequenced-drop state: last seq delivered per (peer, channel).
#[derive(Default)]
struct SeqState {
    last: HashMap<(u64, u8), u64>,
}

impl SeqState {
    /// True when the message should be dropped (stale sequenced).
    fn stale(&mut self, peer: u64, channel: u8, seq: u64) -> bool {
        if channel != CH_SEQUENCED {
            return false;
        }
        let last = self.last.entry((peer, channel)).or_insert(0);
        if seq <= *last {
            return true;
        }
        *last = seq;
        false
    }
}

/// The host's end of a relayed session: one quic leg to the relay, a lobby
/// code for friends, and the same [`Transport`] the sessions already speak —
/// peers appear exactly as if they had connected directly.
pub struct RelayHost {
    inner: QuicClient,
    code: Option<String>,
    seq: u64,
    dedup: SeqState,
    /// Why the relay turned this host away, if it did.
    ///
    /// A managed relay's refusals are the product's own words: "connect your
    /// project at fopull.com/cloud", "this game is at its 20-player limit on
    /// the free plan". Dropped, they would arrive as a three second wait and
    /// then "no lobby code (is a relay running there?)", which points at the
    /// wrong thing entirely.
    refused: Option<String>,
    /// Things the relay said about this lobby that a developer should read,
    /// drained by whoever is hosting — see [`Transport::take_notices`].
    notices: Vec<String>,
    /// Declared through [`RelayHost::declare_dedicated`], and re-sent on every
    /// re-host.
    dedicated: bool,
    /// The code this host asks to reclaim, remembered so it goes again with
    /// every re-host.
    wanted: Option<String>,
    /// Everything needed to host again after the relay goes away:
    /// where it is, and what to ask it for.
    relay_addr: String,
    ask: RelayMsg,
    /// When to try again, and how long to wait after that.
    ///
    /// A relay restart is a routine operation — a version upgrade is the
    /// common one — and a dedicated server is a long-lived process on a box
    /// nobody is watching. Needing a human to restart every server in a region
    /// after each relay upgrade is not an operational model, it is a chore
    /// nobody will remember.
    retry_at: Option<Instant>,
    backoff: Duration,
}

/// First wait after losing the relay, and the ceiling the backoff climbs to.
///
/// The first is short because the overwhelmingly common cause is a relay that
/// is restarting and will be back in a second or two; the ceiling is there so a
/// relay that is gone for an afternoon is not hammered by every server in the
/// region.
const RELAY_RETRY_MIN: Duration = Duration::from_secs(1);
const RELAY_RETRY_MAX: Duration = Duration::from_secs(30);

impl RelayHost {
    /// Connect to a relay and host a lobby. Blocks briefly (≤ ~3 s) for the
    /// lobby code — one click, one code.
    pub fn host(relay_addr: &str) -> Result<(Self, String), String> {
        Self::connect_and_host(relay_addr, RelayMsg::Host, None)
    }

    /// Host as a registered game, presenting the project's Floptle Cloud key
    /// (and its build hash, when the build has one).
    ///
    /// A managed relay refuses a keyless host, so this is the call an exported
    /// game makes once its project is connected. A self-hosted relay ignores
    /// the key: it has nothing to check it against.
    ///
    /// A relay older than managed mode cannot decode `HostKeyed` and drops it
    /// silently, as an unknown postcard variant is dropped. So the plain
    /// `Host` goes out after [`OLD_RELAY_FALLBACK_POLLS`] and that relay hosts
    /// the game normally, rather than a three second timeout and a wrong
    /// diagnosis. On a managed relay the answer arrives long before then.
    pub fn host_keyed(
        relay_addr: &str,
        key: &str,
        build: Option<&str>,
    ) -> Result<(Self, String), String> {
        Self::connect_and_host(
            relay_addr,
            RelayMsg::HostKeyed {
                key: key.to_string(),
                build: build.map(str::to_string),
            },
            Some(RelayMsg::Host),
        )
    }

    /// Host with a key, **asking to reclaim `code`**.
    ///
    /// A managed server that is restarting, waking, or meeting a relay that
    /// itself restarted brings the code it already had. The relay honours it
    /// only when its snapshot reserves that code for this key, so this is a
    /// request and never an instruction — a host that asks for a code it does
    /// not own is simply minted a fresh one, exactly as before.
    ///
    /// **This is what makes six characters survive.** Without it a restart
    /// mints a new code and every player holding the old one is refused, which
    /// has now happened twice in production — once from a relay upgrade and
    /// once from ordinary agent maintenance.
    pub fn host_keyed_reclaiming(
        relay_addr: &str,
        key: &str,
        build: Option<&str>,
        code: &str,
    ) -> Result<(Self, String), String> {
        Self::connect_and_host_wanting(
            relay_addr,
            RelayMsg::HostKeyed {
                key: key.to_string(),
                build: build.map(str::to_string),
            },
            Some(RelayMsg::Host),
            Some(code.to_uppercase()),
        )
    }

    fn connect_and_host(
        relay_addr: &str,
        ask: RelayMsg,
        fallback: Option<RelayMsg>,
    ) -> Result<(Self, String), String> {
        Self::connect_and_host_wanting(relay_addr, ask, fallback, None)
    }

    fn connect_and_host_wanting(
        relay_addr: &str,
        ask: RelayMsg,
        fallback: Option<RelayMsg>,
        wanted: Option<String>,
    ) -> Result<(Self, String), String> {
        let mut inner = QuicClient::connect(relay_addr)?;
        // ⚠ **The claim goes first, ahead of the host request.** Both ride the
        // same ordered stream, and the relay opens the lobby — choosing a code
        // — the instant it reads the host request. Sent afterwards this arrives
        // one message too late, every time: the code is already minted and the
        // marker is dropped as a late arrival. That is not a race that
        // sometimes bites; it is the guaranteed order, and it cost a green
        // policy test with a red end-to-end one to see.
        if let Some(c) = &wanted {
            inner.send(
                SERVER,
                Channel::Reliable,
                &RelayMsg::WantCode { code: c.clone() }.encode(),
            );
        }
        inner.send(SERVER, Channel::Reliable, &ask.encode());
        let mut me = Self {
            inner,
            code: None,
            seq: 0,
            dedup: SeqState::default(),
            refused: None,
            notices: Vec::new(),
            dedicated: false,
            wanted,
            relay_addr: relay_addr.to_string(),
            ask: ask.clone(),
            retry_at: None,
            backoff: RELAY_RETRY_MIN,
        };
        let mut fallback = fallback;
        for i in 0..600 {
            let _ = me.poll(); // stashes Hosted{code} / Refused{reason} when it lands
            if let Some(c) = me.code.clone() {
                return Ok((me, c));
            }
            // The relay answered, and the answer was no. Its words, not ours.
            if let Some(reason) = me.refused.take() {
                return Err(reason);
            }
            if i == OLD_RELAY_FALLBACK_POLLS
                && let Some(f) = fallback.take()
            {
                me.inner.send(SERVER, Channel::Reliable, &f.encode());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(format!(
            "relay {relay_addr}: no lobby code (is a relay running there?)"
        ))
    }

    /// The lobby code (known after [`Self::host`] returns).
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Tell the relay this host is a **dedicated server**, not somebody playing.
    ///
    /// A relay counts a lobby as its clients plus its host. That is right for a
    /// listen host and an off-by-one for a box nobody is sitting at, which is
    /// why an idle dedicated server read as one concurrent player and held one
    /// of its account's ceiling forever.
    ///
    /// Remembered as well as sent, because a relay that restarts loses every
    /// lobby and the marker has to go again with the re-host.
    pub fn declare_dedicated(&mut self) {
        self.dedicated = true;
        self.inner.send(
            SERVER,
            Channel::Reliable,
            &RelayMsg::HostIsDedicated.encode(),
        );
    }

    /// Is the lobby live on the relay right now?
    ///
    /// False from the moment the leg drops until a re-host succeeds — which is
    /// also exactly the window in which [`Self::code`] is `None`, because a
    /// code the relay has never heard of is worse than no code at all: it is
    /// published, printed, handed to players, and refuses every one of them.
    pub fn live(&self) -> bool {
        self.code.is_some()
    }

    /// The relay went away. Forget the lobby and start trying to get it back.
    fn lost_the_relay(&mut self) {
        if self.code.take().is_some() {
            self.notices.push(format!(
                "lost the connection to the relay at {} — the lobby code is not valid until \
                 it is back, and nobody can join. Reconnecting.",
                self.relay_addr
            ));
        }
        self.dedup = SeqState::default();
        self.retry_at = Some(Instant::now() + self.backoff);
    }

    /// One reconnection attempt, if one is due.
    ///
    /// **Nothing here blocks.** `QuicClient::connect` hands the handshake to
    /// its own runtime and returns, and the host request goes out immediately
    /// after — the relay's answer arrives through the ordinary poll, the same
    /// way the first one did. A reconnect that blocked would stall the tick of
    /// a server whose whole job is to tick.
    fn retry_if_due(&mut self) {
        let Some(at) = self.retry_at else { return };
        if Instant::now() < at {
            return;
        }
        // Climb before the attempt, so a relay that is down for an afternoon is
        // not asked every second by every server in the region.
        self.backoff = (self.backoff * 2).min(RELAY_RETRY_MAX);
        self.retry_at = Some(Instant::now() + self.backoff);
        let Ok(mut fresh) = QuicClient::connect(&self.relay_addr) else {
            return;
        };
        // Same ordering as the first host: the claim must be read before the
        // request that acts on it.
        if let Some(c) = &self.wanted {
            fresh.send(
                SERVER,
                Channel::Reliable,
                &RelayMsg::WantCode { code: c.clone() }.encode(),
            );
        }
        fresh.send(SERVER, Channel::Reliable, &self.ask.encode());
        // **The marker goes with the re-host, not after it.** This arrives
        // before the relay has opened the lobby, which is the case the server
        // side remembers per connection rather than per code.
        if self.dedicated {
            fresh.send(
                SERVER,
                Channel::Reliable,
                &RelayMsg::HostIsDedicated.encode(),
            );
        }
        self.inner = fresh;
    }
}

impl Transport for RelayHost {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        let seq = if channel == Channel::UnreliableSequenced {
            self.seq += 1;
            self.seq
        } else {
            0
        };
        let msg = RelayMsg::ToPeer {
            peer,
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

    fn take_notices(&mut self) -> Vec<String> {
        // The transport's own (a certificate that did not verify) and the
        // relay's, in the order they happened.
        let mut out = self.inner.take_warnings();
        out.append(&mut self.notices);
        out
    }

    fn lobby_code(&self) -> Option<String> {
        self.code.clone()
    }

    fn poll(&mut self) -> Vec<Incoming> {
        // Before reading: if the relay went away, this is where getting it back
        // is attempted. Every host drains its transport every tick, so the
        // retry needs no thread and no timer of its own.
        self.retry_if_due();
        let mut out = Vec::new();
        for inc in self.inner.poll() {
            match inc {
                Incoming::Message(_, _, bytes) => match RelayMsg::decode(&bytes) {
                    Some(RelayMsg::Hosted { code }) => {
                        // ⚠ **A reclaim that failed says so here, on the side that
                        // asked**.
                        //
                        // The control plane detects a mismatch one report later and
                        // keeps its reservation rather than adopting
                        // the new code — adopting it looks helpful and produces a
                        // restart loop, which is how a live server was stopped for
                        // two minutes. So the only other evidence is a silence, and
                        // the process that actually knows is this one.
                        //
                        // Every reason is on the relay's side and none is
                        // actionable from here: the code may have been reserved for
                        // a different key, already be in use, or the relay may be
                        // too old to have heard of the request at all. So this
                        // reports rather than retries.
                        if let Some(wanted) = &self.wanted
                            && *wanted != code
                        {
                            self.notices.push(format!(
                                "asked the relay at {} to reclaim lobby code {wanted} and was \
                             given {code} instead — players holding {wanted} cannot join. \
                             The relay did not agree that this game owns it.",
                                self.relay_addr
                            ));
                        }
                        // A re-host after an outage: the relay lost every lobby, so
                        // this is a new code and the old one is gone for good. Said
                        // out loud because a developer who read the old one to a
                        // friend needs to know it changed under them.
                        if self.retry_at.is_some() {
                            self.notices.push(format!(
                                "back on the relay at {} — the lobby code is now {code}",
                                self.relay_addr
                            ));
                            self.retry_at = None;
                            self.backoff = RELAY_RETRY_MIN;
                        }
                        self.code = Some(code);
                    }
                    // Carried, not swallowed. A managed relay refuses a host
                    // for reasons the player has to be able to act on, and
                    // every one of them is a sentence somebody wrote for them.
                    Some(RelayMsg::Refused { reason }) => {
                        if self.code.is_none() {
                            self.refused = Some(reason.clone());
                        }
                        out.push(Incoming::refused(SERVER, reason));
                    }
                    // Kept for the host process rather than turned into an
                    // `Incoming`: it is not a peer event and not an error, and
                    // a session that treated it as either would be wrong about
                    // both.
                    Some(RelayMsg::Notice { text }) => self.notices.push(text),
                    Some(RelayMsg::PeerJoined { peer }) => out.push(Incoming::Connected(peer)),
                    Some(RelayMsg::PeerLeft { peer }) => out.push(Incoming::dropped(peer)),
                    Some(RelayMsg::FromPeer {
                        peer,
                        channel,
                        seq,
                        bytes,
                    }) if !self.dedup.stale(peer, channel, seq) => {
                        out.push(Incoming::Message(peer, tag_channel(channel), bytes));
                    }
                    _ => {}
                },
                Incoming::Disconnected(_, _) => {
                    // The relay leg died: every player is unreachable now, and
                    // the cause is the relay rather than any one of them —
                    // worth saying, because "everyone left at once" is not a
                    // conclusion a game should be left to draw on its own.
                    let peers: Vec<u64> = self.dedup.last.keys().map(|(p, _)| *p).collect();
                    for p in peers {
                        out.push(Incoming::refused(p, "lost the connection to the relay"));
                    }
                    self.lost_the_relay();
                }
                Incoming::Connected(_) => {}
            }
        }
        out
    }

    fn stats(&self, _peer: PeerId) -> LinkStats {
        // Only the host↔relay leg is visible from here — a relayed packet's
        // second hop belongs to a connection this endpoint has never seen. The
        // number a game actually wants is host↔player, and it is measured a
        // level up by `NetSession::peer_rtt_ms`, which probes end to end and so
        // works the same over every transport. This stays as the transport's
        // honest answer about the only link it owns.
        self.inner.stats(SERVER)
    }
}

/// A client's end of a relayed session: joins by lobby code; the host appears
/// as [`SERVER`], exactly like a direct connection.
pub struct RelayClient {
    inner: QuicClient,
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
    /// Connect to a relay and join lobby `code`. Non-blocking: the session's
    /// `Hello` rides the same ordered stream right behind the `Join`, so the
    /// handshake completes as soon as the relay lets us in ([`Incoming`]
    /// carries a `Disconnected` if it refuses).
    pub fn join(relay_addr: &str, code: &str) -> Result<Self, String> {
        let mut inner = QuicClient::connect(relay_addr)?;
        inner.send(
            SERVER,
            Channel::Reliable,
            &RelayMsg::Join {
                code: code.to_uppercase(),
            }
            .encode(),
        );
        Ok(Self {
            inner,
            seq: 0,
            dedup: SeqState::default(),
            starting: None,
            code: code.to_uppercase(),
            retry_at: None,
        })
    }
}

impl Transport for RelayClient {
    fn take_notices(&mut self) -> Vec<String> {
        self.inner.take_warnings()
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

#[cfg(test)]
mod tests {

    /// **Every wire variant keeps the number it was born with.**
    ///
    /// Postcard indexes enum variants by declaration order, so inserting one
    /// anywhere but the end renumbers everything after it — and a build in the
    /// wild then sends `HostKeyed` at an index the relay now reads as something
    /// else. The failure is silent on both sides: a decode returns `None` and
    /// the message is skipped, so a game simply hangs with nothing said.
    ///
    /// The file says this in prose above `Host` and `HostKeyed`, and prose did
    /// not stop it happening while `Notice` was being added — it went in before
    /// `HostKeyed` first. So the order is pinned here as bytes: each of these
    /// is the discriminant that shipped, and changing one is a wire break
    /// rather than a refactor.
    #[test]
    fn every_wire_variant_keeps_the_number_it_shipped_with() {
        // The first byte postcard writes for a variant is its index.
        let index = |m: &RelayMsg| RelayMsg::encode(m)[0];

        assert_eq!(index(&RelayMsg::Host), 0);
        assert_eq!(
            index(&RelayMsg::Join {
                code: String::new()
            }),
            1
        );
        assert_eq!(
            index(&RelayMsg::Hosted {
                code: String::new()
            }),
            2
        );
        assert_eq!(index(&RelayMsg::JoinOk), 3);
        assert_eq!(
            index(&RelayMsg::Refused {
                reason: String::new()
            }),
            4
        );
        assert_eq!(index(&RelayMsg::PeerJoined { peer: 0 }), 5);
        assert_eq!(index(&RelayMsg::PeerLeft { peer: 0 }), 6);
        assert_eq!(
            index(&RelayMsg::ToPeer {
                peer: 0,
                channel: 0,
                seq: 0,
                bytes: vec![]
            }),
            7
        );
        assert_eq!(
            index(&RelayMsg::FromPeer {
                peer: 0,
                channel: 0,
                seq: 0,
                bytes: vec![]
            }),
            8
        );
        assert_eq!(
            index(&RelayMsg::ToHost {
                channel: 0,
                seq: 0,
                bytes: vec![]
            }),
            9
        );
        assert_eq!(
            index(&RelayMsg::FromHost {
                channel: 0,
                seq: 0,
                bytes: vec![]
            }),
            10
        );
        assert_eq!(
            index(&RelayMsg::HostKeyed {
                key: String::new(),
                build: None
            }),
            11
        );
        // Anything added from here on takes the next number and never a used one.
        assert_eq!(
            index(&RelayMsg::Notice {
                text: String::new()
            }),
            12
        );
        assert_eq!(index(&RelayMsg::HostIsDedicated), 13);
        assert_eq!(
            index(&RelayMsg::Starting {
                detail: String::new()
            }),
            14
        );
        assert_eq!(
            index(&RelayMsg::WantCode {
                code: String::new()
            }),
            15
        );
    }

    /// A relay that has never heard of a message skips it rather than dying,
    /// which is the other half of why appending is safe: an old host meeting a
    /// `Notice` must carry on hosting.
    #[test]
    fn an_unknown_variant_decodes_to_nothing_rather_than_breaking_the_link() {
        // Index 99: no variant, now or plausibly ever.
        assert!(RelayMsg::decode(&[99, 0, 0, 0]).is_none());
        assert!(RelayMsg::decode(&[]).is_none());
    }

    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    /// A relay stepping on a background thread until dropped.
    pub(super) struct TestRelay {
        pub(super) port: u16,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
        /// `RelayServer::limit_drops`, mirrored out of the relay thread every
        /// step so a test can read the count without owning the relay.
        pub(super) drops: Arc<std::sync::atomic::AtomicU64>,
        pub(super) lobbies: Arc<std::sync::atomic::AtomicUsize>,
    }

    /// A policy that answers from a table instead of a control plane — the
    /// managed rules without the network. `floptle-relay`'s real one pulls the
    /// same shape from a key snapshot; what is being asserted here is the
    /// relay's behaviour given an answer, which is the half that lives in this
    /// crate.
    pub(super) struct TablePolicy {
        /// key → ccu_limit. Anything absent is an unknown key.
        keys: HashMap<String, usize>,
        /// Live peers per lobby code, so a cap has something to count.
        live: HashMap<String, usize>,
        /// code → key, because the relay does not know what a key means.
        of_lobby: HashMap<String, String>,
        /// Lobbies whose host said it is a dedicated server. Shared, so a test
        /// can watch the marker cross the wire rather than infer it.
        dedicated: Arc<Mutex<Vec<String>>>,
        /// code → the key entitled to reclaim it.
        reserved: HashMap<String, String>,
        /// Has this policy pulled a snapshot? A relay that has not must not
        /// invent codes it cannot vet.
        primed: bool,
    }

    impl TablePolicy {
        pub(super) fn with(key: &str, limit: usize) -> Self {
            let mut keys = HashMap::new();
            keys.insert(key.to_string(), limit);
            Self {
                keys,
                live: HashMap::new(),
                of_lobby: HashMap::new(),
                dedicated: Arc::new(Mutex::new(Vec::new())),
                reserved: HashMap::new(),
                primed: true,
            }
        }

        /// A handle on what the policy was told, for a test that hands the
        /// policy itself to a relay it no longer owns.
        pub(super) fn dedicated_seen(&self) -> Arc<Mutex<Vec<String>>> {
            self.dedicated.clone()
        }

        /// Reserve `code` for `key`, the way a control-plane snapshot would.
        pub(super) fn reserving(mut self, code: &str, key: &str) -> Self {
            self.reserved.insert(code.to_string(), key.to_string());
            self
        }

        /// Let a second key host, for a test about one key claiming another's
        /// code.
        pub(super) fn allow_key(&mut self, key: &str, limit: usize) {
            self.keys.insert(key.to_string(), limit);
        }

        /// A relay that has not pulled a snapshot yet and must not mint.
        pub(super) fn unprimed(mut self) -> Self {
            self.primed = false;
            self
        }
    }

    impl RelayPolicy for TablePolicy {
        fn claim_code(&mut self, key: Option<&str>, code: &str) -> bool {
            key.is_some_and(|k| self.reserved.get(code).is_some_and(|owner| owner == k))
        }
        fn code_is_reserved(&self, code: &str) -> bool {
            self.reserved.contains_key(code)
        }
        fn may_mint(&self) -> bool {
            self.primed
        }
        fn host_is_dedicated(&mut self, code: &str) {
            self.dedicated.lock().unwrap().push(code.to_string());
        }

        fn admit_host(&mut self, key: Option<&str>, _build: Option<&str>) -> HostAdmission {
            let Some(key) = key.filter(|k| !k.is_empty()) else {
                return HostAdmission::Refuse {
                    reason: "This relay is Floptle Cloud. Connect your project to a game at \
                             fopull.com/cloud, or self-host floptle-relay."
                        .into(),
                };
            };
            match self.keys.contains_key(key) {
                true => HostAdmission::Allow { prefix: Some('U') },
                false => HostAdmission::Refuse {
                    reason: "That game key is not one this relay knows. Check project.ron, \
                             or connect the project at fopull.com/cloud."
                        .into(),
                },
            }
        }

        fn admit_join(&mut self, code: &str) -> JoinAdmission {
            let limit = self
                .of_lobby
                .get(code)
                .and_then(|k| self.keys.get(k))
                .copied()
                .unwrap_or(usize::MAX);
            // The host counts against the cap as well as the clients.
            if self.live.get(code).copied().unwrap_or(0) + 1 >= limit {
                return JoinAdmission::Refuse {
                    reason: format!(
                        "Floptle Cloud: this game is at its {limit}-player limit on the free \
                         plan. Upgrade at fopull.com/cloud."
                    ),
                };
            }
            JoinAdmission::Allow
        }

        fn lobby_opened(&mut self, code: &str, key: Option<&str>, _from: Option<std::net::IpAddr>) {
            self.live.insert(code.to_string(), 0);
            if let Some(k) = key {
                self.of_lobby.insert(code.to_string(), k.to_string());
            }
        }
        fn lobby_closed(&mut self, code: &str) {
            self.live.remove(code);
            self.of_lobby.remove(code);
        }
        fn peer_joined(&mut self, code: &str) {
            *self.live.entry(code.to_string()).or_insert(0) += 1;
        }
        fn peer_left(&mut self, code: &str) {
            if let Some(n) = self.live.get_mut(code) {
                *n = n.saturating_sub(1);
            }
        }
    }

    impl TestRelay {
        pub(super) fn start() -> Self {
            Self::start_with(None)
        }

        pub(super) fn managed(policy: TablePolicy) -> Self {
            Self::start_with(Some(Box::new(policy)))
        }

        /// A managed relay whose host-grace window is `grace`.
        pub(super) fn managed_with_grace(policy: TablePolicy, grace: Duration) -> Self {
            Self::start_with_grace(Some(Box::new(policy)), grace)
        }

        /// [`Self::managed_with_grace`] with its limits replaced too — for a
        /// reaper that has to fire inside a test.
        pub(super) fn managed_limited(
            policy: TablePolicy,
            grace: Duration,
            limits: RelayLimits,
        ) -> Self {
            let mut relay = RelayServer::bind(0).expect("relay bind");
            relay.set_policy(Box::new(policy));
            relay.set_grace(grace);
            relay.set_limits(limits);
            Self::run(relay)
        }

        /// A relay on a **named** port, so a test can stop one and start
        /// another at the same address — which is what a relay upgrade looks
        /// like from a host's point of view.
        pub(super) fn restart_on(port: u16) -> Self {
            let relay = RelayServer::bind(port).expect("the old relay's port is free again");
            Self::run(relay)
        }

        /// An open relay with its limits replaced, so a test can trip one
        /// with a handful of connections rather than a thousand.
        pub(super) fn limited(limits: RelayLimits) -> Self {
            let mut relay = RelayServer::bind(0).expect("relay bind");
            relay.set_grace(Duration::from_millis(150));
            relay.set_limits(limits);
            Self::run(relay)
        }

        fn run(mut relay: RelayServer) -> Self {
            let port = relay.port();
            let stop = Arc::new(AtomicBool::new(false));
            let drops = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let lobbies = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let (s, d, l) = (stop.clone(), drops.clone(), lobbies.clone());
            let thread = std::thread::spawn(move || {
                while !s.load(Ordering::Relaxed) {
                    relay.step();
                    d.store(relay.limit_drops(), Ordering::Relaxed);
                    l.store(relay.lobby_count(), Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
            Self {
                port,
                stop,
                thread: Some(thread),
                drops,
                lobbies,
            }
        }

        /// `127.0.0.1:<port>`, which is what every endpoint call wants.
        pub(super) fn addr(&self) -> String {
            format!("127.0.0.1:{}", self.port)
        }

        fn start_with(policy: Option<Box<dyn RelayPolicy>>) -> Self {
            Self::start_with_grace(policy, Duration::from_millis(150))
        }

        fn start_with_grace(policy: Option<Box<dyn RelayPolicy>>, grace: Duration) -> Self {
            let mut relay = RelayServer::bind(0).expect("relay bind");
            if let Some(p) = policy {
                relay.set_policy(p);
            }
            // Tests must not sleep for the production grace window.
            relay.set_grace(grace);
            Self::run(relay)
        }
    }

    impl Drop for TestRelay {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    /// **A relay restart must not leave a server advertising a dead code**.
    ///
    /// Upgrading the relay is routine — twice in three days on the live region
    /// — and it destroys every lobby on it. Before this, the host did not
    /// reconnect, logged nothing at all, and its status file went on reporting
    /// the code it had been handed at startup. The control plane published it,
    /// the game page showed it, a shipped build was given it, and every player
    /// who typed those six characters was refused with no way to find out why.
    /// The only signal anywhere was that usage samples stopped arriving.
    ///
    /// Three separate things are asserted, because the failure had three
    /// halves: the code goes away, somebody is told, and it comes back by
    /// itself.
    #[test]
    fn a_host_survives_its_relay_restarting_and_stops_advertising_a_dead_code() {
        let relay = TestRelay::start();
        let port = relay.port;
        let addr = relay.addr();
        let (mut host, first) = RelayHost::host(&addr).expect("host via relay");
        assert_eq!(
            host.lobby_code().as_deref(),
            Some(first.as_str()),
            "live to begin with"
        );

        // The relay goes away, exactly as an upgrade does.
        drop(relay);
        let mut noticed = false;
        for _ in 0..600 {
            let _ = host.poll();
            if host.lobby_code().is_none() {
                noticed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            noticed,
            "the host went on believing a code the relay no longer had"
        );
        assert!(
            !host.live(),
            "and it must say so rather than looking healthy"
        );
        let said = host.take_notices();
        assert!(
            said.iter()
                .any(|n| n.contains("lost the connection to the relay")),
            "silence is the defect: {said:?}"
        );

        // The relay comes back at the same address, and the host must return
        // without anybody restarting it.
        let _relay = TestRelay::restart_on(port);
        let mut back = None;
        for _ in 0..2000 {
            let _ = host.poll();
            if let Some(c) = host.lobby_code() {
                back = Some(c);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let back = back.expect("the host never re-hosted; a human would have had to restart it");
        assert!(host.live());
        let said = host.take_notices();
        assert!(
            said.iter().any(|n| n.contains(&back)),
            "a NEW code is not the old one, and whoever read the old one aloud needs it: {said:?}"
        );
    }

    // `a_full_session_replicates_through_the_relay` stays in the engine: it drives the engine's own game session
    // (`NetSession`, `floptle_core::World`), which does not come over.
    // Fontelle's end-to-end test of this path is `tests/relay.rs`.

    // `rollback_inputs_cross_a_relay_in_both_directions` stays in the engine: it drives the engine's own game session
    // (`NetSession`, `floptle_core::World`), which does not come over.
    // Fontelle's end-to-end test of this path is `tests/relay.rs`.

    #[test]
    fn bad_codes_refuse_and_lobbies_die_with_their_host() {
        let relay = TestRelay::start();
        let addr = format!("127.0.0.1:{}", relay.port);

        // Join with a garbage code → refused (a Disconnected on the client).
        let mut nope = RelayClient::join(&addr, "XXXXX").expect("connects to the relay fine");
        // The refusal must arrive with the relay's reason. A disconnect that
        // carries nothing is indistinguishable from the host closing their
        // laptop — and mistyping the code is the most common thing that will
        // ever go wrong in an online session, so it is the one failure a game
        // most needs to be able to describe.
        let mut why: Option<Option<String>> = None;
        for _ in 0..400 {
            if let Some(r) = nope.poll().iter().find_map(|i| match i {
                Incoming::Disconnected(SERVER, r) => Some(r.clone()),
                _ => None,
            }) {
                why = Some(r);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let why = why.expect("a bad code must refuse");
        let why = why.expect("the refusal must carry the relay's reason, not just end the link");
        assert!(
            why.contains("XXXXX"),
            "the reason should name the code that failed so a game can print it — got {why:?}"
        );

        // A real lobby: the host vanishing kills it for the client.
        let (host_t, code) = RelayHost::host(&addr).expect("host");
        let mut client = RelayClient::join(&addr, &code).expect("join");
        let mut joined = false;
        for _ in 0..400 {
            if client
                .poll()
                .iter()
                .any(|i| matches!(i, Incoming::Connected(SERVER)))
            {
                joined = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(joined, "the good code must admit");
        drop(host_t);
        let mut dead = false;
        for _ in 0..400 {
            if client
                .poll()
                .iter()
                .any(|i| matches!(i, Incoming::Disconnected(SERVER, _)))
            {
                dead = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            dead,
            "the lobby must die with its host once the grace window passes"
        );
    }
}

/// Managed mode (Floptle Cloud): who may host this relay, who
/// may join, and what a refusal says.
///
/// These drive a **real relay over real quic** with a policy that answers from
/// a table, because the thing worth asserting is what an endpoint experiences —
/// a code, or a sentence, or (the bug that started this) three seconds of
/// silence and a wrong diagnosis.
#[cfg(test)]
mod managed_tests {
    use super::tests::*;
    use super::*;
    use std::sync::atomic::Ordering;

    const KEY: &str = "fk_live_ATESTKEYTHATISNOTREAL0000000";

    /// Poll a host for a moment and return everything it was told.
    fn drain(host: &mut RelayHost) -> Vec<Incoming> {
        let mut out = Vec::new();
        for _ in 0..40 {
            out.extend(host.poll());
            std::thread::sleep(Duration::from_millis(5));
        }
        out
    }

    /// Pump both ends for a moment and return the refusal the client was given,
    /// if it was given one. The host is polled too, so its own leg keeps
    /// draining and a disturbed session would show up here.
    fn settle(client: &mut RelayClient, host: &mut RelayHost) -> Option<String> {
        let mut why = None;
        for _ in 0..60 {
            for inc in client.poll() {
                if let Incoming::Disconnected(_, Some(reason)) = inc {
                    why = Some(reason);
                }
            }
            let _ = host.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        why
    }

    /// The refusal text, or a failure naming what was expected. `RelayHost` is
    /// not `Debug` (it owns a quic endpoint), so `expect_err` is unavailable.
    fn refusal(r: Result<(RelayHost, String), String>, what: &str) -> String {
        match r {
            Err(e) => e,
            Ok((_, code)) => panic!("{what}, but it hosted with code {code}"),
        }
    }

    /// The negative control, and it is the product: the relay stays open and
    /// self-hostable. A relay with no policy hosts keyless hosts with five
    /// character codes and nothing to authorize against. If this ever goes
    /// red, managed mode has leaked into the open relay, and that is a licence
    /// question rather than a bug.
    #[test]
    fn a_self_hosted_relay_still_hosts_keyless_with_a_five_character_code() {
        let relay = TestRelay::start();
        let (_t, code) = RelayHost::host(&relay.addr()).expect("a self-hosted relay hosts");
        assert_eq!(code.len(), 5, "the open relay's code is unprefixed: {code}");
    }

    /// A self-hosted relay is handed a key by a Cloud-connected project and
    /// **ignores it** rather than checking it: it has nothing to check against,
    /// and refusing would break a developer's own relay the day they connected
    /// their project to Cloud.
    #[test]
    fn a_self_hosted_relay_ignores_a_key_rather_than_refusing_it() {
        let relay = TestRelay::start();
        let (_t, code) =
            RelayHost::host_keyed(&relay.addr(), KEY, None).expect("keys are not its business");
        assert_eq!(code.len(), 5, "still the open relay's own code: {code}");
    }

    /// **A dedicated server says so, and a listen host does not**.
    ///
    /// The relay counts a lobby as its clients plus its host. That is a person
    /// for a listen host and a machine for a dedicated one, and the relay
    /// cannot tell them apart by looking — so the host says which it is, on the
    /// same connection, straight after the host request.
    ///
    /// Asserted end to end over real quic rather than as a call, because this
    /// is a seam: a marker that is sent and never routed to the policy looks
    /// exactly like one that works, and the number it corrects is only read
    /// somewhere else entirely.
    #[test]
    fn a_dedicated_host_tells_the_relay_and_a_listen_host_does_not() {
        let policy = TablePolicy::with(KEY, 20);
        let seen = policy.dedicated_seen();
        let relay = TestRelay::managed(policy);

        // A listen host: nothing declared, and nothing must arrive.
        let (mut listen, listen_code) =
            RelayHost::host_keyed(&relay.addr(), KEY, None).expect("a listen host hosts");
        for _ in 0..60 {
            let _ = listen.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            seen.lock().unwrap().is_empty(),
            "a listen host IS a player and must not be exempted: {:?}",
            seen.lock().unwrap()
        );
        drop(listen);

        // A dedicated one declares itself, and the relay routes it through.
        let (mut server, code) =
            RelayHost::host_keyed(&relay.addr(), KEY, None).expect("a dedicated server hosts");
        server.declare_dedicated();
        let mut arrived = false;
        for _ in 0..200 {
            let _ = server.poll();
            if seen.lock().unwrap().contains(&code) {
                arrived = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            arrived,
            "the marker never reached the policy; seen {:?}",
            seen.lock().unwrap()
        );
        assert!(
            !seen.lock().unwrap().contains(&listen_code),
            "the wrong lobby was marked — the marker must name the connection's own"
        );
    }

    /// The rule, path 1: **a game with no key cannot use a managed relay** —
    /// and is told where to go rather than left guessing.
    #[test]
    fn a_managed_relay_refuses_a_keyless_host_and_says_where_to_go() {
        let relay = TestRelay::managed(TablePolicy::with(KEY, 20));
        let err = refusal(RelayHost::host(&relay.addr()), "a keyless host is refused");
        assert!(
            err.contains("fopull.com/cloud"),
            "the refusal must name the fix: {err}"
        );
        assert!(
            err.contains("self-host floptle-relay"),
            "…and the other way out, because it is a real one: {err}"
        );
    }

    /// The rule, path 2. An unknown key gets a **different** sentence to a
    /// missing one: a developer who has not connected their project yet and a
    /// developer whose key was revoked need different next actions, and one
    /// message for both sends the first one hunting for a problem they do not
    /// have.
    /// Poll `c` until `f` matches, or give up.
    fn settle_for(c: &mut RelayClient, f: impl Fn(&Incoming) -> bool) -> bool {
        for _ in 0..400 {
            if c.poll().iter().any(&f) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// Poll `c` for a while and report whether it was ever disconnected.
    fn evicted(c: &mut RelayClient) -> bool {
        for _ in 0..40 {
            if c.poll()
                .iter()
                .any(|i| matches!(i, Incoming::Disconnected(SERVER, _)))
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// ⚠ **A reclaimed lobby is not reaped in the second it is restored**.
    /// On the first real use of the reclaim, Forgery's
    /// server restarted onto a new bundle, asked for its code, was granted it
    /// with "host is back, players kept" — and the idle reaper ended the lobby
    /// in the same second, because the restored lobby carried the idle clock
    /// of a server that had sat open, alone, since morning, and the dedicated
    /// marker lives on a connection the restart had just replaced. The server
    /// went on advertising six characters that answered nothing.
    ///
    /// The lobby stays open here across the restart, and stays open for
    /// another whole idle window after it, because it is a server's.
    #[test]
    fn a_dedicated_server_that_reclaims_its_code_is_not_reaped_for_having_been_idle() {
        let idle = Duration::from_millis(120);
        let relay = TestRelay::managed_limited(
            TablePolicy::with(KEY, 20).reserving("U5FEFJ", KEY),
            Duration::from_secs(30),
            RelayLimits {
                idle_lobby: idle,
                ..RelayLimits::default()
            },
        );
        let (mut server, code) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), KEY, None, "U5FEFJ").expect("hosts");
        server.declare_dedicated();
        assert_eq!(code, "U5FEFJ");
        // Open, alone, for longer than the idle window: a server's job.
        std::thread::sleep(idle * 3);
        let _ = drain(&mut server);
        assert_eq!(
            relay.lobbies.load(Ordering::Relaxed),
            1,
            "the idle server was reaped before the restart"
        );

        // The restart: the process is gone, a new one asks for the same code.
        drop(server);
        let (mut again, back) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), KEY, None, "U5FEFJ").expect("re-hosts");
        again.declare_dedicated();
        assert_eq!(back, "U5FEFJ");
        // The sweep after the reclaim, and the whole window after that.
        std::thread::sleep(idle * 3);
        let events = drain(&mut again);
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Incoming::Disconnected(..))),
            "the reclaimed lobby was ended: {events:?}"
        );
        assert_eq!(
            relay.lobbies.load(Ordering::Relaxed),
            1,
            "the reaper ended the lobby the reclaim restored"
        );
        assert_eq!(again.lobby_code().as_deref(), Some("U5FEFJ"));
    }

    /// **And a player's reclaimed lobby gets a fresh idle window**, not the
    /// remainder of the old one: the clock restarts at the reclaim.
    #[test]
    fn a_reclaimed_lobbys_idle_clock_restarts_at_the_reclaim() {
        let idle = Duration::from_millis(200);
        let relay = TestRelay::managed_limited(
            TablePolicy::with(KEY, 20).reserving("U5FEFJ", KEY),
            Duration::from_secs(30),
            RelayLimits {
                idle_lobby: idle,
                ..RelayLimits::default()
            },
        );
        let (host, _) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), KEY, None, "U5FEFJ").expect("hosts");
        // Most of the window spent, then the blip.
        std::thread::sleep(idle / 2);
        drop(host);
        let (mut again, back) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), KEY, None, "U5FEFJ").expect("re-hosts");
        assert_eq!(back, "U5FEFJ", "not a reclaim");
        // Past the old deadline, inside the new one: still open. (No `drain`
        // here — it polls for 200 ms, which is the whole window.)
        std::thread::sleep(idle * 3 / 4);
        let _ = again.poll();
        assert_eq!(
            relay.lobbies.load(Ordering::Relaxed),
            1,
            "the old clock was carried over the reclaim"
        );
        // And a player's lobby still ends once its window has run.
        std::thread::sleep(idle);
        let _ = drain(&mut again);
        assert_eq!(
            relay.lobbies.load(Ordering::Relaxed),
            0,
            "a reclaimed player lobby became immortal"
        );
    }

    /// A host whose connection blips keeps its match.
    ///
    /// A relay that destroys the lobby the instant the host's connection
    /// closes ends a ten-minute match three times on a healthy link, and for
    /// the joiner every networked thing in the world vanishes while the skybox
    /// and the HUD stay. Held instead, a host that comes back reclaims the
    /// same lobby with its players still in it, so a NAT rebinding a UDP
    /// mapping costs a stutter.
    #[test]
    fn a_host_that_reconnects_inside_the_grace_window_keeps_its_players() {
        let relay = TestRelay::managed_with_grace(
            TablePolicy::with(KEY, 20).reserving("U5FEFJ", KEY),
            Duration::from_secs(30),
        );
        let (host, code) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), KEY, None, "U5FEFJ").expect("hosts");
        assert_eq!(code, "U5FEFJ");
        let mut client = RelayClient::join(&relay.addr(), &code).expect("join");
        assert!(
            settle_for(&mut client, |i| matches!(i, Incoming::Connected(SERVER))),
            "the client must get in first"
        );

        // The host's link dies — a rebind, a roam, a burst of loss.
        drop(host);
        std::thread::sleep(Duration::from_millis(300));

        // ⚠ The client must not have been told the match is over.
        assert!(
            !evicted(&mut client),
            "a blip threw the player out of a match that was still there"
        );

        // The host comes back and reclaims. Same code, same lobby, same player.
        let (_again, back) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), KEY, None, "U5FEFJ").expect("re-hosts");
        assert_eq!(
            back, "U5FEFJ",
            "the returning host was given a different lobby"
        );
        // ⚠ And the player is still in it. A reclaim that opened a fresh lobby
        // under the same code would leave this client attached to the old one —
        // which is the failure mode the grace window exists to prevent, wearing
        // the right code.
        assert!(
            !evicted(&mut client),
            "the player was dropped when the host came back"
        );
    }

    /// **A host that really is gone still ends the lobby**, with a reason.
    ///
    /// The grace window is a window, not a lease: a lobby must not outlive its
    /// host indefinitely, holding a code and a room full of people.
    #[test]
    fn a_host_that_never_returns_ends_the_lobby_with_a_reason() {
        let relay =
            TestRelay::managed_with_grace(TablePolicy::with(KEY, 20), Duration::from_millis(100));
        let (host, code) = RelayHost::host_keyed(&relay.addr(), KEY, None).expect("hosts");
        let mut client = RelayClient::join(&relay.addr(), &code).expect("join");
        assert!(settle_for(&mut client, |i| matches!(
            i,
            Incoming::Connected(SERVER)
        )));
        drop(host);

        let mut why = None;
        for _ in 0..400 {
            if let Some(r) = client.poll().iter().find_map(|i| match i {
                Incoming::Disconnected(SERVER, r) => Some(r.clone()),
                _ => None,
            }) {
                why = r;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        // ⚠ Not just "the link ended". A player who is dropped needs a sentence,
        // and a bare disconnect is what left the last one staring at an empty
        // world with nothing to explain it.
        let why = why.expect("a dropped player must be told why, not just cut off");
        assert!(
            why.contains("host"),
            "the reason should name what happened: {why:?}"
        );
    }

    /// ⚠ **A managed server gets the code it asks for, end to end**.
    ///
    /// This is the product's central promise: six characters a player wrote
    /// down keep working. It has now failed in production twice — once when a
    /// relay upgrade destroyed every lobby, and once when
    /// routine agent maintenance rewrote a unit and restarted the server. Both
    /// times the server came back with a **different code** and everybody
    /// holding the old one was refused.
    ///
    /// Driven through a real relay rather than the policy alone, because the
    /// thing being asserted is that the request survives the wire: `WantCode`
    /// is a separate message racing the host request on the same ordered
    /// stream, and the relay has to read it while the connection is still
    /// fresh.
    #[test]
    fn a_managed_server_reclaims_the_lobby_code_it_was_given_before() {
        let relay = TestRelay::managed(TablePolicy::with(KEY, 20).reserving("U5FEFJ", KEY));
        let (_t, code) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), KEY, None, "U5FEFJ").expect("hosts");
        assert_eq!(
            code, "U5FEFJ",
            "the server was handed a new code instead of its own"
        );
    }

    /// ⚠ **A server that could not reclaim its code says so itself**.
    ///
    /// W's control plane detects the mismatch one report later and
    /// keeps its reservation rather than adopting the new code — adopting it
    /// looks helpful and produced a restart loop that stopped a live server for
    /// two minutes. So on this side a failed reclaim would otherwise be a
    /// silence, and the process that actually knows is this one.
    #[test]
    fn a_server_that_could_not_reclaim_its_code_says_so_rather_than_going_quiet() {
        let mut keys = TablePolicy::with(KEY, 20).reserving("U5FEFJ", "fk_live_SOMEONE_ELSE");
        keys.allow_key("fk_live_MINE", 20);
        let relay = TestRelay::managed(keys);
        let (mut host, code) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), "fk_live_MINE", None, "U5FEFJ")
                .expect("an unowned claim still hosts");
        assert_ne!(code, "U5FEFJ");

        let mut said = Vec::new();
        for _ in 0..200 {
            said.extend(host.take_notices());
            if !said.is_empty() {
                break;
            }
            let _ = host.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        let all = said.join(" | ");
        assert!(
            all.contains("U5FEFJ") && all.contains("cannot join"),
            "a failed reclaim was silent on the side that asked: {all:?}"
        );
    }

    /// And a reclaim that **worked** says nothing — a line per successful host
    /// is a line nobody reads, which is how the one that matters gets missed.
    #[test]
    fn a_reclaim_that_worked_is_not_announced() {
        let relay = TestRelay::managed(TablePolicy::with(KEY, 20).reserving("U5FEFJ", KEY));
        let (mut host, code) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), KEY, None, "U5FEFJ").expect("hosts");
        assert_eq!(code, "U5FEFJ");
        let _ = host.poll();
        let said = host.take_notices().join(" | ");
        assert!(said.is_empty(), "a successful reclaim chattered: {said:?}");
    }

    /// ⚠ **A stranger asking for somebody else's code is simply given a fresh
    /// one** — not refused, and emphatically not granted.
    ///
    /// The game key is the only proof of ownership. If naming a code were
    /// enough to take it, anybody could hijack a lobby address by guessing six
    /// characters, and the reservation would be worse than no reservation at
    /// all.
    #[test]
    fn a_host_cannot_take_a_code_reserved_for_a_different_key() {
        let mut keys = TablePolicy::with(KEY, 20).reserving("U5FEFJ", "fk_live_SOMEONE_ELSE");
        keys.allow_key("fk_live_INTRUDER", 20);
        let relay = TestRelay::managed(keys);
        let (_t, code) =
            RelayHost::host_keyed_reclaiming(&relay.addr(), "fk_live_INTRUDER", None, "U5FEFJ")
                .expect("an unowned claim still hosts, with a fresh code");
        assert_ne!(code, "U5FEFJ", "a stranger took a code they do not own");
    }

    /// ⚠ **A relay that has not learned its reservations refuses to host at
    /// all**, rather than inventing a code that might already be promised.
    ///
    /// This is the window right after a relay restarts. Every code it could
    /// draw is one it cannot know is reserved for a sleeping deployment, and
    /// handing those six characters to a stranger points one memorised code at
    /// two different games — the exact failure reservations exist to prevent.
    /// A few seconds of "try again" is the cheap side of that trade.
    #[test]
    fn a_relay_that_has_not_pulled_a_snapshot_refuses_rather_than_minting() {
        let relay = TestRelay::managed(TablePolicy::with(KEY, 20).unprimed());
        let why = refusal(
            RelayHost::host_keyed(&relay.addr(), KEY, None),
            "an unprimed relay must not mint",
        );
        assert!(
            why.contains("starting up"),
            "the host needs to know this is a wait, not a rejection: {why}"
        );
    }

    #[test]
    fn an_unknown_key_is_refused_with_different_words_to_a_missing_one() {
        let relay = TestRelay::managed(TablePolicy::with(KEY, 20));
        let missing = refusal(RelayHost::host(&relay.addr()), "keyless is refused");
        let unknown = refusal(
            RelayHost::host_keyed(&relay.addr(), "fk_live_NOPE", None),
            "an unknown key is refused",
        );
        assert!(
            unknown.contains("project.ron"),
            "point at where the key lives: {unknown}"
        );
        assert_ne!(
            missing, unknown,
            "two different problems must not read identically"
        );
    }

    /// **The refusal arrives as a refusal.** This is the bug the managed work
    /// found in the existing code: the host leg parsed `Refused` into `_ => {}`,
    /// so every one of these sentences was dropped and the caller waited three
    /// seconds and then reported "no lobby code (is a relay running there?)" —
    /// which is both unhelpful and false, since the relay is plainly running
    /// and plainly answering. Watched failing.
    #[test]
    fn a_refusal_reaches_the_host_instead_of_a_timeout_that_blames_the_relay() {
        let relay = TestRelay::managed(TablePolicy::with(KEY, 20));
        let t0 = std::time::Instant::now();
        let err = refusal(RelayHost::host(&relay.addr()), "refused");
        assert!(
            !err.contains("is a relay running there"),
            "the timeout message means the answer was thrown away: {err}"
        );
        assert!(
            t0.elapsed() < Duration::from_secs(2),
            "it answered immediately; waiting {:?} means we polled past the answer",
            t0.elapsed()
        );
    }

    /// A good key hosts, and the code carries the **region letter** so a client
    /// can map it back to a relay without asking anybody — which is what keeps
    /// the join path independent of the control plane.
    #[test]
    fn a_good_key_hosts_and_the_code_carries_its_region() {
        let relay = TestRelay::managed(TablePolicy::with(KEY, 20));
        let (_t, code) = RelayHost::host_keyed(&relay.addr(), KEY, None).expect("hosts");
        assert_eq!(code.len(), 6, "region letter + five: {code}");
        assert!(code.starts_with('U'), "us-east is U: {code}");
    }

    /// The CCU cap turns away **the next arrival**, and the sentence names the
    /// plan and where to change it. A cap that dropped a live player would be a
    /// worse product than no cap at all.
    #[test]
    fn the_cap_refuses_the_next_joiner_and_never_the_live_session() {
        // Two: the host, and one client. The second client is over.
        let relay = TestRelay::managed(TablePolicy::with(KEY, 2));
        let (mut host, code) = RelayHost::host_keyed(&relay.addr(), KEY, None).expect("hosts");
        let mut first = RelayClient::join(&relay.addr(), &code).expect("connects");
        // `join` is non-blocking by design — the verdict arrives on `poll`,
        // which is how a real client learns it too.
        let admitted = settle(&mut first, &mut host);
        assert_eq!(
            admitted, None,
            "the first joiner is under the cap: {admitted:?}"
        );

        let mut second = RelayClient::join(&relay.addr(), &code).expect("connects");
        let msg = settle(&mut second, &mut host)
            .expect("the 2nd client was admitted over a 2-player cap");
        assert!(msg.contains("limit"), "say what happened: {msg}");
        assert!(
            msg.contains("fopull.com/cloud"),
            "…and where to fix it: {msg}"
        );

        // **The live session is untouched.** Nobody was dropped to make room —
        // a cap that evicted a player mid-game would be a worse product than
        // no cap at all.
        assert_eq!(
            settle(&mut first, &mut host),
            None,
            "the seated player was disturbed"
        );
    }
}

/// **The relay's own limits** — what one connection, one address and one
/// lobby may do on any relay, the open one included. Each cap is tripped by
/// exactly one and the refusal or drop asserted; the byte budget asserts the
/// count moved, not merely that nothing arrived.
#[cfg(test)]
mod limit_tests {
    use super::tests::TestRelay;
    use super::*;
    use std::sync::atomic::Ordering;

    fn drain(host: &mut RelayHost) -> Vec<Incoming> {
        let mut out = Vec::new();
        for _ in 0..40 {
            out.extend(host.poll());
            std::thread::sleep(Duration::from_millis(5));
        }
        out
    }

    /// The refusal a joiner is given, if any.
    fn join_refusal(addr: &str, code: &str) -> Option<String> {
        let mut c = match RelayClient::join(addr, code) {
            Ok(c) => c,
            Err(e) => return Some(e),
        };
        for _ in 0..80 {
            for inc in c.poll() {
                if let Incoming::Disconnected(_, Some(reason)) = inc {
                    return Some(reason);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }

    #[test]
    fn a_relay_stops_at_its_lobby_limit_and_says_so() {
        let relay = TestRelay::limited(RelayLimits {
            max_lobbies: 2,
            ..RelayLimits::default()
        });
        let _a = RelayHost::host(&relay.addr()).expect("first");
        let _b = RelayHost::host(&relay.addr()).expect("second");
        let e = RelayHost::host(&relay.addr())
            .err()
            .expect("the third opened a lobby past the limit");
        assert!(e.contains("lobby limit"), "{e}");
        assert_eq!(relay.lobbies.load(Ordering::Relaxed), 2);
        assert!(
            relay.drops.load(Ordering::Relaxed) >= 1,
            "the refusal was not counted"
        );
    }

    #[test]
    fn one_connection_hosts_one_lobby() {
        let relay = TestRelay::start();
        let (mut host, code) = RelayHost::host(&relay.addr()).expect("hosts");
        // A second host request on the same connection.
        host.inner
            .send(SERVER, Channel::Reliable, &RelayMsg::Host.encode());
        let refused = drain(&mut host).into_iter().find_map(|i| match i {
            Incoming::Disconnected(_, Some(r)) => Some(r),
            _ => None,
        });
        // The refusal lands as a `Refused` the host reports; either way the
        // relay still holds exactly one lobby, under the first code.
        assert_eq!(
            relay.lobbies.load(Ordering::Relaxed),
            1,
            "a second lobby was opened: {refused:?}"
        );
        assert_eq!(host.lobby_code().as_deref(), Some(code.as_str()));
        assert!(
            relay.drops.load(Ordering::Relaxed) >= 1,
            "the second host was not counted as refused"
        );
    }

    #[test]
    fn a_lobby_stops_at_its_client_limit_and_the_next_joiner_is_told() {
        let relay = TestRelay::limited(RelayLimits {
            max_clients_per_lobby: 2,
            ..RelayLimits::default()
        });
        let (mut host, code) = RelayHost::host(&relay.addr()).expect("hosts");
        let mut a = RelayClient::join(&relay.addr(), &code).expect("a");
        let mut b = RelayClient::join(&relay.addr(), &code).expect("b");
        // Let both seat before the third asks.
        for _ in 0..40 {
            let _ = host.poll();
            let _ = a.poll();
            let _ = b.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        let why =
            join_refusal(&relay.addr(), &code).expect("the third joiner was seated past the limit");
        assert!(why.contains("full"), "{why}");
    }

    #[test]
    fn an_address_may_open_only_so_many_lobbies_a_minute() {
        let relay = TestRelay::limited(RelayLimits {
            opens_per_address: 2,
            ..RelayLimits::default()
        });
        let _a = RelayHost::host(&relay.addr()).expect("first");
        let _b = RelayHost::host(&relay.addr()).expect("second");
        let e = RelayHost::host(&relay.addr())
            .err()
            .expect("a third lobby from one address inside the window");
        assert!(e.contains("this address"), "{e}");
    }

    #[test]
    fn an_address_may_join_only_so_many_times_a_minute() {
        let relay = TestRelay::limited(RelayLimits {
            joins_per_address: 1,
            ..RelayLimits::default()
        });
        let (mut host, code) = RelayHost::host(&relay.addr()).expect("hosts");
        let mut a = RelayClient::join(&relay.addr(), &code).expect("a");
        for _ in 0..40 {
            let _ = host.poll();
            let _ = a.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        let why = join_refusal(&relay.addr(), &code)
            .expect("a second join from one address inside the window");
        assert!(why.contains("this address"), "{why}");
    }

    /// The byte budget: a connection over it has its messages dropped and
    /// the count moves; over it for three windows running, it is closed.
    #[test]
    fn a_connection_over_its_byte_budget_is_dropped_counted_and_then_closed() {
        let relay = TestRelay::limited(RelayLimits {
            bytes_per_window: 4096,
            window: Duration::from_millis(60),
            strikes: 3,
            ..RelayLimits::default()
        });
        let (mut host, code) = RelayHost::host(&relay.addr()).expect("hosts");
        let mut c = RelayClient::join(&relay.addr(), &code).expect("joins");
        for _ in 0..40 {
            let _ = host.poll();
            let _ = c.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        let before = relay.drops.load(Ordering::Relaxed);
        // Six kilobytes in one window: the second message is over.
        c.send(SERVER, Channel::Reliable, &[7u8; 3000]);
        c.send(SERVER, Channel::Reliable, &[7u8; 3000]);
        std::thread::sleep(Duration::from_millis(30));
        let got = drain(&mut host)
            .iter()
            .filter(|i| matches!(i, Incoming::Message(..)))
            .count();
        assert_eq!(got, 1, "the over-budget message was forwarded");
        assert!(
            relay.drops.load(Ordering::Relaxed) > before,
            "the drop was not counted"
        );
        // Keep it up for three windows: closed.
        let mut closed = false;
        for _ in 0..12 {
            c.send(SERVER, Channel::Reliable, &[7u8; 3000]);
            c.send(SERVER, Channel::Reliable, &[7u8; 3000]);
            if c.poll()
                .iter()
                .any(|i| matches!(i, Incoming::Disconnected(SERVER, _)))
            {
                closed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        assert!(closed, "a repeat offender was never closed");
    }

    /// A client cannot push a frame at a host that is bigger than anything a
    /// client legitimately sends; a host's ceiling is the larger one, sized
    /// from the largest prefab a game ships.
    #[test]
    fn oversized_reliable_frames_are_dropped_on_the_relay_leg() {
        let relay = TestRelay::start();
        let (mut host, code) = RelayHost::host(&relay.addr()).expect("hosts");
        let mut c = RelayClient::join(&relay.addr(), &code).expect("joins");
        for _ in 0..40 {
            let _ = host.poll();
            let _ = c.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        let before = relay.drops.load(Ordering::Relaxed);
        c.send(
            SERVER,
            Channel::Reliable,
            &vec![1u8; MAX_CLIENT_RELIABLE + 1],
        );
        c.send(SERVER, Channel::Reliable, &[2u8; 64]);
        let got: Vec<usize> = drain(&mut host)
            .iter()
            .filter_map(|i| match i {
                Incoming::Message(_, _, b) => Some(b.len()),
                _ => None,
            })
            .collect();
        assert_eq!(
            got,
            vec![64],
            "the oversized frame reached the host: {got:?}"
        );
        assert!(
            relay.drops.load(Ordering::Relaxed) > before,
            "the drop was not counted"
        );
        // The host's ceiling is above a client's and above a real prefab.
        const { assert!(MAX_HOST_RELIABLE > MAX_CLIENT_RELIABLE && MAX_HOST_RELIABLE >= 4 * 28_248) };
    }

    /// An empty lobby ends after the idle window; a dedicated server's does
    /// not, however long it waits.
    #[test]
    fn an_idle_lobby_ends_unless_its_host_is_a_dedicated_server() {
        let relay = TestRelay::limited(RelayLimits {
            idle_lobby: Duration::from_millis(120),
            ..RelayLimits::default()
        });
        let (mut idle, _) = RelayHost::host(&relay.addr()).expect("hosts");
        let (mut server, _) = RelayHost::host(&relay.addr()).expect("hosts");
        server.declare_dedicated();
        std::thread::sleep(Duration::from_millis(250));
        let _ = drain(&mut idle);
        let _ = drain(&mut server);
        assert_eq!(
            relay.lobbies.load(Ordering::Relaxed),
            1,
            "the idle lobby survived, or the server's did not"
        );
        assert!(server.lobby_code().is_some());
    }
}
