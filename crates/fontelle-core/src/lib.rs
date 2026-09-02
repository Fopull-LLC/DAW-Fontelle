//! The Fontelle sampler engine (FONTELLE_TDD.md §7). RT-safe. Must not depend on
//! anything above it in the workspace — no DAW, no document model, no files, no GUI
//! (INVARIANT 4). If an addition here needs any of those, escalate rather than reach
//! for "just this one thing from the model layer."

mod mod_matrix;
mod patch;
mod patch_format;
pub mod patch_params;
mod playback;
mod sampler;
mod streaming;
mod voice;

pub use mod_matrix::{Curve, ModDest, ModMatrix, ModRoute, ModSource};
pub use patch::{FilterSlot, Layer, Lfo, Patch, SILENT_DB, Source, ZoneId};
pub use patch_format::{
    LoadedPatch, PATCH_FORMAT_VERSION, PatchFormatError, UnresolvedSample, referenced_samples,
};
pub use playback::{LoopMode, PlaybackConfig};
pub use sampler::{PrepareContext, Sampler, SamplerContext};
pub use streaming::{SampleBuffer, SampleStore};
pub use voice::{
    NoteTrigger, OSC_ROOT_HZ, RetriggerMode, StealPolicy, UnisonConfig, Voice, VoiceConfig,
    VoicePool, velocity_to_gain,
};
