//! DisgustingBeat: a few seconds of memory with curves drawn over it
//! (`docs/disgusting-beat-plan.md` §4).
//!
//! The whole machine is one line. Let `o(φ)` be the time lane's offset in
//! beats, negative for the past:
//!
//! ```text
//! delay(t) = −o(φ(t)) · samples_per_beat
//! read(t)  = write(t) − delay(t)
//! rate(t)  = d(read)/dt = 1 − d(delay)/dt
//! ```
//!
//! Nothing else is written. A segment falling at one beat per beat holds the
//! sound still (`rate` 0), half that plays it an octave down, twice that
//! plays it backwards, and a vertical drop is a stutter's repeat. The lane's
//! value is in **lane-lengths** for exactly this reason: the freeze is then
//! the 45° diagonal whatever the lane's length is, which is what the window
//! can draw a guide for.
//!
//! What it owns: the ring, two read heads and the crossfade between them, the
//! four lane evaluations, the tone filter and the peak buckets the window
//! draws. What it does not: the dry signal (`EffectNode`'s, by rule 7) and
//! the events (the node walks those and hands over a [`NoteInput`]).

use fontelle_dsp::{Interpolation, SvfCoeffs, SvfFilter, SvfMode, interpolate};
use fontelle_types::{
    DISGUSTING_BEAT_MEMORY_SECONDS, DisgustingBeatConfig, DisgustingBeatGrid,
    DisgustingBeatLaneKind, DisgustingBeatNotes, DisgustingBeatQuality, DisgustingBeatSync,
    DisgustingBeatTone, MusicalTime, lane_ticks,
};

use crate::NoteInput;

/// Channels one DisgustingBeat will keep a memory for.
const MAX_CHANNELS: usize = 2;

/// How often the three slow lanes are read, in samples.
///
/// The time lane is read per sample — it is the read position, and a stair in
/// it is a stair in the pitch. The other three are control signals and are
/// ramped across the step, which is the synth's own `MOD_STEP` and its
/// reasoning: eight samples is 6 kHz of control bandwidth, past anything a
/// drawn curve contains.
const STEP: usize = 8;

/// How many buckets of peak envelope the window is given.
pub const DISGUSTING_BEAT_BUCKETS: usize = 512;

/// A jump in the read position bigger than this starts a crossfade.
///
/// Half a millisecond: under it, a moving head is a slide and crossfading
/// would only smear it.
const JUMP_SAMPLES: f32 = 24.0;

/// What the window is told about what the machine is doing
/// (`docs/disgusting-beat-plan.md` §7.2).
#[derive(Debug, Clone, Copy)]
pub struct DisgustingBeatFrame {
    /// Where the pattern is, 0..1 round the time lane.
    pub phase: f32,
    /// Where the read head is, as an offset in lane-lengths — the same unit
    /// the lane is drawn in, so the window puts it straight on the grid.
    pub offset: f32,
    /// How fast the memory is being read: 1 is live, 0 is frozen, negative is
    /// backwards.
    pub rate: f32,
    /// Whether the read is against the end of what has actually been written
    /// — the window says so rather than letting it sound wrong quietly.
    pub clamped: bool,
    /// Seconds of memory actually written, up to [`DISGUSTING_BEAT_MEMORY_SECONDS`].
    pub filled_seconds: f32,
}

impl Default for DisgustingBeatFrame {
    fn default() -> Self {
        Self {
            phase: 0.0,
            offset: 0.0,
            rate: 1.0,
            clamped: false,
            filled_seconds: 0.0,
        }
    }
}

/// One read head into the memory.
#[derive(Debug, Clone, Copy, Default)]
struct Head {
    /// Absolute position in written samples, fractional.
    position: f64,
}

