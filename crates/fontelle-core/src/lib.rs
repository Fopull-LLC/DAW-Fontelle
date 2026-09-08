//! The Fontelle sampler engine (FONTELLE_TDD.md §7). RT-safe. Must not depend on
//! anything above it in the workspace — no DAW, no document model, no files, no GUI
//! (INVARIANT 4). If an addition here needs any of those, escalate rather than reach
//! for "just this one thing from the model layer."

mod drum_kit;
pub mod flopsynth;
mod lfo;
mod mod_matrix;
mod patch;
mod patch_format;
pub mod patch_params;
mod playback;
mod sampler;
mod streaming;
mod voice;
mod wavetable_set;

pub use drum_kit::{
    DrumKitStyle, DrumSlot, GM_DRUM_MAP, GmSlot, KIT_HEADROOM_DB, drum_kit, drum_slots,
};
pub use lfo::{LfoState, free_phase};
pub use mod_matrix::{Curve, ModDest, ModMatrix, ModRoute, ModSource};
pub use patch::{
    FilterSlot, Layer, Lfo, LfoMode, MACRO_COUNT, MAX_PATCH_FX, Macro, Patch, PatchFx, SILENT_DB,
    Source, ZoneId,
};
pub use patch_format::{
    LoadedPatch, PATCH_FORMAT_VERSION, PatchFormatError, UnresolvedSample, referenced_samples,
};
pub use playback::{LoopMode, PlaybackConfig};
pub use sampler::{PrepareContext, Sampler, SamplerContext};
pub use streaming::{AudioBuffer, AudioStore, SampleBuffer, SampleStore};
pub use voice::{
    DEFAULT_BEND_RANGE_SEMITONES, FILTER_STEP, MAX_LFOS, NoteTrigger, OSC_ROOT_HZ, Performance,
    RenderClock, RetriggerMode, StealPolicy, UnisonConfig, Voice, VoiceConfig, VoicePool,
    velocity_to_gain,
};
pub use wavetable_set::WavetableSet;
