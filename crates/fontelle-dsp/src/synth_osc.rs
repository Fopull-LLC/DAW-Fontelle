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
//! The **wavetable**, and the **recording**. `SynthState::next_sample_from`
//! takes a [`SynthInput`] — a `&Wavetable` or a [`SampleData`] — resolved by
//! `Sampler::prepare` and never looks one up itself, because looking one up
//! locks a mutex and may allocate — both forbidden on the RT thread
//! (INVARIANT 1).
//!
//! # Three kinds of source, one oscillator
//!
//! A **table** is one cycle, read at the note's pitch. A **sample** is a
//! whole recording, read at the ratio of the note to the pitch it was
//! recorded at — the same read head, the same unison stack of detuned
//! copies, the same warp modulator; what differs is only that it has an end
//! and a start. A **string** is not read at all: it is a bank of decaying
//! partials placed where a stiff string puts them, which is the one thing
//! neither of the other two can be (see [`StringModel`]).

use crate::{
    Decimator, Interpolation, MAX_OVERSAMPLE, Oversampling, WAVETABLE_LEVELS, Wavetable,
    WavetableId, interpolate, wavetable_level_for,
};

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
    /// One of the patch's own **recordings**, played as it is across the
    /// keyboard rather than cut into cycles — `Patch::samples`, by index, for
    /// the reason [`User`](Self::User) is an index.
    ///
    /// > *"we could actually sample a real piano sound and then do effects
    /// > and modulating and layering with other oscilators and stuff."*
    ///
    /// The oscillator's `position` is where in the recording the note
    /// starts, and [`SynthOsc::sample`] says whether and where it loops.
    /// A layer naming a recording the patch does not carry is silent.
    Sample(u8),
    /// A stiff string: a bank of decaying partials at the frequencies a real
    /// string rings at, which are **not** multiples of the fundamental. The
    /// oscillator's `position` is the strike's brightness;
    /// [`SynthOsc::string`] is the string itself.
    String,
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

/// How a **sample** source's head moves through its recording.
///
/// The first two are a sampler's. The other three are why the recording is
/// in a synthesiser: a tape head cannot bounce, and a cloud of grains is
/// what turns a note into a texture — the start knob stops being where the
/// note begins and becomes *where in the sound the note is*, which a route
/// can then move.
///
/// > *"experimental, synthy, modulating ... genuinely stand up to other
/// > synths like omnisphere and serum."* — Ty, 2026-09-16
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SampleLoop {
    /// Play through to the end and stop. A recording of a struck thing wants
    /// this: its own decay is the note's.
    #[default]
    Off,
    /// Go round between the loop points for as long as the note is held.
    Forward,
    /// Go back and forth between the loop points: the head turns round at
    /// each rather than jumping, so a loop that never lands on its own cycle
    /// still has no seam.
    PingPong,
    /// Backwards, once: the recording's end is the note's start, and the
    /// start knob counts from the end so a knob at rest plays all of it.
    Reverse,
    /// A cloud of short grains, each landing at the start knob (scattered by
    /// [`SampleSettings::spray`]) and played at the note. The recording never
    /// ends because the grains keep coming, and a route on the start knob
    /// scans through it.
    Grains,
}

impl SampleLoop {
    pub const ALL: [Self; 5] = [
        Self::Off,
        Self::Forward,
        Self::PingPong,
        Self::Reverse,
        Self::Grains,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Once",
            Self::Forward => "Loop",
            Self::PingPong => "Bounce",
            Self::Reverse => "Reverse",
            Self::Grains => "Grains",
        }
    }
}

/// The shortest and longest grain, in milliseconds. Five is where a grain
/// stops being a piece of the sound and becomes a click at the grain rate;
/// half a second is where it stops being a grain.
pub const GRAIN_MIN_MS: f32 = 5.0;
pub const GRAIN_MAX_MS: f32 = 500.0;

/// How a **sample** source reads its recording — the part of the read that a
/// table has no equivalent of.
///
/// Every field but the first three arrived after the first files that carry
/// this block were written, so each defaults on its own: a file that says
/// only the loop still reads as it did.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SampleSettings {
    pub loop_mode: SampleLoop,
    /// The loop, as fractions of the recording, 0..1. A loop end at or before
    /// its start is no loop at all.
    pub loop_start: f32,
    pub loop_end: f32,
    /// [`SampleLoop::Grains`] only: how long each grain is, in milliseconds
    /// ([`GRAIN_MIN_MS`]..=[`GRAIN_MAX_MS`]). Short grains smear a tone
    /// into a band; long ones keep the recording's own texture.
    pub grain_ms: f32,
    /// [`SampleLoop::Grains`] only: how far either side of the start knob a
    /// grain may land, as a fraction of the recording, 0..1. Nothing means
    /// every grain is the same moment of the sound, frozen; one means they
    /// come from anywhere in it.
    pub spray: f32,
    /// Play **this** zone of the recording for every key, transposed from
    /// its own root, instead of the zone whose range holds the key. What a
    /// kit's snare across the keyboard wants, and what a set of one
    /// recording per hit is otherwise unable to give.
    pub zone: Option<u8>,
}

