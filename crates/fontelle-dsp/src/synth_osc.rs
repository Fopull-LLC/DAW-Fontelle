//! Flopsynth's oscillator (`docs/flopsynth-plan.md` §3.1).
//!
//! One description — [`SynthOsc`] — covers all five of a Flopsynth patch's
//! layers: A, B, C, the Sub and the Noise. The window shows fewer controls for
//! the last two; the voice runs the same code for all of them, which is what
//! keeps "a patch with a sampled layer appended is still a valid patch" true.
//!
//! # It is `Copy`, and everything in it is a number
//!
//! INVARIANT 6: the voice is a fixed topology and every new thing in it is a
//! fixed-size array. So a unison stack is `[f32; MAX_UNISON]` of phases rather
//! than a `Vec` of voices, the noise generator is an xorshift rather than an
//! allocation, and [`SynthState`] — the per-voice half — is eighty bytes of
//! plain data that a `Voice` holds by value.
//!
//! # What it does not own
//!
//! The **wavetable**. `SynthState::next_sample` takes a `&Wavetable` resolved
//! by `Sampler::prepare` and never looks one up itself, because looking one up
//! locks a mutex and may allocate — both forbidden on the RT thread
//! (INVARIANT 1).

use crate::{WAVETABLE_LEVELS, Wavetable, WavetableId, wavetable_level_for};

/// The most voices one oscillator's unison stack may have.
///
/// Eight, and a fixed array rather than a limit that a `Vec` would make
/// negotiable: three oscillators of eight voices each is twenty-four table
/// reads per sample per note, which is already most of §10's budget.
pub const MAX_UNISON: usize = 8;

/// Where an oscillator's samples come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SynthSource {
    Table(WavetableId),
    /// Broadband noise through a tilt filter. Ignores `position`, `warp` and
    /// unison — there is no cycle to position within, no phase to warp and
    /// nothing for a second copy of the same noise to beat against.
    Noise,
    /// One of the **patch's own** tables, built from a sound somebody dropped
    /// in — `Patch::wavetables`, and [`Wavetable::from_samples`].
    ///
    /// > *"i want to be able to drag audio files into it to use those
    /// > waveforms in the synthesis."*
    ///
    /// An index rather than a name, because the patch owns the list and a
    /// name is a thing a person may change. A patch that names a table it
    /// does not have is silent for that layer, the same answer a missing
    /// bank table gets: a wrong sound is harder to diagnose than no sound.
    User(u8),
}

impl Default for SynthSource {
    fn default() -> Self {
        Self::Table(WavetableId::Saw)
    }
}

/// How the table read is bent. Each is a **continuum** under
/// [`SynthOsc::warp_amount`] and each is a wire at zero (catalogue rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum WarpMode {
    #[default]
    Off,
    /// Skews the read phase — the first half of the cycle stretched and the
    /// second squeezed, or the other way. The gentlest of them: it changes
    /// the harmonic balance without adding a discontinuity.
    Bend,
    /// Hard sync. The table is read at 1..8× the note's pitch and restarted
    /// every time the note's own cycle ends, which is what puts the lines at
    /// multiples of the *note* while the timbre follows the synced frequency.
    Sync,
    /// Folds the phase back on itself, so the cycle plays forwards and then
    /// backwards. A symmetric wave has only even harmonics, which is the
    /// PWM-like hollowing this is for.
    Mirror,
    /// Holds the read to 2..64 steps per cycle — the digital grit, and the
    /// one warp that is a *reduction*.
    Quantise,
    /// Through-zero phase modulation by the modulator layer's sample.
    Fm,
    /// Multiplies by the modulator layer's sample.
    Rm,
}

impl WarpMode {
    pub const ALL: [Self; 7] = [
        Self::Off,
        Self::Bend,
        Self::Sync,
        Self::Mirror,
        Self::Quantise,
        Self::Fm,
        Self::Rm,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Bend => "Bend",
            Self::Sync => "Sync",
            Self::Mirror => "Mirror",
            Self::Quantise => "Quantise",
            Self::Fm => "FM",
            Self::Rm => "RM",
        }
    }

    /// Whether this warp reads another layer, and therefore whether the panel
    /// offers the modulator chooser at all.
    pub fn needs_a_modulator(self) -> bool {
        matches!(self, Self::Fm | Self::Rm)
    }
}

