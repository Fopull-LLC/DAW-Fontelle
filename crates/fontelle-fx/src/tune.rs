//! The pitch corrector (`docs/tune-plan.md` §3).
//!
//! Its parameters are `fontelle_types::TuneConfig` — the document owns those,
//! this owns the tracker, the shifter, the pitch track and the vibrato's
//! phase. The primitives underneath it are `fontelle_dsp::PitchTracker` and
//! `fontelle_dsp::PsolaShifter`, which know nothing about scales, MIDI or
//! knobs; everything here is *what to do with a pitch once it is known*.
//!
//! # The path
//!
//! ```text
//! in ─► mono sum ─► tracker ─► slow / fast split ─► target ─► strength ─┐
//!  │                                                                     ▼
//!  │                                                        retune glide, humanize
//!  ▼                                                                     │
//! delay line ────────────► PSOLA, one mark schedule for both channels ◄──┘
//!                                       │
//!                                       ▼
//!                              formant read ─► output gain
//! ```
//!
//! Detection is on the mono sum and shifting is per channel through **one**
//! mark schedule, so a stereo source keeps its image (§3.7). Everything above
//! the delay line runs once a hop; everything below runs per sample.
//!
//! # What it is not
//!
//! Not polyphonic: a chord has no single period and this has no answer for
//! one. Not an offline note editor: that is a clip operation and belongs with
//! the clip's others (§13).

use fontelle_dsp::{GrainEngine, PitchTracker, PsolaShifter, cents_to_hz};
use fontelle_types::{
    TUNE_FROM_MIDI, TUNE_LOCK_CENTS, TUNE_LOCKED, TUNE_VOICED, TuneConfig, TuneControl, TuneEngine,
    TuneFrame,
};

const MAX_CHANNELS: usize = 2;

/// How fast the "note being sung" follows the detected pitch, in seconds. Any
/// slower and a real melody lags; any faster and the vibrato leaks into it.
const SLOW_TAU: f32 = 0.070;

/// A jump bigger than this is a new note rather than a wobble, in cents.
const ONSET_CENTS: f32 = 80.0;

/// How long a note has to hold still before humanize lets it drift, in
/// seconds, and how still "still" is.
///
/// §3.3 writes the second one as "three cents per hop", which at a 64-sample
/// hop is two thousand cents a second — a glide of a whole octave every half
/// second would count as a held note, and humanize would let go of it. It is a
/// **rate**: a hundred and twenty cents a second is a note drifting, and
/// anything faster is a line being sung.
const SETTLE_SECONDS: f32 = 0.300;
const SETTLE_CENTS_PER_SECOND: f32 = 120.0;

/// How wide the hysteresis round the current target is, in cents. A note sung
/// on the boundary between two scale degrees must not flip between them.
const TARGET_HYSTERESIS_CENTS: f32 = 15.0;

/// How long the ratio takes to relax to one over an unvoiced stretch, in
/// seconds.
const UNVOICED_RELAX_SECONDS: f32 = 0.005;

/// How long the added vibrato takes to fade in once its onset has passed.
const VIBRATO_FADE_SECONDS: f32 = 0.100;

/// The notes reaching this insert from the channel it listens to (§5).
///
/// A frame's worth of held keys, flattened: the node walks the events and this
/// is what it hands over, so `fontelle-dsp` and this module never see an
/// event and the DSP stays a function of numbers.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct NoteInput {
    /// The last key pressed and not yet released — last-note priority.
    pub last: Option<u8>,
    /// Every held key's pitch class, bit 0 = C.
    pub mask: u16,
    /// The channel's pitch bend, in cents.
    pub bend_cents: f32,
}

impl NoteInput {
    pub fn is_empty(&self) -> bool {
        self.last.is_none() && self.mask == 0
    }
}

