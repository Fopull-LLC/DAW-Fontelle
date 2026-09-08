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

use std::ops::Range;

use crate::{AssetRef, ClipId, FilterConfig, MixerTrackId, NodeId, Sample};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Fade {
    /// In **source frames**, not ticks.
    ///
    /// A fade measured in ticks would change length when the tempo changed,
    /// and a fade that changes length is a different sound. The window
    /// converts for the handle it draws.
    pub frames: Sample,
    pub curve: FadeCurve,
    /// How the curve is **bent** between its ends, −1..1 (§15.2's "curve
    /// adjustable by dragging the fade's midpoint").
    ///
    /// *"you can bend the control node to bend the curve like fl studios
    /// too."* Zero is the shape as `curve` draws it; positive holds the
    /// gain back before letting it rise, negative brings it up early. The
    /// ends never move, so a bend is a bend and not a different length.
    /// Applied over `curve` by [`Fade::at`], which the player, the block on
    /// the arrangement and the editor's waveform all read.
    pub tension: f32,
}

impl Fade {
    /// No fade at all.
    pub const NONE: Self = Self {
        frames: 0,
        curve: FadeCurve::Linear,
        tension: 0.0,
    };

    /// The fade's gain at `t` along it, 0..1: the curve's shape, bent by the
    /// tension. The one function every reading of a fade goes through, so
    /// what is drawn is what is heard.
    pub fn at(&self, t: f64) -> f64 {
        bend(self.curve.at(t), self.tension)
    }
}

/// `t` bent by `tension`: a power curve, `t` raised to four-to-the-tension,
/// so ±1 is a quarter or four times — brought forward or held back — and 0
/// is `t` itself. The ends stay put whatever the tension.
pub fn bend(t: f64, tension: f32) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let tension = f64::from(tension.clamp(-1.0, 1.0));
    if tension == 0.0 {
        return t;
    }
    t.powf(4f64.powf(tension))
}

/// The tension that puts a linear fade's midpoint at `gain` — the inverse
/// of [`bend`] at a half, so a node dragged to a height lands under the
/// pointer rather than near it. Clamped to what a bend can reach.
pub fn tension_for_midpoint(gain: f32) -> f32 {
    // bend(0.5, k) = 0.5 ^ (4 ^ k)  ⇒  4 ^ k = ln(gain) / ln(0.5)
    let reachable = f64::from(gain).clamp(0.5f64.powi(4), 0.5f64.powf(0.25));
    let exponent = reachable.ln() / 0.5f64.ln();
    (exponent.ln() / 4f64.ln()).clamp(-1.0, 1.0) as f32
}

/// What a clip does when it reaches the end of the section it was trimmed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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

/// Whether a clip follows the song's tempo, and how (§15.1's stretch mode).
///
/// > *"audio clips arent stretching to match tempo changes in realtime right
/// > now."*
///
/// The block a clip occupies on the arrangement is measured in **ticks**, so
/// the sequencer already turns it into a sample range through the tempo map: a
/// tempo change moves that range without the player being told anything. All
/// a following clip has to do is *fill* the range it is given rather than read
/// its file at the file's own rate and stop when it runs out. That is why
/// this needs no clock of its own and no new plumbing — see
/// [`AudioClipData::read_ratio`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ClipStretch {
    /// The file plays at its own rate and the block is a window onto it.
    ///
    /// **The default, and it has to be**: a take you just recorded and a
    /// one-shot you dropped in are both things whose speed is not the song's,
    /// and a clip that changed pitch the moment somebody nudged the tempo box
    /// would be the worst possible default.
    #[default]
    Off,
    /// The file is read faster or slower so that it fills its block exactly,
    /// which makes it follow the tempo — and **moves its pitch with it**,
    /// because that is what varispeed is.
    ///
    /// The mode a loop wants. A pitch-preserving stretch is a different thing
    /// and a much larger one: it needs the stretch engine §3.3 puts in v2, and
    /// offering it here as a third row that quietly resampled would be a lie
    /// about what you were hearing.
    Resample,
}

