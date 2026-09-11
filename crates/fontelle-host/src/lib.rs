//! Loading and running plugins somebody else wrote (TDD §8.4).
//!
//! > *"the next natural step would be trying to actually use a real third
//! > party vst in our modular instrument system."*
//!
//! §8.4 said the boundary was already there — *"the `AudioNode` trait (§5.1)
//! and the parameter contract (§8.2) are the entire boundary a future CLAP
//! host would plug into"* — and told whoever wrote this not to build
//! speculative abstractions before there was a host. That turned out to be
//! exactly right, and this crate is what fitting into it looks like: a plugin
//! is a source of audio with a list of parameters, which is what every node in
//! `fontelle-engine` already was.
//!
//! # What is here
//!
//! - [`PluginScan`] — what is installed on this machine, read off the folders
//!   the format says to look in.
//! - [`PluginHost`] — the one thing that dlopens a bundle, and the cache that
//!   makes sure a bundle holding twenty plugins is opened once.
//! - [`HostedPlugin`] — one running plugin, on the **main thread**: its
//!   parameters, its state, and the handle that activates it.
//! - [`HostedProcessor`] — the audio half, which is [`Send`] and goes into the
//!   graph. This split is CLAP's, not ours: the specification names which
//!   calls belong to which thread, and `clack` encodes that in the types.
//!
//! # Two formats, one seam
//!
//! §8.4's order of intent is CLAP first, LV2 second, VST3 through a bridge if
//! it is ever justified, and §3.4 says why: CLAP is MIT with nothing to sign
//! and lilv is ISC, while the VST3 SDK has historically wanted a signed
//! agreement to so much as *host* it. Both of the first two are here now —
//! see [`lv2`] for what is different about the second — and everything
//! public is named by [`PluginFormat`] rather than by either, so the format
//! is an arm in a `match` and not a second host. A key in a format this
//! build cannot load is refused with [`HostError::Unsupported`], which is a
//! sentence a person can act on — unless a **bridge** for that format is
//! installed, which is the third arm: see [`bridge`].
//!
//! # What a plugin is allowed to do to the audio thread
//!
//! INVARIANT 1 says our code never allocates, locks, blocks or syscalls on the
//! RT thread. A hosted plugin is not our code, and the invariant cannot follow
//! it across the boundary — CLAP's own contract is what binds it there. What
//! this crate guarantees is the half it owns: every buffer, event list and
//! port array a block needs is allocated in [`HostedPlugin::activate`], and
//! [`HostedProcessor::process_effect`] and its instrument twin do no more than
//! copy samples, drain atomics and call the plugin.
//!
//! [`PluginFormat`]: fontelle_types::PluginFormat

mod atom;
pub mod bridge;
pub mod gui;
// LV2 is loaded through `lilv`, a system library that lives on Linux where
// the format does. Elsewhere the same module names answer "not here" — see
// `lv2_stub.rs` for the shape and the reason.
#[cfg(target_os = "linux")]
pub mod lv2;
#[cfg(not(target_os = "linux"))]
#[path = "lv2_stub.rs"]
pub mod lv2;
mod lv2_state;
#[cfg(target_os = "linux")]
mod lv2_ui;
#[cfg(not(target_os = "linux"))]
#[path = "lv2_ui_stub.rs"]
mod lv2_ui;
mod param;
mod plugin;
mod processor;
mod scan;

pub use atom::{AtomPipe, AtomPipes, MAX_ATOM_BYTES, trace as atom_trace};
pub use bridge::{BridgeFailure, Bridges, bridge_search_paths};
pub use gui::{GuiError, GuiPoll, GuiSize, PluginWindow};
pub use lv2_state::{Lv2Property, Lv2State};
pub use param::{HostedParam, ParamValues};
pub use plugin::{EditorRequests, HostError, HostedPlugin, NoteDialect, PluginHost};
pub use processor::{HostedProcessor, ProcessorBay};
pub use scan::{
    PluginInfo, PluginScan, ScanFailure, scan_bundle, scan_bundle_with, search_paths,
    search_paths_with,
};