/// A monophonic pitch corrector: a tracker, a shifter, and the decisions
/// between them.
pub struct Tune {
    tracker: PitchTracker,
    shifter: PsolaShifter,
    /// The mono sum the tracker listens to, sized in `prepare`.
    mono: Vec<f32>,
    sample_rate: f32,
    /// What `prepare` was told. A change of range or mode is a graph rebuild
    /// (§3.8), so these are read from here rather than from the live config —
    /// a ring cannot be resized on the audio thread (INVARIANT 1).
    prepared: TuneConfig,

    // ---- the pitch track (§3.3)
    /// The note being sung, in MIDI cents: the detected pitch, low-passed.
    slow: f32,
    /// What is left: the vibrato and the scoop.
    fast: f32,
    /// The detected pitch this hop.
    sung: f32,
    /// Where the correction is pulling to, in cents.
    target: f32,
    /// How much of the way there it has got, in cents.
    correction: f32,
    /// How long the note has been still, 0..=1.
    settled: f32,
    voiced: bool,
    /// The ratio in force, and what it is heading for.
    ratio: f32,
    /// Seconds since the last onset, for the vibrato's own clock.
    since_onset: f32,
    vibrato_phase: f32,

    /// What the window draws, one entry per hop of this block.
    trace: Vec<TuneFrame>,

    // ---- character (§4.8), per channel and carried between blocks
    /// The sample-and-hold's held value and how far through its hold it is.
    crush_held: [f32; MAX_CHANNELS],
    crush_phase: f32,
    /// The drive's measured make-up, smoothed across blocks.
    drive_gain: f32,
    /// The air shelf's one pole, per channel.
    air_z: [f32; MAX_CHANNELS],
}

impl Tune {
    pub fn new() -> Self {
        Self {
            tracker: PitchTracker::new(100.0, 1_000.0, 64),
            shifter: PsolaShifter::new(MAX_CHANNELS),
            mono: Vec::new(),
            sample_rate: 48_000.0,
            prepared: TuneConfig::new(),
            slow: 6_900.0,
            fast: 0.0,
            sung: 6_900.0,
            target: 6_900.0,
            correction: 0.0,
            settled: 0.0,
            voiced: false,
            ratio: 1.0,
            since_onset: 0.0,
            vibrato_phase: 0.0,
            trace: Vec::new(),
            crush_held: [0.0; MAX_CHANNELS],
            crush_phase: 0.0,
            drive_gain: 1.0,
            air_z: [0.0; MAX_CHANNELS],
        }
    }

    /// Off-RT: builds the tracker and the shifter for **this** range and mode.
    ///
    /// The config is a parameter rather than a later reading because the range
    /// sizes every ring and the mode sets the hop, and neither can be changed
    /// on the audio thread. That is why §3.8 makes a change of either a graph
    /// rebuild: the rebuild is what calls this again.
    pub fn prepare(&mut self, sample_rate: f32, config: &TuneConfig) {
        self.sample_rate = sample_rate.max(1.0);
        self.prepared = *config;
        let range = config.range;
        let hop = config.mode.hop();
        self.tracker = PitchTracker::new(range.min_hz(), range.max_hz(), hop);
        self.tracker.prepare(self.sample_rate);
        self.shifter = PsolaShifter::new(MAX_CHANNELS);
        self.shifter.prepare(
            self.sample_rate,
            range.max_period(self.sample_rate),
            MAX_BLOCK,
            config.latency_samples(self.sample_rate),
        );
        self.mono = vec![0.0; MAX_BLOCK as usize];
        self.trace = Vec::with_capacity(MAX_BLOCK as usize / hop as usize + 2);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.tracker.reset();
        self.shifter.reset();
        self.slow = 6_900.0;
        self.fast = 0.0;
        self.sung = 6_900.0;
        self.target = 6_900.0;
        self.correction = 0.0;
        self.settled = 0.0;
        self.voiced = false;
        self.ratio = 1.0;
        self.since_onset = 0.0;
        self.vibrato_phase = 0.0;
        self.crush_held = [0.0; MAX_CHANNELS];
        self.crush_phase = 0.0;
        self.drive_gain = 1.0;
        self.air_z = [0.0; MAX_CHANNELS];
        self.trace.clear();
    }