impl ClipStretch {
    pub const ALL: [Self; 2] = [Self::Off, Self::Resample];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Resample => "Resample",
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
    /// The rate the **file** was recorded at.
    ///
    /// Held on the clip rather than looked up in whatever is holding the audio,
    /// because it is what relates the clip's *time* to the file's *frames* and
    /// two of the things that need it cannot see an audio store: the editor
    /// showing a fade in milliseconds, and the cut tool deciding where in the
    /// file a seam falls. A split that guessed proportionally is right exactly
    /// when the clip is the same length as its audio.
    ///
    /// Zero for a clip whose file could not be read, which every reader treats
    /// as "say nothing about time" rather than dividing by it.
    pub sample_rate: u32,
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
    /// Whether the clip follows the song's tempo — see [`ClipStretch`].
    ///
    /// A project written before this field carries none, and absent reads as
    /// `Off`, which is what every clip already did.
    #[serde(default)]
    pub stretch: ClipStretch,
}

impl AudioClipData {
    /// A clip over the whole of a file `frames` frames long, changing nothing
    /// about it.
    ///
    /// **The identity.** Dragging a file onto the arrangement has to sound
    /// exactly like the file, and opening the editor and closing it again has
    /// to not be an edit — which is only true if every knob starts where it
    /// does nothing.
    pub fn whole(asset: AssetRef, frames: Sample, sample_rate: u32) -> Self {
        Self {
            asset,
            mixer_track: None,
            source_start: 0,
            source_end: frames.max(0),
            sample_rate,
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
            stretch: ClipStretch::Off,
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
            0,
        )
    }

    /// How long it is, at the file's own rate. Zero when the rate is not
    /// known.
    pub fn seconds(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.source_frames() as f64 / f64::from(self.sample_rate)
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

    /// How many frames of the **file** one frame of the song consumes, before
    /// [`rate`](Self::rate)'s pitch and speed are applied on top.
    ///
    /// `span` is how long one pass of the block is, in song frames — the
    /// arrangement's repeat period when it has one, and the whole block when
    /// it does not.
    ///
    /// The two modes are two different questions, and that is the whole of
    /// [`ClipStretch`]:
    ///
    /// - **Off** asks *how fast is this file*, and answers with the ratio
    ///   between its rate and the device's. The block is a window; when the
    ///   file runs out, so does the sound.
    /// - **Resample** asks *how long is this bar*, and answers with the ratio
    ///   that makes the file exactly fill it. Nothing about the file's own
    ///   rate comes into it, which is why a tempo change re-stretches the clip
    ///   with no other part of the program being told.
    ///
    /// Pitch and speed still multiply on top in either mode, deliberately: at
    /// their defaults a following clip locks to the bar, and moving them is
    /// then a deliberate offset from it rather than a control that has
    /// silently stopped working.
    ///
    /// Zero for a clip with no audio or no block — the caller reads that as
    /// silence, which is the only honest answer and is never a division by it.
    pub fn read_ratio(&self, file_rate: u32, device_rate: f64, span: Sample) -> f64 {
        match self.stretch {
            ClipStretch::Off => f64::from(file_rate.max(1)) / device_rate.max(1.0),
            ClipStretch::Resample => {
                let frames = self.source_frames();
                if span <= 0 || frames <= 0 {
                    return 0.0;
                }
                frames as f64 / span as f64
            }
        }
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
            gain *= self.fade_in.at(position / self.fade_in.frames as f64);
        }
        if self.fade_out.frames > 0 && length > 0.0 {
            let left = length - position;
            gain *= self.fade_out.at(left / self.fade_out.frames as f64);
        }
        gain as f32
    }

    /// Whether the clip's filter would do anything at all.
    ///
    /// A fresh [`FilterConfig`] is a wire — a 24 dB low-pass wide open, no
    /// resonance, no drive, nothing moving it — and running an SVF per sample
    /// to reproduce its input exactly is a cost every unfiltered clip in a
    /// project would pay. This is the question the player asks before it
    /// bothers.
    pub fn filter_engaged(&self) -> bool {
        let f = &self.filter;
        f.mix > 0.0
            && (f.cutoff_hz < crate::MAX_FILTER_HZ
                || f.resonance > 0.0
                || f.drive > 0.0
                || f.env_amount != 0.0
                || f.lfo_amount > 0.0
                || f.output_db != 0.0)
    }

    /// The boost as a multiplier.
    pub fn gain(&self) -> f32 {
        10f32.powf(self.gain_db.clamp(MIN_CLIP_GAIN_DB, MAX_CLIP_GAIN_DB) / 20.0)
    }
}

