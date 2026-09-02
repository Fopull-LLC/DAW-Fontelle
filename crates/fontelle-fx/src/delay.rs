//! The delay's DSP. Its parameters are `fontelle_types::DelayConfig` — the
//! document owns those, this owns the two seconds of memory.
//!
//! **What comes out is the repeats and only the repeats.** `EffectNode` owns
//! the dry/wet blend for every effect, so a delay that mixed its own dry
//! signal back in would be blended twice and a fully dry insert would still be
//! audible. The same rule the EQ follows, and the reason a delay opens at 35 %
//! rather than fully wet — see `EffectKind::is_time_based`.

use fontelle_types::{DelayConfig, MAX_DELAY_MS};

/// Left and right, as everywhere else in this crate.
const MAX_CHANNELS: usize = 2;

/// How long the read pointer takes to reach a new delay time, as a one-pole
/// time constant.
///
/// A read pointer that jumped would splice two unrelated points of the signal
/// together, which is a click at full scale every time somebody drags the
/// knob. Gliding instead resamples the line while it moves, so the repeats
/// bend in pitch and land at the new time — which is what tape does, and the
/// reason a delay's time knob is a performance control on hardware.
///
/// Long rather than short: at 50 ms a half-second move bends the pitch by a
/// factor of four, which reads as a glitch rather than a slide.
const GLIDE_SECONDS: f32 = 0.2;

/// A stereo delay line with a filtered, saturated feedback path (TDD §13.4).
pub struct Delay {
    /// One ring per channel, sized once in [`prepare`](Self::prepare) to the
    /// longest time the document allows. Never resized after: this runs on the
    /// audio thread (INVARIANT 1).
    lines: [Vec<f32>; MAX_CHANNELS],
    write: usize,
    /// Where the read pointer is now, in samples behind `write`. Fractional,
    /// because it glides — see [`GLIDE_SECONDS`].
    read_behind: f32,
    /// Whether `read_behind` means anything yet. The first block after a
    /// prepare or a reset snaps to the configured time instead of gliding up
    /// from zero, which would otherwise make every delay start with a sweep.
    started: bool,
    /// The damping filter's state, one pole per channel.
    damp: [f32; MAX_CHANNELS],
    sample_rate: f32,
}

impl Delay {
    pub fn new() -> Self {
        Self {
            lines: [Vec::new(), Vec::new()],
            write: 0,
            read_behind: 0.0,
            started: false,
            damp: [0.0; MAX_CHANNELS],
            sample_rate: 48_000.0,
        }
    }

    /// Sizes the lines. **The only place this type allocates** — everything
    /// after it runs on the audio thread.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        // Two samples of headroom past the longest time the spec allows, so
        // the interpolating read never asks for the sample being written.
        let length = (MAX_DELAY_MS * self.sample_rate / 1000.0).ceil() as usize + 4;
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
        self.damp = [0.0; MAX_CHANNELS];
        self.started = false;
    }

    /// Runs `channels` through the delay in place, replacing them with the
    /// repeats.
    ///
    /// `bpm` is the tempo where this block sits, for a config set in note
    /// values. What a dotted eighth *is* stays in the document — see
    /// [`DelayConfig::effective_time_ms`] — so this only has to hand it the
    /// number and take back a duration.
    pub fn process(&mut self, channels: &mut [&mut [f32]], config: &DelayConfig, bpm: f32) {
        let length = self.lines[0].len();
        if length == 0 || channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);

        // Clamped here as well as in the document: a config built by hand
        // must not be able to index past the buffer, and this is the audio
        // thread.
        let target = (config.effective_time_ms(bpm) * self.sample_rate / 1000.0)
            .clamp(1.0, (length - 2) as f32);
        if !self.started {
            self.read_behind = target;
            self.started = true;
        }
        let glide = 1.0 / (GLIDE_SECONDS * self.sample_rate).max(1.0);
        let feedback = config.feedback.clamp(0.0, 0.95);
        let damping = one_pole(config.damping_hz, self.sample_rate);
        let drive = config.drive.clamp(0.0, 1.0);

        for frame in 0..frames {
            self.read_behind += (target - self.read_behind) * glide;
            let read = self.write as f32 - self.read_behind;

            let mut delayed = [0.0f32; MAX_CHANNELS];
            for (channel, slot) in delayed.iter_mut().enumerate().take(used) {
                *slot = read_at(&self.lines[channel], read);
            }

            for channel in 0..used {
                // Ping-pong crosses the *feedback*, not the input: a repeat
                // that came back on the other side is the effect, and moving
                // the input would move the dry image with it.
                let source = if config.ping_pong {
                    delayed[(channel + 1) % used]
                } else {
                    delayed[channel]
                };
                // Damping in the loop, so each repeat is duller than the last.
                // On the output it would be a tone control (see the test).
                self.damp[channel] += (source - self.damp[channel]) * damping;
                let shaped = saturate(self.damp[channel], drive);
                self.lines[channel][self.write] = channels[channel][frame] + feedback * shaped;
                channels[channel][frame] = delayed[channel];
            }
            // Channels the delay has no line for are silenced rather than left
            // holding their input, which would be dry signal in the wet path.
            for channel in used..channels.len() {
                channels[channel][frame] = 0.0;
            }
            self.write = (self.write + 1) % length;
        }
    }
}

impl Default for Delay {
    fn default() -> Self {
        Self::new()
    }
}

/// The coefficient of a one-pole low-pass at `cutoff_hz`.
///
/// A one-pole rather than the crate's state-variable filter: this is a
/// character control inside a feedback loop, where a gentle slope is what is
/// wanted and a resonant one would ring the loop.
fn one_pole(cutoff_hz: f32, sample_rate: f32) -> f32 {
    let w = std::f32::consts::TAU * cutoff_hz.max(1.0) / sample_rate.max(1.0);
    (1.0 - (-w).exp()).clamp(0.0, 1.0)
}

/// Soft saturation, blended in by `drive` so that zero drive is **exactly** a
/// wire rather than nearly one.
///
/// Normalised by `tanh(k)` so full scale stays full scale: a drive knob that
/// also turned the repeats down would be a drive knob nobody could hear.
fn saturate(sample: f32, drive: f32) -> f32 {
    if drive <= 0.0 {
        return sample;
    }
    let k = 1.0 + drive * 8.0;
    let shaped = (k * sample).tanh() / k.tanh();
    sample + (shaped - sample) * drive
}

/// Reads `line` at a fractional position, wrapping.
///
/// Linear interpolation, which is what makes a gliding read pointer a slide
/// rather than a stair. Its high-frequency loss is real and is part of why a
/// moving tape delay sounds the way it does.
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