    /// What this insert delays its track by, in samples. The document's
    /// answer, asked here so the node and the graph read one function.
    pub fn latency_samples(&self) -> u32 {
        self.prepared.latency_samples(self.sample_rate)
    }

    /// The hops that completed in the last block, oldest first — what the tap
    /// copies out and the window draws (§7.3).
    pub fn trace(&self) -> &[TuneFrame] {
        &self.trace
    }

    /// One block, in place.
    ///
    /// `notes` is the frame's held keys from the channel this insert listens
    /// to; `bpm` is the tempo, read every block so a synced vibrato follows a
    /// tempo *change*.
    pub fn process(
        &mut self,
        channels: &mut [&mut [f32]],
        notes: NoteInput,
        config: &TuneConfig,
        bpm: f32,
    ) {
        self.trace.clear();
        if channels.is_empty() || self.mono.is_empty() {
            return;
        }
        let used = channels.len().min(MAX_CHANNELS);
        let frames = channels
            .iter()
            .take(used)
            .map(|c| c.len())
            .min()
            .unwrap_or(0)
            .min(self.mono.len());
        if frames == 0 {
            return;
        }

        // The tracker listens to the mono sum: a corrector has one opinion
        // about the note, and two channels disagreeing about it would move the
        // image (§3.7).
        let scale = 1.0 / used as f32;
        for frame in 0..frames {
            let mut sum = 0.0;
            for channel in 0..used {
                sum += channels[channel][frame];
            }
            self.mono[frame] = sum * scale;
        }

        self.tracker.set_tracking(config.tracking);
        self.tracker.set_gate_db(config.gate_db);
        self.shifter
            .set_engine(engine_of(config.engine), config.texture, config.grain_ms);

        // The block is walked in hop-aligned pieces, so the control rate and
        // the tracker's agree: a ratio that changed half a hop late would be a
        // correction applied to the wrong note.
        let mut at = 0usize;
        while at < frames {
            let step = (self.tracker.samples_to_hop() as usize)
                .max(1)
                .min(frames - at);
            let mut landed = None;
            self.tracker
                .push(&self.mono[at..at + step], &mut |frame| landed = Some(frame));
            if let Some(frame) = landed {
                self.hop(frame, notes, config, bpm);
            }
            self.shifter.set_period(self.period());
            self.shifter.set_ratio(self.ratio);
            self.shifter.set_formant(self.formant(config));
            // Each channel's own slice of this piece, shifted through the one
            // mark schedule the shifter holds.
            let mut heads: [Option<&mut [f32]>; MAX_CHANNELS] = [None, None];
            for (index, channel) in channels.iter_mut().take(used).enumerate() {
                heads[index] = Some(&mut channel[at..at + step]);
            }
            match used {
                1 => {
                    let mut one: [&mut [f32]; 1] = [heads[0].take().expect("one channel")];
                    self.shifter.process(&mut one);
                }
                _ => {
                    let mut two: [&mut [f32]; 2] = [
                        heads[0].take().expect("left"),
                        heads[1].take().expect("right"),
                    ];
                    self.shifter.process(&mut two);
                }
            }
            at += step;
        }

        self.character(channels, used, frames, config);

        let gain = 10.0f32.powf(config.output_db / 20.0);
        if (gain - 1.0).abs() > 1e-6 {
            for channel in channels.iter_mut().take(used) {
                for sample in channel[..frames].iter_mut() {
                    *sample *= gain;
                }
            }
        }
    }

