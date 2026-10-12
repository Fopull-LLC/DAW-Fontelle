//! The audio exchange between Fontelle and a driver host in another process.
//!
//! Ty, 2026-10-12: users on Windows hear clicks, and ASIO is what their
//! interfaces are made for. Steinberg's SDK can now be used under GPLv3, but
//! Fontelle is MIT/Apache and loads closed add-ons, so the SDK lives in a
//! **separate program**, `fontelle-asio` (`crates/fontelle-asio-host`, GPL),
//! and the two talk through what is in this crate and nothing else: plain
//! numbers and `f32` frames in a block of shared memory, two doorbells, and a
//! few lines of text on the helper's standard input and output. Neither side
//! ever loads the other's code. The whole protocol is written down in
//! `docs/asio.md`; it is public and versioned ([`PROTOCOL_VERSION`]).
//!
//! - [`layout`]: where everything is in the shared block.
//! - [`handshake`]: one buffer's round trip — the helper rings, Fontelle
//!   renders, Fontelle rings back — with a deadline, so a late Fontelle is a
//!   counted dropout and never a stalled driver.
//! - [`diagnostics`]: telling a device that missed a buffer from a Fontelle
//!   that missed its deadline, and how busy the audio thread is.
//! - [`control`]: the text lines.
//! - [`os`]: the shared memory and the doorbells on Windows (named events)
//!   and Linux (a futex on the shared word), behind [`Doorbell`].
//!
//! Everything but [`os`] is plain Rust over a block of bytes, so the tests run
//! on any machine.

pub mod control;
pub mod diagnostics;
pub mod handshake;
pub mod layout;
pub mod os;

pub use control::{ClientLine, HelperEvent, HelperLine, ReadyInfo, RunArgs};
pub use diagnostics::{GapDetector, LoadMeter};
pub use handshake::{ClientSide, Cycle, Doorbell, Exchange, HelperSide, Wake};
pub use layout::{HEADER_BYTES, Header, Layout, MAGIC, PROTOCOL_VERSION, Shared, State};
pub use os::{Bells, PollBell, Region, channel_name};
