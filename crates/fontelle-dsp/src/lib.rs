mod drum;
mod envelope;
mod filter;
mod interpolation;
mod meter;
mod oscillator;
mod spectrum;
mod synth_filter;
mod synth_osc;
mod wavetable;

pub use drum::{DECAY_SPAN_DB, DrumBody, DrumModel, DrumSynth, DrumVoice};
pub use envelope::{
    DECIBEL_SPAN_DB, EnvelopeConfig, EnvelopeCurve, EnvelopeGenerator, EnvelopeStage,
};
pub use filter::{DcBlocker, SvfCoeffs, SvfFilter, SvfMode};
pub use interpolation::{Interpolation, interpolate};
pub use meter::PeakRmsMeter;
pub use oscillator::{OscKind, Oscillator};
pub use spectrum::{
    SPECTRUM_FLOOR_DB, SPECTRUM_SIZE, SpectrumAnalyser, bin_width_hz, fft_in_place,
};
pub use synth_filter::{
    COMB_LEN, FilterModel, FilterSlope, SynthFilter, SynthFilterSettings, clean_coeffs,
    key_tracked_cutoff, response_db,
};
pub use synth_osc::{
    FilterRoute, MAX_UNISON, OSC_FIXED_HZ, SynthOsc, SynthSource, SynthState, Unison, WarpMode,
};
pub use wavetable::{
    VOWEL_FORMANTS, WAVETABLE_LEN, WAVETABLE_LEVELS, Wavetable, WavetableBank, WavetableId,
    vowel_at, wavetable_level_for, wavetables,
};
