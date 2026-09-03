//! What an audio clip is (TDD §15.1).
//!
//! Reported from using the window:
//!
//! > *"double clicking on an audio clip should open a menu that lets me make
//! > changes to that audio like basic changes that would be widely general
//! > useful across all audio editing so for example changing the boost or the
//! > cutoff or resonance of the sound or even changing a fade in or fade out or
//! > stuff like that."*
//!
//! §15.1 is emphatic about the shape, and it is the right shape: **every
//! property is stored on the clip and applied at playback; the source file is
//! never modified.** So a clip is a reference plus a list of numbers. Nothing
//! is destructive, undo costs nothing, two clips can point at one file and
//! sound completely different, and *"make this one clip darker"* does not cost
//! a mixer track and a plugin slot — which is the reason §15.1 puts the filter
//! here rather than in an insert.
//!
//! # Why this lives in `fontelle-types` rather than in the document
//!
//! Because both ends need it. The **document** owns it, serialises it, and
//! undoes edits to it; the **engine** reads it on the audio thread to decide
//! which frame of the file to play and how loud. `fontelle-engine` may not
//! depend on `fontelle-model` (INVARIANT 4, TDD §4.1), so the shape lives here
//! beside [`AssetRef`] and [`crate::FilterConfig`], which are here for exactly
//! the same reason.
//!
//! The arithmetic lives here too, and that is deliberate: *"where in the file
//! does frame n of this clip come from"* is asked by the player, by the
//! waveform, and by the editor's read-outs, and three answers that drift apart
//! is a clip that sounds like one thing and draws as another.

use crate::{AssetRef, FilterConfig, MixerTrackId, Sample};

/// The steepest boost a clip may be given, in dB.
///
/// A number typed into a box reaches the audio thread as a multiplier, and
/// `10^(1e9/20)` is `inf`: one silent NaN through a mixer takes the whole
/// output with it. Twenty-four decibels is four times, which is past anything
/// a take needs and well short of anything that breaks.
pub const MAX_CLIP_GAIN_DB: f32 = 24.0;

/// And the deepest cut, past which it may as well be muted.
pub const MIN_CLIP_GAIN_DB: f32 = -60.0;

/// The fastest and slowest a clip may be read.
///
/// Sixteen times either way is four octaves, which is as far as varispeed is
/// musical; the floor matters more than the ceiling, because a speed of zero
/// is a player that never advances and a negative one runs backwards through
/// [`AudioClipData::reverse`]'s arithmetic as well as its own.
pub const MAX_CLIP_SPEED: f64 = 16.0;
pub const MIN_CLIP_SPEED: f64 = 1.0 / 16.0;

/// The shape a fade takes between silence and full level.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub enum FadeCurve {
    /// A straight line. Correct for a crossfade of correlated material and
    /// the one everybody pictures.
    #[default]
    Linear,
    /// Slow to start, then quick — what a fade *in* usually wants, because
    /// loudness is not linear and a linear fade in sounds like it arrives
    /// early.
    Exponential,
    /// Quick to start, then slow. The same argument pointed the other way,
    /// and what a fade out usually wants.
    Logarithmic,
    /// Flat at both ends. The equal-power-ish one: two of these crossing over
    /// hold their level through the middle, where two linear fades dip.
    SCurve,
}

impl FadeCurve {
    pub const ALL: [Self; 4] = [
        Self::Linear,
        Self::Exponential,
        Self::Logarithmic,
        Self::SCurve,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Exponential => "Exponential",
            Self::Logarithmic => "Logarithmic",
            Self::SCurve => "S-curve",
        }
    }

    /// The curve at `t`, which is 0 at the silent end and 1 at the loud one.
    ///
    /// Every shape here is monotonic and lands exactly on 0 and 1 at the ends —
    /// a fade that overshoots is one that makes a clip *louder* than it is,
    /// which on a full mix is a click.
    pub fn at(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Exponential => t * t,
            Self::Logarithmic => 1.0 - (1.0 - t) * (1.0 - t),
            // Smoothstep. Its derivative is zero at both ends, which is what
            // makes two of them meeting in the middle sound like one gesture.
            Self::SCurve => t * t * (3.0 - 2.0 * t),
        }
    }
}

/// A fade: how long, and what shape (§15.2).
#[derive(
    Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(default)]
pub struct Fade {
    /// In **source frames**, not ticks.
    ///
    /// A fade measured in ticks would change length when the tempo changed,
    /// and a fade that changes length is a different sound. The window
    /// converts for the handle it draws.
    pub frames: Sample,
    pub curve: FadeCurve,
}

impl Fade {
    /// No fade at all.
    pub const NONE: Self = Self {
        frames: 0,
        curve: FadeCurve::Linear,
    };
}

/// What a clip does when it reaches the end of the section it was trimmed to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ClipLoopMode {
    /// Plays once and then nothing. A take.
    #[default]
    Once,
    /// Comes round again for as long as the block is long. A loop, which is
    /// what half of *"import different sounds and loops"* means.
    Loop,
}

impl ClipLoopMode {
    pub const ALL: [Self; 2] = [Self::Once, Self::Loop];

    pub fn label(self) -> &'static str {
        match self {
            Self::Once => "Once",
            Self::Loop => "Loop",
        }
    }
}

