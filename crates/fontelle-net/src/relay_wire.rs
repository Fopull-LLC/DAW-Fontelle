//! What crosses a relay leg: the message vocabulary both ends speak, and the
//! end-to-end drop rule for sequenced traffic. Split from [`crate::relay`] so
//! a browser, which runs no relay and opens no QUIC socket, can still join a
//! relayed lobby over the leg it does have — see [`crate::relay_client`].

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::transport::Channel;

/// Wire channel tags inside relay messages.
pub(crate) const CH_RELIABLE: u8 = 0;
pub(crate) const CH_UNRELIABLE: u8 = 1;
pub(crate) const CH_SEQUENCED: u8 = 2;

pub(crate) fn channel_tag(c: Channel) -> u8 {
    match c {
        Channel::Reliable => CH_RELIABLE,
        Channel::Unreliable => CH_UNRELIABLE,
        Channel::UnreliableSequenced => CH_SEQUENCED,
    }
}

pub(crate) fn tag_channel(t: u8) -> Channel {
    match t {
        CH_RELIABLE => Channel::Reliable,
        CH_SEQUENCED => Channel::UnreliableSequenced,
        _ => Channel::Unreliable,
    }
}

/// Everything that crosses a relay leg.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum RelayMsg {
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
    /// Endpoint → relay: **this host is the managed deployment `id`.**
    ///
    /// Sent ahead of the host request, with [`RelayMsg::WantCode`]. A fleet box
    /// restarts a server by starting a new process while the relay may still
    /// hold the old one's connection: systemd stops and starts inside a few
    /// milliseconds, and a connection that went without a goodbye stays live
    /// until it times out. The new process asks for its own code 0.02 s later
    /// and, with the lobby apparently still hosted, was minted a different one.
    /// The same game key *and* the same deployment is the same server, so the
    /// relay hands it the lobby, players and all, and lets the stale
    /// connection go.
    ///
    /// Appended last, for the reason every variant above it gives; a relay
    /// that has never heard of it skips it and behaves as before.
    Deployment {
        id: String,
    },
    /// Relay → host, right after [`RelayMsg::Hosted`]: the secret that proves
    /// this host is the one that opened lobby `code`.
    ///
    /// A host that drops keeps its lobby for [`HOST_GRACE`], and getting it
    /// back has to be possible without a reservation (a listen host has none)
    /// and impossible for anyone else. The code cannot be the proof, since
    /// every player who joined knows it, and neither can the game key, which
    /// is in every copy of the game. Sixteen random bytes only this connection
    /// was told are.
    ///
    /// Appended last: a host that has never heard of it skips it.
    ReclaimToken {
        code: String,
        token: [u8; 16],
    },
    /// Host → relay, ahead of the host request with [`RelayMsg::WantCode`]:
    /// the token this host was given for the code it wants back. A held lobby
    /// whose token matches is handed back, players and all, with or without a
    /// reservation.
    ///
    /// Appended last: a relay that has never heard of it skips it, and the
    /// host is minted a fresh code as it always was.
    Reclaim {
        token: [u8; 16],
    },
}

impl RelayMsg {
    pub(crate) fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("relay messages always encode")
    }

    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
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

/// End-to-end sequenced-drop state: last seq delivered per (peer, channel).
#[derive(Default)]
pub(crate) struct SeqState {
    pub(crate) last: HashMap<(u64, u8), u64>,
}

impl SeqState {
    /// True when the message should be dropped (stale sequenced).
    pub(crate) fn stale(&mut self, peer: u64, channel: u8, seq: u64) -> bool {
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
