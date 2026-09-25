//! Working on one song together over the internet (`docs/collab-plan.md`).
//!
//! The seam between a shared session and the wire. A session speaks
//! [`Transport`] and never a socket, so the same host and joiner code runs
//! over [`MemoryHub`] — two studios in one process, which is how every test
//! of the feature runs — and over the Floptle relay.
//!
//! # Lifted, not written
//!
//! `transport.rs` is the Floptle engine's own (`Fopull-LLC/Floptle`,
//! `crates/floptle-net/src/transport.rs` at `16481c30`, v0.97.1 "Square On"),
//! copied rather than depended on because it lives inside a crate that
//! brings the game engine with it (§9.1). It is kept as it was — formatting
//! aside — so a diff against the engine's shows only what the engine changed;
//! a change here that is not in the engine is a change to argue for there
//! (the hub card to E, §16). The relay's client (`quic.rs`, `relay.rs`)
//! comes over the same way in Phase 3.
//!
//! Depends on nothing in the workspace but `fontelle-types` (INVARIANT 4's
//! spirit): the messages a session sends are `fontelle_model::wire`'s, and the
//! app joins the two.

mod transport;

pub use transport::{
    Channel, Incoming, LinkStats, MemoryHub, MemoryTransport, PeerId, SERVER, Transport,
};
