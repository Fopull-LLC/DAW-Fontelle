/// A look-ahead brickwall limiter, for the master bus.
///
/// Not in TDD §13.4's effect table, which names a compressor but no limiter.
/// It is here because the alternative is a fader set by hand: a whole
/// arrangement summing onto one bus peaks wherever the material puts it, and
/// picking a master gain that neither clips nor throws away 20 dB is a
/// judgement about the piece rather than something a tool can settle. A
/// limiter settles it.
#[derive(Debug, Clone, Copy)]
pub struct LimiterConfig {
    /// The highest output magnitude, linear. Slightly under full scale by
    /// default: an inter-sample peak can exceed the sample values either side
    /// of it, and a converter reconstructing the waveform will overshoot a
    /// signal limited to exactly 1.0.
    pub ceiling: f32,
    /// How far ahead the gain computer sees, in milliseconds. This is also the
    /// limiter's latency, and the time over which it can bring the gain down
    /// before a transient arrives.
    pub lookahead_ms: f32,
    /// How long the gain takes to come back up, in milliseconds.
    pub release_ms: f32,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            // -0.3 dBFS.
            ceiling: 0.966,
            lookahead_ms: 2.0,
            release_ms: 60.0,
        }
    }
}

/// A ring of the last `window` gain targets, with the minimum available in
/// amortised O(1).
///
/// The naive alternative — scanning the window each sample — is a couple of
/// hundred operations per sample per channel, which is not something an RT
/// thread can spend on a safety net. A monotonic deque holds only the indices
/// that could still become the minimum, and each index is pushed and popped
/// once.
#[derive(Debug, Default)]
struct SlidingMin {
    /// `(index, value)`, values strictly increasing from front to back, so the
    /// front is always the window's minimum.
    entries: Vec<(u64, f32)>,
    head: usize,
    tail: usize,
    window: usize,
    next_index: u64,
}

impl SlidingMin {
    /// Off-RT: sizes the ring so `push` never allocates.
    fn prepare(&mut self, window: usize) {
        self.window = window.max(1);
        // One more slot than the window, so a full ring is distinguishable
        // from an empty one without a separate length.
        self.entries.clear();
        self.entries.resize(self.window + 1, (0, 0.0));
        self.head = 0;
        self.tail = 0;
        self.next_index = 0;
    }

    fn capacity(&self) -> usize {
        self.entries.len()
    }

    fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Adds `value` and returns the minimum over the last `window` values.
    fn push(&mut self, value: f32) -> f32 {
        let index = self.next_index;
        self.next_index += 1;

        // Anything already in the deque that is no smaller than the new value
        // can never be the minimum again: the new value outlives it.
        while !self.is_empty() {
            let back = (self.tail + self.capacity() - 1) % self.capacity();
            if self.entries[back].1 >= value {
                self.tail = back;
            } else {
                break;
            }
        }
        self.entries[self.tail] = (index, value);
        self.tail = (self.tail + 1) % self.capacity();

        // Drop whatever has fallen out of the window.
        while !self.is_empty() && self.entries[self.head].0 + self.window as u64 <= index {
            self.head = (self.head + 1) % self.capacity();
        }

        self.entries[self.head].1
    }
}

/// See [`LimiterConfig`]. Stereo-linked: both channels take the same gain, so
/// limiting one side does not shift the image toward the other.
#[derive(Debug, Default)]
pub struct Limiter {
    /// The audio, delayed so the gain reduction arrives before the peak does.
    delay: Vec<f32>,
    delay_len: usize,
    write: usize,
    /// The sliding minimum of the instantaneous gain targets.
    minimum: SlidingMin,
    /// A box average over the minimum, which is what makes the gain
    /// *continuous* rather than stepping down the moment a peak enters the
    /// look-ahead window.
    box_ring: Vec<f32>,
    box_write: usize,
    box_sum: f64,
    /// The released gain: follows the box average down immediately and comes
    /// back up over `release_ms`.
    gain: f32,
    sample_rate: f32,
    /// The most gain reduction applied since the last `take_max_reduction_db`,
    /// in decibels, as a positive number.
    max_reduction_db: f32,
}

impl Limiter {
    pub fn new() -> Self {
        Self {
            gain: 1.0,
            ..Self::default()
        }
    }

