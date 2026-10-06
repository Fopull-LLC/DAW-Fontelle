//! A **study**: one piece of audio as Analyze Musically works on it
//! (`docs/analyze-musically-plan.md` §3.9).
//!
//! What is in the document is what a person *did*: the pitch edits, and the
//! file a render of them made. What the analysis *found* — the notes, the
//! key — is derived and cached beside the song, never saved in it, so a newer
//! model or another machine's rounding can never orphan an edit. That is why
//! an edit names a **span of samples** and not a detected note.

use crate::{AssetRef, ClipId, PersistentId, Sample};

/// Where a study's audio comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StudySource {
    /// An audio clip of the arrangement: the window opened from its menu.
    Clip(ClipId),
    /// Nothing in the arrangement any more (or never): a file, a recording,
    /// or a clip since deleted. Never lost — it is still listed.
    Standalone,
}

/// Melody (one voice, editable) or chords, as the person chose it; `None`
/// follows the analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StudyMode {
    Melody,
    Chords,
}

/// Which engines a study was made with: permanent strings, so a later build
/// knows whose numbers these are.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StudyEngines {
    /// What heard the notes.
    pub transcriber: String,
    /// What moves them (`fontelle-analysis`'s `Resynth::id`).
    pub resynth: String,
}

impl Default for StudyEngines {
    fn default() -> Self {
        Self {
            transcriber: "basic-pitch+pyin".to_string(),
            resynth: "psola".to_string(),
        }
    }
}

/// One note's edit, by the span it covered when it was made.
///
/// The edited pitch is `f0_in · 2^((shift(t) − flatten·drift(t) +
/// (vibrato − 1)·vib(t)) / 1200)`, where `shift(t)` eases in over
/// `glide_in_ms` from the start and out over `glide_out_ms` to the end (plan
/// §2.4): the singer's own scoops and vibrato survive a move, which is what
/// makes a moved note sound sung rather than tuned.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PitchEdit {
    /// The note's extent at edit time, in frames of the study's **original
    /// file** (not of the clip's span), half open.
    pub span: (Sample, Sample),
    /// How far the note moves, in cents.
    pub shift_cents: f32,
    /// How much of the slow drift about the centre is taken out, 0..1.
    pub flatten: f32,
    /// The vibrato's depth, as a factor of the sung one: 1 as sung, 0 none.
    pub vibrato: f32,
    /// How long the move takes to arrive and to leave, in milliseconds.
    pub glide_in_ms: f32,
    pub glide_out_ms: f32,
    /// The formants' shift, in cents (0 keeps the voice's own).
    #[serde(default)]
    pub formant_cents: f32,
    #[serde(default)]
    pub gain_db: f32,
    /// P6: a chord note moved experimentally, at the centre it was masked at.
    #[serde(default)]
    pub experimental_poly: Option<f32>,
}

/// The transition a moved note eases over when nobody has dragged its ends.
pub const DEFAULT_GLIDE_MS: f32 = 40.0;

impl PitchEdit {
    /// No change to the note over `span`: what a reset leaves.
    pub fn none(span: (Sample, Sample)) -> Self {
        Self {
            span,
            shift_cents: 0.0,
            flatten: 0.0,
            vibrato: 1.0,
            glide_in_ms: DEFAULT_GLIDE_MS,
            glide_out_ms: DEFAULT_GLIDE_MS,
            formant_cents: 0.0,
            gain_db: 0.0,
            experimental_poly: None,
        }
    }

    /// Whether this changes nothing a listener could hear: such an edit is
    /// not kept.
    pub fn is_identity(&self) -> bool {
        self.shift_cents.abs() < 0.05
            && self.flatten.abs() < 1e-3
            && (self.vibrato - 1.0).abs() < 1e-3
            && self.formant_cents.abs() < 0.05
            && self.gain_db.abs() < 1e-3
    }

    /// Whether this edit's span and `other` share a sample.
    pub fn overlaps(&self, other: (Sample, Sample)) -> bool {
        self.span.0 < other.1 && other.0 < self.span.1
    }
}

/// One study.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Study {
    pub name: String,
    pub source: StudySource,
    /// The audio it studies, never modified: every render starts from here,
    /// so edits never compound a resynthesis's artefacts.
    pub original: AssetRef,
    /// The file the edits were last rendered to, while the clip plays it.
    #[serde(default)]
    pub rendered: Option<AssetRef>,
    #[serde(default)]
    pub engine: StudyEngines,
    #[serde(default)]
    pub mode: Option<StudyMode>,
    /// In the order they were made; no two overlap.
    #[serde(default)]
    pub pitch_edits: Vec<PitchEdit>,
    /// The Slice page's markers, in frames of the original, by time.
    #[serde(default)]
    pub markers: Vec<StudyMarker>,
    /// The Clean page: trim, fades, gain and the denoiser.
    #[serde(default)]
    pub clean: StudyClean,
    /// The Record page's takes, oldest first.
    #[serde(default)]
    pub takes: Vec<StudyTake>,
    /// The comp built from them: spans of takes, by take id.
    #[serde(default)]
    pub comp: Vec<StudyCompSpan>,
    /// The Analyze Musically insert that records into this study, by the id
    /// its settings carry (`AnalyzeConfig::study`); `None` for any other.
    /// An insert removed leaves the study as it is: still listed, never lost.
    #[serde(default)]
    pub insert: Option<PersistentId>,
    /// Which take is in the lane, when the audio studied is one of them.
    #[serde(default)]
    pub current_take: Option<u32>,
}

