//! RT-safe DSP primitives (FONTELLE_TDD.md §4.1): filters, envelopes,
//! oscillators, interpolators, meters, a pitch tracker, a PSOLA shifter, a
//! modal resonator bank and the wavetables. No allocation after
//! construction, no I/O, and no knowledge of the DAW above it — every type
//! here is a building block that `fontelle-core` (the instruments) and
//! `fontelle-fx` (the effects) assemble, and neither of those is allowed to
//! reach past it (INVARIANT 4).
//!
//! The one rule that shapes everything here: a `process` call on the audio
//! thread must be a pure function of its state and its input. Sizing,
//! tables and coefficients that need memory are made at construction or on
//! `prepare`, off that thread.

mod drum;
mod envelope;
mod filter;
mod interpolation;
mod meter;
mod modal;
mod oscillator;
mod oversample;
mod pitch;
mod psola;
mod spectrum;
mod synth_filter;
mod synth_osc;
mod wavetable;

pub use drum::{DECAY_SPAN_DB, DrumBody, DrumModel, DrumSynth, DrumVoice};
pub use envelope::{
    DECIBEL_SPAN_DB, EnvStage, EnvelopeConfig, EnvelopeCurve, EnvelopeGenerator, EnvelopeStage,
    shape_progress,
};
pub use filter::{DcBlocker, SvfCoeffs, SvfFilter, SvfMode};
pub use interpolation::{Interpolation, interpolate};
pub use meter::PeakRmsMeter;
pub use modal::{MAX_MODES, ModalBank, ModalMode};
pub use oscillator::{OscKind, Oscillator};
pub use oversample::{
    Decimator, Interpolator, MAX_OVERSAMPLE, OVERSAMPLE_TAPS, Oversampling, oversample_kernel,
};
pub use pitch::{
    DEFAULT_TRACKING_THRESHOLD, PitchFrame, PitchTracker, RELAXED_TRACKING_THRESHOLD,
    STRICT_TRACKING_THRESHOLD, cents_to_hz, hz_to_cents,
};
pub use psola::{GrainEngine, MAX_GRAIN_MS, MIN_GRAIN_MS, PsolaShifter};
pub use spectrum::{
    SPECTRUM_FLOOR_DB, SPECTRUM_SIZE, SpectrumAnalyser, bin_width_hz, fft_in_place,
};
pub use synth_filter::{
    COMB_LEN, FilterModel, FilterSlope, SynthFilter, SynthFilterSettings, clean_coeffs,
    key_tracked_cutoff, response_db,
};
pub use synth_osc::{
    FilterRoute, GRAIN_MAX_MS, GRAIN_MIN_MS, GRAINS, MAX_PARTIALS, MAX_UNISON, OSC_FIXED_HZ,
    STRING_UNISON, SampleData, SampleLoop, SampleSettings, StringModel, StringPartials, SynthInput,
    SynthOsc, SynthSource, SynthState, Unison, WarpMode, string_partials,
};
pub use wavetable::{
    MAX_USER_FRAMES, VOWEL_FORMANTS, WAVETABLE_LEN, WAVETABLE_LEVELS, Wavetable, WavetableBank,
    WavetableId, vowel_at, wavetable_level_for, wavetables,
};
