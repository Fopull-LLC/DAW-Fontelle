//! Offline musical analysis of audio (docs/analyze-musically-plan.md).
//!
//! Pure: it is handed samples and returns notes, pitch tracks, keys and
//! confidences. Nothing here touches the UI, the engine or the disk, and none
//! of it may run on the RT thread (INVARIANT 1) — it allocates freely.

pub mod analysis;
pub mod cache;
pub mod chords;
pub mod comp;
pub mod confidence;
pub mod denoise;
pub mod edit;
pub mod key;
pub mod mono;
pub mod onsets;
pub mod render;
pub mod resample;
pub mod resynth;
pub mod slice;
pub mod spectrogram;
pub mod testsignals;
pub mod transcribe;