/// Serum's unison, not [`crate::UnisonConfig`]'s: **per oscillator**, so one
/// layer can be a seven-voice stack over a single-voice sub.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Unison {
    /// 1..=[`MAX_UNISON`]. One is exactly the centre voice: unison at 1 costs
    /// nothing and changes nothing.
    pub voices: u8,
    /// The **outermost** voice's offset. Inner voices sit proportionally
    /// inside it, with a slight bias so eight voices do not stack up in the
    /// middle.
    pub detune_cents: f32,
    /// The level of every voice *but* the centre one, 0..1. At zero the stack
    /// is the centre voice alone, whatever the count says.
    pub blend: f32,
    /// How far the side voices are panned out from the centre, in alternating
    /// pairs. The centre voice is always centred.
    pub width: f32,
}

impl Default for Unison {
    fn default() -> Self {
        Self {
            voices: 1,
            detune_cents: 15.0,
            blend: 1.0,
            width: 0.5,
        }
    }
}

/// Which filter a layer goes through — per layer, which is what lets a sub
/// bypass a closed low-pass while the saw above it is being swept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FilterRoute {
    /// Filter 1 only, then straight to the amp.
    #[default]
    F1,
    /// Filter 2 only.
    F2,
    /// Filter 1 then filter 2 — the classic pair in series.
    Serial,
    /// Around both. What a sub and a noise transient usually want.
    Bypass,
}

impl FilterRoute {
    pub const ALL: [Self; 4] = [Self::F1, Self::F2, Self::Serial, Self::Bypass];

    pub fn label(self) -> &'static str {
        match self {
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::Serial => "F1→F2",
            Self::Bypass => "Bypass",
        }
    }
}

/// One oscillator of a Flopsynth patch.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SynthOsc {
    pub source: SynthSource,
    /// Across the table's frames, 0..1.
    pub position: f32,
    pub warp: WarpMode,
    pub warp_amount: f32,
    /// Which **later** layer feeds FM/RM. Later, so the sample loop can
    /// evaluate the layers last-to-first and have the modulator's sample for
    /// this frame already; the panel refuses anything else.
    pub modulator: Option<u8>,
    pub unison: Unison,
    /// The start phase when [`random_phase`](Self::random_phase) is off, 0..1.
    pub phase: f32,
    pub random_phase: bool,
    /// −36..=36.
    pub semitones: i8,
    /// Whether the note's pitch reaches this oscillator at all. Off is a
    /// drone or a fixed noise band.
    pub key_track: bool,
    pub filter_route: FilterRoute,
    /// [`SynthSource::Noise`] only: 0 white, 0.5 pink-ish, 1 brown.
    pub noise_colour: f32,
}

impl Default for SynthOsc {
    fn default() -> Self {
        Self {
            source: SynthSource::default(),
            position: 0.0,
            warp: WarpMode::Off,
            warp_amount: 0.0,
            modulator: None,
            unison: Unison::default(),
            phase: 0.0,
            random_phase: false,
            semitones: 0,
            key_track: true,
            filter_route: FilterRoute::F1,
            noise_colour: 0.0,
        }
    }
}

impl SynthOsc {
    /// What this oscillator plays for a note at `note_hz`.
    ///
    /// With key tracking off it plays [`OSC_FIXED_HZ`] transposed by its own
    /// semitones and ignores the note, which is what makes a drone a drone.
    pub fn frequency(&self, note_hz: f32) -> f32 {
        let base = if self.key_track {
            note_hz
        } else {
            OSC_FIXED_HZ
        };
        base * 2f32.powf(f32::from(self.semitones) / 12.0)
    }
}

/// The pitch a `key_track: false` oscillator plays at, before its own
/// semitones: middle C, the same reference `Voice`'s oscillator layers use.
pub const OSC_FIXED_HZ: f32 = 261.625_56;