/// A marker on the Slice page: where a slice starts. Study-local ids, so a
/// drag names the marker it moves however the list is sorted.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StudyMarker {
    pub id: u32,
    /// Frames of the study's original file.
    pub at: Sample,
    #[serde(default)]
    pub name: String,
}

/// The curve a study's fades follow (`fontelle_analysis::edit::FadeShape`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum StudyFadeShape {
    Linear,
    /// Equal power: what a fade on a voice sounds natural with.
    #[default]
    Smooth,
    /// Slow, then fast.
    Exponential,
}

impl StudyFadeShape {
    pub const ALL: [Self; 3] = [Self::Linear, Self::Smooth, Self::Exponential];

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Smooth => "Smooth",
            Self::Exponential => "Exponential",
        }
    }
}

/// A captured noise profile: what the noise sounds like, bin by bin
/// (`fontelle_analysis::denoise::NoiseProfile`), and how loud it was.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StudyNoise {
    pub magnitudes: Vec<f32>,
    pub sample_rate: u32,
    /// Its RMS level, dBFS: what the Noise card says was captured.
    pub level_db: f32,
}

/// The denoiser's settings (plan §2.7).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StudyDenoise {
    /// Switched on. Off keeps the settings and the profile.
    pub on: bool,
    pub noise: Option<StudyNoise>,
    /// How far down the noise goes at most, dB.
    pub reduce_db: f32,
    /// 0..1: how hard it subtracts.
    pub amount: f32,
    /// 0..1: how loud something must be against the profile to be kept.
    pub sensitivity: f32,
    /// The voice denoiser (no profile needed), where the build has it.
    #[serde(default)]
    pub voice: bool,
}

impl Default for StudyDenoise {
    fn default() -> Self {
        Self {
            on: false,
            noise: None,
            reduce_db: 12.0,
            amount: 0.5,
            sensitivity: 0.5,
            voice: false,
        }
    }
}

impl StudyDenoise {
    /// Whether it changes the audio: on, and with something to work from.
    pub fn active(&self) -> bool {
        self.on && (self.noise.is_some() || self.voice)
    }
}

/// The Clean page in one value: what the render does before and after the
/// pitch edits. Trim is not cut into a file: Render to clip maps it onto the
/// clip's span (plan §2.7, §3.7).
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct StudyClean {
    /// The span kept, frames of the original; `None` is all of it.
    #[serde(default)]
    pub trim: Option<(Sample, Sample)>,
    /// Fade lengths in frames, from the trim's ends.
    #[serde(default)]
    pub fade_in: Sample,
    #[serde(default)]
    pub fade_out: Sample,
    #[serde(default)]
    pub fade_shape: StudyFadeShape,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub denoise: StudyDenoise,
}

impl StudyClean {
    /// Whether it leaves the audio as it was.
    pub fn is_identity(&self) -> bool {
        self.trim.is_none()
            && self.fade_in <= 0
            && self.fade_out <= 0
            && self.gain_db.abs() < 1e-3
            && !self.denoise.active()
    }

    /// Whether it changes the samples (trim alone does not: it is a span).
    pub fn changes_samples(&self) -> bool {
        self.fade_in > 0 || self.fade_out > 0 || self.gain_db.abs() >= 1e-3 || self.denoise.active()
    }
}

/// One take recorded into a study (plan §6.1, P5).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StudyTake {
    /// Study-local, never reused: a comp span names its take by this.
    pub id: u32,
    pub asset: AssetRef,
    pub name: String,
    /// Where the song was at its first frame (song samples), when the
    /// transport rolled; `None` for a free take.
    pub song_sample: Option<Sample>,
    pub frames: Sample,
    pub sample_rate: u32,
    #[serde(default)]
    pub starred: bool,
    /// Frames the capture lost: not zero is a take with a hole in it.
    #[serde(default)]
    pub dropped_frames: u64,
}

/// One span of a comp: frames `start..end` (of the takes' shared timeline)
/// from take `take`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StudyCompSpan {
    pub take: u32,
    pub start: Sample,
    pub end: Sample,
}

impl Study {
    /// A study of `original`, for `source`, with nothing done to it yet.
    pub fn new(name: impl Into<String>, source: StudySource, original: AssetRef) -> Self {
        Self {
            name: name.into(),
            source,
            original,
            rendered: None,
            engine: StudyEngines::default(),
            mode: None,
            pitch_edits: Vec::new(),
            markers: Vec::new(),
            clean: StudyClean::default(),
            takes: Vec::new(),
            comp: Vec::new(),
            insert: None,
            current_take: None,
        }
    }
}