/// An audio clip's content: a reference to a file, and everything done to it
/// on the way out (TDD §15.1).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default = "AudioClipData::placeholder")]
pub struct AudioClipData {
    pub asset: AssetRef,
    /// Which mixer track this clip's audio arrives on. **`None` is the
    /// master**, matching `Channel::mixer_track`.
    ///
    /// On the **clip**, not on the lane, and for TDD §10.3's reason: a lane is
    /// a visual strip with no audio identity, so a clip dragged from one row to
    /// another must not change what it is routed through. A note clip carries
    /// its channel for exactly the same reason.
    pub mixer_track: Option<MixerTrackId>,
    /// Where in the file this clip starts and stops, in frames. Trimming is
    /// moving these; the file is untouched either way.
    pub source_start: Sample,
    pub source_end: Sample,
    /// *"changing the boost"*.
    pub gain_db: f32,
    /// −1 (left) to 1 (right).
    pub pan: f32,
    /// Coupled to [`speed`](Self::speed): §15.1 says varispeed *"couples pitch
    /// unless time_lock"*, and time lock is a v2 feature behind the stretch
    /// engine (§3.3). So this is a second way of writing the same number, kept
    /// separate because a musician thinks in semitones and a sample-mangler
    /// thinks in percent.
    pub pitch_semitones: f32,
    pub speed: f64,
    pub reverse: bool,
    pub fade_in: Fade,
    pub fade_out: Fade,
    /// Per-clip, so *"make this one clip darker"* costs nothing (§15.1). The
    /// same [`FilterConfig`] the mixer's filter insert uses, so it is the same
    /// sound, the same knobs and the same code — *"the cutoff or resonance"*
    /// with the rest of a synthesiser filter behind them.
    pub filter: FilterConfig,
    /// Bring the loudest sample up to full scale. A *display* of intent rather
    /// than a destructive pass: the player scales, the file does not change.
    pub normalize: bool,
    pub loop_mode: ClipLoopMode,
}

impl AudioClipData {
    /// A clip over the whole of a file `frames` frames long, changing nothing
    /// about it.
    ///
    /// **The identity.** Dragging a file onto the arrangement has to sound
    /// exactly like the file, and opening the editor and closing it again has
    /// to not be an edit — which is only true if every knob starts where it
    /// does nothing.
    pub fn whole(asset: AssetRef, frames: Sample) -> Self {
        Self {
            asset,
            mixer_track: None,
            source_start: 0,
            source_end: frames.max(0),
            gain_db: 0.0,
            pan: 0.0,
            pitch_semitones: 0.0,
            speed: 1.0,
            reverse: false,
            fade_in: Fade::NONE,
            fade_out: Fade::NONE,
            filter: FilterConfig::new(),
            normalize: false,
            loop_mode: ClipLoopMode::Once,
        }
    }

    /// What `serde` fills in for a clip whose JSON is missing everything.
    ///
    /// Only ever reached through `#[serde(default)]`, and only for fields the
    /// file left out — `asset` is not one of them in any file a build of this
    /// program wrote.
    fn placeholder() -> Self {
        Self::whole(
            AssetRef {
                id: crate::AssetId::default(),
                path: std::path::PathBuf::new(),
                content_hash: 0,
                size: 0,
                kind: crate::AssetKind::Sample,
            },
            0,
        )
    }

    /// How many frames of the file this clip covers.
    ///
    /// Never negative: a negative length reaches the audio thread as a loop
    /// bound and a capacity, and both of those are a crash rather than a wrong
    /// sound.
    pub fn source_frames(&self) -> Sample {
        (self.source_end - self.source_start).max(0)
    }

    /// How fast the file is read, pitch and speed together.
    ///
    /// One number rather than two, because they *are* one number until the
    /// stretch engine lands: an octave up and double speed have to give one
    /// answer or a clip pitched in semitones and one pitched in percent play
    /// at different lengths.
    pub fn rate(&self) -> f64 {
        let semitones = f64::from(self.pitch_semitones.clamp(-48.0, 48.0));
        (self.speed.clamp(MIN_CLIP_SPEED, MAX_CLIP_SPEED) * 2f64.powf(semitones / 12.0))
            .clamp(MIN_CLIP_SPEED, MAX_CLIP_SPEED)
    }

    /// Which frame of the **file** the clip's frame `position` comes from.
    ///
    /// Fractional, because the player interpolates between two frames — §7.6's
    /// argument, and the reason import does not resample.
    ///
    /// Past the end of a non-looping clip this deliberately keeps counting
    /// rather than wrapping: the player reads past-the-end as silence, and a
    /// clip that wrapped instead would repeat its own front, which is the
    /// wrong sound and a confusing one to diagnose.
    pub fn source_position(&self, position: f64) -> f64 {
        let length = self.source_frames();
        let mut offset = position.max(0.0) * self.rate();
        if self.loop_mode == ClipLoopMode::Loop && length > 0 {
            offset = offset.rem_euclid(length as f64);
        }
        if self.reverse {
            // From the last frame *inside* the clip, not from one past it: one
            // past is silence, and a reversed clip that begins with a gap is
            // the classic version of this bug.
            (self.source_start + length - 1) as f64 - offset
        } else {
            self.source_start as f64 + offset
        }
    }

    /// The fade envelope at the clip's frame `position`.
    ///
    /// The two fades **multiply**. A short clip given a long fade at each end
    /// is a real thing to ask for by dragging, and it has to produce a shape
    /// rather than a step where one of them wins.
    pub fn fade_gain(&self, position: f64) -> f32 {
        let length = self.source_frames() as f64;
        let mut gain = 1.0;
        if self.fade_in.frames > 0 {
            gain *= self.fade_in.curve.at(position / self.fade_in.frames as f64);
        }
        if self.fade_out.frames > 0 && length > 0.0 {
            let left = length - position;
            gain *= self.fade_out.curve.at(left / self.fade_out.frames as f64);
        }
        gain as f32
    }

    /// The boost as a multiplier.
    pub fn gain(&self) -> f32 {
        10f32.powf(self.gain_db.clamp(MIN_CLIP_GAIN_DB, MAX_CLIP_GAIN_DB) / 20.0)
    }
}
