//! Working on one song together over the internet (`docs/collab-plan.md`).
//!
//! The seam between a shared session and the wire. A session speaks
//! [`Transport`] and never a socket, so the same host and joiner code runs
//! over [`MemoryHub`] — two studios in one process, which is how every test
//! of the feature runs — and over the Floptle relay.
//!
//! # Lifted, not written
//!
//! `transport.rs`, `quic.rs` and `relay.rs` are the Floptle engine's own
//! (`Fopull-LLC/Floptle`, `crates/floptle-net/src/` at `16481c30`, v0.97.1
//! "Square On"), copied rather than depended on because they live inside a
//! crate that brings the game engine with it (§9.1; hub card
//! `tasks/fontelle/0265`). They are kept as they were — formatting aside —
//! so a diff against the engine's shows only what the engine changed; a
//! change here that is not in the engine is a change to argue for there.
//! Three of their tests stay behind, because they drive the engine's game
//! session: each is marked where it was, and `tests/relay.rs` is Fontelle's
//! end-to-end test of the same path. The pinned `RelayMsg` variant-order test
//! came over with the rest: a variant the engine adds is a message this copy
//! cannot read, and that test is what notices.
//!
//! Depends on nothing in the workspace but `fontelle-types` (INVARIANT 4's
//! spirit): the messages a session sends are `fontelle_model::wire`'s, and the
//! app joins the two.

mod connect;
mod framed;
mod paced;
mod quic;
mod relay;
mod transport;

pub use connect::{FONTELLE_CLOUD_KEY, REGIONS, Relay, host, join, reclaim};
pub use framed::{FRAME, Framed};
pub use paced::{Clock, PACE, Paced};

pub use quic::{QuicClient, QuicServer, ServerCertificate, SocketBuffers};
pub use relay::{
    HOST_DECISION_DEADLINE, HOST_GRACE, HostAdmission, JoinAdmission, LobbyEnd,
    MAX_CLIENT_RELIABLE, MAX_HOST_RELIABLE, RelayClient, RelayHost, RelayLimits, RelayPolicy,
    RelayServer,
};
pub use transport::{
    Channel, Incoming, LinkStats, MemoryHub, MemoryTransport, PeerId, SERVER, Transport, Wake,
};