    /// Off-RT: allocates every buffer the processing needs.
    ///
    /// The look-ahead window and the averaging window are the same length, and
    /// the audio is delayed by one sample less than that. That relationship is
    /// what makes the limiter a brickwall rather than an approximation — see
    /// `process`.
    pub fn prepare(&mut self, sample_rate: f32, config: &LimiterConfig) {
        self.sample_rate = sample_rate;
        let window = ((config.lookahead_ms / 1000.0 * sample_rate).round() as usize).max(1);
        self.minimum.prepare(window);
        self.box_ring.clear();
        self.box_ring.resize(window, 1.0);
        self.box_write = 0;
        self.box_sum = window as f64;
        // Exactly `window - 1`, and not a sample more: the guarantee in
        // `process` covers input frame `n - window + 1` at output frame `n`,
        // so a longer delay would put an unprotected sample under a gain
        // computed for a later one.
        self.delay_len = window - 1;
        self.delay.clear();
        self.delay.resize(self.delay_len * CHANNELS, 0.0);
        self.write = 0;
        self.gain = 1.0;
        self.max_reduction_db = 0.0;
    }

    /// Limits `channels` in place.
    ///
    /// **Why this is a brickwall and not a hope.** Let `t[n]` be the gain that
    /// would put input frame *n* exactly on the ceiling, `m[n]` the minimum of
    /// `t` over the last `W` frames, and `g[n]` the average of `m` over the
    /// last `W`. Every `m` in that average covers a window containing frame
    /// `n - W + 1`, so `g[n] <= t[n - W + 1]`. Delaying the audio by `W - 1`
    /// frames therefore guarantees the output never exceeds the ceiling — with
    /// no clipping stage, and no dependence on how fast the gain "can" move.
    ///
    /// The averaging is what keeps it clean: a sliding minimum alone steps
    /// down the instant a peak enters the window, and a step in gain is
    /// audible as a click on quiet material. The box filter turns that step
    /// into a ramp exactly as long as the look-ahead.
    ///
    /// `sidechain`, when given, is what the gain computer measures instead of
    /// the signal itself — the **key** that turns a brickwall into a ducker.
    /// The gain is still applied to (and the delay still runs on) the main
    /// signal, so a loud key pushes the main down under the ceiling: feed a
    /// track a kick's bus and it gets out of the kick's way. The master bus
    /// passes `None` and limits what runs through it, exactly as before.
    ///
    /// RT: no allocation.
    pub fn process(
        &mut self,
        channels: &mut [&mut [f32]],
        sidechain: Option<&[f32]>,
        config: &LimiterConfig,
    ) {
        if self.box_ring.is_empty() || channels.is_empty() {
            return;
        }
        let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let release_coeff = release_coefficient(config.release_ms, self.sample_rate);
        let ceiling = config.ceiling.max(1e-6);
        let used = channels.len().min(CHANNELS);

        for frame in 0..frames {
            // What the gain computer looks at: the key when there is one, and
            // otherwise the signal itself. Stereo-linked either way — one gain
            // for both sides, from whichever is louder — because independent
            // per-channel gains pull the image toward the quieter side every
            // time the other one peaks.
            let peak = match sidechain {
                Some(key) => key.get(frame).copied().unwrap_or(0.0).abs(),
                None => {
                    let mut peak = 0.0f32;
                    for channel in channels.iter().take(used) {
                        peak = peak.max(channel[frame].abs());
                    }
                    peak
                }
            };

            let target = if peak > ceiling { ceiling / peak } else { 1.0 };
            let minimum = self.minimum.push(target);

            // The box average, kept as a running sum so it costs one add and
            // one subtract per frame rather than a scan.
            self.box_sum += (minimum - self.box_ring[self.box_write]) as f64;
            self.box_ring[self.box_write] = minimum;
            self.box_write = (self.box_write + 1) % self.box_ring.len();
            let smoothed = (self.box_sum / self.box_ring.len() as f64) as f32;

            // Down immediately, up over the release. Staying at or below
            // `smoothed` is what preserves the guarantee above.
            self.gain = if smoothed < self.gain {
                smoothed
            } else {
                self.gain + (smoothed - self.gain) * release_coeff
            };

            // Read the delayed frame out, write the incoming one in. A
            // one-sample look-ahead needs no delay line at all, and indexing
            // an empty one would panic.
            if self.delay_len == 0 {
                for channel in channels.iter_mut().take(used) {
                    channel[frame] *= self.gain;
                }
            } else {
                for (index, channel) in channels.iter_mut().take(used).enumerate() {
                    let slot = self.write * CHANNELS + index;
                    let delayed = self.delay[slot];
                    self.delay[slot] = channel[frame];
                    channel[frame] = delayed * self.gain;
                }
                self.write = (self.write + 1) % self.delay_len;
            }
            // Channels past the pair the limiter links are silenced rather
            // than passed: they would arrive ahead of the ones that are
            // delayed, and a master bus wider than stereo is not a thing this
            // limiter claims to handle.
            for channel in channels.iter_mut().skip(used) {
                channel[frame] = 0.0;
            }

            if self.gain < 1.0 {
                let reduction_db = -20.0 * self.gain.max(1e-6).log10();
                self.max_reduction_db = self.max_reduction_db.max(reduction_db);
            }
        }
    }