impl Default for SampleSettings {
    fn default() -> Self {
        Self {
            loop_mode: SampleLoop::Off,
            // The back half: where a recording of a held thing has settled
            // into its sustain, which is the part worth going round.
            loop_start: 0.5,
            loop_end: 1.0,
            // Long enough to carry a note's pitch cleanly, short enough that
            // a moving start knob is heard to move.
            grain_ms: 80.0,
            spray: 0.0,
            zone: None,
        }
    }
}

impl SampleSettings {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A **string** source's string.
///
/// Four numbers, each one thing a real string has and a table cannot:
///
/// - **Stiffness.** A real string is stiff, so its nth partial sits at
///   `n·f0·sqrt(1 + B·n²)` — sharp of the harmonic, more so for every partial
///   up. This is the stretch tuners tune to, and the one property no
///   periodic table can have. The knob is scaled so that the same setting is
///   stiffer on a short treble string than a long bass one, which is how a
///   piano's strings actually run (B grows about tenfold from the middle to
///   the top).
/// - **Damping.** How much faster the high partials die than the low ones.
///   A string's losses grow with frequency, so its top goes first and the
///   note darkens as it rings — the thing a filter envelope over a static
///   table only imitates.
/// - **Strike.** Where along the string the hammer lands, as a fraction of
///   its length. A partial with a node under the hammer is not excited: at
///   an eighth — a piano's — the 8th partial is missing and the 7th weak.
/// - **Decay.** How long the fundamental rings, at middle C. Longer in the
///   bass and shorter up the keyboard, the way a string's is.
///
/// Brightness is the oscillator's own `position`, so the same velocity route
/// that opens a table's frame opens a string's strike.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StringModel {
    /// 0..1.
    pub stiffness: f32,
    /// 0..1.
    pub damping: f32,
    /// 0.02..0.5 of the string's length.
    pub strike: f32,
    /// Seconds, for the fundamental of middle C.
    pub decay_s: f32,
}

impl Default for StringModel {
    fn default() -> Self {
        Self {
            // A piano's middle: B ≈ 4e-4 at middle C, which is what this
            // setting works out to — see `inharmonicity`.
            stiffness: 0.32,
            damping: 0.4,
            strike: 0.12,
            decay_s: 3.0,
        }
    }
}

impl StringModel {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A recording, ready for a **sample** source to read: what `Sampler::prepare`
/// resolves a [`SynthSource::Sample`] to for one note.
///
/// Borrowed, for the reason a table is: the samples are the patch's and are
/// held by an `Arc` off this thread.
#[derive(Debug, Clone, Copy)]
pub struct SampleData<'a> {
    /// Mono, −1..=1.
    pub samples: &'a [f32],
    /// The rate it was recorded at, which is what makes a 44.1 kHz file play
    /// in tune in a 48 kHz session.
    pub sample_rate: f32,
    /// The pitch it was recorded at. A note at this pitch plays the
    /// recording as it is.
    pub root_hz: f32,
    /// The kernel every read between two frames uses: the session's, or
    /// the layer's pin (`patch/quality`). It was `Normal` whatever the
    /// setting until `docs/flopsynth-next.md` §4.1, which is why a bounce
    /// at `High` read a synth's recording no better than playback did.
    pub interpolation: Interpolation,
}

