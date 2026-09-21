//! The Fontelle sampler engine (FONTELLE_TDD.md §7). RT-safe. Must not depend on
//! anything above it in the workspace — no DAW, no document model, no files, no GUI
//! (INVARIANT 4). If an addition here needs any of those, escalate rather than reach
//! for "just this one thing from the model layer."

mod drum_kit;
pub mod factory_samples;
pub mod flopsynth;
pub mod formula;
mod lfo;
mod mod_matrix;
pub mod mod_sources;
mod patch;
mod patch_format;
pub mod patch_params;
mod playback;
mod sampler;
mod streaming;
mod voice;
mod wavetable_edit;
mod wavetable_set;

pub use drum_kit::{
    DrumKitStyle, DrumSlot, GM_DRUM_MAP, GmSlot, KIT_HEADROOM_DB, drum_kit, drum_slots,
};
pub use lfo::{LfoLive, LfoState, free_phase};
pub use mod_matrix::{Curve, ModDest, ModMatrix, ModRoute, ModSource};
pub use patch::{
    FILTER_FM_OCTAVES, FilterSlot, Layer, Lfo, LfoMode, MACRO_COUNT, MAX_PATCH_FX, Macro, Patch,
    PatchFx, SILENT_DB, SampleZone, Source, UserSample, UserWavetable, ZoneId, key_ranges,
    note_name,
};
pub use patch_format::{
    LoadedPatch, PATCH_FORMAT_VERSION, PatchFormatError, UnresolvedSample, referenced_samples,
};
pub use playback::{LoopMode, PlaybackConfig};
pub use sampler::{PrepareContext, Sampler, SamplerContext};
pub use streaming::{AudioBuffer, AudioStore, SampleBuffer, SampleStore};
pub use voice::{
    DEFAULT_BEND_RANGE_SEMITONES, FILTER_STEP, GlideCurve, GlideMode, MAX_LFOS, MAX_MOD_ENVELOPES,
    NoteMod, NoteTrigger, OSC_ROOT_HZ, Performance, RenderClock, RetriggerMode, StealPolicy,
    UnisonConfig, VelocityCurve, Voice, VoiceConfig, VoicePool, velocity_gain, velocity_to_gain,
};
pub use wavetable_edit::{EDIT_HARMONICS, WavetableEdit};
pub use wavetable_set::{WavetableSet, spectral_analysis};