/// The per-voice half: eight phases, a random source and the noise filter's
/// memory. `Copy`, fixed-size, allocation-free.
#[derive(Debug, Clone, Copy)]
pub struct SynthState {
    phases: [f32; MAX_UNISON],
    /// The master phase, for hard sync: the slave is restarted whenever this
    /// wraps, which is what keeps the pitch the note's.
    master_phase: f32,
    rng: u32,
    /// The tilt filter's one pole, for `SynthSource::Noise`.
    noise_pole: f32,
    /// The last sample this oscillator produced, for whichever layer names it
    /// as an FM or RM modulator.
    last: f32,
    /// The stack's per-voice constants, kept until the settings that decide
    /// them move.
    ///
    /// **A `powf` and a pair of pan gains per voice per sample** was what a
    /// unison stack cost before this, and none of it depends on anything that
    /// changes *within* a block: the detune ratio, the phase step, the blend
    /// level and the placement are all functions of the oscillator and the
    /// note. "Supersaw" is three oscillators of seven voices, so that was
    /// twenty-one `powf`s a sample for constants
    /// (`docs/flopsynth-plan.md` §10).
    ///
    /// Keyed on the values themselves rather than on a dirty flag, so the
    /// caller has nothing to remember — `SynthFilter`'s cache is the same
    /// shape, for the same reason. `tests/synth_osc.rs` holds both halves:
    /// that a moved knob is followed on the next sample, and that the cache is
    /// never stale.
    unison: Option<UnisonCache>,
}

/// What one set of unison settings works out to, per voice.
#[derive(Debug, Clone, Copy, PartialEq)]
struct UnisonCache {
    /// Everything the constants below were computed from.
    key: UnisonKey,
    /// Each voice's phase step, and its gain into each channel.
    steps: [f32; MAX_UNISON],
    gains: [(f32, f32); MAX_UNISON],
    /// What the stack is divided by so that turning unison up makes a stack
    /// rather than making it louder.
    loudness: f32,
}

/// The settings a [`UnisonCache`] is only good for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct UnisonKey {
    voices: usize,
    detune_cents: f32,
    blend: f32,
    width: f32,
    base_hz: f32,
    sync_ratio: f32,
    sample_rate: f32,
}

impl Default for SynthState {
    fn default() -> Self {
        Self {
            phases: [0.0; MAX_UNISON],
            master_phase: 0.0,
            // Any non-zero seed; xorshift is stuck at zero forever.
            rng: 0x2545_f491,
            noise_pole: 0.0,
            last: 0.0,
            unison: None,
        }
    }
}