/// What an oscillator reads this sample: nothing, a table, or a recording.
///
/// Resolved before the note by whoever holds the `Arc`s — a voice cannot
/// tell a bank table from a user one or a recording from a file, and that is
/// the point of resolving them in one place.
#[derive(Debug, Clone, Copy)]
pub enum SynthInput<'a> {
    None,
    Table(&'a Wavetable),
    Sample(SampleData<'a>),
}

/// The most partials a string source rings.
///
/// Sixty-four: at middle C that reaches 17 kHz stretched, and in the bass —
/// where a real string has hundreds — the ear stops resolving them long
/// before. The bank is a fixed array for INVARIANT 6's reason.
pub const MAX_PARTIALS: usize = 64;

/// How many partials the string's sample loop advances at once — one AVX
/// register of `f32`s.
const LANES: usize = 8;

/// The most unison voices a string source stacks.
///
/// Four, not [`MAX_UNISON`]: a piano has three strings to a note and each
/// voice here is sixty-four resonators, so this is where the cost is held.
pub const STRING_UNISON: usize = 4;

/// How many grains a [`SampleLoop::Grains`] read has in the air at once.
///
/// Four, evenly staggered: their raised-cosine windows then add to a
/// constant, so a cloud of a steady sound is steady and not a tremolo at
/// the grain rate. More grains would only thicken a spray, and each is a
/// read per unison voice per sample.
pub const GRAINS: usize = 4;

/// One grain of a [`SampleLoop::Grains`] read: where in the recording it
/// landed, and how far through its window it is. The window is a raised
/// cosine over `len` output samples.
#[derive(Debug, Clone, Copy, Default)]
struct Grain {
    start: f64,
    age: u32,
    len: u32,
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
    /// [`SynthSource::Sample`] only: how the recording is read. Defaulted on
    /// the stored side, so every patch written before recordings existed
    /// reads as one that plays its sample once — and **left out** when it
    /// is the default, so the two hundred preset files written before it
    /// existed do not all change the day it does.
    #[serde(default, skip_serializing_if = "SampleSettings::is_default")]
    pub sample: SampleSettings,
    /// [`SynthSource::String`] only: the string. Defaulted and left out for
    /// the same reasons.
    #[serde(default, skip_serializing_if = "StringModel::is_default")]
    pub string: StringModel,
    /// How many times over the session's rate this oscillator renders — see
    /// [`crate::Oversampling`]. `Off` follows the patch's own setting (the
    /// voice resolves that before the note); anything else is this
    /// oscillator's own answer. Left out of the file when it is `Off`, for
    /// the reason `sample` is.
    #[serde(default, skip_serializing_if = "Oversampling::is_off")]
    pub quality: Oversampling,
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
            sample: SampleSettings::default(),
            string: StringModel::default(),
            quality: Oversampling::Off,
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
    /// A **sample** source's read heads, one per unison voice, in the
    /// recording's own frames. `f64`, because a thirty-second recording is a
    /// million and a half frames and an `f32` head walks in steps it cannot
    /// represent by then.
    heads: [f64; MAX_UNISON],
    /// Whether the heads have been put at the start knob for this note. Done
    /// on the first sample rather than at `reset`, because that is when the
    /// recording's length and the modulated start are both in hand.
    heads_placed: bool,
    /// Which way each head is going — a bouncing loop turns each round on
    /// its own, since a detuned stack reaches the loop's end at different
    /// times.
    backwards: [bool; MAX_UNISON],
    /// The grain cloud's grains, for [`SampleLoop::Grains`]. Shared across
    /// the unison stack: a grain is *when* in the recording and *how far
    /// through its window*, and each voice reads it at its own rate from
    /// the same start, which is what makes the stack a chorus of the same
    /// grains rather than eight unrelated clouds.
    grains: [Grain; GRAINS],
    /// Output samples since the grain cloud started, for landing each new
    /// grain in phase with the ones already in the air — see
    /// [`grain_voices`](Self::grain_voices).
    grain_clock: u32,
    /// A **string** source's partials.
    string: StringState,
    /// `2^(semitones/12)` for the semitones it was last asked about — see
    /// [`base_hz`](Self::base_hz).
    semitone_ratio: (i8, f32),
    /// The way back down from an oversampled render, one per channel — see
    /// [`crate::Decimator`]. Untouched at `Off`.
    decimators: [Decimator; 2],
    /// The modulator's sample from the frame before, so an oversampled FM
    /// read can ramp towards this frame's rather than step to it: a
    /// modulator held for four sub-samples is a modulator with images at
    /// the session's rate, and FM by those is more of what oversampling is
    /// here to take out.
    last_modulator: f32,
}

/// The per-voice half of a string: its partials as rotating phasors.
///
/// Structure-of-arrays on purpose — `x`, `y`, the cosine and the sine each a
/// flat run — so the sample loop is one vectorisable pass. Each partial is a
/// complex number rotated by its own angle and shrunk by its own decay every
/// sample: four multiplies and two adds, no transcendental in the loop.
#[derive(Debug, Clone, Copy)]
struct StringState {
    /// Whether the partials have been set ringing for this note.
    struck: bool,
    /// What the rotation coefficients were built for.
    key: Option<StringKey>,
    /// How many partials sit under Nyquist for this note.
    count: usize,
    /// How many of those the strike actually set ringing — the felt takes
    /// the top ones to nothing, and a partial at −72 dB of the loudest is
    /// not worth six multiplies a sample for the rest of the note.
    ringing: usize,
    x: [[f32; MAX_PARTIALS]; STRING_UNISON],
    y: [[f32; MAX_PARTIALS]; STRING_UNISON],
    cos: [[f32; MAX_PARTIALS]; STRING_UNISON],
    sin: [[f32; MAX_PARTIALS]; STRING_UNISON],
    /// The per-sample decay of each partial. One set for the stack: a few
    /// cents of detune moves a partial's loss by nothing anybody hears.
    decay: [f32; MAX_PARTIALS],
}

impl Default for StringState {
    fn default() -> Self {
        Self {
            struck: false,
            key: None,
            count: 0,
            ringing: 0,
            x: [[0.0; MAX_PARTIALS]; STRING_UNISON],
            y: [[0.0; MAX_PARTIALS]; STRING_UNISON],
            cos: [[1.0; MAX_PARTIALS]; STRING_UNISON],
            sin: [[0.0; MAX_PARTIALS]; STRING_UNISON],
            decay: [0.0; MAX_PARTIALS],
        }
    }
}

/// The settings a string's coefficients are only good for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StringKey {
    /// The note, quantised to a fiftieth of a cent, so a bend rebuilds the
    /// bank every few samples rather than every sample — sixty-four sines
    /// and cosines a sample is the one thing this source must not spend.
    hz_q: u32,
    stiffness: f32,
    damping: f32,
    decay_s: f32,
    voices: usize,
    detune_cents: f32,
    sample_rate: f32,
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
            heads: [0.0; MAX_UNISON],
            heads_placed: false,
            backwards: [false; MAX_UNISON],
            grains: [Grain::default(); GRAINS],
            grain_clock: 0,
            string: StringState::default(),
            semitone_ratio: (0, 1.0),
            decimators: [Decimator::default(); 2],
            last_modulator: 0.0,
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
        self.decimators = [Decimator::default(); 2];
        self.last_modulator = 0.0;
        self.heads_placed = false;
        // A string is struck on its first sample, not here: the strike's
        // brightness is the *modulated* position, which `reset` does not
        // have. Marking it unstruck is what makes the next sample the strike.
        self.string.struck = false;
        self.string.key = None;
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

    /// [`SynthOsc::frequency`], with the `powf` paid once per change of
    /// semitones rather than once per sample: the ratio is a function of an
    /// `i8`, and it was three transcendentals a sample on a three-oscillator
    /// patch — twelve at 4× — for a number that never moved. The same
    /// `powf`, so the answer is the same to the bit.
    fn base_hz(&mut self, osc: &SynthOsc, note_hz: f32) -> f32 {
        if self.semitone_ratio.0 != osc.semitones {
            self.semitone_ratio = (osc.semitones, 2f32.powf(f32::from(osc.semitones) / 12.0));
        }
        let base = if osc.key_track { note_hz } else { OSC_FIXED_HZ };
        base * self.semitone_ratio.1
    }

    /// The sample this oscillator produced last — what a layer naming it as an
    /// FM or RM modulator reads.
    pub fn last_sample(&self) -> f32 {
        self.last
    }

    /// One stereo sample pair, for an oscillator reading a table or nothing.
    ///
    /// [`next_sample_from`](Self::next_sample_from) with the table as its
    /// input; kept because every caller that predates recordings reads a
    /// table.
    pub fn next_sample(
        &mut self,
        osc: &SynthOsc,
        table: Option<&Wavetable>,
        note_hz: f32,
        sample_rate: f32,
        modulator: f32,
    ) -> (f32, f32) {
        let input = match table {
            Some(table) => SynthInput::Table(table),
            None => SynthInput::None,
        };
        self.next_sample_from(osc, input, note_hz, sample_rate, modulator)
    }

    /// One stereo sample pair.
    ///
    /// `note_hz` is the note's own pitch, before this oscillator's semitones
    /// and key-tracking switch. `modulator` is the sample the layer named by
    /// [`SynthOsc::modulator`] produced this frame — zero when there is none,
    /// which is what makes FM at full depth with no modulator a silent knob
    /// rather than a broken one.
    ///
    /// `input` is what the source reads, resolved before the note. A source
    /// handed the wrong kind of input — a table for a sample, nothing for a
    /// table — renders silence rather than a substitute: a wrong sound is
    /// harder to diagnose than no sound.
    pub fn next_sample_from(
        &mut self,
        osc: &SynthOsc,
        input: SynthInput<'_>,
        note_hz: f32,
        sample_rate: f32,
        modulator: f32,
    ) -> (f32, f32) {
        if sample_rate <= 0.0 {
            return (0.0, 0.0);
        }
        let factor = osc.quality.factor();
        let out = if factor <= 1 {
            // **Off is the path there always was**, not the oversampled one
            // at a factor of one: `tests/synth_alias.rs` holds that a patch
            // that never asked reads sample for sample what it did.
            self.render_one(osc, input, note_hz, sample_rate, modulator)
        } else {
            // The same render, `factor` times at `factor` times the rate,
            // then the way back down. The sub-samples live on the stack;
            // the decimators are the state (`docs/flopsynth-next.md` §4.1).
            let over = sample_rate * factor as f32;
            let from = self.last_modulator;
            let mut left = [0.0f32; MAX_OVERSAMPLE];
            let mut right = [0.0f32; MAX_OVERSAMPLE];
            for sub in 0..factor {
                let ramped = from + (modulator - from) * (sub + 1) as f32 / factor as f32;
                let pair = self.render_one(osc, input, note_hz, over, ramped);
                left[sub] = pair.0;
                right[sub] = pair.1;
            }
            (
                self.decimators[0].decimate(&left[..factor], osc.quality),
                self.decimators[1].decimate(&right[..factor], osc.quality),
            )
        };
        self.last_modulator = modulator;
        // The modulator reads the *mono* sum, because FM by one side of a
        // panned stack is not a thing anybody means.
        self.last = (out.0 + out.1) * 0.5;
        out
    }

    /// One stereo sample pair at `sample_rate`, whatever that rate is: the
    /// session's from [`next_sample_from`](Self::next_sample_from) at `Off`,
    /// a multiple of it when oversampling.
    fn render_one(
        &mut self,
        osc: &SynthOsc,
        input: SynthInput<'_>,
        note_hz: f32,
        sample_rate: f32,
        modulator: f32,
    ) -> (f32, f32) {
        match (osc.source, input) {
            (SynthSource::Noise, _) => {
                let mono = self.noise(osc);
                (mono, mono)
            }
            // The bank's, or one the patch carries itself: both are a table
            // resolved before the note, and the voice cannot tell them apart
            // — which is the point of resolving them in one place.
            (SynthSource::Table(_) | SynthSource::User(_), SynthInput::Table(table)) => {
                self.table_voices(osc, table, note_hz, sample_rate, modulator)
            }
            (SynthSource::Sample(_), SynthInput::Sample(data)) => {
                self.sample_voices(osc, data, note_hz, sample_rate, modulator)
            }
            (SynthSource::String, _) => self.string_voices(osc, note_hz, sample_rate),
            // A layer whose input has not been resolved renders silence.
            _ => (0.0, 0.0),
        }
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
        let base_hz = self.base_hz(osc, note_hz).clamp(0.0, sample_rate * 0.5);
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
        // Oversampled, the read between the table's samples is the cubic —
        // see `Wavetable::read_smooth` for the measurement behind it.
        let smooth = !osc.quality.is_off();

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
            let mut sample = if smooth {
                table.read_smooth(osc.position.clamp(0.0, 1.0), read_at, level)
            } else {
                table.read(osc.position.clamp(0.0, 1.0), read_at, level)
            };
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

    /// A **sample** source: the recording read at the ratio of the note to
    /// its root, by a stack of unison heads — or, for [`SampleLoop::Grains`],
    /// by a cloud of grains the stack reads together.
    fn sample_voices(
        &mut self,
        osc: &SynthOsc,
        data: SampleData<'_>,
        note_hz: f32,
        sample_rate: f32,
        modulator: f32,
    ) -> (f32, f32) {
        let len = data.samples.len();
        if len == 0 || data.root_hz <= 0.0 || data.sample_rate <= 0.0 {
            return (0.0, 0.0);
        }
        let base_hz = self.base_hz(osc, note_hz).max(0.0);
        let voices = usize::from(osc.unison.voices.clamp(1, MAX_UNISON as u8));
        let blend = osc.unison.blend.clamp(0.0, 1.0);
        let width = osc.unison.width.clamp(0.0, 1.0);
        let amount = osc.warp_amount.clamp(0.0, 1.0);
        let stack = self.unison_constants(UnisonKey {
            voices,
            detune_cents: osc.unison.detune_cents,
            blend,
            width,
            base_hz,
            sync_ratio: 1.0,
            sample_rate,
        });
        let len_f = len as f64;
        let last = (len - 1) as f64;
        let mode = osc.sample.loop_mode;
        // Frames of the recording per output sample, per voice: the
        // stack's own step (the note times the detune, in cycles per output
        // sample) over the root's cycles per recording frame.
        let frames_per_cycle = f64::from(data.sample_rate) / f64::from(data.root_hz);
        // FM on a recording: the read head is pushed back and forth by up to
        // two of the root's cycles, which is the same reach the table's FM
        // has.
        let fm = if osc.warp == WarpMode::Fm {
            f64::from(modulator * amount) * 2.0 * frames_per_cycle
        } else {
            0.0
        };
        let rm = if osc.warp == WarpMode::Rm {
            1.0 - amount + amount * modulator
        } else {
            1.0
        };
        if mode == SampleLoop::Grains {
            return self.grain_voices(
                osc,
                data,
                &stack,
                voices,
                frames_per_cycle,
                fm,
                rm,
                sample_rate,
            );
        }
        if !self.heads_placed {
            // The start knob: where in the recording the note begins. Placed
            // on the first sample because the modulated position is only
            // known here — a velocity route to it is a strike that lands
            // later the softer it is. Backwards, it counts from the end, so
            // the knob at rest is the whole recording either way.
            let start = f64::from(osc.position.clamp(0.0, 1.0)) * len_f;
            let backwards = mode == SampleLoop::Reverse;
            self.heads
                .fill(if backwards { last - start } else { start });
            self.backwards.fill(backwards);
            self.heads_placed = true;
        }
        // The loop, in frames. A loop end at or before its start is none.
        let loop_start = f64::from(osc.sample.loop_start.clamp(0.0, 1.0)) * len_f;
        let loop_end = f64::from(osc.sample.loop_end.clamp(0.0, 1.0)) * len_f;
        let loop_len = loop_end - loop_start;
        let has_loop = loop_len >= 2.0;
        let looping = mode == SampleLoop::Forward && has_loop;
        let bouncing = mode == SampleLoop::PingPong && has_loop;
        // A short crossfade into the seam, so a loop that does not land on
        // its own cycle goes round without a click. Five milliseconds of the
        // recording, or a quarter of the loop when the loop is shorter than
        // that. A bounce has no seam: the head turns round on the sample it
        // reached.
        let fade = (f64::from(data.sample_rate) * 0.005).min(loop_len * 0.25);

        let (mut left, mut right) = (0.0f32, 0.0f32);
        for voice in 0..voices {
            let head = &mut self.heads[voice];
            let backwards = &mut self.backwards[voice];
            if looping {
                // `while`, not `if`: one subtraction is not enough when a
                // note far above the root steps further than the loop is
                // long.
                while *head >= loop_end {
                    *head -= loop_len;
                }
            } else if bouncing {
                // Reflected off whichever point it passed, and turned round.
                // A head that overshot by more than the loop is long (a note
                // far above a short loop) is put back inside rather than
                // reflected out the other side.
                if !*backwards && *head >= loop_end {
                    *head = (2.0 * loop_end - *head).max(loop_start);
                    *backwards = true;
                } else if *backwards && *head < loop_start {
                    *head = (2.0 * loop_start - *head).min(loop_end);
                    *backwards = false;
                }
            } else if *head > last || *head < 0.0 {
                // Past the end of a one-shot — or, backwards, past its
                // start: nothing, and the head stays where it is so the
                // note stays silent.
                continue;
            }
            let at = (*head + fm).clamp(0.0, last);
            let mut sample = interpolate(data.samples, at, data.interpolation);
            if looping && fade > 0.0 && *head > loop_end - fade {
                // Into the seam: blend towards where the loop restarts.
                let t = ((*head - (loop_end - fade)) / fade).clamp(0.0, 1.0) as f32;
                let wrapped = (at - loop_len).clamp(0.0, last);
                let ahead = interpolate(data.samples, wrapped, data.interpolation);
                sample += (ahead - sample) * t;
            }
            sample *= rm;
            let step = f64::from(stack.steps[voice]) * frames_per_cycle;
            *head += if *backwards { -step } else { step };
            let (gl, gr) = stack.gains[voice];
            left += sample * gl;
            right += sample * gr;
        }
        (left * stack.loudness, right * stack.loudness)
    }

    /// The grain cloud.
    ///
    /// [`GRAINS`] grains in the air, each a raised-cosine window over a
    /// piece of the recording that starts where the start knob points (give
    /// or take the spray) and is read at the note. A grain whose window has
    /// closed is respawned where the knob points *now* — which is why a
    /// route on the knob scans the recording, and why a knob left alone is
    /// a sound frozen at one moment of it. The windows are staggered a
    /// quarter of a grain apart at the note's start, so four of them add to
    /// a constant and a cloud of a steady sound is steady.
    ///
    /// **The grains are landed in phase.** Four grains that all start at the
    /// same frame but a hop apart in time read the recording a hop apart in
    /// phase, and at any grain length where that hop is an odd number of
    /// half-cycles they cancel — the metallic comb every granular freeze has.
    /// This one knows the recording's pitch (`root_hz`), so each grain is
    /// landed a fraction of a period along from the knob: the fraction the
    /// clock has advanced since the cloud began, modulo the period. Every
    /// grain then reads the same phase of the recording at the same moment,
    /// whatever its length, and a frozen note is a note rather than a comb.
    #[allow(clippy::too_many_arguments)]
    fn grain_voices(
        &mut self,
        osc: &SynthOsc,
        data: SampleData<'_>,
        stack: &UnisonCache,
        voices: usize,
        frames_per_cycle: f64,
        fm: f64,
        rm: f32,
        sample_rate: f32,
    ) -> (f32, f32) {
        let len_f = data.samples.len() as f64;
        let last = len_f - 1.0;
        let grain_len =
            ((osc.sample.grain_ms.clamp(GRAIN_MIN_MS, GRAIN_MAX_MS) * 1e-3 * sample_rate) as u32)
                .max(2);
        // The centre voice's frames per output sample: what the phase
        // alignment is worked out against. The side voices drift from it by
        // their detune, a few frames over a grain, which is the chorus.
        let rate = f64::from(stack.steps[0]) * frames_per_cycle;
        if !self.heads_placed {
            self.grain_clock = 0;
            for index in 0..GRAINS {
                let age = (grain_len as usize * index / GRAINS) as u32;
                // Staggered grains began before the clock did.
                let start = self.land(osc, len_f, -f64::from(age) * rate, frames_per_cycle);
                let grain = &mut self.grains[index];
                grain.len = grain_len;
                grain.age = age;
                grain.start = start;
            }
            self.heads_placed = true;
        }
        let elapsed = f64::from(self.grain_clock) * rate;
        let (mut left, mut right) = (0.0f32, 0.0f32);
        for index in 0..GRAINS {
            if self.grains[index].age >= self.grains[index].len {
                // Its window has closed: land again where the knob is now,
                // at the length the knob says now.
                let start = self.land(osc, len_f, elapsed, frames_per_cycle);
                let grain = &mut self.grains[index];
                grain.start = start;
                grain.age = 0;
                grain.len = grain_len;
            }
            let grain = &mut self.grains[index];
            let t = grain.age as f32 / grain.len as f32;
            let window = 0.5 - 0.5 * (std::f32::consts::TAU * t).cos();
            let age = f64::from(grain.age);
            for voice in 0..voices {
                let at = grain.start + age * f64::from(stack.steps[voice]) * frames_per_cycle + fm;
                // A grain that runs off either end of the recording reads
                // nothing there, rather than holding the last frame as a
                // level: the recording's end is silence, not a value.
                if !(0.0..=last).contains(&at) {
                    continue;
                }
                let sample = interpolate(data.samples, at, data.interpolation) * window * rm;
                let (gl, gr) = stack.gains[voice];
                left += sample * gl;
                right += sample * gr;
            }
            grain.age += 1;
        }
        self.grain_clock = self.grain_clock.wrapping_add(1);
        // Four staggered raised cosines add to two.
        let norm = stack.loudness * 2.0 / GRAINS as f32;
        (left * norm, right * norm)
    }

    /// Where a grain lands: the start knob, pushed either way by up to the
    /// spray, kept inside the recording — and then along by the fraction
    /// of a period the cloud's clock has reached (`elapsed` frames, modulo
    /// `period` frames), so that it reads in phase with the grains already
    /// in the air.
    fn land(&mut self, osc: &SynthOsc, len_f: f64, elapsed: f64, period: f64) -> f64 {
        let position = f64::from(osc.position.clamp(0.0, 1.0));
        let spray = f64::from(osc.sample.spray.clamp(0.0, 1.0));
        let offset = if spray > 0.0 {
            let mut x = self.rng;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.rng = x;
            (f64::from(x >> 8) / 8_388_608.0 - 1.0) * spray
        } else {
            0.0
        };
        let phase = if period > 0.0 {
            elapsed.rem_euclid(period)
        } else {
            0.0
        };
        // A grain sprayed past either end is folded back in rather than
        // piled up on it: clamped, a wide spray from near the start put
        // half its grains on frame zero.
        let folded = (position + offset).rem_euclid(2.0);
        let folded = if folded > 1.0 { 2.0 - folded } else { folded };
        (folded * len_f + phase).min((len_f - 1.0).max(0.0))
    }

    /// A **string** source: the stack's strings, each a bank of partials
    /// rotating and decaying.
    fn string_voices(&mut self, osc: &SynthOsc, note_hz: f32, sample_rate: f32) -> (f32, f32) {
        let base_hz = self.base_hz(osc, note_hz).clamp(0.0, sample_rate * 0.5);
        if base_hz <= 0.0 {
            return (0.0, 0.0);
        }
        let voices = usize::from(osc.unison.voices.clamp(1, STRING_UNISON as u8));
        let blend = osc.unison.blend.clamp(0.0, 1.0);
        let width = osc.unison.width.clamp(0.0, 1.0);
        let stack = self.unison_constants(UnisonKey {
            voices,
            detune_cents: osc.unison.detune_cents,
            blend,
            width,
            base_hz,
            sync_ratio: 1.0,
            sample_rate,
        });
        let key = StringKey {
            hz_q: (base_hz * 50.0) as u32,
            stiffness: osc.string.stiffness,
            damping: osc.string.damping,
            decay_s: osc.string.decay_s,
            voices,
            detune_cents: osc.unison.detune_cents,
            sample_rate,
        };
        if self.string.key != Some(key) {
            self.tune_string(osc, &stack, base_hz, voices, sample_rate);
            self.string.key = Some(key);
        }
        if !self.string.struck {
            self.strike_string(osc, base_hz, voices, sample_rate);
            self.string.struck = true;
        }

        let count = self.string.ringing;
        let (mut left, mut right) = (0.0f32, 0.0f32);
        for voice in 0..voices {
            let (x, _) = self.string.x[voice][..count].as_chunks_mut::<LANES>();
            let (y, _) = self.string.y[voice][..count].as_chunks_mut::<LANES>();
            let (cos, _) = self.string.cos[voice][..count].as_chunks::<LANES>();
            let (sin, _) = self.string.sin[voice][..count].as_chunks::<LANES>();
            let (decay, _) = self.string.decay[..count].as_chunks::<LANES>();
            // Eight partials at a time into eight running sums, and the
            // sums added at the end: a single `sum +=` across the loop is a
            // chain the compiler may not reorder, and it was the one thing
            // keeping this loop scalar. `ringing` is a multiple of `LANES`,
            // so there is no remainder to walk.
            let mut acc = [0.0f32; LANES];
            for ((((px, py), c), s), d) in
                x.iter_mut().zip(y.iter_mut()).zip(cos).zip(sin).zip(decay)
            {
                for lane in 0..LANES {
                    let (ox, oy) = (px[lane], py[lane]);
                    // Read where the partial *is*, then advance it, so the
                    // first sample of a note is the rest it was struck from.
                    acc[lane] += oy;
                    px[lane] = (ox * c[lane] - oy * s[lane]) * d[lane];
                    py[lane] = (ox * s[lane] + oy * c[lane]) * d[lane];
                }
            }
            let sum: f32 = acc.iter().sum();
            let (gl, gr) = stack.gains[voice];
            left += sum * gl;
            right += sum * gr;
        }
        (left * stack.loudness, right * stack.loudness)
    }

    /// Places the partials for this note: where each sits, and how fast it
    /// dies. Rebuilt when the note or the string moves; the ringing state is
    /// left alone, so a bend re-tunes a sounding string rather than
    /// restriking it.
    fn tune_string(
        &mut self,
        osc: &SynthOsc,
        stack: &UnisonCache,
        base_hz: f32,
        voices: usize,
        sample_rate: f32,
    ) {
        let partials = string_partials(&osc.string, osc.position, base_hz, sample_rate * 0.45);
        // How long the fundamental rings at *this* note: longer in the bass,
        // shorter up the keyboard, gently — the preset's own key routes do
        // the rest.
        let decay_s = osc.string.decay_s.max(0.01) * (261.625_56 / base_hz).powf(0.3);
        // Losses grow with the square of the frequency, which is what takes
        // a string's top away first.
        let damp = osc.string.damping.clamp(0.0, 1.0).powi(2) * 4.0;
        for n in 0..partials.count {
            let hz = partials.ratio[n] * base_hz;
            let khz = hz / 1_000.0;
            let tau = decay_s / (1.0 + damp * khz * khz);
            self.string.decay[n] = (-1.0 / (tau * sample_rate)).exp();
            for voice in 0..voices {
                // The stack's detune, as the ratio of this voice's step to
                // the centre's — every partial of a detuned string moves
                // together.
                let detune = if stack.steps[0] > 0.0 {
                    stack.steps[voice] / stack.steps[0]
                } else {
                    1.0
                };
                let angle = std::f32::consts::TAU * hz * detune / sample_rate;
                self.string.cos[voice][n] = angle.cos();
                self.string.sin[voice][n] = angle.sin();
            }
        }
        self.string.count = partials.count;
    }

    /// Sets the partials ringing: each starts at rest with the amplitude the
    /// hammer gives it, which is where it lands and how hard.
    fn strike_string(&mut self, osc: &SynthOsc, base_hz: f32, voices: usize, sample_rate: f32) {
        let partials = string_partials(&osc.string, osc.position, base_hz, sample_rate * 0.45);
        // Ring only what was struck: past the last partial within 72 dB of
        // the loudest, the rest are inaudible and cost the same per sample
        // as the ones that matter. A soft note on a dull string rings a
        // dozen; a hard one at the bottom rings them all.
        let loudest = partials.amp[..partials.count]
            .iter()
            .fold(0.0f32, |a, b| a.max(*b));
        let floor = loudest * 2.5e-4;
        let ringing = (0..partials.count)
            .rev()
            .find(|i| partials.amp[*i] > floor)
            .map_or(0, |i| i + 1);
        // Rounded up to whole lanes; the lanes past the last struck partial
        // are at rest and stay there, whatever coefficients they carry.
        self.string.ringing = ringing.div_ceil(LANES) * LANES;
        for voice in 0..voices {
            // Every partial, not only the ones ringing: the slots past
            // `count` hold what the last note left, and a lane that reads
            // them would ring a ghost of it.
            for i in 0..MAX_PARTIALS {
                // At rest with a velocity: displacement zero, so the note
                // starts from nothing rather than from a step.
                self.string.x[voice][i] = if i < partials.count {
                    partials.amp[i]
                } else {
                    0.0
                };
                self.string.y[voice][i] = 0.0;
            }
        }
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

/// Where a string's partials sit and how hard each is struck — the one
/// description the voice rings and the window draws, so the picture cannot
/// lie about the sound.
#[derive(Debug, Clone, Copy)]
pub struct StringPartials {
    /// How many sit under `nyquist_hz`.
    pub count: usize,
    /// Each partial's frequency as a multiple of the fundamental: `n` for a
    /// harmonic, more for a stiff string's.
    pub ratio: [f32; MAX_PARTIALS],
    /// Each partial's amplitude at the strike, normalised so they sum to one
    /// — the loudest instant a bank can reach is when they align, and that
    /// is full scale.
    pub amp: [f32; MAX_PARTIALS],
}

/// [`StringPartials`] for `string` struck at `bright` (the oscillator's
/// position, 0..1) on a note at `base_hz`, keeping only partials under
/// `nyquist_hz`.
pub fn string_partials(
    string: &StringModel,
    bright: f32,
    base_hz: f32,
    nyquist_hz: f32,
) -> StringPartials {
    let b = inharmonicity(string.stiffness, base_hz);
    // The first partial *is* the note: the raw stiff-string formula
    // stretches it too, and a tuner tunes the partial they hear, not the
    // ideal string underneath it.
    let first = (1.0 + b).sqrt();
    let h = string.strike.clamp(0.02, 0.5);
    let bright = bright.clamp(0.0, 1.0);
    // A soft strike is a dull one: the spectrum falls faster with the
    // partial and the felt's own corner sits lower. The corner spans five
    // octaves over the knob — 300 Hz to nearly 10 kHz — because a piano's
    // touch is mostly this: a hard blow shortens the hammer's contact and
    // throws energy into partials a soft one never reaches.
    let tilt = 2.0 - 1.3 * bright;
    let corner_hz = 300.0 * 2f32.powf(5.0 * bright);
    let mut out = StringPartials {
        count: 0,
        ratio: [0.0; MAX_PARTIALS],
        amp: [0.0; MAX_PARTIALS],
    };
    let mut total = 0.0f32;
    for n in 1..=MAX_PARTIALS {
        let n_f = n as f32;
        let ratio = n_f * (1.0 + b * n_f * n_f).sqrt() / first;
        let hz = ratio * base_hz;
        if hz >= nyquist_hz {
            break;
        }
        // A partial with a node under the hammer is not excited.
        let node = (n_f * std::f32::consts::PI * h).sin().abs();
        let felt = 1.0 / (1.0 + (hz / corner_hz).powi(2));
        let amp = node * n_f.powf(-tilt) * felt;
        out.ratio[n - 1] = ratio;
        out.amp[n - 1] = amp;
        total += amp;
        out.count = n;
    }
    if total > 0.0 {
        for amp in &mut out.amp[..out.count] {
            *amp /= total;
        }
    }
    out
}

/// A string's inharmonicity coefficient `B` for `stiffness` at `hz`.
///
/// The knob is the string at **middle C**: its square, so the useful range
/// is spread over the travel, times a constant that puts the default at a
/// grand's own 4e-4. From there `B` climbs with the pitch — about tenfold from
/// middle C to the top, which is what a piano's short stiff treble strings
/// measure — so one setting is the whole keyboard's worth of strings rather
/// than one string transposed.
fn inharmonicity(stiffness: f32, hz: f32) -> f32 {
    let at_middle_c = stiffness.clamp(0.0, 1.0).powi(2) * 0.004;
    let octaves = (hz.max(1.0) / 261.625_56).log2();
    at_middle_c * 2f32.powf(octaves * 1.2)
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