    /// **Character** (§4.8): what the corrected voice is made of.
    ///
    /// Four stages in the order they belong in — saturate, then decimate, then
    /// tilt, then spread. Drive before crush because a saturator after a
    /// sample-and-hold smooths the very steps the crush was for; the tilt
    /// after both because it is meant to shape what they made; width last
    /// because it is the only one that is about two channels rather than one.
    ///
    /// Every one of them is a no-op at its default, and the whole function
    /// returns early when they all are — a fresh corrector pays nothing for
    /// knobs nobody turned.
    fn character(
        &mut self,
        channels: &mut [&mut [f32]],
        used: usize,
        frames: usize,
        config: &TuneConfig,
    ) {
        let drive = config.drive.clamp(0.0, 1.0);
        let crush = config.crush.clamp(0.0, 1.0);
        let air = config.air.clamp(-1.0, 1.0);
        let width = config.width.clamp(0.0, 2.0);
        let any = drive > 1e-4 || crush > 1e-4 || air.abs() > 1e-4 || (width - 1.0).abs() > 1e-4;
        if !any {
            return;
        }

        // ---- drive: a soft symmetrical curve with its gain measured back.
        //
        // Two obvious normalisations are both wrong, and the bank found each
        // of them in turn. `tanh(k·x)/tanh(k)` fixes the curve at full scale,
        // a level real signals never reach, so a vocal stem at −20 dBFS comes
        // out eight to seventeen decibels **louder** — the knob is an upward
        // compressor wearing a saturator's name. `tanh(k·x)/k` fixes the
        // slope at the origin instead, which puts the ceiling at 1/k: at the
        // top of the knob that is −21 dBFS, and the same stem comes out
        // eighteen decibels **quieter**. A curve cannot match both ends,
        // because compressing the loud part and leaving the quiet part alone
        // is what saturation *is*.
        //
        // So the gain is measured rather than assumed: the block's RMS before
        // and after, and the ratio applied back. That is what the auto-gain on
        // a real saturator does, it costs one extra pass over a block, and it
        // is the only version of this knob that leaves the preset bank at one
        // volume — which is the property that matters, because a bank you
        // cannot audition without riding the fader is a bank nobody browses.
        if drive > 1e-4 {
            let k = 1.0 + drive * 11.0;
            let shape = k.tanh();
            let mut before = 0.0f64;
            let mut after = 0.0f64;
            for channel in channels.iter_mut().take(used) {
                for sample in channel[..frames].iter_mut() {
                    before += f64::from(*sample) * f64::from(*sample);
                    *sample = (*sample * k).tanh() / shape;
                    after += f64::from(*sample) * f64::from(*sample);
                }
            }
            // Silence has no gain to measure: hold the last one rather than
            // dividing by nothing, so a gap between words does not slam the
            // level when the singer comes back.
            if after > 1e-12 && before > 1e-12 {
                let want = (before / after).sqrt() as f32;
                // One pole a block, so the gain walks rather than steps. A
                // block is 128 samples here; a quarter of the distance per
                // block settles in about twenty milliseconds, under a note.
                self.drive_gain += (want.clamp(0.05, 20.0) - self.drive_gain) * 0.25;
            }
            for channel in channels.iter_mut().take(used) {
                for sample in channel[..frames].iter_mut() {
                    *sample *= self.drive_gain;
                }
            }
        }

        // ---- crush: sample and hold, the same phase on every channel.
        //
        // One phase for all channels rather than one each, so a stereo signal
        // is decimated on the same grid and the image does not shimmer. The
        // hold runs from one sample (no-op) to sixteen, which at 48 kHz is a
        // 3 kHz sampler — far enough down to be an effect and not so far that
        // the note disappears.
        if crush > 1e-4 {
            let hold = 1.0 + crush * 15.0;
            let step = 1.0 / hold;
            let mut phase = self.crush_phase;
            let mut held = self.crush_held;
            for frame in 0..frames {
                if phase >= 1.0 {
                    phase -= 1.0;
                    for (channel, slot) in channels.iter().take(used).zip(held.iter_mut()) {
                        *slot = channel[frame];
                    }
                }
                for (channel, slot) in channels.iter_mut().take(used).zip(held.iter()) {
                    channel[frame] = *slot;
                }
                phase += step;
            }
            self.crush_phase = phase;
            self.crush_held = held;
        }

        // ---- air: one high shelf, tilted either way.
        //
        // A one-pole split into its low and high halves, recombined with the
        // high half weighted. `air` at −1 takes 9 dB off the top and at +1
        // puts 9 dB on; at 0 the two halves sum back to the input exactly, so
        // the stage is bit-transparent when the knob is centred rather than
        // "nearly" so.
        if air.abs() > 1e-4 {
            // ~3 kHz at any sample rate: the coefficient of a one-pole whose
            // corner is there, so the shelf sits above the voice's body and on
            // its consonants.
            let a = (-std::f32::consts::TAU * 3_000.0 / self.sample_rate).exp();
            let lift = 1.0 + air * 1.8;
            for (index, channel) in channels.iter_mut().take(used).enumerate() {
                let mut z = self.air_z[index];
                for sample in channel[..frames].iter_mut() {
                    z = *sample * (1.0 - a) + z * a;
                    *sample = z + (*sample - z) * lift;
                }
                self.air_z[index] = z;
            }
        }

        // ---- width: mid/side, and only when there is a side to move.
        //
        // Mono is left alone by construction: with one channel there is no
        // side, and a "width" that made one up would be a chorus wearing the
        // wrong name.
        if used >= 2 && (width - 1.0).abs() > 1e-4 {
            let (left, rest) = channels.split_at_mut(1);
            let (left, right) = (&mut left[0], &mut rest[0]);
            for frame in 0..frames {
                let (l, r) = (left[frame], right[frame]);
                let mid = (l + r) * 0.5;
                let side = (l - r) * 0.5 * width;
                left[frame] = mid + side;
                right[frame] = mid - side;
            }
        }
    }

