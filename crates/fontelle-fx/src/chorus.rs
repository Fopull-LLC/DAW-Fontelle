//! The chorus and ensemble (`docs/effects-catalogue.md` §2.4). Its parameters
//! are `fontelle_types::ChorusConfig` — the document owns those, this owns
//! the delay line and the phases.
//!
//! **What comes out is the voices and only the voices**, like the delay and
//! the reverb and for the same reason: `EffectNode` owns the dry/wet blend,
//! so an effect that mixed its own dry signal back in would be blended twice.
//! It is also why a chorus opens half wet — see `EffectKind::is_time_based`.
//! A chorus *is* the interference between the copies and the signal that made
//! them, and here that interference happens in the node.
//!
//! # The path
//!
//! ```text
//! in → line ──┬─ read at centre₀ + swing·lfo(φ + 0/n) ─┐
//!             ├─ read at centre₁ + swing·lfo(φ + 1/n) ─┤
//!             └─ ...                                    ├→ ÷n → tone → out
//!                                                       │
//!             ←──────────── × feedback ─────────────────┘
//! ```
//!
//! # Why the voices are at different delays and not only at different phases
//!
//! Spreading *n* voices evenly around one LFO's cycle is the obvious design
//! and it has a hole in it: at any instant when the LFO's waveform takes the
//! same value at two of those points — which for a sine is most of the time
//! for an even *n*, and at every zero crossing for any *n* — two voices are
//! reading the same place, and one of them is doing nothing. So each voice
//! also has its **own centre**, spread across ±15 % of the knob, which is
//! what a hardware ensemble did with its four taps. Two voices are then never
//! the same voice, whatever the LFO is doing.
//!
//! # What it is not
//!
//! Not a flanger, though the feedback reaches into its territory: a flanger's
//! centre goes down to a tenth of a millisecond and this one stops at five,
//! which is where a comb's teeth stop being a resonance and start being
//! thickness (`docs/effects-catalogue.md` §2.4). Not a vibrato: a vibrato has
//! no dry signal, and the dry is the node's.

use fontelle_types::{
    CHORUS_TONE_OPEN_HZ, ChorusConfig, ChorusMode, MAX_CHORUS_DELAY_MS, MAX_CHORUS_VOICES,
    MIN_CHORUS_DELAY_MS,
};

const MAX_CHANNELS: usize = 2;

/// How many voices the arrays are sized for.
const VOICES: usize = MAX_CHORUS_VOICES as usize;

/// The longest the line has to hold: the widest centre a voice can be given,
/// plus the widest swing, plus room for the interpolator.
const MAX_LINE_MS: f32 = 64.0;

/// How close to the write head a read is ever allowed to get, in
/// milliseconds. Under this the interpolator would be reading the sample
/// being written, which is not a short delay, it is a wrap.
const MIN_READ_MS: f32 = 1.0;

/// How far apart the voices' own centres are: the outermost pair sit at
/// ±15 % of the delay knob. See the module doc for why they are spread at
/// all.
const VOICE_SPREAD: f32 = 0.3;

/// What each voice's LFO rate is multiplied by in ensemble mode.
///
/// Ratios that do not divide each other, so the voices never come back into
/// step — which is the whole difference between an ensemble and a chorus. The
/// first is 1.0 so that one voice in ensemble mode is one voice at the rate
/// the knob says.
const ENSEMBLE_RATES: [f32; VOICES] = [1.0, 1.31, 0.73, 1.87];

/// A multi-voice modulated delay (`docs/effects-catalogue.md` §2.4).
pub struct Chorus {
    /// One ring per channel, sized once in [`prepare`](Self::prepare).
    lines: [Vec<f32>; MAX_CHANNELS],
    write: usize,
    /// Each voice's LFO phase, 0..1. Only the first is read in chorus mode,
    /// where the others are that one plus an offset; all four are read in
    /// ensemble mode, where each runs at its own rate.
    ///
    /// `f64` for one reason, and it is measurable: at 2 Hz this accumulates a
    /// step of about four hundredths of a millionth per sample, and in `f32`
    /// the rounding of a hundred thousand of those adds up to a third of a
    /// sample of drift in the read position — which is a chorus that is not
    /// quite in the same place a cycle later. `the_rate_is_how_often_the_
    /// sweep_comes_round` is what says so.
    phases: [f64; VOICES],
    /// The tone control's state, one pole per channel.
    tone: [f32; MAX_CHANNELS],
    sample_rate: f32,
}

impl Chorus {
    pub fn new() -> Self {
        Self {
            lines: [Vec::new(), Vec::new()],
            write: 0,
            phases: [0.0; VOICES],
            tone: [0.0; MAX_CHANNELS],
            sample_rate: 48_000.0,
        }
    }