    /// The limiter's own latency, in samples. On the master it delays the
    /// whole mix equally, so nothing is out of time with anything else; it
    /// is in what `Realised::latency_samples` reports, which is the number
    /// that says what a configuration costs (TDD §5.5).
    pub fn latency_samples(&self) -> u32 {
        self.delay_len as u32
    }

    /// The most gain reduction since this was last called, in positive
    /// decibels, and resets the maximum. A meter reads it; so does the offline
    /// render, to say how hard it had to work.
    pub fn take_max_reduction_db(&mut self) -> f32 {
        std::mem::take(&mut self.max_reduction_db)
    }

    /// Back to a clean pass-through, keeping the buffers `prepare` sized: the
    /// gain back at unity, the delay line and the detector windows cleared. What
    /// a transport stop needs of an insert limiter, the way every other effect's
    /// `reset` clears its state without reallocating (INVARIANT 1).
    pub fn reset(&mut self) {
        self.gain = 1.0;
        self.max_reduction_db = 0.0;
        self.write = 0;
        self.delay.fill(0.0);
        // Refill the look-ahead windows with "no reduction", the value they
        // hold on a fresh `prepare`.
        let window = self.box_ring.len();
        self.minimum.prepare(window);
        self.box_ring.fill(1.0);
        self.box_write = 0;
        self.box_sum = window as f64;
    }
}

/// The most channels the limiter links. Stereo is what a master bus is.
const CHANNELS: usize = 2;