    /// The period the shifter's analysis marks walk by, in samples.
    fn period(&self) -> f32 {
        (self.sample_rate / cents_to_hz(self.sung)).clamp(2.0, self.sample_rate / 8.0)
    }

    /// The stride a grain is read at (§3.5).
    ///
    /// On the granular engine the pitch and the formants are the same
    /// operation, so `formant_follow` has nothing to move — the engine follows
    /// by construction, and the knob left here is the throat-length offset.
    /// That is written down rather than worked around: it is *why* the cheap
    /// engine sounds cheap.
    fn formant(&self, config: &TuneConfig) -> f32 {
        let offset = (config.formant / 12.0).exp2();
        match config.engine {
            TuneEngine::Grain => offset,
            _ => offset * self.ratio.powf(config.formant_follow.clamp(0.0, 1.0)),
        }
    }

    /// One hop of the pitch track, in the order §3.3 lists its steps.
    fn hop(
        &mut self,
        frame: Option<fontelle_dsp::PitchFrame>,
        notes: NoteInput,
        config: &TuneConfig,
        bpm: f32,
    ) {
        let hop_seconds = self.prepared.mode.hop() as f32 / self.sample_rate;
        self.since_onset += hop_seconds;

        let Some(frame) = frame else {
            // 1. Nothing to follow: `slow` and `fast` are frozen, and the
            //    ratio relaxes to one so the shifter becomes the delay line it
            //    already is at ratio one (§3.4).
            self.voiced = false;
            let relax = coefficient(UNVOICED_RELAX_SECONDS, hop_seconds);
            self.ratio += (1.0 - self.ratio) * relax;
            self.correction += (0.0 - self.correction) * relax;
            self.push_trace(false, false);
            return;
        };

        // 1. Split the note being sung from the vibrato and the scoop.
        let cents = frame.cents;
        let was_voiced = self.voiced;
        self.voiced = true;
        self.sung = cents;
        let previous_slow = self.slow;
        // 2. An onset is an unvoiced→voiced edge or a jump of more than a
        //    tone's worth of drift. It restarts the glide from the note *as
        //    sung*, which is what "retune speed" means on every record that
        //    has one — and the yodel between notes on a hard-tuned vocal is
        //    exactly this restart at zero.
        let onset = !was_voiced || (cents - self.slow).abs() > ONSET_CENTS;
        if onset {
            self.slow = cents;
            self.correction = 0.0;
            self.settled = 0.0;
            self.since_onset = 0.0;
        } else {
            self.slow += (cents - self.slow) * coefficient(SLOW_TAU, hop_seconds);
        }
        self.fast = cents - self.slow;

        // 3. The target, from whichever of the three readings is in force.
        let from_midi = self.choose_target(notes, config);

        // 4. Strength: how much of the distance is asked for at all.
        let distance = self.target - self.slow;
        let flex_strength = if matches!(config.control, TuneControl::MidiMelody)
            && !notes.is_empty()
        {
            // A forced note is forced: flex is about letting a *sung* line
            // keep its shape, and there is no sung line to keep here.
            1.0
        } else {
            let edge = fontelle_types::TUNE_FLEX_EDGE_CENTS * (1.0 - config.flex.clamp(0.0, 1.0));
            1.0 - smoothstep(edge, fontelle_types::TUNE_FLEX_EDGE_CENTS, distance.abs())
        };
        let humanize_strength = 1.0 - config.humanize.clamp(0.0, 1.0) * self.settled;
        let strength = config.amount.clamp(0.0, 1.0) * flex_strength * humanize_strength;

        // 5. The glide.
        let goal = distance * strength;
        if config.retune_is_instant() {
            self.correction = goal;
        } else {
            self.correction +=
                (goal - self.correction) * coefficient(config.retune_ms / 1000.0, hop_seconds);
        }

        // 6. Humanize's clock, for the *next* hop: a note that has held still
        //    for three hundred milliseconds is one a singer meant.
        let drift = (self.slow - previous_slow).abs() / hop_seconds.max(1e-6);
        if !onset && drift < SETTLE_CENTS_PER_SECOND {
            self.settled = (self.settled + hop_seconds / SETTLE_SECONDS).min(1.0);
        } else {
            self.settled = 0.0;
        }

        // 7. What comes out.
        let vibrato = self.vibrato(config, bpm, hop_seconds);
        let out = self.slow
            + self.correction
            + self.fast * config.natural_vibrato.clamp(0.0, 1.0)
            + vibrato
            + 100.0 * f32::from(config.transpose)
            + config.detune_cents;

        // 8. The ratio, against the *instantaneous* detected pitch rather than
        //    against `slow` — so the shifter is exact even while `slow` lags.
        self.ratio = ((out - cents) / 1200.0).exp2().clamp(0.25, 4.0);
        let locked = distance.abs() < TUNE_LOCK_CENTS;
        self.push_trace(locked, from_midi);
    }