pub struct DisgustingBeat {
    sample_rate: f32,
    /// The memory, one `Vec` per channel, sized in `prepare` and never
    /// resized after (INVARIANT 1).
    memory: Vec<Vec<f32>>,
    /// How many samples have been written since the last reset. The write
    /// position is this modulo the ring's length, and the *absolute* count is
    /// what the read heads work in — a ring index wraps and a read that has
    /// to know how far back it may go cannot.
    written: u64,
    /// The two heads and the fade between them.
    head: Head,
    old: Head,
    /// Where the read was last sample, so a jump can be told from a slide.
    ///
    /// A field rather than a local in `process`, and that is not a detail: a
    /// pattern's jumps land on musical boundaries, and at 120 bpm a musical
    /// boundary is a whole number of 128-sample blocks. A local reset per
    /// block missed every jump the pattern made, and the smoothing knob did
    /// nothing whatever it was set to.
    last_read: f64,
    /// 0 when the fade is done; counts down in samples.
    fade: u32,
    fade_length: u32,
    /// The tone lane's filter, one per channel, and the second one the band
    /// mode needs.
    tone: Vec<SvfFilter>,
    tone_extra: Vec<SvfFilter>,
    /// Their coefficients, rebuilt once a step rather than once a sample.
    tone_coeffs: SvfCoeffs,
    tone_extra_coeffs: SvfCoeffs,
    /// Where the three slow lanes are going, read once a step.
    volume: f32,
    pan: f32,
    tone_value: f32,
    /// And where they are now, walking towards it a sample at a time.
    ///
    /// **Ramped rather than held**, which is what keeps a gate's edge from
    /// being a click: a volume lane stepping 1 → 0 between two samples is a
    /// discontinuity at full scale, and every preset in the *Gate* category
    /// is made of those edges. Eight samples is 0.17 ms — still snappy
    /// enough to read as a gate rather than a fade.
    volume_now: f32,
    volume_step: f32,
    pan_now: f32,
    pan_step: f32,
    /// Where the free and retriggered clocks count from, in ticks.
    origin: f64,
    /// The phase a stopped transport keeps for itself, in ticks.
    idle_tick: f64,
    /// The last note-on count seen, so a repeated key still retriggers.
    last_ons: u32,
    /// Which scene a note picked, if one has.
    note_scene: Option<u8>,
    /// The peak envelope the window draws, min and max per bucket.
    buckets: Vec<(f32, f32)>,
    /// Where the read head was when each bucket was written, in buckets
    /// behind the write head. The *history* of the read, which is the picture
    /// that says what a curve does: a freeze walks away from the present, a
    /// stutter saws, a reverse climbs back into the past.
    trail: Vec<f32>,
    /// How many samples go in one bucket.
    bucket_samples: usize,
    bucket_fill: usize,
    frame: DisgustingBeatFrame,
}

impl DisgustingBeat {
    pub fn new() -> Self {
        Self {
            sample_rate: 48_000.0,
            memory: Vec::new(),
            written: 0,
            head: Head::default(),
            old: Head::default(),
            last_read: f64::NAN,
            fade: 0,
            fade_length: 0,
            tone: Vec::new(),
            tone_extra: Vec::new(),
            tone_coeffs: SvfFilter::coeffs(SvfMode::Lowpass, 20_000.0, 0.7, 0.0, 48_000.0),
            tone_extra_coeffs: SvfFilter::coeffs(SvfMode::Highpass, 20.0, 0.7, 0.0, 48_000.0),
            volume: 1.0,
            pan: 0.0,
            tone_value: 0.0,
            volume_now: 1.0,
            volume_step: 0.0,
            pan_now: 0.0,
            pan_step: 0.0,
            origin: 0.0,
            idle_tick: 0.0,
            last_ons: 0,
            note_scene: None,
            buckets: Vec::new(),
            trail: Vec::new(),
            bucket_samples: 1,
            bucket_fill: 0,
            frame: DisgustingBeatFrame::default(),
        }
    }

