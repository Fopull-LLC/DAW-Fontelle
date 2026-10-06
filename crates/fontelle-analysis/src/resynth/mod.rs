//! Moving a note's pitch in recorded audio (`docs/analyze-musically-plan.md`
//! §2.4).
//!
//! One plain seam, [`Resynth`]: an engine is handed a span of audio, the
//! pitch track under it and the ratio to move each frame of that track by,
//! and hands back the span moved. Which engine a study used is its `id`,
//! stored in the study, permanent. [`Psola`] is the standard one.
//!
//! An engine never decides *what* to move: the edited contour (the shift
//! eased in and out, the drift flattened, the vibrato scaled) is
//! [`crate::render`]'s, and so is splicing the span back.

mod psola;

pub use psola::Psola;

use crate::mono::F0Track;

/// What an engine is asked to move.
#[derive(Debug, Clone)]
pub struct SpanRequest<'a> {
    /// The whole audio, interleaved: an engine reads around the span (a grain
    /// reaches a period either side).
    pub input: &'a [f32],
    pub channels: usize,
    pub sample_rate: u32,
    /// The pitch track of the audio from frame `track_start` on: frame `i`
    /// of it sits at input frame `track_start + i·hop·rate`.
    pub f0: &'a F0Track,
    pub track_start: usize,
    /// For each of the track's frames, the factor to move the pitch by (1
    /// leaves it). Unvoiced frames are never moved, whatever this says.
    pub ratio: &'a [f32],
    /// The formants' shift, in cents (0 keeps the voice's own envelope).
    pub formant_cents: f32,
    /// The frames wanted back, half open.
    pub span: std::ops::Range<usize>,
}

/// Why an engine could not move a span.
#[derive(Debug, Clone, PartialEq)]
pub struct ResynthError(pub String);

impl std::fmt::Display for ResynthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ResynthError {}

/// A pitch-moving engine. Offline: it allocates freely and must never run on
/// the RT thread (INVARIANT 1).
pub trait Resynth: Send + Sync {
    /// Permanent: stored in the study (`StudyEngines::resynth`).
    fn id(&self) -> &'static str;
    /// `request.span`, moved, into `out` (interleaved, `span.len() ·
    /// channels` samples; whatever `out` held is replaced).
    fn render_span(
        &self,
        request: &SpanRequest<'_>,
        out: &mut Vec<f32>,
    ) -> Result<(), ResynthError>;
}