    /// Writes what this hop did, for the window's trace.
    fn push_trace(&mut self, locked: bool, from_midi: bool) {
        let mut flags = 0;
        if self.voiced {
            flags |= TUNE_VOICED;
        }
        if locked {
            flags |= TUNE_LOCKED;
        }
        if from_midi {
            flags |= TUNE_FROM_MIDI;
        }
        let out = if self.voiced {
            self.sung + 1200.0 * self.ratio.max(1e-6).log2()
        } else {
            self.sung
        };
        // The trace is `Vec`-backed and cleared, not grown, per block: its
        // capacity is the most hops a block can hold and `prepare` reserved it
        // (INVARIANT 1).
        if self.trace.len() < self.trace.capacity() {
            self.trace.push(TuneFrame {
                sung_cents: self.sung,
                out_cents: out,
                target_cents: self.target,
                flags,
            });
        }
    }

    /// Step 3: where the note is being pulled to. Returns whether a held key
    /// decided it.
    fn choose_target(&mut self, notes: NoteInput, config: &TuneConfig) -> bool {
        match config.control {
            TuneControl::MidiMelody => {
                if let Some(key) = notes.last {
                    let bend = if config.midi_bend {
                        notes.bend_cents
                    } else {
                        0.0
                    };
                    self.target = 100.0 * f32::from(key) + bend;
                    return true;
                }
                self.target = nearest_in_mask(self.slow, config.active_mask(), self.target);
                false
            }
            TuneControl::MidiScale => {
                let mask = if notes.mask != 0 {
                    notes.mask
                } else {
                    config.active_mask()
                };
                self.target = nearest_in_mask(self.slow, mask, self.target);
                notes.mask != 0
            }
            TuneControl::Scale => {
                self.target = nearest_in_mask(self.slow, config.active_mask(), self.target);
                false
            }
        }
    }