    /// The only place that allocates.
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(1.0);
        let frames = (DISGUSTING_BEAT_MEMORY_SECONDS * self.sample_rate).ceil() as usize;
        self.memory = (0..MAX_CHANNELS).map(|_| vec![0.0; frames]).collect();
        self.tone = (0..MAX_CHANNELS).map(|_| SvfFilter::new()).collect();
        self.tone_extra = (0..MAX_CHANNELS).map(|_| SvfFilter::new()).collect();
        self.buckets = vec![(0.0, 0.0); DISGUSTING_BEAT_BUCKETS];
        self.trail = vec![0.0; DISGUSTING_BEAT_BUCKETS];
        self.bucket_samples = (frames / DISGUSTING_BEAT_BUCKETS).max(1);
        self.reset();
    }

    pub fn reset(&mut self) {
        for channel in &mut self.memory {
            channel.fill(0.0);
        }
        self.written = 0;
        self.head = Head::default();
        self.old = Head::default();
        self.last_read = f64::NAN;
        self.fade = 0;
        self.volume = 1.0;
        self.pan = 0.0;
        self.tone_value = 0.0;
        self.volume_now = 1.0;
        self.volume_step = 0.0;
        self.pan_now = 0.0;
        self.pan_step = 0.0;
        self.origin = 0.0;
        self.idle_tick = 0.0;
        self.note_scene = None;
        for filter in self.tone.iter_mut().chain(self.tone_extra.iter_mut()) {
            filter.reset();
        }
        self.buckets.fill((0.0, 0.0));
        self.trail.fill(0.0);
        self.bucket_fill = 0;
        self.frame = DisgustingBeatFrame::default();
    }

    /// What the window draws.
    pub fn frame(&self) -> DisgustingBeatFrame {
        self.frame
    }

    /// The peak envelope of the memory, as peak/RMS pairs, in ring order.
    pub fn buckets(&self) -> &[(f32, f32)] {
        &self.buckets
    }

    /// Where the read head has been, one entry per bucket, in the same ring
    /// order as [`buckets`](Self::buckets).
    pub fn trail(&self) -> &[f32] {
        &self.trail
    }

    /// Which bucket the write head is in, so a reader can unwrap the ring.
    pub fn newest_bucket(&self) -> usize {
        if self.buckets.is_empty() {
            return 0;
        }
        ((self.written / self.bucket_samples as u64) as usize) % self.buckets.len()
    }

    /// One block, in place.
    pub fn process(
        &mut self,
        channels: &mut [&mut [f32]],
        notes: NoteInput,
        grid: &DisgustingBeatGrid,
        config: &DisgustingBeatConfig,
        music: MusicalTime,
    ) {
        if self.memory.is_empty() || channels.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels
            .iter()
            .take(used)
            .map(|c| c.len())
            .min()
            .unwrap_or(0);
        if frames == 0 {
            return;
        }

        let scene = self.scene_of(notes, config, music);
        let beats_per_bar = music.beats_per_bar.max(1);
        let time_lane = grid.lane(scene, DisgustingBeatLaneKind::Time);
        let volume_lane = grid.lane(scene, DisgustingBeatLaneKind::Volume);
        let tone_lane = grid.lane(scene, DisgustingBeatLaneKind::Tone);
        let pan_lane = grid.lane(scene, DisgustingBeatLaneKind::Pan);

        let samples_per_tick = music.samples_per_tick();
        let time_ticks = lane_ticks(time_lane.length, config.rate, beats_per_bar);
        // The time lane's value is in lane-lengths, so this is what turns one
        // into samples — and it is why a freeze is a freeze at any length.
        let lane_samples = time_ticks * samples_per_tick;
        let look_samples =
            config.latency_samples(music.bpm, beats_per_bar, self.sample_rate) as f64;
        let quality = match config.quality {
            DisgustingBeatQuality::Normal => Interpolation::Normal,
            DisgustingBeatQuality::High => Interpolation::High,
        };

        let ring = self.memory[0].len() as f64;
        let fade_length = ((config.smooth_ms / 1000.0) * self.sample_rate).round() as u32;
        self.fade_length = fade_length;

        // Where the song is at this block's first sample, in ticks, on
        // whichever clock the chooser names.
        let mut tick = self.clock_tick(config, music);
        let tick_step = music.ticks_per_sample;

        let mut clamped_anywhere = false;
        let mut last_rate = 1.0f64;

        for frame in 0..frames {
            // ---- write first, always: a memory with a hole in it is the bug
            // where a stutter thrown in after four bars of silence plays four
            // bars of silence.
            let write_index = (self.written % self.memory[0].len() as u64) as usize;
            let mut bucket_peak = 0.0f32;
            for (channel, memory) in channels.iter().take(used).zip(self.memory.iter_mut()) {
                let sample = channel[frame];
                memory[write_index] = sample;
                bucket_peak = bucket_peak.max(sample.abs());
            }
            self.push_bucket(bucket_peak);
            self.written += 1;
            let now = self.written as f64 - 1.0;

            // ---- the phase, and what the lanes say there
            let phase_time = self.phase_of(tick, time_ticks, config.swing);
            let offset_lanes = time_lane.value_at(phase_time, 0.0) * config.time as f64;
            let offset_samples = offset_lanes * lane_samples;

            // With lookahead the whole read sits `look_samples` back, so a
            // positive offset reads what has arrived since. Without it, the
            // read is at `now` and a positive offset would be a read of
            // nothing — clamped below, with the window told.
            let base = now - look_samples;
            if base < 0.0 {
                // Only reachable in the first beat or bar after a reset, and
                // only with lookahead on. Its latency is a contract the graph
                // compensates against, so "there is no song here yet" is
                // silence — playing live would put this track ahead of every
                // other one for a beat.
                for channel in channels.iter_mut().take(used) {
                    channel[frame] = 0.0;
                }
                tick += tick_step;
                continue;
            }
            let mut read = base + offset_samples;
            if read > now {
                // Reading the future without having asked for it: only
                // reachable with `look` off and a positive curve, and the
                // honest answer is the newest thing there is.
                read = now;
                clamped_anywhere = true;
            }
            let full = self.written >= self.memory[0].len() as u64;
            let oldest = self.written as f64 - ring;
            if full {
                // The request is deeper than the memory goes. The oldest
                // thing in it is still *from the past*, which is what the
                // curve asked for, so hold there.
                if read < oldest {
                    read = oldest;
                    clamped_anywhere = true;
                }
            } else if read < 0.0 {
                // Nothing has been written that far back — a stop, a seek, a
                // loop wrap or an un-bypass a moment ago. Playing the live
                // signal until the memory catches up is a mild wrongness for
                // one bar; playing silence is what somebody files as a bug.
                read = base;
                clamped_anywhere = true;
            }

            // ---- the jump, and the crossfade over it
            if self.last_read.is_finite() {
                let expected = self.last_read + 1.0;
                if (read - expected).abs() > JUMP_SAMPLES as f64 && fade_length > 0 {
                    self.old = self.head;
                    self.fade = fade_length;
                }
            }
            let rate = if self.last_read.is_finite() {
                read - self.last_read
            } else {
                1.0
            };
            self.last_read = read;
            self.head.position = read;
            last_rate = rate;
            // One number per bucket, written with the sample that goes in it:
            // how far behind the present the read was at that moment. The
            // window draws it as a line over the memory.
            self.push_trail(now - read);

            // ---- read it
            let fade_t = if self.fade > 0 && self.fade_length > 0 {
                let t = 1.0 - self.fade as f32 / self.fade_length as f32;
                self.fade -= 1;
                t
            } else {
                1.0
            };
            let (new_gain, old_gain) = crossfade(fade_t);

            // ---- the three control lanes, at step rate
            if frame % STEP == 0 {
                let tone_ticks = lane_ticks(tone_lane.length, config.rate, beats_per_bar);
                let volume_ticks = lane_ticks(volume_lane.length, config.rate, beats_per_bar);
                let pan_ticks = lane_ticks(pan_lane.length, config.rate, beats_per_bar);
                self.volume = (volume_lane
                    .value_at(self.phase_of(tick, volume_ticks, config.swing), 1.0)
                    as f32)
                    .mul_add(config.volume, 1.0 - config.volume);
                self.tone_value =
                    tone_lane.value_at(self.phase_of(tick, tone_ticks, config.swing), 0.0) as f32
                        * config.tone;
                self.pan = pan_lane.value_at(self.phase_of(tick, pan_ticks, config.swing), 0.0)
                    as f32
                    * config.pan;
                self.set_tone(config);
                self.volume_step = (self.volume - self.volume_now) / STEP as f32;
                self.pan_step = (self.pan - self.pan_now) / STEP as f32;
            }
            self.volume_now += self.volume_step;
            self.pan_now += self.pan_step;

            let (left_gain, right_gain) = pan_gains(self.pan_now);
            let out_gain = if config.output_db == 0.0 {
                1.0
            } else {
                10f32.powf(config.output_db / 20.0)
            };

            for (index, channel) in channels.iter_mut().take(used).enumerate() {
                let memory = &self.memory[index];
                let mut sample =
                    read_ring(memory, self.head.position, self.written, quality) * new_gain;
                if old_gain > 0.0 {
                    let old = self.old.position + (self.fade_length - self.fade) as f64;
                    sample += read_ring(memory, old, self.written, quality) * old_gain;
                }
                sample *= self.volume_now * out_gain;
                if self.pan_now != 0.0 {
                    sample *= if index == 0 { left_gain } else { right_gain };
                }
                if self.tone_value != 0.0 {
                    sample = self.filter(index, sample, config.tone_mode);
                }
                channel[frame] = sample;
            }

            tick += tick_step;
        }

        // What the window is told, from the block's last sample.
        self.frame = DisgustingBeatFrame {
            phase: self.phase_of(tick, time_ticks, config.swing) as f32,
            offset: (time_lane.value_at(self.phase_of(tick, time_ticks, config.swing), 0.0)
                * config.time as f64) as f32,
            rate: last_rate as f32,
            clamped: clamped_anywhere,
            filled_seconds: (self.written as f32 / self.sample_rate)
                .min(DISGUSTING_BEAT_MEMORY_SECONDS),
        };
        if !music.rolling {
            self.idle_tick = tick;
        }
    }

    /// Which scene is playing: the knob, or the note that overrode it.
    fn scene_of(
        &mut self,
        notes: NoteInput,
        config: &DisgustingBeatConfig,
        music: MusicalTime,
    ) -> usize {
        if config.notes != DisgustingBeatNotes::Off {
            if notes.ons != self.last_ons {
                self.last_ons = notes.ons;
                if config.notes == DisgustingBeatNotes::Retrigger {
                    self.origin = self.song_tick(music);
                }
            }
            match notes.last {
                // The pitch class picks it, in any octave.
                Some(key) => self.note_scene = Some(key % 12),
                None => self.note_scene = None,
            }
        } else {
            self.note_scene = None;
        }
        self.note_scene.unwrap_or(config.scene) as usize
    }

    /// The song's own tick, whether or not the transport is moving.
    fn song_tick(&self, music: MusicalTime) -> f64 {
        if music.rolling {
            music.tick
        } else {
            self.idle_tick
        }
    }

    /// Where the pattern counts from, on whichever clock the chooser names.
    fn clock_tick(&mut self, config: &DisgustingBeatConfig, music: MusicalTime) -> f64 {
        let song = self.song_tick(music);
        match config.sync {
            DisgustingBeatSync::Song => song,
            DisgustingBeatSync::Retrigger | DisgustingBeatSync::Free => {
                if self.origin == 0.0 && song != 0.0 && config.sync == DisgustingBeatSync::Free {
                    self.origin = song;
                }
                song - self.origin
            }
        }
    }

    /// Where in a lane of `ticks` the song is, 0..1, with `swing` leaning on
    /// the offbeat.
    ///
    /// Swing is applied to the **phase** rather than to a grid laid over it,
    /// so it bends the drawn curve rather than quantising it: a ramp drawn
    /// across a beat comes out of a swung lane as a ramp that takes longer to
    /// start.
    fn phase_of(&self, tick: f64, ticks: f64, swing: f32) -> f64 {
        let phase = (tick / ticks.max(1.0)).rem_euclid(1.0);
        if swing <= 0.0 {
            return phase;
        }
        // Each quarter of the lane is a beat holding a pair of eighths; the
        // first of each pair is stretched and the second shortened, so the
        // **beats do not move** and the offbeats lean. Full swing is the
        // triplet feel — the offbeat two thirds of the way through the beat,
        // which is what "100 %" means everywhere else in music.
        let pairs = 4.0;
        let scaled = phase * pairs;
        let pair = scaled.floor();
        let inside = scaled - pair;
        let pivot = 0.5 + swing as f64 / 6.0;
        let bent = if inside < pivot {
            inside / pivot * 0.5
        } else {
            0.5 + (inside - pivot) / (1.0 - pivot) * 0.5
        };
        ((pair + bent) / pairs).rem_euclid(1.0)
    }

    /// Rebuilds the tone lane's coefficients — once a step, not once a
    /// sample: a `tan` per sample per channel for a curve that moves at
    /// control rate is a cost with nothing behind it.
    ///
    /// The lane is bipolar around a silent centre. Below it a low-pass closes
    /// from 18 kHz; above it a high-pass opens from 20 Hz. `Band` runs both,
    /// so the far side narrows as well; `Tilt` uses gentler corners so the
    /// two ends trade rather than one end disappearing.
    fn set_tone(&mut self, config: &DisgustingBeatConfig) {
        if self.tone_value == 0.0 {
            return;
        }
        let octaves = config.tone_range * self.tone_value.abs();
        let rate = self.sample_rate;
        let (mode, hz) = match config.tone_mode {
            DisgustingBeatTone::LowHigh | DisgustingBeatTone::Band => {
                if self.tone_value < 0.0 {
                    (SvfMode::Lowpass, 18_000.0 / 2f32.powf(octaves * 2.0))
                } else {
                    (SvfMode::Highpass, 20.0 * 2f32.powf(octaves * 2.0))
                }
            }
            DisgustingBeatTone::Tilt => {
                if self.tone_value < 0.0 {
                    (SvfMode::Lowpass, 8_000.0 / 2f32.powf(octaves))
                } else {
                    (SvfMode::Highpass, 40.0 * 2f32.powf(octaves))
                }
            }
        };
        self.tone_coeffs = SvfFilter::coeffs(mode, hz.clamp(20.0, 20_000.0), 0.7, 0.0, rate);
        if config.tone_mode == DisgustingBeatTone::Band {
            let (other_mode, other_hz) = if self.tone_value < 0.0 {
                (SvfMode::Highpass, 20.0 * 2f32.powf(octaves))
            } else {
                (SvfMode::Lowpass, 18_000.0 / 2f32.powf(octaves))
            };
            self.tone_extra_coeffs =
                SvfFilter::coeffs(other_mode, other_hz.clamp(20.0, 20_000.0), 0.7, 0.0, rate);
        }
    }

    fn filter(&mut self, index: usize, sample: f32, mode: DisgustingBeatTone) -> f32 {
        let first = self.tone[index].process(sample, &self.tone_coeffs);
        if mode == DisgustingBeatTone::Band {
            self.tone_extra[index].process(first, &self.tone_extra_coeffs)
        } else {
            first
        }
    }

    /// Where the read head was, into the bucket the sample just written
    /// belongs to.
    ///
    /// The **last** sample of a bucket wins rather than an average, because
    /// an average across a stutter's jump lands between the two reads, where
    /// the head never was.
    fn push_trail(&mut self, behind: f64) {
        if self.trail.is_empty() {
            return;
        }
        let written = self.written.saturating_sub(1);
        let bucket = ((written / self.bucket_samples as u64) as usize) % self.trail.len();
        self.trail[bucket] = (behind / self.bucket_samples as f64) as f32;
    }

    fn push_bucket(&mut self, peak: f32) {
        if self.buckets.is_empty() {
            return;
        }
        let bucket = ((self.written / self.bucket_samples as u64) as usize) % self.buckets.len();
        if self.bucket_fill == 0 {
            self.buckets[bucket] = (0.0, 0.0);
        }
        let slot = &mut self.buckets[bucket];
        slot.0 = slot.0.max(peak);
        slot.1 += peak * peak;
        self.bucket_fill += 1;
        if self.bucket_fill >= self.bucket_samples {
            slot.1 = (slot.1 / self.bucket_samples as f32).sqrt();
            self.bucket_fill = 0;
        }
    }
}