impl SynthState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a note.
    ///
    /// `seed` is the voice's own age counter, so a random start phase is
    /// different on every note and **repeatable** for a given note — which is
    /// what lets a test measure it at all.
    pub fn reset(&mut self, osc: &SynthOsc, seed: u32) {
        self.rng = seed.wrapping_mul(2_654_435_761).wrapping_add(0x9e37_79b9) | 1;
        self.master_phase = osc.phase.rem_euclid(1.0);
        self.noise_pole = 0.0;
        self.last = 0.0;
        for (index, phase) in self.phases.iter_mut().enumerate() {
            // The **centre voice keeps its start phase** even in a
            // random-phase stack, which is §3.1's exception and not an
            // oversight: `random_phase` exists so that eight voices do not all
            // step together on the first sample and make a click eight times
            // the size of any one of them. Voice 0 has nobody to pile up with,
            // and a bass note whose one voice starts somewhere different every
            // time is a bass note with a different transient every time.
            *phase = if osc.random_phase && index != 0 {
                let mut x = self.rng;
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                self.rng = x;
                (x >> 8) as f32 / 16_777_216.0
            } else {
                osc.phase.rem_euclid(1.0)
            };
        }
    }

    /// The sample this oscillator produced last — what a layer naming it as an
    /// FM or RM modulator reads.
    pub fn last_sample(&self) -> f32 {
        self.last
    }

    /// One stereo sample pair.
    ///
    /// `note_hz` is the note's own pitch, before this oscillator's semitones
    /// and key-tracking switch. `modulator` is the sample the layer named by
    /// [`SynthOsc::modulator`] produced this frame — zero when there is none,
    /// which is what makes FM at full depth with no modulator a silent knob
    /// rather than a broken one.
    pub fn next_sample(
        &mut self,
        osc: &SynthOsc,
        table: Option<&Wavetable>,
        note_hz: f32,
        sample_rate: f32,
        modulator: f32,
    ) -> (f32, f32) {
        if sample_rate <= 0.0 {
            return (0.0, 0.0);
        }
        let out = match osc.source {
            SynthSource::Noise => {
                let mono = self.noise(osc);
                (mono, mono)
            }
            // The bank's, or one the patch carries itself: both are a table
            // resolved before the note, and the voice cannot tell them apart
            // — which is the point of resolving them in one place.
            SynthSource::Table(_) | SynthSource::User(_) => match table {
                Some(table) => self.table_voices(osc, table, note_hz, sample_rate, modulator),
                // A layer whose table has not been resolved renders silence
                // rather than a substitute: a wrong sound is harder to
                // diagnose than no sound.
                None => (0.0, 0.0),
            },
        };
        // The modulator reads the *mono* sum, because FM by one side of a
        // panned stack is not a thing anybody means.
        self.last = (out.0 + out.1) * 0.5;
        out
    }

    fn noise(&mut self, osc: &SynthOsc) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        let white = (x >> 8) as f32 / 8_388_608.0 - 1.0;
        let colour = osc.noise_colour.clamp(0.0, 1.0);
        if colour <= 0.0 {
            return white;
        }
        // A one-pole low-pass whose corner falls as the colour rises: white at
        // 0, a −3 dB/oct pink-ish tilt in the middle, a −6 dB/oct brown at 1.
        // The make-up keeps the level roughly steady across the knob, so
        // turning the colour up is not also turning the noise down.
        let coefficient = colour.powf(0.6) * 0.995;
        self.noise_pole += (white - self.noise_pole) * (1.0 - coefficient);
        let make_up = (1.0 + coefficient) / (1.0 - coefficient).max(1e-4).sqrt();
        (self.noise_pole * make_up * 0.35).clamp(-4.0, 4.0)
    }

    fn table_voices(
        &mut self,
        osc: &SynthOsc,
        table: &Wavetable,
        note_hz: f32,
        sample_rate: f32,
        modulator: f32,
    ) -> (f32, f32) {
        let base_hz = osc.frequency(note_hz).clamp(0.0, sample_rate * 0.5);
        let voices = usize::from(osc.unison.voices.clamp(1, MAX_UNISON as u8));
        let blend = osc.unison.blend.clamp(0.0, 1.0);
        let width = osc.unison.width.clamp(0.0, 1.0);
        let amount = osc.warp_amount.clamp(0.0, 1.0);

        // Hard sync's master runs at the note; the slave runs faster and is
        // restarted whenever the master wraps.
        let sync_ratio = if osc.warp == WarpMode::Sync {
            1.0 + amount * 7.0
        } else {
            1.0
        };
        let master_step = base_hz / sample_rate;

        // The mip level is chosen for what is actually being read: a sync at
        // 6× reads six times the harmonics, and choosing the level for the
        // note would alias every one of them.
        let level =
            wavetable_level_for(base_hz * sync_ratio, sample_rate).min(WAVETABLE_LEVELS - 1);

        // Read the master where it is, advance it after: a voice's phase is
        // the one it had at the start of the sample, and the two have to move
        // in lockstep or a ratio of 1 would drift a sample apart from an
        // unsynced read.
        let syncing = osc.warp == WarpMode::Sync;
        let advanced = self.master_phase + master_step;
        let master_wraps = syncing && advanced >= 1.0;
        let master_overshoot = advanced - advanced.floor();
        self.master_phase = if advanced >= 1.0 {
            master_overshoot
        } else {
            advanced
        };

        // Every per-voice constant, worked out once and kept until one of the
        // settings behind it moves — see [`UnisonCache`].
        let stack = self.unison_constants(UnisonKey {
            voices,
            detune_cents: osc.unison.detune_cents,
            blend,
            width,
            base_hz,
            sync_ratio,
            sample_rate,
        });

        let (mut left, mut right) = (0.0f32, 0.0f32);
        for voice in 0..voices {
            let step = stack.steps[voice];

            let phase = &mut self.phases[voice];
            let read_at = warp_phase(osc, *phase, amount, modulator);
            let mut sample = table.read(osc.position.clamp(0.0, 1.0), read_at, level);
            if osc.warp == WarpMode::Rm {
                // Dry at amount 0, fully ring-modulated at 1 — a continuum,
                // like every other warp.
                sample *= 1.0 - amount + amount * modulator;
            }

            *phase += step;
            if *phase >= 1.0 {
                *phase -= phase.floor();
            }
            // **The slave restarts when the master's cycle ends**, which is
            // what hard sync is and what puts the lines at multiples of the
            // note rather than of the synced frequency.
            //
            // The restart is computed from the master's *overshoot* rather
            // than set to zero, so the edge lands inside the sample the wrap
            // happened in — and so that a ratio of exactly 1 is a wire sample
            // for sample, which is what makes the amount knob a continuum with
            // "off" at one end (catalogue rule 1).
            if syncing && master_wraps {
                *phase = master_overshoot * sync_ratio;
                *phase -= phase.floor();
            }

            let (gl, gr) = stack.gains[voice];
            left += sample * gl;
            right += sample * gr;
        }

        (left * stack.loudness, right * stack.loudness)
    }

    /// The stack's per-voice constants for `key`, built only when it changes.
    fn unison_constants(&mut self, key: UnisonKey) -> UnisonCache {
        if let Some(cache) = &self.unison
            && cache.key == key
        {
            return *cache;
        }
        let mut steps = [0.0f32; MAX_UNISON];
        let mut gains = [(0.0f32, 0.0f32); MAX_UNISON];
        for voice in 0..key.voices.min(MAX_UNISON) {
            let cents = detune_of(voice, key.voices, key.detune_cents);
            let hz = key.base_hz * 2f32.powf(cents / 1200.0);
            steps[voice] = hz * key.sync_ratio / key.sample_rate;

            // The centre voice is voice 0 and is always at full level and
            // centred; the sides carry the blend and the width.
            let (gain, pan) = if voice == 0 {
                (1.0, 0.0)
            } else {
                // Alternating pairs, so the stack stays balanced with an even
                // count as well as an odd one.
                let side = if voice.is_multiple_of(2) { 1.0 } else { -1.0 };
                let depth = voice.div_ceil(2) as f32 / (key.voices / 2).max(1) as f32;
                (key.blend, side * depth * key.width)
            };
            let (gl, gr) = pan_gains(pan);
            gains[voice] = (gain * gl, gain * gr);
        }
        // Normalised by the *effective* voice count, so turning unison up
        // makes a stack rather than making it louder — which is what lets a
        // preset's level survive somebody adding voices to it. Kept as a
        // reciprocal, so the sample loop multiplies rather than divides.
        let loudness = (1.0 + (key.voices - 1) as f32 * key.blend * key.blend).sqrt();
        let cache = UnisonCache {
            key,
            steps,
            gains,
            loudness: 1.0 / loudness,
        };
        self.unison = Some(cache);
        cache
    }
}

