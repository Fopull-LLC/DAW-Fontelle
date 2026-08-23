//! The Fontelle sampler engine (FONTELLE_TDD.md §7). RT-safe. Must not depend on
//! anything above it in the workspace — no DAW, no document model, no files, no GUI
//! (INVARIANT 4). If an addition here needs any of those, escalate rather than reach
//! for "just this one thing from the model layer."

mod mod_matrix;
mod patch;
mod playback;
mod sampler;
mod streaming;
mod voice;

pub use mod_matrix::{Curve, ModDest, ModMatrix, ModRoute, ModSource};
pub use patch::{FilterSlot, Layer, Lfo, Patch, Source, ZoneId};
pub use playback::{LoopMode, PlaybackConfig};
pub use sampler::{PrepareContext, Sampler, SamplerContext};
pub use streaming::{SampleResidency, SampleStore};
pub use voice::{RetriggerMode, StealPolicy, UnisonConfig, Voice, VoiceConfig, VoicePool};