impl Default for DisgustingBeat {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads the ring at an absolute written-sample position.
///
/// `written` is how many samples have ever been written, so the position is
/// turned into a ring index here and nowhere else. A position outside what is
/// in the ring reads silence rather than whatever the wrap landed on.
fn read_ring(memory: &[f32], position: f64, written: u64, quality: Interpolation) -> f32 {
    let length = memory.len();
    if length == 0 || written == 0 {
        return 0.0;
    }
    // **The edges are held, not zeroed.** Every kernel here but `Draft` wants
    // samples either side of the read position, and the ring has none past
    // the write head — so a window with zeros in those taps puts a cliff
    // under a cubic, and a cubic through a cliff overshoots. That was the
    // crunch: one sample at 0.664 in a stretch of 0.625, every time a curve's
    // read touched live at a fractional position, which is what every segment
    // with a rate above one does on its way back to the present.
    //
    // Holding the edge sample is what a resampler does at the end of a
    // buffer, and the error it makes is bounded by the signal rather than by
    // full scale.
    let newest = written as f64 - 1.0;
    let oldest = (written as f64 - length as f64).max(0.0);
    let position = position.clamp(oldest, newest);
    // `interpolate` wants a slice and a position inside it, and the ring
    // wraps — so the four (or eight) taps are gathered into a stack window
    // first. Eight is the widest kernel `Interpolation` has.
    let base = position.floor();
    let fraction = position - base;
    // **An integer position is the sample.** Every kernel here is
    // interpolating in theory — at a whole sample each tap but the centre
    // lands on a zero of the sinc — but `sin(pi*n)/(pi*n)` computed in
    // floating point is not exactly zero, so the sum comes back a few ULPs
    // off. A fresh DisgustingBeat is a wire *sample for sample*, and a wire
    // that moved
    // the last bit at `high` quality is not one.
    if fraction == 0.0 {
        return memory[(base as i64).rem_euclid(length as i64) as usize];
    }
    let mut window = [0.0f32; 16];
    let centre = 8usize;
    for (offset, slot) in window.iter_mut().enumerate() {
        let at = (base + offset as f64 - centre as f64).clamp(oldest, newest);
        let index = (at as i64).rem_euclid(length as i64) as usize;
        *slot = memory[index];
    }
    interpolate(&window, centre as f64 + fraction, quality)
}

/// The crossfade between the two read heads: **linear**, and that is not the
/// usual choice.
///
/// An equal-power fade is right when the two sides are unrelated. Here they
/// are the *same recording at two positions*, which is very often strongly
/// correlated — that is what a repeat is — and two correlated signals under
/// an equal-power fade sum to √2 in the middle. `nothing_it_makes_is_louder_
/// than_what_went_in` is that, measured: a stutter on a sine peaked at 1.41
/// before this was a linear fade, which is a clipped master on every repeat.
///
/// The price is a 3 dB dip in the middle of a fade between *un*correlated
/// slices, over a window of a few milliseconds. That is much the smaller
/// harm, and it is the one a person can hear and draw around.
fn crossfade(t: f32) -> (f32, f32) {
    if t >= 1.0 {
        return (1.0, 0.0);
    }
    let t = t.clamp(0.0, 1.0);
    (t, 1.0 - t)
}

/// The pan lane's gains. Zero is exactly unity on both, so a lane at its
/// neutral value costs nothing.
fn pan_gains(pan: f32) -> (f32, f32) {
    if pan == 0.0 {
        return (1.0, 1.0);
    }
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5 * std::f32::consts::FRAC_PI_2;
    (
        angle.cos() * std::f32::consts::SQRT_2,
        angle.sin() * std::f32::consts::SQRT_2,
    )
}
