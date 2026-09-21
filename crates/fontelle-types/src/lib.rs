//! The vocabulary every crate shares (FONTELLE_TDD.md §4): identity,
//! addressing, time, events, and the configuration types that cross the
//! `fontelle-core` / `fontelle-model` / `fontelle-engine` boundary.
//!
//! It sits at the bottom of the workspace on purpose. A document holds a
//! `ChannelId`; the sequencer stamps a `TimedEvent` with a `Sample`; a
//! parameter is named by a `ParamAddress` that never changes once shipped
//! (INVARIANT 7); an effect's settings are an `EffectConfig` the window
//! edits, the model stores and the engine realises — and none of those three
//! crates may depend on the other two, so the types they agree on live here.
//! Everything is plain data: `serde` on what a project file carries, no
//! behaviour beyond validation, ranges and the arithmetic that keeps two
//! readings of a number the same.

mod asset;
mod audio_clip;
mod base64;
mod effect;
mod effect_next;
mod effect_presets;
mod event;
mod favorite;
mod id;
mod import;
mod instrument;
mod lfo_shape;
mod pan;
mod param;
mod patch_data;
mod plugin;
mod preset;
mod time;
mod wavetable_edit;

pub use asset::{AssetKind, AssetRef};
pub use audio_clip::{
    AudioClipData, AudioPlacement, ClipLoopMode, ClipStretch, Fade, FadeCurve, MAX_CLIP_GAIN_DB,
    MAX_CLIP_SPEED, MIN_CLIP_GAIN_DB, MIN_CLIP_SPEED, bend, tension_for_midpoint,
};
pub use base64::{decode_base64, encode_base64};
pub use effect::{
    BANDS, BUTTERWORTH_Q, BandChannel, BandType, BitcrushConfig, BitcrushPreset,
    CHORUS_TONE_OPEN_HZ, ChorusConfig, ChorusMode, CompressorConfig, Decimation, DelayConfig,
    DetectionMode, DistortionConfig, DistortionCurve, DistortionPreset, Dither, EffectConfig,
    EffectKind, EqBand, EqConfig, FILTER_MOD_OCTAVES, FilterConfig, FilterShape, GATE_FLOOR_DB,
    GATE_KEY_OFF_HZ, GateConfig, LIMITER_LOOKAHEAD_MS, LfoWave, LimiterConfig, MAX_BITS,
    MAX_CHORUS_DELAY_MS, MAX_CHORUS_VOICES, MAX_CRUSH_RATE_HZ, MAX_DELAY_MS, MAX_FILTER_HZ,
    MAX_GATE_LOOKAHEAD_MS, MAX_GATE_RATIO, MAX_LFO_RATE_HZ, MAX_PRE_DELAY_MS, MAX_TUNE_GRAIN_MS,
    MAX_TUNE_RETUNE_MS, MIN_CHORUS_DELAY_MS, MIN_FILTER_HZ, MIN_LFO_RATE_HZ, MIN_TUNE_GRAIN_MS,
    MIN_TUNE_RETUNE_MS, MIX, NoteDivision, Oversampling, Quantiser, ReverbConfig, SoftenConfig,
    SoftenPreset, TUNE_FLEX_EDGE_CENTS, TUNE_FROM_MIDI, TUNE_INSTANT_MS, TUNE_LOCK_CENTS,
    TUNE_LOCKED, TUNE_NOTE_PARAMS, TUNE_ROOTS, TUNE_VOICED, TuneConfig, TuneControl, TuneEngine,
    TuneFrame, TuneMode, TunePreset, TuneRange, TuneScale, UTILITY_DC_OFF_HZ, UTILITY_MONO_OFF_HZ,
    UtilityConfig, VibratoShape, cents_of_hz, hz_of_cents,
};
pub use effect_next::{
    FlangerConfig, FoldConfig, HyperConfig, MAX_FLANGER_DELAY_MS, MAX_HYPER_VOICES,
    MAX_HYPER_WINDOW_MS, MAX_PHASER_STAGES, MAX_SHIFT_HZ, MIN_FLANGER_DELAY_MS,
    MIN_HYPER_WINDOW_MS, MIN_PHASER_STAGES, MultibandConfig, PHASER_SWEEP_OCTAVES, PhaserConfig,
    ShiftDirection, ShifterConfig, WIDTH_MONO_OFF_HZ, WidthConfig,
};
pub use effect_presets::{
    ChorusPreset, CompressorPreset, DelayPreset, EqPreset, FilterPreset, FlangerPreset, FoldPreset,
    GatePreset, HyperPreset, LimiterPreset, MultibandPreset, PhaserPreset, ReverbPreset,
    ShifterPreset, UtilityPreset, WidthPreset,
};
pub use event::{CompiledTimeline, DEFAULT_BPM, EventPayload, EventSink, TimedEvent, VoiceOrigin};
pub use favorite::Favorite;
pub use id::{
    AssetId, AudioInputId, ChannelId, ClipId, LaneId, MarkerId, MixerTrackId, NodeId, NoteId,
    PersistentId, PointId, PrefabId,
};
pub use import::{FolderKind, is_multisample_path};
pub use instrument::InstrumentKind;
pub use lfo_shape::{LfoPoint, LfoShape, LfoShapeMode, MAX_LFO_POINTS};
pub use pan::{PanLaw, pan_unit};
pub use param::{
    ParamAddress, ParamSection, ParamSpec, ParamTarget, TEMPO_MAX_BPM, TEMPO_MIN_BPM, Taper, Unit,
    normalised_tempo, tempo_from_normalised,
};
pub use patch_data::{PatchData, SampleRef};
pub use plugin::{PluginFormat, PluginKey, PluginParamValue, PluginState};
pub use preset::{
    DeviceKind, PRESET_FORMAT_VERSION, Preset, PresetOrigin, PresetPayload, PresetRef, TrackChain,
    TrackInsert, TrackPreset,
};
pub use time::{PPQN, Sample, Tick};
pub use wavetable_edit::{WaveTool, WavetableEdit};