    /// Sizes the lines. **The only place this type allocates.**
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        let length = (MAX_LINE_MS * self.sample_rate / 1000.0).ceil() as usize + 4;
        for line in &mut self.lines {
            line.clear();
            line.resize(length, 0.0);
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.fill(0.0);
        }
        self.write = 0;
        // Staggered, so that ensemble mode's voices start apart and drift
        // rather than starting stacked and taking a cycle to separate. The
        // first is at zero, where a sine is rising fastest: a chorus that
        // starts at the top of its sweep starts standing still.
        for (voice, phase) in self.phases.iter_mut().enumerate() {
            *phase = voice as f64 / VOICES as f64;
        }
        self.tone = [0.0; MAX_CHANNELS];
    }

    /// Runs `channels` through the chorus in place, replacing them with the
    /// voices.
    ///
    /// `bpm` is the tempo where this block sits, for a rate set in note
    /// values — the same arrangement the delay has, and what a whole note
    /// *is* stays in the document. See [`ChorusConfig::effective_rate_hz`].
    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &ChorusConfig, bpm: f32) {
        let length = self.lines[0].len();
        if length == 0 || channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);

        let voices = config.voices.clamp(1, MAX_CHORUS_VOICES) as usize;
        let rate = config.effective_rate_hz(bpm);
        let step = f64::from(rate) / f64::from(self.sample_rate);
        let spread = config.spread.clamp(0.0, 1.0);
        // Never quite one: a loop at unity gain is an oscillator, and this one
        // has an interpolator in it that would take a while to notice.
        let feedback = config.feedback.clamp(-0.9, 0.9);

        // The centres and the swing, in samples. Each voice sits at its own
        // place around the knob — see the module doc — and the swing is
        // measured against the room the *innermost* voice has, so no read
        // head can be driven into the write head whatever the two knobs say.
        let centre = config
            .delay_ms
            .clamp(MIN_CHORUS_DELAY_MS, MAX_CHORUS_DELAY_MS);
        let per_ms = self.sample_rate / 1000.0;
        let room = (centre * (1.0 - VOICE_SPREAD * 0.5) - MIN_READ_MS).max(0.0);
        let swing = config.depth.clamp(0.0, 1.0) * room * per_ms;
        let mut centres = [0.0f32; VOICES];
        for (voice, slot) in centres.iter_mut().enumerate().take(voices) {
            // One voice sits exactly on the knob; more than one spread evenly
            // across ±`VOICE_SPREAD / 2` of it, so the knob stays the middle
            // of what is heard.
            let across = if voices > 1 {
                voice as f32 / (voices - 1) as f32 - 0.5
            } else {
                0.0
            };
            *slot = centre * (1.0 + VOICE_SPREAD * across) * per_ms;
        }

        let toning = config.tone_hz < CHORUS_TONE_OPEN_HZ - 1.0;
        let tone = one_pole(config.tone_hz, self.sample_rate);
        let share = 1.0 / voices as f32;

        for frame in 0..frames {
            // Each channel reads the same voices at its own point of the
            // sweep: half a cycle apart at full spread, which is as wide as a
            // chorus goes without inventing information.
            let mut wet = [0.0f32; MAX_CHANNELS];
            for channel in 0..used {
                let offset = if channel == 0 {
                    0.0
                } else {
                    f64::from(spread) * 0.5
                };
                let mut sum = 0.0f32;
                for voice in 0..voices {
                    let phase = match config.mode {
                        // One LFO, the voices spread evenly around its cycle.
                        ChorusMode::Chorus => {
                            self.phases[0] + voice as f64 / voices as f64 + offset
                        }
                        // One LFO each, at rates that never come back into
                        // step.
                        ChorusMode::Ensemble => self.phases[voice] + offset,
                    };
                    let lfo = (std::f64::consts::TAU * phase).sin() as f32;
                    let behind = centres[voice] + swing * lfo;
                    sum += read_at(&self.lines[channel], self.write as f32 - behind);
                }
                wet[channel] = sum * share;
            }

            for channel in 0..used {
                // The feedback is the voices *before* the tone control, so
                // that turning the tone down does not also change how long
                // the loop rings.
                self.lines[channel][self.write] = channels[channel][frame] + feedback * wet[channel];
                let out = if toning {
                    self.tone[channel] += (wet[channel] - self.tone[channel]) * tone;
                    self.tone[channel]
                } else {
                    wet[channel]
                };
                channels[channel][frame] = out;
            }
            // Channels the chorus has no line for are silenced rather than
            // left holding their input, which would be dry signal in the wet
            // path.
            for channel in used..channels.len() {
                channels[channel][frame] = 0.0;
            }

            for (voice, phase) in self.phases.iter_mut().enumerate() {
                *phase = (*phase + step * f64::from(ENSEMBLE_RATES[voice])).fract();
            }
            self.write = (self.write + 1) % length;
        }
    }
}

impl Default for Chorus {
    fn default() -> Self {
        Self::new()
    }
}

/// The coefficient of a one-pole low-pass at `cutoff_hz`.
///
/// A one-pole rather than the crate's state-variable filter, for the delay's
/// reason: this is a character control, and a resonant slope on four detuned
/// copies would be a whistle rather than a tone knob.
fn one_pole(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let w = std::f32::consts::TAU * cutoff_hz.max(1.0) / sample_rate.max(1.0);
    (1.0 - (-w).exp()).clamp(0.0, 1.0)
}

/// Reads `line` at a fractional position, wrapping.
///
/// Linear interpolation, which is what makes a moving read head a slide
/// rather than a stair. Its high-frequency loss is real, and on a chorus it
/// is part of why the copies sit under the dry signal rather than on top of
/// it.
fn read_at(line: &[f32], position: f32) -> f32 {
    let length = line.len();
    let wrapped = position.rem_euclid(length as f32);
    // `rem_euclid` of a position a hair under zero comes back a hair under
    // `length`, and a hair under `length` rounds *to* `length` in `f32` once
    // the line is a few thousand samples long — which is an index one past
    // the end, on the audio thread, on the first block after a reset. The
    // wrap belongs here rather than in every caller.
    let index = wrapped as usize;
    let (index, fraction) = if index >= length {
        (0, 0.0)
    } else {
        (index, wrapped - index as f32)
    };
    let next = if index + 1 == length { 0 } else { index + 1 };
    line[index] + (line[next] - line[index]) * fraction
}