/// Constant power either side of centre, so a voice panned out is no quieter
/// than one in the middle.
///
/// The same −3 dB law `fontelle_types::PanLaw::Minus3Db` uses, written again
/// here rather than depended on: `fontelle-dsp` sees nothing but numbers
/// (INVARIANT 4), and this is four lines of trigonometry.
fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * std::f32::consts::PI;
    (angle.cos(), angle.sin())
}

/// Where unison voice `index` of `count` sits, in cents.
///
/// `detune` is the **outermost** voice's offset. The exponent bends the inner
/// voices slightly outward so that eight voices do not pile up around the
/// centre, which is what makes a wide stack sound wide rather than merely
/// thick.
fn detune_of(index: usize, count: usize, detune: f32) -> f32 {
    if index == 0 || count < 2 {
        return 0.0;
    }
    let side = if index.is_multiple_of(2) { 1.0 } else { -1.0 };
    let step = index.div_ceil(2) as f32;
    let outermost = (count / 2).max(1) as f32;
    side * detune * (step / outermost).powf(0.8)
}

/// The read phase, after the warp that is not a multiply.
fn warp_phase(osc: &SynthOsc, phase: f32, amount: f32, modulator: f32) -> f32 {
    match osc.warp {
        WarpMode::Off | WarpMode::Sync | WarpMode::Rm => phase,
        // `t^(2^(±k))`: the amount's two halves bend it either way, and the
        // middle is the identity.
        WarpMode::Bend => {
            if amount <= 0.0 {
                return phase;
            }
            let exponent = 2f32.powf(amount * 2.0);
            phase.max(0.0).powf(exponent)
        }
        // Forwards then backwards, blended in by the amount.
        WarpMode::Mirror => {
            let folded = if phase < 0.5 {
                phase * 2.0
            } else {
                2.0 - phase * 2.0
            };
            phase + (folded - phase) * amount
        }
        // 64 steps down to 2: the last of the travel is where the grit is.
        WarpMode::Quantise => {
            if amount <= 0.0 {
                return phase;
            }
            let steps = (64.0 * (1.0 - amount) + 2.0 * amount).max(2.0);
            (phase * steps).floor() / steps
        }
        // Through-zero phase modulation, index up to two whole cycles (4π).
        WarpMode::Fm => (phase + modulator * amount * 2.0).rem_euclid(1.0),
    }
}
