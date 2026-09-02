//! Compiles `fontelle-model`'s document into a flat, immutable, sample-timestamped
//! event timeline (FONTELLE_TDD.md §11). The RT thread never sees the prefab graph
//! (INVARIANT 3) — this crate is the mechanism that makes that true.

mod collision;
mod compile;
mod incremental;

pub use collision::voice_context_for_clip;
pub use compile::{compile, sort_events};
pub use incremental::{DirtyBars, recompile_dirty};