/// One audio clip, placed on the song (TDD §11.1's job, for §15's content).
///
/// The compiled counterpart of a `TimedEvent`, and it is a different shape for
/// a real reason: a note clip becomes **events** and a sampler turns them into
/// sound, but an audio clip is a **continuous stream** that has to be at one
/// place on the song and nowhere else. There is no moment to send; there is a
/// range to be inside of.
///
/// Built by `fontelle-sequencer` and read on the audio thread, so everything in
/// it is owned and nothing in it allocates once it exists.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioPlacement {
    /// Which node plays it — the audio player in front of a mixer track.
    pub target: NodeId,
    /// Which clip it came from, so the player can keep a filter's state with
    /// the clip it belongs to rather than with a position in a list.
    pub clip: ClipId,
    /// Where on the song it sounds, in samples.
    pub range: Range<Sample>,
    /// How often the content comes round, in samples — the compiled form of
    /// `Clip::loop_length`. **Zero is no repeat**, which is what every clip
    /// does by default.
    ///
    /// Not the same thing as [`ClipLoopMode`], and both exist: this is *the
    /// arrangement* repeating a block, and that is *the file* coming round
    /// when the block outlasts it. A four-bar loop dragged out to sixteen bars
    /// uses one; a one-shot dropped on a long block uses neither.
    pub repeat: Sample,
    /// How far in from the **front** of the block the automatic crossfade
    /// runs, in song samples — the length of the overlap with the clip
    /// before it on the same row (TDD §15.2). Zero is none.
    ///
    /// *"when audio clips are overlapping ... it should also blend together
    /// like a transition the timing based on how long the overlap section
    /// is."* A fact about two clips on one row, so it belongs to the
    /// placement and is worked out by the compiler — not to the clip, whose
    /// own fades ([`AudioClipData::fade_in`]) are in the file's frames and
    /// go with it wherever it is put. Both apply; see [`auto_gain`](Self::auto_gain).
    pub crossfade_in: Sample,
    /// The same, from the **back**: the overlap with the clip after it.
    pub crossfade_out: Sample,
    pub data: AudioClipData,
}

impl AudioPlacement {
    /// How long it sounds for.
    pub fn frames(&self) -> Sample {
        (self.range.end - self.range.start).max(0)
    }

    /// The automatic crossfade's gain at song sample `at`: rising over
    /// [`crossfade_in`](Self::crossfade_in), falling over
    /// [`crossfade_out`](Self::crossfade_out), one in between — and nothing
    /// outside the block.
    ///
    /// **Equal power**: a sine on the way in, so that against the cosine the
    /// clip before it is leaving on, the two sum to constant power at every
    /// sample. They are two different recordings, and a linear blend of
    /// uncorrelated material dips three decibels in the middle — a crossfade
    /// you can hear as a dip is not a transition.
    ///
    /// Measured against the **whole block**, not a loop's pass: a one-bar
    /// loop dragged over the end of another clip fades in once, at the
    /// front, rather than stuttering in at every repeat. A fade longer than
    /// the block is the block — a clip dropped wholly inside another fades
    /// in over all of itself.
    pub fn auto_gain(&self, at: Sample) -> f32 {
        if at < self.range.start || at >= self.range.end {
            return 0.0;
        }
        let frames = self.frames();
        let offset = at - self.range.start;
        let mut gain = 1.0f64;
        let fade_in = self.crossfade_in.clamp(0, frames);
        if fade_in > 0 && offset < fade_in {
            gain *= (offset as f64 / fade_in as f64 * std::f64::consts::FRAC_PI_2).sin();
        }
        let fade_out = self.crossfade_out.clamp(0, frames);
        if fade_out > 0 {
            let left = frames - offset;
            if left <= fade_out {
                gain *= (left as f64 / fade_out as f64 * std::f64::consts::FRAC_PI_2).sin();
            }
        }
        gain as f32
    }

    /// Where in the clip's own time the song sample `at` falls, or `None` if it
    /// falls outside the block entirely.
    ///
    /// The repeat is applied here rather than in the player, so *"which frame
    /// of the clip is this"* has one answer that the waveform and the sound can
    /// both be measured against.
    pub fn position(&self, at: Sample) -> Option<Sample> {
        if at < self.range.start || at >= self.range.end {
            return None;
        }
        let offset = at - self.range.start;
        Some(if self.repeat > 0 {
            offset.rem_euclid(self.repeat)
        } else {
            offset
        })
    }
}