    /// The vibrato this effect *adds*, in cents (§3.5).
    fn vibrato(&mut self, config: &TuneConfig, bpm: f32, hop_seconds: f32) -> f32 {
        if config.vibrato_depth <= 0.0 {
            return 0.0;
        }
        let hz = config.vibrato_hz(bpm);
        self.vibrato_phase = (self.vibrato_phase + hz * hop_seconds).fract();
        let onset = config.vibrato_onset_ms.max(0.0) / 1000.0;
        let into = self.since_onset - onset;
        if into <= 0.0 {
            return 0.0;
        }
        let fade = (into / VIBRATO_FADE_SECONDS).clamp(0.0, 1.0);
        config.vibrato_shape.value(self.vibrato_phase) * config.vibrato_depth * fade
    }
}

impl Default for Tune {
    fn default() -> Self {
        Self::new()
    }
}

/// The most frames one block may hold. The device fixes the block size and
/// nothing in this engine exceeds it; sizing for it here is what lets the
/// tracker and the shifter allocate once.
const MAX_BLOCK: u32 = 4_096;

fn engine_of(engine: TuneEngine) -> GrainEngine {
    match engine {
        TuneEngine::Smooth => GrainEngine::Smooth,
        TuneEngine::Hard => GrainEngine::Hard,
        TuneEngine::Grain => GrainEngine::Grain,
    }
}

/// A one-pole coefficient for a time constant, at this control rate.
fn coefficient(tau_seconds: f32, step_seconds: f32) -> f32 {
    if tau_seconds <= 1e-6 {
        return 1.0;
    }
    1.0 - (-step_seconds / tau_seconds).exp()
}

/// Hermite's S-curve between two edges.
///
/// A degenerate span — which is where `flex` at zero puts it, because the
/// lower edge slides *up* to the fixed upper one — is **zero everywhere**, not
/// a step. That is the whole meaning of the knob at rest: at flex zero every
/// pitch is corrected in full, however far from its note it is, and a scale
/// with gaps in it puts notes a hundred and fifty cents from theirs.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    if b <= a || x <= a {
        return 0.0;
    }
    if x >= b {
        return 1.0;
    }
    let t = (x - a) / (b - a);
    t * t * (3.0 - 2.0 * t)
}

/// The nearest pitch in `mask` to `cents`, in cents, keeping `current` while
/// it is within the hysteresis.
///
/// The hysteresis is the reason a note sung exactly between two scale degrees
/// does not flip between them dozens of times a second — the same defect the
/// gate's hysteresis exists for, one level up.
fn nearest_in_mask(cents: f32, mask: u16, current: f32) -> f32 {
    let mask = mask & 0x0FFF;
    if mask == 0 {
        return cents;
    }
    let semitone = cents / 100.0;
    let mut best = current;
    let mut best_distance = f32::MAX;
    for class in 0..12u16 {
        if mask & (1 << class) == 0 {
            continue;
        }
        // The octave of this class nearest the note being sung.
        let octaves = ((semitone - f32::from(class)) / 12.0).round();
        let candidate = (f32::from(class) + 12.0 * octaves) * 100.0;
        let distance = (candidate - cents).abs();
        if distance < best_distance {
            best_distance = distance;
            best = candidate;
        }
    }
    // Was the note already going somewhere in the scale? Keep it unless the
    // new answer is better by more than the hysteresis.
    let current_class = ((current / 100.0).round() as i32).rem_euclid(12) as u16;
    if mask & (1 << current_class) != 0 {
        let current_distance = (current - cents).abs();
        if current_distance <= best_distance + TARGET_HYSTERESIS_CENTS && current_distance < 1_200.0
        {
            return current;
        }
    }
    best
}