/// The per-sample coefficient for an exponential release reaching ~63% of its
/// target in `release_ms`.
fn release_coefficient(release_ms: f32, sample_rate: f32) -> f32 {
    if release_ms <= 0.0 || sample_rate <= 0.0 {
        return 1.0;
    }
    1.0 - (-1.0 / (release_ms / 1000.0 * sample_rate)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn limited(input: &[f32], config: &LimiterConfig) -> Vec<f32> {
        let mut limiter = Limiter::new();
        limiter.prepare(SR, config);
        let mut left = input.to_vec();
        let mut right = input.to_vec();
        limiter.process(&mut [&mut left[..], &mut right[..]], None, config);
        left
    }

    /// The claim the whole design exists to make. Not "usually under the
    /// ceiling" — never over it, on material chosen to be as hostile as a
    /// limiter ever sees.
    #[test]
    fn nothing_ever_exceeds_the_ceiling() {
        let config = LimiterConfig::default();
        // A quiet passage, then a full-scale step with no warning, then a
        // sustained overload at four times the ceiling.
        let mut input = vec![0.01; 4_000];
        input.extend(std::iter::repeat_n(4.0, 2_000));
        input.extend(std::iter::repeat_n(0.01, 4_000));
        input.extend((0..8_000).map(|i| 3.0 * (i as f32 / 7.0).sin()));

        let out = limited(&input, &config);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak <= config.ceiling + 1e-4,
            "peak {peak} is over the {} ceiling",
            config.ceiling
        );
    }

    /// A limiter that quietly attenuated everything would pass the test above
    /// and be useless.
    #[test]
    fn material_already_under_the_ceiling_comes_through_untouched() {
        let config = LimiterConfig::default();
        let input: Vec<f32> = (0..4_000)
            .map(|i| 0.5 * (std::f32::consts::TAU * i as f32 / 100.0).sin())
            .collect();
        let out = limited(&input, &config);

        // Past the look-ahead delay, output must equal input sample for
        // sample.
        let delay = ((config.lookahead_ms / 1000.0 * SR).round() as usize) - 1;
        for i in delay..input.len() {
            assert!(
                (out[i] - input[i - delay]).abs() < 1e-6,
                "sample {i}: {} against {}",
                out[i],
                input[i - delay]
            );
        }
    }

    /// The gain has to arrive *before* the transient. A limiter that reacted
    /// on the sample itself would let the first cycle of every attack through
    /// at full height, which is exactly the overshoot look-ahead exists to
    /// remove.
    #[test]
    fn the_gain_is_already_down_when_the_transient_arrives() {
        let config = LimiterConfig::default();
        let mut input = vec![0.0; 2_000];
        input[1_000] = 8.0;
        let out = limited(&input, &config);

        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak <= config.ceiling + 1e-4, "overshoot: {peak}");
        assert!(peak > 0.0, "the transient must still be there at all");
    }

    /// A sliding minimum on its own steps the gain down the instant a peak
    /// enters the window, and a step in gain on quiet material is a click.
    #[test]
    fn the_gain_ramps_rather_than_stepping() {
        let config = LimiterConfig::default();
        let mut input = vec![0.2; 3_000];
        for sample in input.iter_mut().skip(1_500) {
            *sample = 2.0;
        }
        let out = limited(&input, &config);

        // The largest sample-to-sample jump through the approach to the
        // transient, over material that is otherwise constant. Skipping the
        // first few hundred samples: the delay line starts empty, so the
        // signal's own arrival is a step and has nothing to do with the gain.
        let jump = out[500..1_500]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            jump < 0.01,
            "the gain change should be spread over the look-ahead, largest \
             step was {jump}"
        );
    }

    #[test]
    fn the_gain_comes_back_up_after_the_overload_passes() {
        let config = LimiterConfig {
            release_ms: 10.0,
            ..LimiterConfig::default()
        };
        let mut input = vec![4.0; 2_000];
        input.extend(std::iter::repeat_n(0.5, 8_000));
        let out = limited(&input, &config);

        // Well past the release, a 0.5 input must be back at 0.5.
        let settled = out[7_000];
        assert!(
            (settled - 0.5).abs() < 1e-3,
            "expected the gain back at unity, got {settled}"
        );
    }

    #[test]
    fn gain_reduction_is_reported_and_resets_when_read() {
        let config = LimiterConfig::default();
        let mut limiter = Limiter::new();
        limiter.prepare(SR, &config);
        let mut left = vec![2.0; 4_000];
        let mut right = vec![2.0; 4_000];
        limiter.process(&mut [&mut left[..], &mut right[..]], None, &config);

        let reduction = limiter.take_max_reduction_db();
        // 2.0 down to 0.966 is about 6.3 dB.
        assert!(
            (reduction - 6.3).abs() < 0.5,
            "expected ~6.3 dB of reduction, got {reduction}"
        );
        assert_eq!(
            limiter.take_max_reduction_db(),
            0.0,
            "reading it must reset it, or a meter shows the loudest moment of \
             the session forever"
        );
    }

    /// Stereo-linked: limiting one side by its own peak would pull the image
    /// toward the other every time it fired.
    #[test]
    fn both_channels_take_the_same_gain() {
        let config = LimiterConfig::default();
        let mut limiter = Limiter::new();
        limiter.prepare(SR, &config);
        // Left overloads, right is quiet and constant.
        let mut left = vec![4.0; 4_000];
        let mut right = vec![0.5; 4_000];
        limiter.process(&mut [&mut left[..], &mut right[..]], None, &config);

        let settled_right = right[3_000];
        let expected = 0.5 * (config.ceiling / 4.0);
        assert!(
            (settled_right - expected).abs() < 1e-3,
            "the right channel must be pulled down with the left: expected \
             {expected}, got {settled_right}"
        );
    }
}
