use fontelle_dsp::SynthInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StealPolicy {
    Oldest,
    Quietest,
    LowestPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnisonConfig {
    pub voices: u8,
    pub detune_cents: f32,
    pub spread: f32,
    pub randomise_phase: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RetriggerMode {
    Poly,
    Mono,
    Legato,
}

/// How a note's velocity becomes a gain (`docs/flopsynth-next.md` §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum VelocityCurve {
    /// The gain is the velocity.
    Linear,
    /// The velocity squared — SF2's default modulator, and every patch's
    /// until this existed: velocity 64 lands twelve decibels down.
    #[default]
    Square,
    /// The square root: louder low down, for a pad played gently.
    Soft,
    /// The cube: quieter until the key is really hit.
    Hard,
    /// The gains at velocities 32, 64, 96 and 127, straight lines between
    /// them and up from nought.
    Custom([f32; 4]),
}

impl VelocityCurve {
    /// One of each, in the chooser's order; the custom one at its default
    /// points, which are the linear curve's.
    pub const ALL: [Self; 5] = [
        Self::Linear,
        Self::Square,
        Self::Soft,
        Self::Hard,
        Self::Custom([0.25, 0.5, 0.75, 1.0]),
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Square => "square",
            Self::Soft => "soft",
            Self::Hard => "hard",
            Self::Custom(_) => "custom",
        }
    }

    /// Which of [`ALL`](Self::ALL) this is, whatever its points.
    pub fn index(self) -> usize {
        match self {
            Self::Linear => 0,
            Self::Square => 1,
            Self::Soft => 2,
            Self::Hard => 3,
            Self::Custom(_) => 4,
        }
    }
}

/// The shape of a glide from one pitch to the next (§4.6): how the way
/// there is spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GlideCurve {
    /// A straight line in semitones — what every glide was.
    #[default]
    Linear,
    /// The capacitor's curve: quick off the mark and asymptotic at the
    /// end, which is what a portamento knob on an analogue synth does.
    Exponential,
    /// Most of the way early, then easing in.
    Fast,
    /// Hardly moving at first, then arriving in a rush.
    Slow,
}

impl GlideCurve {
    pub const ALL: [Self; 4] = [Self::Linear, Self::Exponential, Self::Fast, Self::Slow];

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Exponential => "expo",
            Self::Fast => "fast",
            Self::Slow => "slow",
        }
    }

    /// How far along the glide is at `t` of its time, 0..=1 — an identity
    /// at both ends whatever the shape, so a glide starts where it starts
    /// and lands where it lands.
    pub fn progress(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            // 1 − 2^(−8t), scaled so t = 1 is exactly there: eight halvings
            // is the knee a portamento pot has.
            Self::Exponential => (1.0 - (-8.0 * t).exp2()) / (1.0 - (-8.0f32).exp2()),
            Self::Fast => 1.0 - (1.0 - t) * (1.0 - t),
            Self::Slow => t * t,
        }
    }
}

/// When a note glides from the one before it (§4.6).
///
/// `Notes` and `Legato` are the two positions the `glide_legato_only`
/// switch had, and the switch's address still reads and writes them;
/// `Always` is the third — portamento in a poly patch, Serum's *always*
/// mode — which the switch could not say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlideMode {
    /// Between the notes of a mono line, overlapping or not.
    #[default]
    Notes,
    /// Between notes that overlap only — how every mono synth with a
    /// legato switch behaves.
    Legato,
    /// Every note, in every mode: a poly chord slides in from wherever the
    /// last note was.
    Always,
}

impl GlideMode {
    pub const ALL: [Self; 3] = [Self::Notes, Self::Legato, Self::Always];

    pub fn label(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::Legato => "legato",
            Self::Always => "always",
        }
    }
}

/// How the mode is written: the switch every file has, and a second flag
/// for the third position, left out when it is off — so a patch at either
/// old position writes what it always wrote.
#[derive(serde::Serialize, serde::Deserialize)]
struct GlideFlags {
    #[serde(default)]
    glide_legato_only: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    glide_always: bool,
}

impl From<GlideFlags> for GlideMode {
    fn from(flags: GlideFlags) -> Self {
        if flags.glide_always {
            Self::Always
        } else if flags.glide_legato_only {
            Self::Legato
        } else {
            Self::Notes
        }
    }
}

impl From<GlideMode> for GlideFlags {
    fn from(mode: GlideMode) -> Self {
        Self {
            glide_legato_only: mode == GlideMode::Legato,
            glide_always: mode == GlideMode::Always,
        }
    }
}

impl serde::Serialize for GlideMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        GlideFlags::from(*self).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for GlideMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        GlideFlags::deserialize(deserializer).map(Self::from)
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VoiceConfig {
    /// 1..=256.
    pub polyphony: u16,
    pub steal_policy: StealPolicy,
    pub glide_time_s: f32,
    /// Flattened into the file as `glide_legato_only` and, for `Always`,
    /// `glide_always` — see [`GlideMode`].
    #[serde(flatten)]
    pub glide_mode: GlideMode,
    /// Absent from the file at `Linear`, which is every glide there was.
    #[serde(default, skip_serializing_if = "is_default")]
    pub glide_curve: GlideCurve,
    /// Absent from the file at `Square`, which is every patch there was.
    #[serde(default, skip_serializing_if = "is_default")]
    pub velocity_curve: VelocityCurve,
    pub unison: UnisonConfig,
    pub retrigger: RetriggerMode,
    /// How far a full pitch bend goes, in semitones either way.
    ///
    /// Two by default, which is what every keyboard ships with and what
    /// SF2 2.04's always-present pitch-wheel default modulator amounts to.
    /// It is the **patch's** setting and not the wheel's, beside the glide
    /// and the polyphony: a lead that bends an octave and a pad that bends
    /// a tone are the same wheel and different instruments.
    ///
    /// `serde(default)` because a patch written before this existed is not
    /// a broken one, and the default is what it was silently doing: see
    /// [`DEFAULT_BEND_RANGE_SEMITONES`].
    #[serde(default = "default_bend_range")]
    pub bend_range_semitones: f32,
}

/// See [`VoiceConfig::bend_range_semitones`].
pub const DEFAULT_BEND_RANGE_SEMITONES: f32 = 2.0;

fn default_bend_range() -> f32 {
    DEFAULT_BEND_RANGE_SEMITONES
}

/// What the hand playing an instrument is doing right now, beside the notes.
///
/// **Channel-wide and live**, which is what separates these from the five
/// per-note properties a `NoteTrigger` carries (§16.5): a wheel moves what
/// is already sounding, so it is read at render rather than captured at
/// note-on, exactly like the channel's own pan — which is why it rides here
/// with it.
///
/// The bend is bipolar (`-1.0..=1.0`), the other two run `0.0..=1.0`, and
/// all three are what the matrix reads for `ModSource::PitchBend`,
/// `ModWheel` and `Aftertouch`. The bend is *also* applied to the note's own
/// pitch over [`VoiceConfig::bend_range_semitones`], because every keyboard
/// bends pitch and a patch should not have to wire a route to get what the
/// wheel is for. The other two go wherever the patch sends them and nowhere
/// by default: inventing a destination for a mod wheel would be a mapping
/// nobody asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Performance {
    /// The channel's placement — see [`Voice::render_with_pan`].
    pub pan: f32,
    /// `ModSource::ModWheel`, `0.0..=1.0`.
    pub mod_wheel: f32,
    /// `ModSource::PitchBend`, `-1.0..=1.0`.
    pub pitch_bend: f32,
    /// `ModSource::Aftertouch` — channel pressure, `0.0..=1.0`.
    pub aftertouch: f32,
    /// Where the transport is, for the LFOs that read it.
    ///
    /// It rides here rather than as a sixth argument to `render_performing`
    /// for the reason the wheels do: it is **channel-wide and live**, it is
    /// read at render rather than captured at note-on, and every call site
    /// that already builds a `Performance` is exactly the set of call sites
    /// that has a transport to fill it in from.
    pub clock: RenderClock,
}

/// Where the transport is, as a voice needs to know it.
///
/// Two numbers, because two things read them: a synced LFO needs the **tempo**
/// to work out its rate from its division, and a free-running LFO needs the
/// **position** to work out its phase. Both are already on
/// `ProcessContext::transport`, so nothing new is measured — it is only
/// carried one level further in than it used to be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderClock {
    pub bpm: f32,
    pub position_sample: u64,
}

impl Default for RenderClock {
    fn default() -> Self {
        Self {
            bpm: fontelle_types::DEFAULT_BPM,
            position_sample: 0,
        }
    }
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            polyphony: 64,
            steal_policy: StealPolicy::Oldest,
            glide_time_s: 0.0,
            glide_mode: GlideMode::Notes,
            glide_curve: GlideCurve::Linear,
            velocity_curve: VelocityCurve::Square,
            unison: UnisonConfig {
                voices: 1,
                detune_cents: 0.0,
                spread: 0.0,
                randomise_phase: false,
            },
            retrigger: RetriggerMode::Poly,
            bend_range_semitones: DEFAULT_BEND_RANGE_SEMITONES,
        }
    }
}

/// The gain a note-on velocity contributes, as a linear amplitude multiplier.
///
/// SF2 2.04 §8.4.1 specifies a default modulator that is present on every zone
/// unless the file overrides it: MIDI note-on velocity -> initial attenuation,
/// concave curve, negative direction, amount 960 centibels. Feeding the
/// concave curve's 96 dB span through that amount works out to amplitude
/// proportional to the *square* of normalised velocity — velocity 64 lands
/// ~12 dB down, not 6 — which is why soundfonts played with a linear velocity
/// response sound flat and undynamic.
///
/// This is the default modulator's net effect computed directly, not the
/// general modulator machinery: `ModMatrix::evaluate` is still unimplemented,
/// so a file that *overrides* this default is not honoured yet. Every file
/// that doesn't (the overwhelming majority) is now correct. See `PROGRESS.md`.
pub fn velocity_to_gain(velocity: u8) -> f32 {
    velocity_gain(velocity, VelocityCurve::Square)
}

/// The gain a note-on velocity contributes through `curve`
/// (`docs/flopsynth-next.md` §4.2) — [`velocity_to_gain`] is the square.
pub fn velocity_gain(velocity: u8, curve: VelocityCurve) -> f32 {
    // Velocity 0 is a note-off in MIDI, never a very quiet note.
    if velocity == 0 {
        return 0.0;
    }
    let normalised = velocity as f32 / 127.0;
    match curve {
        VelocityCurve::Linear => normalised,
        VelocityCurve::Square => normalised * normalised,
        VelocityCurve::Soft => normalised.sqrt(),
        VelocityCurve::Hard => normalised * normalised * normalised,
        VelocityCurve::Custom(points) => {
            // The points sit at 32, 64, 96 and 127, with nought at nought.
            let knots = [0.0f32, 32.0, 64.0, 96.0, 127.0];
            let levels = [0.0, points[0], points[1], points[2], points[3]];
            let v = f32::from(velocity);
            let mut gain = points[3];
            for pair in 0..4 {
                if v <= knots[pair + 1] {
                    let t = (v - knots[pair]) / (knots[pair + 1] - knots[pair]);
                    gain = levels[pair] + (levels[pair + 1] - levels[pair]) * t;
                    break;
                }
            }
            gain.clamp(0.0, 1.0)
        }
    }
}

/// TDD §7.2: "up to 16" layers. A fixed array, not a `Vec` — per-layer playback
/// state is allocated once with the voice, at pool-construction time, never on
/// note-on (INVARIANT 1, INVARIANT 6).
pub const MAX_LAYERS: usize = 16;

/// Modulation envelopes per voice, on top of `patch.envelopes[0]` — the amp
/// envelope, which every voice always has and which drives the amp stage
/// directly rather than through the matrix.
///
/// A fixed array, like the layers, because a voice's cost has to be knowable
/// before it sounds (INVARIANT 6) and note-on must not allocate (INVARIANT 1).
/// Five beside the amp — six envelopes, Ty's §9.6 of
/// `docs/flopsynth-next.md` (three until 2026-09-20; an SF2 file defines
/// exactly one). Only the ones a route reads are advanced, so an unused slot
/// costs its bytes and nothing else.
pub const MAX_MOD_ENVELOPES: usize = 5;

/// LFOs per voice, for the same reason. SF2 defines two (vibrato and
/// modulation); eight is Serum's count and the plan's (§4.2; four until
/// 2026-09-20), and the strip holds them.
pub const MAX_LFOS: usize = 8;

/// How often, in samples, the filter coefficients are rebuilt along the
/// within-block cutoff ramp (`docs/flopsynth-plan.md` §3.3).
///
/// Eight is 6 kHz at 48 kHz — far above anything an LFO or an envelope moves a
/// corner at, and an eighth of the cost of rebuilding per sample. A patch with
/// no route to cutoff pays it too, and pays almost nothing: the ramp's two
/// endpoints are equal and the `tan` is the same one it would have computed
/// once anyway.
pub const FILTER_STEP: usize = 8;

/// How often, in samples, the modulation sources are read and the
/// destinations resolved (`docs/flopsynth-next.md` §4.2): the mod
/// envelopes and the LFOs advance this often, the matrix's moving routes
/// are summed this often, and a layer's gain, pitch, pan and position are
/// ramped across the step. The cutoff's rhythm, and for the same reason:
/// 6 kHz at 48 kHz is above anything a modulator moves, and an eighth of
/// the cost of resolving per sample.
pub const MOD_STEP: usize = FILTER_STEP;

/// Which of the sources that turn every step at least one route reads.
/// Index 0 of `envelopes` is the amp envelope, which is advanced regardless
/// because it drives the amp stage; the flag is there so the indices line up
/// with `ModSource::Envelope`.
///
/// `via` counts as much as `source` does: a route whose depth is scaled by an
/// LFO needs that LFO turning even though it is not the thing being shaped.
#[derive(Clone, Copy, Default)]
struct InUse {
    envelopes: [bool; MAX_MOD_ENVELOPES + 1],
    lfos: [bool; MAX_LFOS],
    chaos: bool,
    walk: bool,
    follower: bool,
    sequencers: [bool; 2],
}

fn sources_in_use(matrix: &crate::mod_matrix::ModMatrix) -> InUse {
    let mut used = InUse::default();
    for route in &matrix.routes {
        for source in [Some(route.source), route.via].into_iter().flatten() {
            match source {
                crate::mod_matrix::ModSource::Envelope(index) => {
                    if let Some(slot) = used.envelopes.get_mut(index as usize) {
                        *slot = true;
                    }
                }
                crate::mod_matrix::ModSource::Lfo(index) => {
                    if let Some(slot) = used.lfos.get_mut(index as usize) {
                        *slot = true;
                    }
                }
                crate::mod_matrix::ModSource::Chaos => used.chaos = true,
                crate::mod_matrix::ModSource::RandomWalk => used.walk = true,
                crate::mod_matrix::ModSource::EnvelopeFollower => used.follower = true,
                crate::mod_matrix::ModSource::StepSeq(index) => {
                    if let Some(slot) = used.sequencers.get_mut(index as usize) {
                        *slot = true;
                    }
                }
                _ => {}
            }
        }
    }
    used
}

#[derive(Debug, Clone, Copy)]
struct LayerPlayback {
    active: bool,
    /// **Which** of `patch.layers` this slot is playing.
    ///
    /// The slots used to be indexed *by* patch layer — slot `n` played layer
    /// `n` — which quietly made `MAX_LAYERS` a limit on how many zones a patch
    /// could have rather than on how many may sound at once. `zip` stopped at
    /// sixteen and every zone past that never sounded: on a real 46-zone drum
    /// kit, 30 keys of silence that looked exactly like keys that should work.
    ///
    /// A note triggers only the zones covering its key and velocity — one or
    /// two, in a kit — so the slots now hold *those*, and a patch may carry as
    /// many zones as the file does.
    layer: u16,
    /// Fractional sample position within the layer's `SampleBuffer`.
    position: f64,
    /// The phase of a `Source::Oscillator` layer, which has no buffer to hold
    /// a position in.
    ///
    /// Per **slot** rather than per patch layer, and per voice rather than per
    /// patch, for the reason the filter memory is: two notes sounding together
    /// on one oscillator are two different phases, and sharing one would make
    /// a voice's output depend on which other voices rendered before it.
    osc: fontelle_dsp::Oscillator,
    /// The drum machine's per-hit state, for a `Source::Drum` layer.
    ///
    /// Beside `osc` and for exactly the same reason. It carries its own
    /// envelopes, so — unlike every other source here — the length of the
    /// sound is the *hit's* rather than the patch's amp envelope's: a kit
    /// whose kick and hat had to share one decay would not be a kit.
    drum: fontelle_dsp::DrumSynth,
    /// The Flopsynth oscillator's per-voice state, for a `Source::Synth`
    /// layer: eight unison phases, a random source and the noise filter's
    /// memory.
    ///
    /// Beside `osc` and `drum` and for exactly the same reason: two notes
    /// sounding together on one oscillator are two different phases, and
    /// sharing one would make a voice's output depend on which other voices
    /// rendered before it.
    synth: fontelle_dsp::SynthState,
    /// How much of the layer its velocity window lets through, 0..=1: one
    /// inside the window past its fades, less within `vel_fade` of an
    /// edge. Fixed for the note, like the velocity's own gain.
    window_gain: f32,
    /// Whether the hit still has to be fired.
    ///
    /// A drum's envelope coefficients depend on the **sample rate**, which
    /// `trigger_note` does not know — the rate reaches a voice with the block
    /// it is asked to render. So the note-on records that a hit is owed and
    /// the first sample of the first block fires it, which is the same moment
    /// either way and needs no rate plumbed through the note path.
    drum_pending: bool,
}

impl Default for LayerPlayback {
    fn default() -> Self {
        Self {
            active: false,
            layer: 0,
            position: 0.0,
            osc: fontelle_dsp::Oscillator::new(),
            drum: fontelle_dsp::DrumSynth::new(),
            synth: fontelle_dsp::SynthState::new(),
            window_gain: 1.0,
            drum_pending: false,
        }
    }
}

/// Where one prepared layer's samples come from.
///
/// The two are resolved to completely different constants — a step through a
/// buffer against a frequency in hertz — and keeping them in one struct with
/// half its fields unused for either is how a renderer ends up silently
/// skipping the variant nobody filled in, which is what `Source::Oscillator`
/// did for as long as this enum did not exist.
#[derive(Clone, Copy)]
enum PreparedSource<'a> {
    /// PCM straight out of the `SampleStore` — no copy, no allocation.
    Sample {
        data: &'a [f32],
        /// The recording's rate over the session's: what the pitch ratio
        /// is multiplied by to make the step through the buffer.
        rate_ratio: f32,
        loop_end: f64,
        loop_len: f64,
        looping: bool,
        end_offset: f64,
        interpolation: fontelle_dsp::Interpolation,
    },
    /// A shape, run at the note's pitch. **It never ends**: an oscillator
    /// has no last sample to run off, so the note lasts exactly as long as its
    /// amplitude envelope says.
    Oscillator { kind: fontelle_dsp::OscKind },
    /// One drum hit. **It ends itself**, which is what makes it different
    /// from both of the others: a hit's length is its own `decay_s` and the
    /// patch's amp envelope is held open behind it (see
    /// `crate::drum_kit::drum_kit`), so the slot goes quiet when the drum
    /// does rather than when the note is let go.
    ///
    /// The voice is copied in rather than borrowed because it is eighty bytes
    /// of plain numbers and `PreparedLayer` is `Copy`; a reference would tie
    /// the prepared array's lifetime to the patch for no gain.
    Drum(fontelle_dsp::DrumVoice),
    /// One Flopsynth oscillator, with the table `Sampler::prepare` resolved
    /// for it. The copy of the patch's description is what the matrix
    /// moves, per step: the patch is what the document holds and a route
    /// is not an edit to it.
    ///
    /// The table is borrowed rather than owned because it is a megabyte and
    /// the `Arc` that holds it is the sampler's — resolving one *here* would
    /// take a lock and allocate, which is exactly what INVARIANT 1 forbids on
    /// this thread.
    Synth {
        osc: fontelle_dsp::SynthOsc,
        /// The table or the recording — or nothing, for a noise, a string,
        /// or a source naming something the patch does not carry.
        input: fontelle_dsp::SynthInput<'a>,
    },
}

/// What a layer is before the matrix moves it: the note's interval, the
/// zone's level and pan, and an oscillator's four modulated knobs.
///
/// Resolved once a block; the matrix's sums are added to these at every
/// step (`MOD_STEP`) and the result ramped across the step's samples.
#[derive(Clone, Copy, Default)]
struct LayerBase {
    /// From the note's key to the zone's root, plus the glide, the bend,
    /// the note's fine pitch and the zone's own tuning — everything on the
    /// pitch path but the matrix.
    semitones: f32,
    gain_db: f32,
    pan: f32,
    position: f32,
    warp: f32,
    detune: f32,
    blend: f32,
}

/// The ramped values one layer slot is at **right now**: what the sample
/// loop reads, moved a step's worth per sample towards where the matrix put
/// them at the top of the step. Kept on the voice from one block to the
/// next, so the first step of a block ramps from where the last block left
/// off rather than jumping there.
#[derive(Clone, Copy, Default)]
struct LayerLive {
    /// Linear, with the velocity folded in.
    gain: f32,
    /// This layer's place in the stereo field, already through the pan law.
    /// `(1.0, 0.0)` on a mono render: nothing goes to a channel that isn't
    /// there, and the one that is carries the layer unattenuated.
    pan: (f32, f32),
    /// Over the zone's root.
    pitch_ratio: f32,
    /// A table oscillator's read position, 0..1.
    position: f32,
}

/// What the matrix moves on one layer slot, summed, in each destination's
/// own unit — cents, decibels, and knob travel for the rest.
#[derive(Clone, Copy, Default)]
struct LayerMods {
    pitch_cents: f32,
    gain_db: f32,
    pan: f32,
    position: f32,
    warp: f32,
    detune: f32,
    blend: f32,
}

/// What the matrix moves on the LFOs: octaves of rate over three, depth,
/// and phase, per LFO. Kept on the voice from one step to the next, because
/// an LFO's rate for a step is decided before the LFO turns and the walk
/// that decides it comes after — see the step loop in `render_performing`.
#[derive(Clone, Copy, Default)]
struct LfoSums {
    rate: [f32; MAX_LFOS],
    depth: [f32; MAX_LFOS],
    phase: [f32; MAX_LFOS],
}

/// Every destination the matrix reaches on a voice, summed for one step,
/// each in its own unit. Filled by `ModMatrix::accumulate` through
/// [`ModSums::add`]; the layer sums are by **slot**, which is what the
/// sample loop is indexed by.
#[derive(Clone, Copy, Default)]
struct ModSums {
    layer: [LayerMods; MAX_LAYERS],
    cutoff_cents: [f32; 2],
    resonance: [f32; 2],
    drive: [f32; 2],
    character: [f32; 2],
    amp_db: f32,
    lfo: LfoSums,
}

/// No slot plays this patch layer — `slot_of`'s empty entry.
const NO_SLOT: u8 = u8::MAX;

impl ModSums {
    /// Adds `amount` — already in `dest`'s unit — where `dest` is kept.
    /// `slot_of` maps a patch layer's index to the slot playing it. The
    /// envelopes' stages are not here: they are resolved once a block from
    /// the still sources (`modulated_stages`), because a duration wobbled
    /// by the thing it is timing has no right answer.
    fn add(&mut self, dest: crate::mod_matrix::ModDest, amount: f32, slot_of: &[u8; 256]) {
        use crate::mod_matrix::ModDest;
        fn layer<'s>(
            sums: &'s mut ModSums,
            slot_of: &[u8; 256],
            index: u8,
        ) -> Option<&'s mut LayerMods> {
            let slot = slot_of[usize::from(index)];
            (slot != NO_SLOT).then(|| &mut sums.layer[usize::from(slot)])
        }
        match dest {
            ModDest::LayerPitch(i) => {
                if let Some(l) = layer(self, slot_of, i) {
                    l.pitch_cents += amount;
                }
            }
            ModDest::LayerGain(i) => {
                if let Some(l) = layer(self, slot_of, i) {
                    l.gain_db += amount;
                }
            }
            ModDest::LayerPan(i) => {
                if let Some(l) = layer(self, slot_of, i) {
                    l.pan += amount;
                }
            }
            ModDest::OscPosition(i) => {
                if let Some(l) = layer(self, slot_of, i) {
                    l.position += amount;
                }
            }
            ModDest::OscWarp(i) => {
                if let Some(l) = layer(self, slot_of, i) {
                    l.warp += amount;
                }
            }
            ModDest::OscUnisonDetune(i) => {
                if let Some(l) = layer(self, slot_of, i) {
                    l.detune += amount;
                }
            }
            ModDest::OscUnisonBlend(i) => {
                if let Some(l) = layer(self, slot_of, i) {
                    l.blend += amount;
                }
            }
            ModDest::FilterCutoff(i) => {
                if let Some(c) = self.cutoff_cents.get_mut(usize::from(i)) {
                    *c += amount;
                }
            }
            ModDest::FilterResonance(i) => {
                if let Some(r) = self.resonance.get_mut(usize::from(i)) {
                    *r += amount;
                }
            }
            ModDest::FilterDrive(i) => {
                if let Some(d) = self.drive.get_mut(usize::from(i)) {
                    *d += amount;
                }
            }
            ModDest::FilterCharacter(i) => {
                if let Some(c) = self.character.get_mut(usize::from(i)) {
                    *c += amount;
                }
            }
            ModDest::Amp => self.amp_db += amount,
            ModDest::LfoRate(i) => {
                if let Some(r) = self.lfo.rate.get_mut(usize::from(i)) {
                    *r += amount;
                }
            }
            ModDest::LfoDepth(i) => {
                if let Some(d) = self.lfo.depth.get_mut(usize::from(i)) {
                    *d += amount;
                }
            }
            ModDest::LfoPhase(i) => {
                if let Some(p) = self.lfo.phase.get_mut(usize::from(i)) {
                    *p += amount;
                }
            }
            // The stages are `modulated_stages`'; the sample-loop points
            // are not destinations the voice reads (TDD §7.5's minimum
            // set names them, and nothing has wanted them yet); an
            // effect's parameter is the node's (`Sampler::fx_modulation`).
            ModDest::EnvelopeStageTime(..)
            | ModDest::EnvelopeStageLevel(..)
            | ModDest::SampleStartOffset(_)
            | ModDest::LoopStart(_)
            | ModDest::LoopLength(_)
            | ModDest::UnisonDetune
            | ModDest::FxParam(..)
            | ModDest::GlideTime => {}
        }
    }
}

/// One layer's per-render constants, resolved once before the sample loop in
/// `Voice::render` rather than recomputed per sample.
#[derive(Clone, Copy)]
struct PreparedLayer<'a> {
    source: PreparedSource<'a>,
    /// What the matrix's sums are added to, every step.
    base: LayerBase,
    /// Which of the four filter buses this layer is summed into. Every source
    /// but `Synth` is `Serial`, which is what the two filters have always
    /// done — so nothing that existed before this changes.
    route: fontelle_dsp::FilterRoute,
    /// Which patch layer this is, so a `Synth` layer naming another as its FM
    /// or RM modulator can find that layer's sample for this frame.
    layer_index: usize,
}

/// The pitch an oscillator layer plays at its root key: middle C, 261.6256 Hz.
///
/// A `Source::Oscillator` is transposed by exactly the arithmetic a sample is —
/// `key - root_key`, plus every tuning on the pitch path — so it needs one
/// frequency to be transposed *from*. Middle C rather than A440 so that the
/// default `root_key: 60` makes a note play its own pitch, which is the only
/// reading of a synthesiser anybody expects.
pub const OSC_ROOT_HZ: f32 = 261.625_56;

/// What a note's `release: 127` multiplies the patch's release time by.
///
/// Four rather than some larger number because the property has to stay
/// *drawable*: the roll's lane maps 0..127 across a few dozen pixels, and a
/// range wide enough to turn a pluck into a pad puts every musically useful
/// value in the bottom two pixels of it.
pub const MAX_NOTE_RELEASE: f32 = 4.0;

/// Everything a note-on says beyond "start playing".
///
/// One struct rather than seven positional arguments, and the reason is
/// §16.5: `Note` carries pan, fine pitch, release and two free modulation
/// values, and every one of them has to reach a voice. Pan arrived first and
/// the other four followed, each as a field here rather than another argument
/// threaded through four call sites — which is what this struct was shaped
/// for.
///
/// Built with [`NoteTrigger::new`] plus the `with_`/`in_`/`from_` methods, so
/// a caller says only what it means and the rest stays at the default a plain
/// note-on has always had: centred, voice context zero, from the timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteTrigger {
    pub key: u8,
    pub velocity: u8,
    /// `-1.0` hard left, `0.0` centre, `1.0` hard right. Adds to the layer's
    /// pan and the channel's; see [`Voice::render_with_pan`].
    ///
    /// A unit interval rather than the document's byte: this is the audio
    /// side of the seam, and `fontelle_types::pan_unit` is the one crossing.
    pub pan: f32,
    /// Cents off this note's own key. Adds to the layer's tuning and to the
    /// mod matrix's pitch routes, all three being cents.
    pub fine_pitch: i16,
    /// `0..=127`, lengthening this one note's release past the patch's.
    /// `0` is the patch's own — see [`EventPayload::NoteOn`].
    ///
    /// [`EventPayload::NoteOn`]: fontelle_types::EventPayload::NoteOn
    pub release: u8,
    /// §16.5's two free modulation values, `0..=127`, readable by the patch
    /// as [`ModSource::NoteModX`] and [`ModSource::NoteModY`].
    ///
    /// [`ModSource::NoteModX`]: crate::ModSource::NoteModX
    /// [`ModSource::NoteModY`]: crate::ModSource::NoteModY
    pub mod_x: u8,
    pub mod_y: u8,
    /// TDD §11.4's per-clip tag, so a note-off finds the voice it belongs to.
    pub voice_context: u32,
    pub origin: fontelle_types::VoiceOrigin,
}

impl NoteTrigger {
    /// A plain note: centred, voice context zero, from the timeline.
    pub fn new(key: u8, velocity: u8) -> Self {
        Self {
            key,
            velocity,
            pan: 0.0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            voice_context: 0,
            origin: fontelle_types::VoiceOrigin::Timeline,
        }
    }

    pub fn with_pan(mut self, pan: f32) -> Self {
        self.pan = pan;
        self
    }

    pub fn with_fine_pitch(mut self, cents: i16) -> Self {
        self.fine_pitch = cents;
        self
    }

    pub fn with_release(mut self, release: u8) -> Self {
        self.release = release;
        self
    }

    pub fn with_mod_x(mut self, mod_x: u8) -> Self {
        self.mod_x = mod_x;
        self
    }

    pub fn with_mod_y(mut self, mod_y: u8) -> Self {
        self.mod_y = mod_y;
        self
    }

    pub fn in_context(mut self, voice_context: u32) -> Self {
        self.voice_context = voice_context;
        self
    }

    pub fn from_origin(mut self, origin: fontelle_types::VoiceOrigin) -> Self {
        self.origin = origin;
        self
    }
}

/// One playing note. Fixed-topology (INVARIANT 6): Layers → mix → Filter 1 → Filter 2
/// → Amp → Pan → out, with the mod matrix feeding every stage. Predictable per-voice
/// cost, zero allocation on note-on, no graph compilation on the audio thread.
///
/// **Scope note:** `Source::Sample` and `Source::Oscillator` layers render;
/// `Source::Sf2Zone` is still a silent no-op, and can only reach a patch from a
/// build whose importer did not flatten one into a `Sample`. Tracked in
/// `PROGRESS.md`.
pub struct Voice {
    active: bool,
    /// Whether the key that started this voice is **still down**.
    ///
    /// Not the same question as [`active`](Voice::is_active): a voice in its
    /// release is still sounding and still active, but nobody is holding it
    /// any more. The difference is what keeps a note-off from being spent on
    /// a voice that has already had one — see
    /// [`VoicePool::find_active_mut`], where spending it that way left the
    /// note somebody *was* holding with no way to end it.
    held: bool,
    key: u8,
    voice_context: u32,
    origin: fontelle_types::VoiceOrigin,
    /// Set from `VoicePool`'s monotonic counter on every `trigger`, so the pool
    /// can find the oldest active voice to steal without a separate timestamp
    /// clock (INVARIANT 1: no syscalls on the RT thread).
    age: u64,
    layers: [LayerPlayback; MAX_LAYERS],
    /// Filter1 and Filter2 of the fixed voice topology (TDD §7.4), each with
    /// one filter per output channel: `filters[slot][channel]`.
    ///
    /// Per voice, not per patch: two notes sounding at once each need their
    /// own filter memory, and sharing one would make a voice's output depend
    /// on which other voices happened to render before it. Per channel for
    /// the same reason one step down — once layers are panned apart the two
    /// channels carry different signals, and one shared state would let each
    /// side's history bleed into the other, collapsing the image.
    /// **Three** slots, not two, and one filter per output channel:
    /// `filters[path][channel]`.
    ///
    /// Path 0 is filter 1 as the `F1` route uses it, path 1 is filter 2, and
    /// path 2 is *filter 1 again* for the layers routed `F1→F2`. The third
    /// exists because a stateful filter cannot be in two places at once: with
    /// one instance, a patch whose sub goes through F1 alone and whose saw
    /// goes through F1 into F2 would have to split F1's single output between
    /// two destinations, and there is no split that is right — the two signals
    /// are already summed by the time the filter has run. `docs/flopsynth-plan.md`
    /// §3.3 says "four buses, two filters"; the fourth bus needs a third
    /// filter to be exact, and the cost is one more slot of state per channel.
    ///
    /// Per voice, not per patch: two notes sounding at once each need their
    /// own filter memory, and sharing one would make a voice's output depend
    /// on which other voices happened to render before it. Per channel for the
    /// same reason one step down — once layers are panned apart the two
    /// channels carry different signals.
    filters: [[fontelle_dsp::SynthFilter; 2]; 3],
    /// Fixed for the life of the note, from `velocity_to_gain`. Folded into
    /// each layer's gain at the top of `render` so it costs nothing per sample.
    velocity_gain: f32,
    /// §16.5's per-note pan, `-1.0..=1.0`, captured at note-on.
    ///
    /// Per *voice* rather than per channel because that is what "per note"
    /// means: two notes sounding together on one instrument may sit in
    /// different places. It adds to the layer's own pan and to the channel's
    /// live one — see [`Voice::render_with_pan`].
    ///
    /// **Reset on every trigger**, like the filter memory above it: a voice
    /// coming back out of the pool carrying the last note's pan puts a centred
    /// note wherever the previous one was, which is a bug that only appears
    /// once the pool wraps.
    note_pan: f32,
    /// Note-on velocity and key as the mod matrix sees them: normalised to
    /// 0..1, captured once so evaluating a route never has to reach back into
    /// the event that started the note.
    velocity_norm: f32,
    key_norm: f32,
    /// And the velocity as it was, for a zone's velocity window.
    velocity: u8,
    /// §16.5's fine pitch as semitones, captured at note-on. Added to the
    /// note's interval alongside the glide, and for the same reason: it moves
    /// the *sound* and leaves `key` — the note's identity, which a note-off
    /// names — where it is.
    note_detune: f32,
    /// What this note multiplies the patch's release time by, `>= 1.0`.
    ///
    /// A multiplier rather than a time so that it means the same thing on a
    /// plucked patch and a pad: the instrument sets the character and the note
    /// says "hold it longer than that". `1.0` is `release: 0` — the patch's
    /// own, and the default every note carries.
    note_release_scale: f32,
    /// §16.5's two free modulation values, normalised to 0..1 for the matrix.
    mod_x_norm: f32,
    mod_y_norm: f32,
    amp_env: fontelle_dsp::EnvelopeGenerator,
    /// `patch.envelopes[1..]`, as modulation sources. Per voice, because two
    /// notes are at different points in their envelopes.
    mod_envs: [fontelle_dsp::EnvelopeGenerator; MAX_MOD_ENVELOPES],
    /// `patch.lfos`, retriggered on every note-on: a free-running LFO makes
    /// the same note sound different depending on when it was played, which is
    /// a character an instrument can want but not a default anyone can
    /// predict.
    lfos: [crate::lfo::LfoState; MAX_LFOS],
    /// `ModSource::Random`: one value per note, drawn at note-on.
    ///
    /// Per note rather than per block, which is what "random" means for a
    /// modulation source — a value that changed under a held note would be
    /// noise, and there is a sample & hold LFO for that.
    random: f32,
    /// `ModSource::NoteOnCounter`, cycling `0, 1/7, …, 1` per note-on, so an
    /// eight-step alternation is one route with a quantised curve on it.
    note_counter: f32,
    /// Samples since this note started, for `Lfo::delay_s`. One counter for
    /// the voice rather than one per LFO: they all start together.
    age_samples: u64,
    /// How far the voice's pitch is from [`Voice::key`] right now, in
    /// semitones, and where it is heading.
    ///
    /// **The key itself never moves.** A slide bends the sound and leaves the
    /// note's identity alone, which is what lets the note-off the score wrote
    /// for key 60 still end a voice that is currently sounding key 67 — see
    /// [`Voice::glide_to`].
    glide_semitones: f32,
    glide_target: f32,
    /// Where the glide set off from, how long it has, and how far into
    /// that it is — the curve (§4.6) is a function of the fraction. A
    /// total of nought is "already there".
    glide_from: f32,
    glide_total_s: f32,
    glide_elapsed_s: f32,
    glide_curve: GlideCurve,
    /// The patch's velocity curve as the note found it, for a legato
    /// take-over to read the new velocity through.
    velocity_curve: VelocityCurve,
    /// Where each layer slot's ramped values are — see [`LayerLive`] —
    /// and the voice-wide amp gain's, carried from block to block so a
    /// step ramps from where the last one ended.
    live: [LayerLive; MAX_LAYERS],
    amp_live: f32,
    /// Whether `live` is the last note's: set at note-on, so the first step
    /// of a fresh note starts at its values rather than ramping from a
    /// note that has ended.
    live_fresh: bool,
    /// Each LFO's value at the top of the current step, for the matrix —
    /// and, between blocks, the last step's.
    lfo_values: [f32; MAX_LFOS],
    /// What the matrix routed to the LFOs at the last step's walk, read
    /// when they next turn — see [`LfoSums`].
    lfo_sums: LfoSums,
    /// The §4.2 generators, per voice (`crate::mod_sources`): the
    /// attractor, the walk, the follower of this voice's own level, and
    /// the two sequencers — and each one's value at the top of the step.
    chaos: crate::mod_sources::ChaosState,
    walk: crate::mod_sources::WalkState,
    follower: crate::mod_sources::FollowerState,
    sequencers: [crate::mod_sources::SeqState; 2],
    generator_values: GeneratorValues,
    /// The loudest sample this voice put out in the step just rendered —
    /// what the follower is fed at the next.
    step_peak: f32,
}

/// The generators' values at the top of a step, for the matrix.
#[derive(Clone, Copy, Default)]
struct GeneratorValues {
    chaos: f32,
    walk: f32,
    follower: f32,
    sequencers: [f32; 2],
}

impl Voice {
    pub fn new() -> Self {
        Self {
            active: false,
            held: false,
            key: 0,
            voice_context: 0,
            origin: fontelle_types::VoiceOrigin::Timeline,
            age: 0,
            layers: [LayerPlayback::default(); MAX_LAYERS],
            filters: [[fontelle_dsp::SynthFilter::new(); 2]; 3],
            velocity_gain: 0.0,
            note_pan: 0.0,
            velocity_norm: 0.0,
            key_norm: 0.0,
            velocity: 0,
            note_detune: 0.0,
            note_release_scale: 1.0,
            mod_x_norm: 0.0,
            mod_y_norm: 0.0,
            amp_env: fontelle_dsp::EnvelopeGenerator::new(),
            mod_envs: [fontelle_dsp::EnvelopeGenerator::new(); MAX_MOD_ENVELOPES],
            lfos: [crate::lfo::LfoState::new(); MAX_LFOS],
            random: 0.0,
            note_counter: 0.0,
            age_samples: 0,
            glide_semitones: 0.0,
            glide_target: 0.0,
            glide_from: 0.0,
            glide_total_s: 0.0,
            glide_elapsed_s: 0.0,
            glide_curve: GlideCurve::Linear,
            velocity_curve: VelocityCurve::Square,
            live: [LayerLive::default(); MAX_LAYERS],
            amp_live: 1.0,
            live_fresh: true,
            lfo_values: [0.0; MAX_LFOS],
            lfo_sums: LfoSums::default(),
            chaos: Default::default(),
            walk: Default::default(),
            follower: Default::default(),
            sequencers: Default::default(),
            generator_values: GeneratorValues::default(),
            step_peak: 0.0,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether this voice's key is still down — see [`Voice::held`].
    pub fn is_held(&self) -> bool {
        self.held
    }

    /// How loud this voice is right now, as its amp envelope's level times
    /// its velocity — what `StealPolicy::Quietest` compares. Not the
    /// rendered peak: that would need a meter per voice, and the envelope
    /// is what a player hears as "still sounding".
    pub fn loudness(&self) -> f32 {
        self.amp_env.level() * self.velocity_gain
    }

    pub fn key(&self) -> u8 {
        self.key
    }

    pub fn voice_context(&self) -> u32 {
        self.voice_context
    }

    /// Whether the timeline or a player started this voice. See
    /// [`fontelle_types::VoiceOrigin`] — transport stop and seek cut one and
    /// spare the other.
    /// How long this voice has been sounding, in samples — what makes
    /// "the newest voice" a question with an answer.
    pub fn age_samples(&self) -> u64 {
        self.age_samples
    }

    /// Where each of this voice's LFOs is in its cycle, 0..1.
    /// What this voice's own sources read right now — the per-note numbers
    /// and the modulators at the top of the last step. For the chain's
    /// destinations (`ModDest::FxParam`), which read the newest voice; the
    /// macros and the wheels are the sampler's and read nought here.
    pub fn source_value(&self, source: crate::mod_matrix::ModSource) -> f32 {
        use crate::mod_matrix::ModSource;
        match source {
            ModSource::Envelope(0) => self.amp_env.level(),
            ModSource::Envelope(index) => self
                .mod_envs
                .get(usize::from(index) - 1)
                .map_or(0.0, |env| env.level()),
            ModSource::Lfo(index) => self
                .lfo_values
                .get(usize::from(index))
                .copied()
                .unwrap_or(0.0),
            ModSource::Velocity => self.velocity_norm,
            ModSource::Key => self.key_norm,
            ModSource::NoteModX => self.mod_x_norm,
            ModSource::NoteModY => self.mod_y_norm,
            ModSource::Random => self.random,
            ModSource::NoteOnCounter => self.note_counter,
            ModSource::Chaos => self.generator_values.chaos,
            ModSource::RandomWalk => self.generator_values.walk,
            ModSource::EnvelopeFollower => self.generator_values.follower,
            ModSource::StepSeq(index) => self
                .generator_values
                .sequencers
                .get(usize::from(index))
                .copied()
                .unwrap_or(0.0),
            ModSource::Aftertouch
            | ModSource::ModWheel
            | ModSource::PitchBend
            | ModSource::Macro(_) => 0.0,
        }
    }

    pub fn lfo_phases(&self) -> [f32; MAX_LFOS] {
        std::array::from_fn(|index| self.lfos[index].phase())
    }

    pub fn origin(&self) -> fontelle_types::VoiceOrigin {
        self.origin
    }

    /// Starts a centred note the timeline asked for. See
    /// [`Voice::trigger_from`] for one a player did, and
    /// [`Voice::trigger_note`] for one that carries §16.5's per-note
    /// character.
    pub fn trigger(&mut self, patch: &crate::Patch, key: u8, velocity: u8, voice_context: u32) {
        self.trigger_note(
            patch,
            NoteTrigger::new(key, velocity).in_context(voice_context),
        );
    }

    /// As [`Voice::trigger`], recording where the note came from.
    ///
    /// The origin is set here, in the same call that starts the voice, rather
    /// than by a separate setter afterwards: a voice that is briefly active
    /// with the wrong origin is a voice a reset landing in between would treat
    /// as the wrong kind.
    pub fn trigger_from(
        &mut self,
        patch: &crate::Patch,
        key: u8,
        velocity: u8,
        voice_context: u32,
        origin: fontelle_types::VoiceOrigin,
    ) {
        self.trigger_note(
            patch,
            NoteTrigger::new(key, velocity)
                .in_context(voice_context)
                .from_origin(origin),
        );
    }

    /// The one that actually starts a voice; the two above are it with the
    /// defaults filled in.
    ///
    /// It takes a [`NoteTrigger`] rather than a fifth positional argument
    /// because pan was the *first* of `Note`'s five per-note properties to
    /// reach the audio path and was never going to be the last: fine pitch,
    /// release and the two free modulation values followed, and each one is a
    /// field on the struct rather than another rewrite of every call site.
    pub fn trigger_note(&mut self, patch: &crate::Patch, note: NoteTrigger) {
        let NoteTrigger {
            key,
            velocity,
            pan,
            fine_pitch,
            release,
            mod_x,
            mod_y,
            voice_context,
            origin,
        } = note;
        self.active = true;
        self.held = true;
        self.origin = origin;
        self.key = key;
        self.voice_context = voice_context;
        self.velocity_curve = patch.voice_config.velocity_curve;
        self.velocity_gain = velocity_gain(velocity, patch.voice_config.velocity_curve);
        self.note_pan = pan.clamp(-1.0, 1.0);
        self.velocity_norm = velocity as f32 / 127.0;
        self.key_norm = key as f32 / 127.0;
        self.velocity = velocity;
        // Cents to semitones: the document stores cents because that is the
        // unit a musician tunes in, and every other tuning on the pitch path
        // is cents too, so nothing has to be converted twice.
        self.note_detune = fine_pitch as f32 / 100.0;
        // `0` is the patch's own release and 127 is four times it. Only ever
        // longer, because `0` is what every note ever written carries and a
        // property whose default rewrote existing projects is not one worth
        // having — see `EventPayload::NoteOn`.
        self.note_release_scale =
            1.0 + (release.min(127) as f32 / 127.0) * (MAX_NOTE_RELEASE - 1.0);
        self.mod_x_norm = mod_x.min(127) as f32 / 127.0;
        self.mod_y_norm = mod_y.min(127) as f32 / 127.0;
        // A voice comes back out of the pool carrying the last note's filter
        // memory. Left alone, that discharges into the new note as a transient
        // belonging to a note that already ended — a click that only shows up
        // once voices start being reused.
        for slot in &mut self.filters {
            for filter in slot {
                filter.reset();
            }
        }
        self.amp_env.note_on();
        for env in &mut self.mod_envs {
            env.note_on();
        }
        // The **age counter is the seed** for everything random about this
        // note: the LFOs' sample & hold sequences, the unison stacks' start
        // phases and `ModSource::Random`. It is monotonic per pool, so two
        // notes never draw the same numbers and the same sequence of notes
        // draws the same numbers twice — which is what makes any of this
        // testable.
        let seed = self.age as u32;
        for (index, lfo) in patch.lfos.iter().take(MAX_LFOS).enumerate() {
            self.lfos[index].reset(lfo, seed.wrapping_add(index as u32 * 0x9e37));
        }
        for lfo in self.lfos.iter_mut().skip(patch.lfos.len().min(MAX_LFOS)) {
            *lfo = crate::lfo::LfoState::new();
        }
        // The ramps start over: a new note starts at its own gain and
        // pitch, not on the way there from the last note's.
        self.live_fresh = true;
        self.lfo_values = [0.0; MAX_LFOS];
        self.lfo_sums = LfoSums::default();
        // The generators start over with the note, seeded from it like the
        // LFOs: two notes are two orbits and two walks.
        self.chaos.reset(seed.wrapping_add(0x6a09_e667));
        self.walk.reset(seed.wrapping_add(0xbb67_ae85));
        self.follower.reset();
        for sequencer in &mut self.sequencers {
            sequencer.reset();
        }
        self.generator_values = GeneratorValues::default();
        self.step_peak = 0.0;
        // A bipolar value, so a route to pitch is as likely to go down as up.
        let mut x = seed.wrapping_mul(0x2545_f491) | 1;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = (x >> 8) as f32 / 8_388_608.0 - 1.0;
        // Eight steps, so `Curve::Quantised { steps: 7 }` lands on each of
        // them exactly.
        self.note_counter = (self.age % 8) as f32 / 7.0;
        self.age_samples = 0;
        // A fresh note is at its own pitch. Portamento is applied *after*
        // this by whoever retriggered it (see `Sampler::trigger`), because
        // only the sampler knows what was sounding before.
        self.glide_semitones = 0.0;
        self.glide_target = 0.0;
        self.glide_from = 0.0;
        self.glide_total_s = 0.0;
        self.glide_elapsed_s = 0.0;
        self.glide_curve = patch.voice_config.glide_curve;

        // Every slot cleared first: what a voice plays is decided entirely by
        // this note, and a slot left over from the last one is a zone that
        // keeps sounding after the note that wanted it has gone.
        self.layers = [LayerPlayback::default(); MAX_LAYERS];
        let mut slot = 0;
        for (index, layer) in patch.layers.iter().enumerate() {
            if slot >= MAX_LAYERS {
                // The stack is full. `MAX_LAYERS` bounds what sounds *at
                // once* (TDD §7.4), and a seventeenth zone on one key is the
                // one thing it is allowed to drop.
                break;
            }
            let in_key_range = key >= layer.key_range.0 && key <= layer.key_range.1;
            let in_vel_range = velocity >= layer.vel_range.0 && velocity <= layer.vel_range.1;
            if !(in_key_range && in_vel_range) {
                continue;
            }
            // A patch with more zones than a `u16` can name is not a patch,
            // it is a corrupt file; the zones past that are dropped rather
            // than aliased onto a wrong one.
            let Ok(index) = u16::try_from(index) else {
                break;
            };
            self.layers[slot] = LayerPlayback {
                active: true,
                layer: index,
                position: layer.playback.start_offset,
                // A fresh phase, for the reason the filter memory is reset: a
                // voice out of the pool carrying the last note's phase makes
                // the same note sound different depending on what was played
                // before it.
                osc: fontelle_dsp::Oscillator::new(),
                drum: fontelle_dsp::DrumSynth::new(),
                synth: {
                    // A fresh stack, seeded from the note's age for the same
                    // reason the LFOs are.
                    let mut state = fontelle_dsp::SynthState::new();
                    if let crate::patch::Source::Synth(osc) = &layer.source {
                        state.reset(osc, seed.wrapping_add(index as u32 * 0x85eb));
                    }
                    state
                },
                // The window's fade (§4.2): within `vel_fade` velocities of
                // either edge the layer comes in over the fade rather than
                // switching on, which is how two recordings cross over.
                window_gain: {
                    let fade = f32::from(layer.playback.vel_fade);
                    if fade <= 0.0 {
                        1.0
                    } else {
                        let v = f32::from(velocity);
                        let from_low = (v - f32::from(layer.vel_range.0)) / fade;
                        let from_high = (f32::from(layer.vel_range.1) - v) / fade;
                        from_low.min(from_high).clamp(0.0, 1.0)
                    }
                },
                // Fired on the first sample rendered — see `drum_pending`.
                // A hat retriggered sixteen times a bar starts over each time
                // rather than adding to what is still ringing, because
                // `DrumSynth::trigger` rewrites every field.
                drum_pending: matches!(layer.source, crate::patch::Source::Drum(_)),
            };
            slot += 1;
        }
    }

    /// Silences the voice at once and returns it to the state it had before
    /// it ever played: no tail, no filter memory, no envelope position.
    ///
    /// This is what transport stop and seek need. **It is a hard cut**, and on
    /// a sounding note that is a click — which is right when the audio after
    /// the cut belongs to a different part of the song, and wrong as a way to
    /// end a note. `Sampler::release_all` is the graceful one.
    ///
    /// The filter state matters as much as the envelope: left alone it
    /// discharges into the next note as a transient belonging to one that no
    /// longer exists.
    pub fn reset(&mut self) {
        self.active = false;
        self.held = false;
        self.key = 0;
        self.voice_context = 0;
        self.origin = fontelle_types::VoiceOrigin::Timeline;
        self.age = 0;
        self.layers = [LayerPlayback::default(); MAX_LAYERS];
        for slot in &mut self.filters {
            for filter in slot {
                filter.reset();
            }
        }
        self.velocity_gain = 0.0;
        self.velocity_norm = 0.0;
        self.key_norm = 0.0;
        self.amp_env = fontelle_dsp::EnvelopeGenerator::new();
        self.mod_envs = [fontelle_dsp::EnvelopeGenerator::new(); MAX_MOD_ENVELOPES];
        self.lfos = [crate::lfo::LfoState::new(); MAX_LFOS];
        self.random = 0.0;
        self.note_counter = 0.0;
        self.age_samples = 0;
        self.live_fresh = true;
        self.lfo_values = [0.0; MAX_LFOS];
        self.lfo_sums = LfoSums::default();
        self.chaos = Default::default();
        self.walk = Default::default();
        self.follower.reset();
        self.sequencers = Default::default();
        self.generator_values = GeneratorValues::default();
        self.step_peak = 0.0;
    }

    /// Moves this voice onto a new note **without restarting it** — legato.
    ///
    /// The sample keeps playing, the envelopes keep their level, and only the
    /// pitch moves; with a glide time it slides there rather than jumping,
    /// which is portamento. That is what `RetriggerMode::Legato` means and
    /// what makes a line played without gaps sound like one line.
    ///
    /// The **key is taken**, unlike [`Voice::glide_to`]: this voice is that
    /// note now, so the note-off the score wrote for it is the one that ends
    /// it. A slide is the other case — it bends the sound and leaves the
    /// note's identity alone.
    pub fn legato_to(&mut self, note: NoteTrigger, glide_seconds: f32) {
        let from = self.sounding_key();
        // A legato take-over is still a key going down, and it may take over
        // a voice that was already let go of — so this voice is held again,
        // and the note-off coming for `note.key` is the one that ends it.
        //
        // **Whether it had been let go of is the whole question below**, so it
        // is read before it is overwritten.
        let was_held = self.held;
        self.held = true;
        self.key = note.key;
        self.voice_context = note.voice_context;
        self.origin = note.origin;
        self.velocity_gain = velocity_gain(note.velocity, self.velocity_curve_hint());
        self.velocity_norm = note.velocity as f32 / 127.0;
        self.key_norm = note.key as f32 / 127.0;
        self.note_pan = note.pan.clamp(-1.0, 1.0);
        // The envelopes are deliberately not touched **when the voice was
        // still held**: that is the difference between legato and a retrigger,
        // and it is what makes a phrase played without lifting a finger one
        // shape rather than several.
        //
        // > *"notes that are legato and start and end next to another note
        // > makes that note not play if there was one before it next to it."*
        //
        // A voice that had already been let go of is a different case, and
        // leaving its envelopes alone is the bug in that report. Two notes
        // that touch put a note-off and a note-on on the same sample, and
        // `fontelle_sequencer::sort_events` orders the off first on purpose —
        // so this voice is in its *release*, and a take-over that inherits it
        // produces a note that is held, in tune, and on its way to zero.
        // Measured, the second note of a touching pair came out 35 dB under
        // the first: there, and silent.
        //
        // `RetriggerMode::Mono` never had this, because it goes through
        // `trigger_note`, which starts the envelopes again. This is Legato
        // being given the same answer for the same case, and only for that
        // case — a note arriving over a key that is still down still carries
        // the envelope it found.
        if !was_held {
            self.amp_env.note_on();
            for env in &mut self.mod_envs {
                env.note_on();
            }
        }
        self.glide_from(from - note.key as f32, glide_seconds);
    }

    /// Bends this voice to `key` over `seconds`, from wherever its pitch is.
    ///
    /// **A slide, not a note.** The voice's own [`key`](Voice::key) is left
    /// alone, so the note-off the score wrote for the key this voice *started*
    /// on still ends it — which is the whole reason the offset is a separate
    /// number rather than the key being rewritten. Without that, every slid
    /// note in a piece would hang.
    ///
    /// `seconds` of zero arrives immediately, which is what a portamento of
    /// zero has to mean.
    pub fn glide_to(&mut self, key: u8, seconds: f32) {
        self.glide_target = key as f32 - self.key as f32;
        self.set_glide_rate(seconds);
    }

    /// Starts this voice's pitch `semitones` away from its own note and lets
    /// it fall in over `seconds` — the portamento form.
    ///
    /// The other end of [`Voice::glide_to`]: that one moves the target, this
    /// one moves the *start*. A new note in a mono patch is at its own pitch
    /// as far as the score is concerned and has to sound as if it came from
    /// the last one.
    pub fn glide_from(&mut self, semitones: f32, seconds: f32) {
        self.glide_semitones = semitones;
        self.glide_target = 0.0;
        self.set_glide_rate(seconds);
    }

    /// Where this voice's pitch is now, as a key — the note it started on plus
    /// however far a glide has carried it.
    ///
    /// What a *following* glide measures from, so a chain of slides is one
    /// continuous line rather than a series of jumps back to the original key.
    pub fn sounding_key(&self) -> f32 {
        self.key as f32 + self.glide_semitones
    }

    fn set_glide_rate(&mut self, seconds: f32) {
        let distance = (self.glide_target - self.glide_semitones).abs();
        if seconds <= 0.0 || distance <= f32::EPSILON {
            self.glide_semitones = self.glide_target;
            self.glide_total_s = 0.0;
            return;
        }
        self.glide_from = self.glide_semitones;
        self.glide_total_s = seconds;
        self.glide_elapsed_s = 0.0;
    }

    /// The curve a glide started under: the patch's, taken at the note.
    /// A legato take-over keeps the voice's, which is the same patch's.
    fn velocity_curve_hint(&self) -> VelocityCurve {
        self.velocity_curve
    }

    /// Steps the glide on by one block.
    ///
    /// **Block rate, not sample rate**, and deliberately: the pitch a layer
    /// plays at is worked out once per block already — the mod matrix's
    /// `LayerPitch` route, which is what vibrato rides on, is computed in
    /// exactly the same place. A glide that moved per sample would be the only
    /// pitch modulation in this voice that did, and making all of it
    /// per-sample is a change to the render loop rather than to this feature.
    fn advance_glide(&mut self, frames: usize, sample_rate: f32) {
        if self.glide_total_s <= 0.0 || sample_rate <= 0.0 {
            return;
        }
        self.glide_elapsed_s += frames as f32 / sample_rate;
        if self.glide_elapsed_s >= self.glide_total_s {
            self.glide_semitones = self.glide_target;
            self.glide_total_s = 0.0;
            return;
        }
        // The curve (§4.6) bends the fraction of the time, not the pitch:
        // every shape starts where it started and lands where it lands.
        let t = self
            .glide_curve
            .progress(self.glide_elapsed_s / self.glide_total_s);
        self.glide_semitones = self.glide_from + (self.glide_target - self.glide_from) * t;
    }

    /// Voice stealing always ramps out over a short release rather than cutting
    /// hard (TDD §7.4) — never a click. Uses the same envelope release as a
    /// normal note-off; a shorter, dedicated steal-ramp is a later refinement.
    pub fn release(&mut self) {
        self.held = false;
        self.amp_env.note_off();
        // A modulation envelope releases with the note too: a filter envelope
        // that stayed open through the release would keep the tail brighter
        // than the note that produced it.
        for env in &mut self.mod_envs {
            env.note_off();
        }
    }

    /// RT: no allocation. Mixes every active layer into `out`, applying pitch
    /// (root key + fine tune, resampled against the buffer's own rate),
    /// looping, per-layer gain, and the patch's amp envelope (`envelopes[0]`).
    /// Adds into `out` rather than overwriting it — callers mixing multiple
    /// voices into one buffer must clear it first.
    ///
    /// **The loop is sample-major, not layer-major, and that is load-bearing.**
    /// The obvious layer-major shape — add every layer across the whole
    /// buffer, then multiply the buffer by the envelope — applies this voice's
    /// envelope to whatever *other* voices already mixed into `out`, since
    /// `out` is shared. With N voices the first one's output ends up
    /// multiplied by all N envelopes; audibly, holding a chord and adding a
    /// note ducks the held notes for the length of the new note's attack.
    /// That was a real bug here, caught by
    /// `sampler::tests::a_new_notes_attack_does_not_duck_already_sounding_voices`.
    /// Advancing the envelope once per output sample and scaling only this
    /// voice's own mixed sample before adding it is what keeps the additive
    /// contract honest.
    /// `quality` is the session's interpolation setting, used by any layer
    /// that names none of its own — see `Sampler::set_quality`.
    ///
    /// `out` is planar: `[left, right]` for stereo, `[mono]` for one channel,
    /// and anything past the second slice is left alone. **A mono render
    /// ignores `Layer::pan` rather than folding it down**, because the centre
    /// pan-law gain applied to a signal with nowhere to pan is just a
    /// uniform 3 dB of attenuation the caller never asked for — the same call
    /// `MixerTrackNode` makes for a mono track.
    pub fn render(
        &mut self,
        patch: &crate::Patch,
        store: &crate::SampleStore,
        sample_rate: f32,
        quality: fontelle_dsp::Interpolation,
        out: &mut [&mut [f32]],
    ) {
        self.render_with_pan(patch, store, sample_rate, quality, 0.0, out)
    }

    /// As [`Voice::render`], with the channel's own pan folded into every
    /// layer's placement.
    ///
    /// `channel_pan` is the compiled form of MIDI CC10 — a control over the
    /// whole part, distinct from the `pan` an SF2 zone carries for itself and
    /// from the one the *note* carries (§16.5, captured at note-on as
    /// `note_pan`). All three **add**, then clamp: that is what a soundfont
    /// player does, and it is the only reading under which a hard-left zone
    /// on a channel panned right ends up between them rather than at
    /// whichever was consulted last.
    ///
    /// It is read here rather than captured at note-on because it is live:
    /// moving a part's pan has to move the notes already sounding.
    pub fn render_with_pan(
        &mut self,
        patch: &crate::Patch,
        store: &crate::SampleStore,
        sample_rate: f32,
        quality: fontelle_dsp::Interpolation,
        channel_pan: f32,
        out: &mut [&mut [f32]],
    ) {
        self.render_performing(
            patch,
            store,
            // **No wavetables.** These two wrappers are the sampled and
            // oscillator path; a `Source::Synth` layer needs the tables
            // `Sampler::prepare` resolved for it, and therefore needs
            // `render_performing` — which is what the sampler calls. A layer
            // whose table is missing renders silence rather than a
            // substitute: a wrong sound is harder to diagnose than no sound.
            &crate::WavetableSet::EMPTY,
            sample_rate,
            quality,
            Performance {
                pan: channel_pan,
                ..Performance::default()
            },
            out,
        )
    }

    /// As [`Voice::render_with_pan`], with everything else the hand is doing
    /// — see [`Performance`], which is where the wheels are and why they are
    /// read here rather than captured at note-on.
    #[allow(clippy::too_many_arguments)]
    pub fn render_performing(
        &mut self,
        patch: &crate::Patch,
        store: &crate::SampleStore,
        tables: &crate::WavetableSet,
        sample_rate: f32,
        quality: fontelle_dsp::Interpolation,
        performance: Performance,
        out: &mut [&mut [f32]],
    ) {
        let channel_pan = performance.pan;
        if !self.active || out.is_empty() {
            return;
        }
        let stereo = out.len() >= 2;
        let frames = out.iter().take(2).map(|c| c.len()).min().unwrap_or(0);

        let amp_env_config =
            patch
                .envelopes
                .first()
                .copied()
                .unwrap_or(fontelle_dsp::EnvelopeConfig {
                    sustain_level: 1.0,
                    ..Default::default()
                });
        // §16.5's per-note release, applied to the config rather than to the
        // generator: the envelope's shape is the patch's, and the note only
        // gets to say how long the last stage of it takes. A local copy, so
        // two notes on one patch can ring for different lengths.
        let amp_env_config = fontelle_dsp::EnvelopeConfig {
            release_s: amp_env_config.release_s * self.note_release_scale,
            ..amp_env_config
        };

        // Modulation runs at **step rate** (`docs/flopsynth-next.md` §4.2):
        // the block is walked in `MOD_STEP`-sample steps, and at the top of
        // each the mod envelopes and the LFOs are read and advanced, the
        // routes whose source moves are summed, and every destination is
        // resolved. The layer's gain, pitch, pan and position are then
        // ramped across the step, sample by sample, from where the last
        // step left them.
        //
        // It ran at block rate until 2026-09-20 — every source sampled once
        // a block and held. At the engine's 128-frame blocks that is 375 Hz,
        // which is fine for a filter sweep and wrong for two things a synth
        // is used for: a 20 Hz LFO on pitch held in 128-sample steps is a
        // stair, and a stair on pitch is a comb of sidebands at multiples
        // of 375 Hz round the note (`tests/mod_rate.rs` measured it at
        // −40 dB); and a 5 ms envelope read at 0, 2.7 and 5.3 ms never
        // reaches its peak, so the gate is not heard at all. Eight samples
        // is 6 kHz at 48 kHz, the rhythm the cutoff already kept.
        //
        // The routes whose sources do **not** move within a block — key,
        // velocity, the wheels, the macros — are summed once, here, and the
        // per-step walk is over the rest only: a preset of key and velocity
        // routes pays nothing for the rate (§4.2's "walk only the routes
        // whose source moved").
        //
        // The amp envelope is still advanced per sample, because it is a
        // gain rather than a control value and a stepped one is audible as
        // a buzz on fast attacks.
        let used = sources_in_use(&patch.mod_matrix);
        let (env_used, lfo_used) = (used.envelopes, used.lfos);

        // Everything a source can be **except an envelope or an LFO**: the
        // numbers that hold for the whole block.
        let (wheel, bend, pressure) = (
            performance.mod_wheel.clamp(0.0, 1.0),
            performance.pitch_bend.clamp(-1.0, 1.0),
            performance.aftertouch.clamp(0.0, 1.0),
        );
        let (velocity_norm, key_norm) = (self.velocity_norm, self.key_norm);
        let (mod_x_norm, mod_y_norm) = (self.mod_x_norm, self.mod_y_norm);
        let (random, note_counter) = (self.random, self.note_counter);
        // Eight numbers copied out of the patch, so the closure does not
        // borrow it: the *name* of a macro is touched off the RT thread only,
        // and the value is all the audio path ever reads.
        let macro_values: [f32; crate::patch::MACRO_COUNT] =
            std::array::from_fn(|i| patch.macros[i].value.clamp(0.0, 1.0));
        let scalar = move |source: crate::mod_matrix::ModSource| match source {
            crate::mod_matrix::ModSource::Velocity => velocity_norm,
            crate::mod_matrix::ModSource::Key => key_norm,
            crate::mod_matrix::ModSource::NoteModX => mod_x_norm,
            crate::mod_matrix::ModSource::NoteModY => mod_y_norm,
            crate::mod_matrix::ModSource::ModWheel => wheel,
            crate::mod_matrix::ModSource::PitchBend => bend,
            crate::mod_matrix::ModSource::Aftertouch => pressure,
            crate::mod_matrix::ModSource::Random => random,
            crate::mod_matrix::ModSource::NoteOnCounter => note_counter,
            crate::mod_matrix::ModSource::Macro(index) => {
                macro_values.get(index as usize).copied().unwrap_or(0.0)
            }
            crate::mod_matrix::ModSource::Envelope(_)
            | crate::mod_matrix::ModSource::Lfo(_)
            | crate::mod_matrix::ModSource::Chaos
            | crate::mod_matrix::ModSource::RandomWalk
            | crate::mod_matrix::ModSource::EnvelopeFollower
            | crate::mod_matrix::ModSource::StepSeq(_) => 0.0,
        };
        // The amp envelope's stages, modulated. Read from the sources above
        // — key, velocity, the wheels, the macros — rather than from the LFOs
        // and the other envelopes, because a duration wobbled by the thing
        // it is timing has no right answer. A piano's decay following the
        // key is what this is for.
        let amp_env_config = modulated_stages(amp_env_config, 0, &patch.mod_matrix, &scalar);
        // And the mod envelopes' stages, the same way, once a block.
        let mut env_configs = [fontelle_dsp::EnvelopeConfig::default(); MAX_MOD_ENVELOPES];
        for (index, config) in patch
            .envelopes
            .iter()
            .skip(1)
            .take(MAX_MOD_ENVELOPES)
            .enumerate()
        {
            if env_used[index + 1] {
                env_configs[index] =
                    modulated_stages(*config, (index + 1) as u8, &patch.mod_matrix, &scalar);
            }
        }

        // Which **slot** plays each patch layer, for the walk: a `ModDest`
        // names a patch layer with a `u8`, so a patch with more than 256
        // zones has no way to address the ones past that. They get no
        // per-layer modulation rather than being aliased onto layer 0's
        // routes, which is the failure that would be impossible to see.
        let mut slot_of = [NO_SLOT; 256];
        for (slot, playback) in self.layers.iter().enumerate() {
            if playback.active
                && let Ok(index) = u8::try_from(playback.layer)
            {
                slot_of[usize::from(index)] = slot as u8;
            }
        }

        // The still routes, summed once for the block.
        let mut still = ModSums::default();
        patch
            .mod_matrix
            .accumulate(false, &scalar, &mut |dest, amount| {
                still.add(dest, amount, &slot_of)
            });

        // Which layers some *other* layer reads as its modulator.
        //
        // A layer at the floor is normally not rendered at all (see the skip
        // below), and this is the exception: a modulator's level is how much
        // of it you **hear**, not whether it modulates, so an FM operator
        // turned all the way down is still a full-strength operator. "FM
        // Growl" and "EP Tine" are both an oscillator nobody hears.
        let mut is_modulator = [false; MAX_LAYERS];
        for layer in &patch.layers {
            if let crate::patch::Source::Synth(osc) = &layer.source
                && let Some(which) = osc.modulator
                && let Some(flag) = is_modulator.get_mut(usize::from(which))
            {
                *flag = true;
            }
        }

        // The two filter slots' settings for the block: everything but
        // what the matrix moves, which is set per step below.
        let settings: [fontelle_dsp::SynthFilterSettings; 2] = std::array::from_fn(|index| {
            let slot = patch.filters[index];
            fontelle_dsp::SynthFilterSettings {
                model: slot.model,
                mode: slot.mode,
                slope: slot.slope,
                // Key tracking is applied here rather than stored, so one
                // patch sounds the same at every pitch without the document
                // carrying a different cutoff per note.
                cutoff_hz: fontelle_dsp::key_tracked_cutoff(
                    slot.cutoff_hz,
                    self.key,
                    slot.key_track,
                ),
                resonance: slot.resonance,
                drive: slot.drive,
                character: slot.character,
                oversampling: patch.oversampling,
            }
        });
        let enabled = [patch.filters[0].enabled, patch.filters[1].enabled];
        let mut live_filters = settings;

        // Split once, outside the loop: `out[0]` and `out[1]` are distinct
        // slices, and taking both mutably per sample would be a reborrow the
        // compiler can't see through.
        let (left, rest) = out.split_at_mut(1);
        let left = &mut *left[0];
        let mut right = rest.first_mut();

        // Per-layer constants resolved once a block, not once per sample: a
        // fixed-size stack array (INVARIANT 1 — no `Vec`, nothing
        // heap-touching). Built at the first step, once that step's sums
        // are known, because whether a layer is at the floor is decided
        // then.
        let mut prepared: [Option<PreparedLayer<'_>>; MAX_LAYERS] = [None; MAX_LAYERS];
        // Every layer's sample this frame, indexed by **patch layer index**,
        // so an oscillator naming a later layer as its FM or RM modulator can
        // find it. Fixed-size and on the stack, like everything else here.
        let mut layer_out = [0.0f32; MAX_LAYERS];
        // The per-sample increments of the ramped values, per slot, and of
        // the amp; the values themselves live on the voice, so a step ramps
        // from wherever the last one — in this block or the one before —
        // left off.
        let mut ramp = [LayerLive::default(); MAX_LAYERS];

        let mut at = 0usize;
        while at < frames {
            let n = MOD_STEP.min(frames - at);
            let inverse_n = 1.0 / n as f32;

            // --- the sources, at the top of the step -------------------
            // The mod envelopes: read before advancing, so the value a
            // destination uses this step is the one at its start, not its
            // end. Only the ones some route names are advanced (an SF2
            // import gives every patch a modulation envelope whether it
            // uses it or not).
            let mut env_levels = [0.0f32; MAX_MOD_ENVELOPES];
            for index in 0..MAX_MOD_ENVELOPES {
                if !env_used[index + 1] || index >= patch.envelopes.len().saturating_sub(1) {
                    continue;
                }
                let env = &mut self.mod_envs[index];
                env_levels[index] = env.level();
                for _ in 0..n {
                    env.advance(&env_configs[index], sample_rate);
                }
            }
            let amp_level = self.amp_env.level();
            // The LFOs, in order, each with its rate, depth and phase moved
            // by whatever the matrix routed to them **at the last step** —
            // the sums are kept on the voice from one walk to the next, so
            // an envelope or another LFO on an LFO's rate works, one step
            // (an eighth of a millisecond) behind. That is what lets a pair
            // of LFOs point at each other: two coupled oscillators, rather
            // than an order to get wrong.
            for (index, lfo) in patch.lfos.iter().take(MAX_LFOS).enumerate() {
                if !lfo_used[index] {
                    continue;
                }
                // Rate is modulated in **octaves** rather than hertz, so
                // "wobble faster" means the same thing at 1/16 as at 1/2 —
                // three octaves either way at full depth, which is the span
                // between a slow sweep and a growl.
                let octaves = self.lfo_sums.rate[index] * 3.0;
                let live = crate::lfo::LfoLive {
                    depth: (lfo.depth + self.lfo_sums.depth[index]).clamp(0.0, 1.0),
                    phase: lfo.phase + self.lfo_sums.phase[index],
                };
                let rate =
                    crate::lfo::LfoState::rate_hz(lfo, performance.clock.bpm) * 2f32.powf(octaves);
                // A free-running LFO's phase is a fact about the transport,
                // so it is worked out from the clock rather than accumulated:
                // every voice reads the same number and the same bar sounds
                // the same every time it plays.
                let clock_phase = (lfo.mode == crate::patch::LfoMode::Free).then(|| {
                    crate::lfo::free_phase(
                        performance.clock.position_sample + at as u64,
                        sample_rate,
                        rate,
                    )
                });
                self.lfo_values[index] = self.lfos[index].advance_block(
                    lfo,
                    live,
                    rate,
                    sample_rate,
                    n,
                    self.age_samples + at as u64,
                    clock_phase,
                );
            }

            // The generators (`crate::mod_sources`), each only when a route
            // reads it: the attractor and the walk turn on their own, the
            // follower is fed the last step's peak, and a sequencer reads
            // the clock when synced.
            let step_seconds = n as f32 / sample_rate;
            if used.chaos {
                self.generator_values.chaos = self.chaos.advance(patch.chaos.rate_hz, step_seconds);
            }
            if used.walk {
                self.generator_values.walk = self.walk.advance(&patch.walk, step_seconds);
            }
            if used.follower {
                self.generator_values.follower =
                    self.follower.advance(self.step_peak, step_seconds);
            }
            for index in 0..2 {
                if used.sequencers[index] {
                    let clock = (performance.clock.position_sample + at as u64) as f64
                        / f64::from(sample_rate);
                    self.generator_values.sequencers[index] = self.sequencers[index].advance(
                        &patch.sequencers[index],
                        performance.clock.bpm,
                        step_seconds,
                        Some(clock),
                    );
                }
            }
            let generators = self.generator_values;

            // The mod matrix's full view of this voice at this step: the
            // block's numbers, plus the kinds this step has just read.
            // Bound to locals rather than reaching through `self`, so the
            // closure holds no borrow of the voice.
            let lfo_values = self.lfo_values;
            let sources = move |source: crate::mod_matrix::ModSource| match source {
                crate::mod_matrix::ModSource::Chaos => generators.chaos,
                crate::mod_matrix::ModSource::RandomWalk => generators.walk,
                crate::mod_matrix::ModSource::EnvelopeFollower => generators.follower,
                crate::mod_matrix::ModSource::StepSeq(index) => generators
                    .sequencers
                    .get(usize::from(index))
                    .copied()
                    .unwrap_or(0.0),
                // Envelope 0 is the amp envelope. It drives the amp stage
                // directly, and is readable here as well because "louder
                // means brighter" is a route a patch legitimately wants and
                // there is no reason to make it add a second envelope to get
                // it.
                crate::mod_matrix::ModSource::Envelope(0) => amp_level,
                crate::mod_matrix::ModSource::Envelope(index) => {
                    env_levels.get(index as usize - 1).copied().unwrap_or(0.0)
                }
                crate::mod_matrix::ModSource::Lfo(index) => {
                    lfo_values.get(index as usize).copied().unwrap_or(0.0)
                }
                other => scalar(other),
            };
            // The moving routes, on top of the still ones.
            let mut sums = still;
            patch
                .mod_matrix
                .accumulate(true, &sources, &mut |dest, amount| {
                    sums.add(dest, amount, &slot_of)
                });
            self.lfo_sums = sums.lfo;

            // --- the layers, at the first step ---------------------------
            if at == 0 {
                self.prepare_layers(
                    patch,
                    store,
                    tables,
                    sample_rate,
                    quality,
                    bend,
                    &sums,
                    &is_modulator,
                    &mut prepared,
                );
            }

            // --- the destinations, and the ramps to them -----------------
            for (slot, prep) in prepared.iter().enumerate() {
                let Some(prep) = prep else {
                    continue;
                };
                let m = sums.layer[slot];
                // Pitch modulation is in cents, like the tuning it adds to,
                // so a route means the same interval wherever the note sits.
                let semitones = prep.base.semitones + m.pitch_cents / 100.0;
                let pitch_ratio = 2f32.powf(semitones / 12.0);
                // Gain modulation is in decibels, so a tremolo is symmetric
                // in loudness rather than lopsided the way a linear one
                // would be.
                let gain = 10f32.powf((prep.base.gain_db + m.gain_db) / 20.0)
                    * self.velocity_gain
                    * self.layers[slot].window_gain;
                let pan = if stereo {
                    // Three pans, and they add: the zone's own placement
                    // inside the instrument, the note's placement inside the
                    // part, and the part's placement in the mix. Any other
                    // reading throws one of them away — see
                    // `render_with_pan`'s docs.
                    fontelle_types::PanLaw::Minus3Db
                        .gains(prep.base.pan + self.note_pan + channel_pan + m.pan)
                } else {
                    (1.0, 0.0)
                };
                let position = (prep.base.position + m.position).clamp(0.0, 1.0);
                let target = LayerLive {
                    gain,
                    pan,
                    pitch_ratio,
                    position,
                };
                let from = if self.live_fresh {
                    target
                } else {
                    self.live[slot]
                };
                self.live[slot] = from;
                ramp[slot] = LayerLive {
                    gain: (target.gain - from.gain) * inverse_n,
                    pan: (
                        (target.pan.0 - from.pan.0) * inverse_n,
                        (target.pan.1 - from.pan.1) * inverse_n,
                    ),
                    pitch_ratio: (target.pitch_ratio - from.pitch_ratio) * inverse_n,
                    position: (target.position - from.position) * inverse_n,
                };
            }
            // The rest of what the matrix moves on an oscillator, held for
            // the step: how hard its warp is applied, and its stack's
            // detune and blend.
            for (slot, prep) in prepared.iter_mut().enumerate() {
                let Some(PreparedLayer {
                    source: PreparedSource::Synth { osc, .. },
                    base,
                    ..
                }) = prep
                else {
                    continue;
                };
                let m = sums.layer[slot];
                osc.warp_amount = (base.warp + m.warp).clamp(0.0, 1.0);
                osc.unison.detune_cents = (base.detune + m.detune).max(0.0);
                osc.unison.blend = (base.blend + m.blend).clamp(0.0, 1.0);
            }
            // The filters: cutoff modulation is in cents, so it scales the
            // corner rather than shifting it — an octave down means the
            // same thing at 200 Hz as at 8 kHz, which a linear offset would
            // not. Set per step, which is the rhythm the coefficients were
            // already rebuilt at (`FILTER_STEP`); the SVF's zero-delay
            // topology is what makes moving it this often safe.
            for index in 0..2 {
                live_filters[index].cutoff_hz =
                    settings[index].cutoff_hz * 2f32.powf(sums.cutoff_cents[index] / 1200.0);
                live_filters[index].resonance = settings[index].resonance + sums.resonance[index];
                if index == 0 && at == 0 && std::env::var("FONTELLE_DEBUG_CUTOFF").is_ok() {
                    eprintln!(
                        "NEW cutoff {} cents {} lfo {:?}",
                        live_filters[0].cutoff_hz, sums.cutoff_cents[0], self.lfo_values
                    );
                }
                live_filters[index].drive =
                    (settings[index].drive + sums.drive[index]).clamp(0.0, 1.0);
                live_filters[index].character =
                    (settings[index].character + sums.character[index]).clamp(0.0, 1.0);
            }
            // The voice-wide gain the matrix can move, in decibels — one
            // route for a tremolo rather than one per layer.
            let amp_ramp = {
                let target = 10f32.powf(sums.amp_db / 20.0);
                let from = if self.live_fresh {
                    target
                } else {
                    self.amp_live
                };
                self.amp_live = from;
                (target - from) * inverse_n
            };
            self.live_fresh = false;

            // --- the samples ---------------------------------------------
            let mut step_peak = 0.0f32;
            for frame in at..at + n {
                let env = self.amp_env.advance(&amp_env_config, sample_rate);
                self.amp_live += amp_ramp;

                // Four buses, summed before the filters: to F1, to F2, to
                // F1→F2, and around both. Per layer, which is what lets a sub
                // bypass a closed low-pass while the saw above it is being
                // swept.
                let mut to_f1 = (0.0f32, 0.0f32);
                let mut to_f2 = (0.0f32, 0.0f32);
                let mut to_serial = (0.0f32, 0.0f32);
                let mut dry = (0.0f32, 0.0f32);

                // **Last to first.** An oscillator's FM or RM modulator is
                // always a *later* layer (the panel refuses anything else),
                // so walking backwards means the modulator's sample for this
                // frame already exists by the time the layer that reads it
                // is evaluated — with no second pass and no one-sample delay.
                for index in (0..MAX_LAYERS).rev() {
                    let Some(prep) = prepared[index].as_mut() else {
                        continue;
                    };
                    let slot = &mut self.layers[index];
                    if !slot.active {
                        continue;
                    }
                    // This sample's ramped values.
                    let live = &mut self.live[index];
                    let step = ramp[index];
                    live.gain += step.gain;
                    live.pan.0 += step.pan.0;
                    live.pan.1 += step.pan.1;
                    live.pitch_ratio += step.pitch_ratio;
                    live.position += step.position;

                    let (mut sample_l, mut sample_r) = match &mut prep.source {
                        PreparedSource::Sample {
                            data,
                            rate_ratio,
                            loop_end,
                            loop_len,
                            looping,
                            end_offset,
                            interpolation,
                        } => {
                            if *looping {
                                // `while`, not `if`: one subtraction isn't
                                // enough when the playback step exceeds the
                                // loop length, which real extreme upward
                                // transposition of a short loop does.
                                while slot.position >= *loop_end {
                                    slot.position -= *loop_len;
                                }
                            } else if slot.position >= *end_offset {
                                slot.active = false;
                                continue;
                            }
                            let sample =
                                fontelle_dsp::interpolate(data, slot.position, *interpolation);
                            slot.position += f64::from(live.pitch_ratio * *rate_ratio);
                            (sample, sample)
                        }
                        // No end to run off and no buffer to walk: the phase
                        // is the whole of its position, and it advances
                        // itself. The note's own pitch is read off the same
                        // ratio a sample is transposed by: at the default
                        // root of middle C a note plays itself, a higher root
                        // plays it lower, and every tuning on the pitch path
                        // is already in there. Above Nyquist there is no
                        // waveform left to draw, only aliases folding back
                        // down.
                        PreparedSource::Oscillator { kind } => {
                            let freq_hz =
                                (OSC_ROOT_HZ * live.pitch_ratio).clamp(0.0, sample_rate * 0.5);
                            let sample = slot.osc.next_sample(*kind, freq_hz, sample_rate);
                            (sample, sample)
                        }
                        // The hit ends itself. Marking the slot inactive when
                        // it does is what stops a finished drum costing a
                        // `tanh` and a filter per sample for the rest of the
                        // note.
                        PreparedSource::Drum(voice) => {
                            if slot.drum_pending {
                                slot.drum.trigger(voice, sample_rate);
                                slot.drum_pending = false;
                            }
                            if slot.drum.is_done() {
                                slot.active = false;
                                continue;
                            }
                            let sample = slot.drum.next_sample(voice, sample_rate);
                            (sample, sample)
                        }
                        PreparedSource::Synth { osc, input } => {
                            // The modulator's sample from *this* frame,
                            // already computed because the walk is backwards.
                            // A layer naming a modulator that is not there
                            // reads zero, which makes an FM knob with nothing
                            // to modulate a silent knob rather than a broken
                            // one.
                            let modulator = osc
                                .modulator
                                .map(|m| layer_out.get(usize::from(m)).copied().unwrap_or(0.0))
                                .unwrap_or(0.0);
                            osc.position = live.position;
                            slot.synth.next_sample_from(
                                osc,
                                *input,
                                OSC_ROOT_HZ * live.pitch_ratio,
                                sample_rate,
                                modulator,
                            )
                        }
                    };
                    // **Before the level knob**, and before the pan.
                    //
                    // A modulator's level is how much of it you *hear*; the
                    // warp amount is how hard it modulates. Reading the
                    // post-gain sample would fold the two into one knob and
                    // — worse — make a modulator turned down to silence stop
                    // modulating, which is exactly the setup a dedicated FM
                    // operator wants ("FM Growl" and "EP Tine" are both an
                    // oscillator nobody hears). Before the pan for the same
                    // kind of reason: FM by one side of a panned stack is
                    // not a thing anybody means.
                    if prep.layer_index < MAX_LAYERS {
                        layer_out[prep.layer_index] = (sample_l + sample_r) * 0.5;
                    }
                    sample_l *= live.gain;
                    sample_r *= live.gain;

                    let placed = (sample_l * live.pan.0, sample_r * live.pan.1);
                    let bus = match prep.route {
                        fontelle_dsp::FilterRoute::F1 => &mut to_f1,
                        fontelle_dsp::FilterRoute::F2 => &mut to_f2,
                        fontelle_dsp::FilterRoute::Serial => &mut to_serial,
                        fontelle_dsp::FilterRoute::Bypass => &mut dry,
                    };
                    bus.0 += placed.0;
                    bus.1 += placed.1;
                }

                // buses -> Filter1 / Filter2 -> Amp (TDD §7.4). The filters
                // sit ahead of the amp stage and operate on this voice's own
                // mixed sample rather than on the shared output buffer.
                let mut mixed = dry;
                if enabled[0] {
                    mixed.0 += self.filters[0][0].process(to_f1.0, &live_filters[0], sample_rate);
                    if right.is_some() {
                        mixed.1 +=
                            self.filters[0][1].process(to_f1.1, &live_filters[0], sample_rate);
                    }
                } else {
                    mixed.0 += to_f1.0;
                    mixed.1 += to_f1.1;
                }
                // The serial path through its **own** copy of filter 1 — see
                // `Voice::filters`, which is where the third slot is argued.
                let mut serial = to_serial;
                if enabled[0] {
                    serial.0 = self.filters[2][0].process(serial.0, &live_filters[0], sample_rate);
                    if right.is_some() {
                        serial.1 =
                            self.filters[2][1].process(serial.1, &live_filters[0], sample_rate);
                    }
                }
                let into_f2 = (to_f2.0 + serial.0, to_f2.1 + serial.1);
                if enabled[1] {
                    mixed.0 += self.filters[1][0].process(into_f2.0, &live_filters[1], sample_rate);
                    if right.is_some() {
                        mixed.1 +=
                            self.filters[1][1].process(into_f2.1, &live_filters[1], sample_rate);
                    }
                } else {
                    mixed.0 += into_f2.0;
                    mixed.1 += into_f2.1;
                }

                let gain = env * self.amp_live;
                left[frame] += mixed.0 * gain;
                if let Some(right) = right.as_deref_mut() {
                    right[frame] += mixed.1 * gain;
                }
                if used.follower {
                    // The louder side, after the amp: the level a listener
                    // would put a meter on.
                    step_peak = step_peak.max((mixed.0 * gain).abs().max((mixed.1 * gain).abs()));
                }
            }
            self.step_peak = step_peak;
            at += n;
        }

        self.age_samples += frames as u64;
        self.advance_glide(frames, sample_rate);

        let any_layer_active = self.layers.iter().any(|s| s.active);
        if !self.amp_env.is_active() || !any_layer_active {
            self.active = false;
        }
    }

    /// Resolves every active slot's layer into what the sample loop reads —
    /// the source, the base the matrix moves, the bus — once a block.
    ///
    /// `sums` is the first step's, because whether a layer is at the floor
    /// is decided here from the gain the matrix has put on it.
    #[allow(clippy::too_many_arguments)]
    fn prepare_layers<'a>(
        &self,
        patch: &'a crate::Patch,
        store: &'a crate::SampleStore,
        tables: &'a crate::WavetableSet,
        sample_rate: f32,
        quality: fontelle_dsp::Interpolation,
        bend: f32,
        sums: &ModSums,
        is_modulator: &[bool; MAX_LAYERS],
        prepared: &mut [Option<PreparedLayer<'a>>; MAX_LAYERS],
    ) {
        for (prepared_index, slot) in self.layers.iter().enumerate() {
            if !slot.active {
                continue;
            }
            // The slot names its zone; `prepared` is indexed by *slot*, while
            // every `ModDest` is addressed by the zone's own index, because
            // that is what a saved mod route names.
            let index = usize::from(slot.layer);
            let Some(layer) = patch.layers.get(index) else {
                continue;
            };
            let sample_file = match &layer.source {
                crate::patch::Source::Sample { file } => Some(*file),
                crate::patch::Source::Sf2Zone { .. } => continue,
                // None of these names a file, so none has a buffer to look up
                // — see the source match below, which is where they are
                // resolved instead.
                crate::patch::Source::Oscillator(_)
                | crate::patch::Source::Drum(_)
                | crate::patch::Source::Synth(_) => None,
            };
            let buffer = match sample_file {
                Some(file) => match store.get(file) {
                    Some(buffer) => Some(buffer),
                    None => continue,
                },
                None => None,
            };
            let mods = sums.layer[prepared_index];

            // **A layer at the floor is off**, which is what `SILENT_DB` means
            // everywhere else in this program — `Sampler::set_gain_db` reads
            // the same threshold as a gain of exactly zero, and the presets
            // use it to say "this oscillator is not in use" (§6: the Init
            // patch is five layers with one of them up).
            //
            // Skipping it here is the difference between a Flopsynth voice
            // costing one oscillator and costing five: everything in the
            // sample loop — the table read, the unison stack, the pan, the
            // filter feed — is per sample. The one exception is above: a
            // layer somebody modulates with is rendered whatever its level.
            //
            // Resolved per block like the rest of this, so a route that
            // brings a layer up brings it back on the next block.
            if layer.gain_db + mods.gain_db <= crate::SILENT_DB
                && !is_modulator[index.min(MAX_LAYERS - 1)]
            {
                continue;
            }

            // The glide adds to the note's own interval rather than moving
            // the key: the key is the note's *identity*, and a note-off names
            // it (see `glide_to`). The bend adds the same way, and a patch
            // that also *routes* the bend gets both, which is what a route is
            // for.
            let mut base = LayerBase {
                semitones: (self.key as f32 - layer.root_key as f32)
                    + self.glide_semitones
                    + bend * patch.voice_config.bend_range_semitones
                    + self.note_detune
                    + layer.fine_tune_cents / 100.0,
                gain_db: layer.gain_db,
                pan: layer.pan,
                ..LayerBase::default()
            };

            let mut route = fontelle_dsp::FilterRoute::Serial;
            let source = match (&layer.source, buffer) {
                (crate::patch::Source::Oscillator(kind), _) => {
                    PreparedSource::Oscillator { kind: *kind }
                }
                // Before the buffer arm: a drum names no file, so `buffer`
                // is `None` for one and it would otherwise fall through to
                // the `continue` at the bottom and render silence.
                (crate::patch::Source::Drum(voice), _) => PreparedSource::Drum(*voice),
                (crate::patch::Source::Synth(osc), _) => {
                    let mut osc = *osc;
                    // The patch's oversampling, unless this oscillator has
                    // its own: one answer, resolved here rather than in
                    // the oscillator, which never sees the patch.
                    if osc.quality.is_off() {
                        osc.quality = patch.oversampling;
                    }
                    base.position = osc.position;
                    base.warp = osc.warp_amount;
                    base.detune = osc.unison.detune_cents;
                    base.blend = osc.unison.blend;
                    route = osc.filter_route;
                    let input = match osc.source {
                        fontelle_dsp::SynthSource::Table(id) => {
                            tables.get(id).map_or(SynthInput::None, SynthInput::Table)
                        }
                        // One the patch carries itself — see `UserWavetable`.
                        fontelle_dsp::SynthSource::User(at) => tables
                            .get_user(at as usize)
                            .map_or(SynthInput::None, SynthInput::Table),
                        // A recording, by the zone that serves this key —
                        // see `UserSample::zone_for` — or by the one zone
                        // the oscillator is locked to, whatever the key,
                        // which is how one hit of a kit becomes an
                        // instrument. Resolved here rather than per sample
                        // because which zone a note plays is decided when
                        // it starts. A lock on a zone the recording has not
                        // got is silence, like every other thing a patch
                        // names and does not have.
                        fontelle_dsp::SynthSource::Sample(at) => tables
                            .get_sample(at as usize)
                            .and_then(|sample| match osc.sample.zone {
                                Some(locked) => sample.zones.get(usize::from(locked)),
                                None => sample.zone_for_note(self.key, self.velocity),
                            })
                            .map_or(SynthInput::None, |zone| {
                                SynthInput::Sample(fontelle_dsp::SampleData {
                                    samples: &zone.samples,
                                    sample_rate: zone.sample_rate as f32,
                                    root_hz: zone.root_hz(),
                                    // The layer's pin or the session's,
                                    // the same as a sampled layer's read.
                                    interpolation: layer.playback.interpolation.unwrap_or(quality),
                                    gain: 10f32.powf(zone.gain_db / 20.0),
                                    loop_frames: zone.loop_frames,
                                })
                            }),
                        // The same zone choice as a plain read, and the
                        // zone's analysis in place of its audio.
                        fontelle_dsp::SynthSource::Spectral(at) => tables
                            .get_sample(at as usize)
                            .and_then(|sample| {
                                let zone = match osc.sample.zone {
                                    Some(locked) => usize::from(locked),
                                    None => {
                                        let serving =
                                            sample.zone_for_note(self.key, self.velocity)?;
                                        sample
                                            .zones
                                            .iter()
                                            .position(|z| std::ptr::eq(z, serving))?
                                    }
                                };
                                tables.get_spectral(at as usize, zone)
                            })
                            .map_or(SynthInput::None, SynthInput::Spectral),
                        fontelle_dsp::SynthSource::Noise | fontelle_dsp::SynthSource::String => {
                            SynthInput::None
                        }
                    };
                    PreparedSource::Synth { osc, input }
                }
                (_, Some(buffer)) => {
                    let loop_len = layer.playback.loop_end - layer.playback.loop_start;
                    PreparedSource::Sample {
                        data: &buffer.data,
                        rate_ratio: buffer.sample_rate as f32 / sample_rate,
                        loop_end: layer.playback.loop_end,
                        loop_len,
                        // `loop_len > 0.0` also guards the wrap loop in the
                        // sample loop against spinning forever on a
                        // degenerate zero-length loop.
                        looping: matches!(layer.playback.loop_mode, crate::LoopMode::Forward)
                            && loop_len > 0.0,
                        end_offset: layer.playback.end_offset,
                        interpolation: layer.playback.interpolation.unwrap_or(quality),
                    }
                }
                (_, None) => continue,
            };

            prepared[prepared_index] = Some(PreparedLayer {
                source,
                base,
                route,
                layer_index: index,
            });
        }
    }
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

/// A pre-allocated pool sized to `VoiceConfig::polyphony` at `prepare()` time
/// (TDD §7.4) — no allocation on note-on, ever.
pub struct VoicePool {
    voices: Vec<Voice>,
    next_age: u64,
    /// How many of `voices` new notes may use, `1..=voices.len()`.
    ///
    /// **The live polyphony**, which is not the same thing as the pool's size:
    /// `patch/voice/polyphony` is automatable (§12.3), and growing a `Vec` on
    /// the audio thread is exactly what INVARIANT 1 forbids. So the pool keeps
    /// the size it was built at and this moves inside it, which is what a
    /// polyphony limit means anyway — a note that finds nothing free under the
    /// limit steals, exactly as it does when the pool is full.
    ///
    /// The pool is built from the patch, so the knob's value is the ceiling; a
    /// lane can go down from there and back up, and turning the knob rebuilds
    /// the graph and raises it.
    limit: usize,
}

impl VoicePool {
    pub fn with_capacity(capacity: u16) -> Self {
        Self {
            voices: (0..capacity).map(|_| Voice::new()).collect(),
            next_age: 0,
            limit: usize::from(capacity),
        }
    }

    /// How many voices the pool physically has — the ceiling a lane cannot
    /// raise the limit past.
    pub fn capacity(&self) -> usize {
        self.voices.len()
    }

    /// Sets how many voices new notes may use, clamped into the pool.
    ///
    /// Voices already sounding **above** the new limit are left alone rather
    /// than cut: they ring out and their slots come back as they finish, which
    /// is what lowering a polyphony knob does everywhere else. Cutting them
    /// would put a click exactly where somebody was reaching for a swell.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.clamp(1, self.voices.len().max(1));
    }

    pub fn active_count(&self) -> usize {
        self.voices.iter().filter(|v| v.is_active()).count()
    }

    /// Finds a free voice, or steals one per `policy`.
    ///
    /// `Oldest` takes the voice that has sounded longest. `Quietest` takes
    /// the one with the least [`Voice::loudness`] — its amp envelope's level
    /// times its velocity — which on a chord under a soft afterthought is the
    /// afterthought, not the root. `LowestPriority` takes a voice nobody is
    /// holding first (a released tail is the thing least missed), and among
    /// those, or failing any, the quietest. Ties fall to age, never to pool
    /// order: a tie broken by slot would take slot 0 every time, which on a
    /// repeated chord is the same note.
    ///
    /// The last two were `Oldest` under other names from the first build to
    /// v0.9.0 (`docs/flopsynth-next.md` §1.4(9)); `tests/voice_stealing.rs`
    /// holds each to the voice it says it takes.
    pub fn allocate(&mut self, policy: StealPolicy) -> Option<&mut Voice> {
        let age = self.next_age;
        self.next_age = self.next_age.wrapping_add(1);

        // Only the voices under the limit are candidates — see `limit`. A
        // note arriving while the ones above it are still ringing out steals
        // from inside the limit rather than reaching past it, which is what
        // keeps a lowered polyphony *lowered* while the tail of the old
        // setting decays.
        let usable = self.limit.min(self.voices.len());
        let index = match self.voices[..usable].iter().position(|v| !v.is_active()) {
            Some(i) => i,
            None => {
                let candidates = self.voices[..usable].iter().enumerate();
                // A key for `min_by`: the policy's own measure first, age
                // to break the tie. Loudness is a float and is compared
                // as one; a NaN would be a broken voice, not a quiet one.
                let quietest = |v: &Voice| (v.loudness(), v.age);
                match policy {
                    StealPolicy::Oldest => candidates.min_by_key(|(_, v)| v.age),
                    StealPolicy::Quietest => candidates.min_by(|(_, a), (_, b)| {
                        quietest(a)
                            .partial_cmp(&quietest(b))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }),
                    StealPolicy::LowestPriority => candidates.min_by(|(_, a), (_, b)| {
                        (a.is_held(), quietest(a))
                            .partial_cmp(&(b.is_held(), quietest(b)))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    }),
                }
                .map(|(i, _)| i)?
            }
        };

        let voice = self.voices.get_mut(index)?;
        voice.age = age;
        Some(voice)
    }

    /// Finds the voice a note-off for `(key, voice_context)` should end — used
    /// by `Sampler::note_off` (TDD §11.4: voice-context tagging keeps
    /// overlapping clips' note-offs from killing each other's voices).
    ///
    /// **A voice that has already been released is not a candidate, and among
    /// those still held the newest wins.** Both halves were a hung note:
    ///
    /// - Press a key, let go, press it again before the first press has
    ///   finished ringing out, and that key has two voices — one releasing,
    ///   one held. Taking the first in pool order spent the note-off on the
    ///   one that was already released, and the held one was left sounding
    ///   with nothing left that could address it. On a patch that sustains
    ///   that is a drone; low notes reached it first, their tails being the
    ///   longest. Reported as *"it seems to want to often just hold a note
    ///   forever if i spam lower notes"*.
    /// - And when two presses of one key really are both down, the newest is
    ///   the one to let go of, so that a lost note-off strands an old voice
    ///   the steal path will reclaim rather than the one being played now.
    pub fn find_active_mut(&mut self, key: u8, voice_context: u32) -> Option<&mut Voice> {
        self.voices
            .iter_mut()
            .filter(|v| {
                v.is_active() && v.is_held() && v.key() == key && v.voice_context() == voice_context
            })
            .max_by_key(|v| v.age)
    }

    /// Every sounding voice, in pool order.
    pub fn iter_active(&self) -> impl DoubleEndedIterator<Item = &Voice> {
        self.voices.iter().filter(|v| v.is_active())
    }

    pub fn iter_active_mut(&mut self) -> impl Iterator<Item = &mut Voice> {
        self.voices.iter_mut().filter(|v| v.is_active())
    }

    /// Silences every voice at once, including the inactive ones — an inactive
    /// voice still carries the filter and envelope state of whatever it last
    /// played, and that is exactly what a reset is for.
    ///
    /// The age counter is left alone: it only orders voices against each
    /// other, and restarting it would make the first voice allocated after a
    /// reset look older than one allocated before it.
    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.reset();
        }
    }

    /// Silences only the voices the timeline started, leaving live ones
    /// sounding — transport stop and seek (TDD §6.3 against §14).
    ///
    /// A sequenced voice belongs to a moment in the song the playhead has
    /// left. A live voice belongs to a key somebody is still holding, and
    /// stop is a statement about the sequencer rather than about the player.
    pub fn reset_sequenced(&mut self) {
        for voice in &mut self.voices {
            if voice.origin() == fontelle_types::VoiceOrigin::Timeline {
                voice.reset();
            }
        }
    }
}

/// `config` with every stage time the matrix routes to scaled into place.
///
/// `ModDest::EnvelopeStageTime(envelope, stage)` numbers the AHDSR stages
/// from one — attack, hold, decay, sustain, release — and its unit is octaves
/// of time (`ModDest::full_scale`), so a route's value is an exponent on the
/// stored time and a route at zero leaves it exactly alone. Sustain is a level
/// and not a time, so stage four has no entry here.
///
/// Called once per block per envelope, which is what makes reading it from
/// the block-rate sources rather than per sample the right cost: four
/// evaluations over a matrix of a dozen routes, beside a filter.
fn modulated_stages(
    config: fontelle_dsp::EnvelopeConfig,
    envelope: u8,
    matrix: &crate::mod_matrix::ModMatrix,
    sources: &dyn Fn(crate::mod_matrix::ModSource) -> f32,
) -> fontelle_dsp::EnvelopeConfig {
    let scale = |stage: u8, seconds: f32| {
        let dest = crate::mod_matrix::ModDest::EnvelopeStageTime(envelope, stage);
        let octaves = matrix.evaluate(dest, sources);
        if octaves == 0.0 {
            seconds
        } else {
            seconds * 2f32.powf(octaves * dest.full_scale())
        }
    };
    // The sustain is a level, not a time: the whole of its travel at full
    // depth, added. The window has offered *Env n sustain* since the Matrix
    // page existed and nothing read it until 2026-09-20 — a route that
    // lied, which is the one thing a picture may not do.
    let sustain_dest = crate::mod_matrix::ModDest::EnvelopeStageLevel(envelope, 4);
    let sustain = matrix.evaluate(sustain_dest, sources) * sustain_dest.full_scale();
    fontelle_dsp::EnvelopeConfig {
        attack_s: scale(1, config.attack_s),
        hold_s: scale(2, config.hold_s),
        decay_s: scale(3, config.decay_s),
        sustain_level: (config.sustain_level + sustain).clamp(0.0, 1.0),
        release_s: scale(5, config.release_s),
        ..config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::{FilterSlot, Layer, Patch, Source};
    use crate::playback::{LoopMode, PlaybackConfig};
    use crate::streaming::{SampleBuffer, SampleStore};
    use fontelle_dsp::{EnvelopeConfig, EnvelopeCurve, Interpolation, SvfMode};

    const SR: f32 = 48_000.0;

    fn instant_envelope(sustain: f32) -> EnvelopeConfig {
        EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.0,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: sustain,
            release_s: 0.01,
            curve: EnvelopeCurve::Linear,
            ..Default::default()
        }
    }

    fn disabled_filter() -> FilterSlot {
        FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            enabled: false,
            ..Default::default()
        }
    }

    /// A one-shot (non-looping) buffer of constant amplitude `level`, `len` samples
    /// long, at the engine's own sample rate so pitch/rate conversion is 1:1 and
    /// every test assertion is about envelope/gain/range logic, not resampling.
    fn flat_patch(store: &mut SampleStore, level: f32, len: usize, gain_db: f32) -> Patch {
        let asset = store.insert(SampleBuffer {
            data: std::sync::Arc::from(vec![level; len]),
            sample_rate: SR as u32,
        });
        Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Off,
                    interpolation: Some(Interpolation::Draft),
                    end_offset: len as f64,
                    ..PlaybackConfig::default()
                },
                gain_db,
                pan: 0.0,
            }],
            filters: [disabled_filter(), disabled_filter()],
            envelopes: vec![instant_envelope(1.0), instant_envelope(1.0)],
            lfos: Vec::new(),
            mod_matrix: crate::mod_matrix::ModMatrix::default(),
            voice_config: VoiceConfig::default(),
            ..Default::default()
        }
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    #[test]
    fn silent_before_trigger() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let mut voice = Voice::new();
        assert!(!voice.is_active());

        let mut out = vec![0.0; 128];
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
        assert_eq!(rms(&out), 0.0);
    }

    #[test]
    fn triggered_note_in_key_range_produces_sound() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let mut voice = Voice::new();

        voice.trigger(&patch, 60, 127, 0);
        assert!(voice.is_active());

        let mut out = vec![0.0; 128];
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
        assert!(
            rms(&out) > 0.5,
            "expected near-full-scale output, got rms {}",
            rms(&out)
        );
    }

    #[test]
    fn note_outside_key_range_stays_silent() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].key_range = (60, 72);
        let mut voice = Voice::new();

        voice.trigger(&patch, 40, 127, 0);

        let mut out = vec![0.0; 128];
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
        assert_eq!(
            rms(&out),
            0.0,
            "a note outside the layer's key range must produce silence"
        );
    }

    #[test]
    fn gain_db_attenuates_output() {
        let mut store_full = SampleStore::new();
        let patch_full = flat_patch(&mut store_full, 1.0, 1000, 0.0);
        let mut voice_full = Voice::new();
        voice_full.trigger(&patch_full, 60, 127, 0);
        let mut out_full = vec![0.0; 64];
        voice_full.render(
            &patch_full,
            &store_full,
            SR,
            Interpolation::Normal,
            &mut [&mut out_full[..]],
        );

        let mut store_quiet = SampleStore::new();
        let patch_quiet = flat_patch(&mut store_quiet, 1.0, 1000, -6.0);
        let mut voice_quiet = Voice::new();
        voice_quiet.trigger(&patch_quiet, 60, 127, 0);
        let mut out_quiet = vec![0.0; 64];
        voice_quiet.render(
            &patch_quiet,
            &store_quiet,
            SR,
            Interpolation::Normal,
            &mut [&mut out_quiet[..]],
        );

        let ratio = rms(&out_quiet) / rms(&out_full);
        let expected = 10f32.powf(-6.0 / 20.0); // -6dB ~= 0.5012
        assert!(
            (ratio - expected).abs() < 0.01,
            "expected ~{expected} amplitude ratio for -6dB, got {ratio}"
        );
    }

    #[test]
    fn release_fades_out_and_deactivates_the_voice() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 100_000, 0.0);
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);

        // Settle into sustain. `render` documents that it *adds* into `out`
        // rather than clearing it (so multiple voices can mix into one
        // buffer) — a test that wants to inspect a single call's output must
        // clear the buffer itself first, same as any real caller would.
        let mut scratch = vec![0.0; 256];
        for _ in 0..10 {
            scratch.fill(0.0);
            voice.render(
                &patch,
                &store,
                SR,
                Interpolation::Normal,
                &mut [&mut scratch[..]],
            );
        }
        assert!(voice.is_active());

        voice.release();
        // release_s = 0.01s @ 48kHz = 480 samples; render well past that.
        for _ in 0..20 {
            scratch.fill(0.0);
            voice.render(
                &patch,
                &store,
                SR,
                Interpolation::Normal,
                &mut [&mut scratch[..]],
            );
        }
        assert!(
            !voice.is_active(),
            "voice must deactivate once its release finishes"
        );
        assert_eq!(
            rms(&scratch),
            0.0,
            "a fully-released voice must render silence"
        );
    }

    #[test]
    fn forward_loop_keeps_the_voice_alive_past_the_natural_buffer_end() {
        let mut store = SampleStore::new();
        // Buffer shorter than one render block, looped, so a non-looping voice
        // would necessarily go silent partway through this single render call.
        let asset = store.insert(SampleBuffer {
            data: std::sync::Arc::from(vec![1.0; 32]),
            sample_rate: SR as u32,
        });
        let patch = Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Forward,
                    loop_start: 0.0,
                    loop_end: 32.0,
                    end_offset: 32.0,
                    interpolation: Some(Interpolation::Draft),
                    ..PlaybackConfig::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }],
            filters: [disabled_filter(), disabled_filter()],
            envelopes: vec![instant_envelope(1.0), instant_envelope(1.0)],
            lfos: Vec::new(),
            mod_matrix: crate::mod_matrix::ModMatrix::default(),
            voice_config: VoiceConfig::default(),
            ..Default::default()
        };

        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);

        let mut out = vec![0.0; 256]; // 8x the buffer length
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
        assert!(
            voice.is_active(),
            "a Forward-looped voice must not stop at the buffer's natural end"
        );
        assert!(
            rms(&out) > 0.9,
            "looping must keep producing full-scale output, got rms {}",
            rms(&out)
        );
    }

    #[test]
    fn velocity_scales_output_on_the_sf2_default_curve() {
        // Two identical patches, two velocities. SF2's always-present default
        // velocity -> initial-attenuation modulator makes amplitude scale with
        // the square of normalised velocity, so half velocity is roughly a
        // quarter of the amplitude (-12 dB), not half and certainly not the
        // same.
        let render_at = |velocity: u8| {
            let mut store = SampleStore::new();
            let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
            let mut voice = Voice::new();
            voice.trigger(&patch, 60, velocity, 0);
            let mut out = vec![0.0; 64];
            voice.render(
                &patch,
                &store,
                SR,
                Interpolation::Normal,
                &mut [&mut out[..]],
            );
            rms(&out)
        };

        let full = render_at(127);
        let half = render_at(64);
        let ratio = half / full;
        let expected = (64.0f32 / 127.0).powi(2);
        assert!(
            (ratio - expected).abs() < 0.01,
            "velocity 64 against 127 should be ~{expected} of the amplitude, got {ratio}"
        );
    }

    #[test]
    fn full_velocity_is_unity_gain() {
        // The velocity curve must not quietly attenuate everything: 127 is the
        // reference point, so a full-scale sample at velocity 127 and 0 dB
        // layer gain still comes out at full scale.
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut out = vec![0.0; 64];
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Normal,
            &mut [&mut out[..]],
        );
        assert!(
            (rms(&out) - 1.0).abs() < 1e-4,
            "velocity 127 must be unity gain, got {}",
            rms(&out)
        );
    }

    #[test]
    fn velocity_to_gain_spans_the_full_sf2_attenuation_range() {
        assert_eq!(velocity_to_gain(127), 1.0);
        assert!((velocity_to_gain(64) - 0.253_9).abs() < 1e-3);
        assert_eq!(
            velocity_to_gain(0),
            0.0,
            "velocity 0 is a note-off in MIDI and must never make sound"
        );
        assert!(
            velocity_to_gain(1) < 1e-4,
            "the default modulator's 960 cB amount puts velocity 1 ~84 dB down"
        );
    }

    /// A buffer alternating +1/-1: content sitting exactly at Nyquist, which no
    /// lowpass worth the name lets through.
    fn bright_patch(store: &mut SampleStore, len: usize) -> Patch {
        let data: Vec<f32> = (0..len)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let asset = store.insert(SampleBuffer {
            data: std::sync::Arc::from(data),
            sample_rate: SR as u32,
        });
        Patch {
            layers: vec![Layer {
                source: Source::Sample { file: asset },
                key_range: (0, 127),
                vel_range: (0, 127),
                root_key: 60,
                fine_tune_cents: 0.0,
                playback: PlaybackConfig {
                    loop_mode: LoopMode::Off,
                    interpolation: Some(Interpolation::Draft),
                    end_offset: len as f64,
                    ..PlaybackConfig::default()
                },
                gain_db: 0.0,
                pan: 0.0,
            }],
            filters: [disabled_filter(), disabled_filter()],
            envelopes: vec![instant_envelope(1.0), instant_envelope(1.0)],
            lfos: Vec::new(),
            mod_matrix: crate::mod_matrix::ModMatrix::default(),
            voice_config: VoiceConfig::default(),
            ..Default::default()
        }
    }

    /// A steady tone at `freq_hz`, for measuring a rolloff. Unlike the
    /// alternating-sample fixture above this sits *below* Nyquist, where a
    /// bilinear-transform lowpass has a finite response — the bilinear map puts
    /// a double zero exactly at Nyquist, so Nyquist content is annihilated by
    /// the first filter and says nothing about the second.
    fn tone_patch(store: &mut SampleStore, freq_hz: f32, len: usize) -> Patch {
        let data: Vec<f32> = (0..len)
            .map(|i| (std::f32::consts::TAU * freq_hz * i as f32 / SR).sin())
            .collect();
        let asset = store.insert(SampleBuffer {
            data: std::sync::Arc::from(data),
            sample_rate: SR as u32,
        });
        let mut patch = bright_patch(&mut SampleStore::new(), len);
        patch.layers[0].source = Source::Sample { file: asset };
        patch
    }

    fn render_patch(patch: &Patch, store: &SampleStore, len: usize) -> Vec<f32> {
        let mut voice = Voice::new();
        voice.trigger(patch, 60, 127, 0);
        let mut out = vec![0.0; len];
        voice.render(patch, store, SR, Interpolation::Draft, &mut [&mut out[..]]);
        out
    }

    #[test]
    fn an_enabled_lowpass_removes_high_frequency_content() {
        let mut store = SampleStore::new();
        let mut patch = bright_patch(&mut store, 1000);
        let unfiltered = rms(&render_patch(&patch, &store, 256));

        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 500.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
            ..Default::default()
        };
        let filtered = rms(&render_patch(&patch, &store, 256));

        assert!(unfiltered > 0.9, "the fixture should be full scale");
        assert!(
            filtered < unfiltered * 0.05,
            "a 500 Hz lowpass should all but remove Nyquist content: {filtered} vs {unfiltered}"
        );
    }

    #[test]
    fn a_disabled_filter_slot_changes_nothing() {
        // `enabled` has to be a real bypass, not a filter set wide open — an
        // "off" filter that still runs costs CPU on every voice and colours
        // the signal at the top of the band.
        let mut store = SampleStore::new();
        let mut patch = bright_patch(&mut store, 1000);
        let baseline = render_patch(&patch, &store, 256);

        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 500.0,
            resonance: 4.0,
            enabled: false,
            ..Default::default()
        };
        assert_eq!(baseline, render_patch(&patch, &store, 256));
    }

    #[test]
    fn both_filter_slots_are_applied_in_series() {
        let mut store = SampleStore::new();
        // Two octaves above the corner: about -24 dB through one 2-pole
        // section and -48 dB through the pair, both comfortably measurable.
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        let slot = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 1_500.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
            ..Default::default()
        };
        patch.filters[0] = slot;
        let one_pole_pair = rms(&render_patch(&patch, &store, 256)[128..]);
        patch.filters[1] = slot;
        let two_pole_pairs = rms(&render_patch(&patch, &store, 256)[128..]);

        assert!(
            two_pole_pairs < one_pole_pair * 0.5,
            "a second identical lowpass must steepen the rolloff: \
             {two_pole_pairs} vs {one_pole_pair}"
        );
    }

    #[test]
    fn filter_state_does_not_leak_from_the_previous_note() {
        // A voice is reused from the pool, so a note that inherits the last
        // note's filter memory starts with a transient that has nothing to do
        // with it — a click, and one that only appears under voice reuse.
        let mut store = SampleStore::new();
        let mut patch = bright_patch(&mut store, 1000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 800.0,
            resonance: 6.0,
            enabled: true,
            ..Default::default()
        };

        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut first = vec![0.0; 256];
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut first[..]],
        );

        // Same voice, second note.
        voice.trigger(&patch, 60, 127, 1);
        let mut second = vec![0.0; 256];
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut second[..]],
        );

        assert_eq!(
            first, second,
            "a retriggered voice must start from a clean filter, not the last note's tail"
        );
    }

    #[test]
    fn each_voice_filters_only_its_own_contribution() {
        // The same shared-output-buffer trap the amp envelope fell into: the
        // filter must run on this voice's mixed sample, not on whatever is
        // already sitting in `out`.
        let mut store = SampleStore::new();
        let mut patch = bright_patch(&mut store, 1000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 500.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
            ..Default::default()
        };

        let mut alone = vec![0.0; 256];
        let mut voice_a = Voice::new();
        voice_a.trigger(&patch, 60, 127, 0);
        voice_a.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut alone[..]],
        );

        let mut together = vec![0.0; 256];
        let mut first = Voice::new();
        let mut second = Voice::new();
        first.trigger(&patch, 60, 127, 0);
        second.trigger(&patch, 60, 127, 1);
        first.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut together[..]],
        );
        second.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut together[..]],
        );

        for (i, (one, two)) in alone.iter().zip(together.iter()).enumerate() {
            assert!(
                (two - one * 2.0).abs() < 1e-4,
                "sample {i}: two identical voices should sum to twice one, got {two} vs {}",
                one * 2.0
            );
        }
    }

    fn cutoff_route(depth: f32, invert: bool) -> crate::mod_matrix::ModMatrix {
        crate::mod_matrix::ModMatrix {
            routes: vec![crate::mod_matrix::ModRoute {
                source: crate::mod_matrix::ModSource::Velocity,
                destination: crate::mod_matrix::ModDest::FilterCutoff(0),
                depth,
                curve: crate::mod_matrix::Curve::Linear,
                via: None,
                invert,
                bypass: false,
            }],
        }
    }

    /// Brightness independent of loudness: velocity scales amplitude too, so a
    /// raw level comparison would measure the velocity curve rather than the
    /// filter. Dividing by the known velocity gain isolates the cutoff.
    fn brightness_at(patch: &Patch, store: &SampleStore, velocity: u8) -> f32 {
        let mut voice = Voice::new();
        voice.trigger(patch, 60, velocity, 0);
        let mut out = vec![0.0; 512];
        voice.render(patch, store, SR, Interpolation::Draft, &mut [&mut out[..]]);
        rms(&out[256..]) / velocity_to_gain(velocity)
    }

    #[test]
    fn a_velocity_to_cutoff_route_makes_soft_notes_darker() {
        // What every sampler does and Fontelle did not: play quietly and the
        // tone closes down, not just the level.
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 6_000.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
            ..Default::default()
        };
        // SF2's own default: full velocity leaves the cutoff alone, and it
        // falls away as velocity drops.
        patch.mod_matrix = cutoff_route(-0.25, true);

        let loud = brightness_at(&patch, &store, 127);
        let soft = brightness_at(&patch, &store, 20);
        assert!(
            soft < loud * 0.5,
            "a soft note must be audibly darker once the cutoff is modulated: \
             {soft} against {loud}"
        );
    }

    #[test]
    fn without_a_route_velocity_leaves_the_cutoff_alone() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 6_000.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
            ..Default::default()
        };

        let loud = brightness_at(&patch, &store, 127);
        let soft = brightness_at(&patch, &store, 20);
        assert!(
            (soft - loud).abs() < loud * 0.02,
            "with no route, velocity must change loudness only: {soft} against {loud}"
        );
    }

    #[test]
    fn an_uninverted_route_brightens_hard_notes_instead() {
        // The same route without SF2's negative direction: the modulation
        // rises with velocity rather than falling away from full scale, so a
        // hard note opens up past the patch's own cutoff.
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 1_500.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
            ..Default::default()
        };
        patch.mod_matrix = cutoff_route(0.25, false);

        assert!(brightness_at(&patch, &store, 127) > brightness_at(&patch, &store, 20) * 2.0);
    }

    #[test]
    fn cutoff_modulation_is_ignored_when_the_filter_is_off() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.mod_matrix = cutoff_route(-0.25, true);
        assert!(
            (brightness_at(&patch, &store, 20) - brightness_at(&patch, &store, 127)).abs() < 1e-3,
            "a disabled filter slot has no cutoff to modulate"
        );
    }

    // --- Stereo: `Layer::pan` becomes real -----------------------------------

    /// Renders one block of `frames` into a fresh stereo pair.
    fn render_stereo(
        patch: &Patch,
        store: &SampleStore,
        key: u8,
        frames: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let mut voice = Voice::new();
        voice.trigger(patch, key, 127, 0);
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        voice.render(
            patch,
            store,
            SR,
            Interpolation::Draft,
            &mut [&mut left[..], &mut right[..]],
        );
        (left, right)
    }

    #[test]
    fn a_hard_left_layer_is_silent_on_the_right() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].pan = -1.0;

        let (left, right) = render_stereo(&patch, &store, 60, 64);
        assert!(
            (left[0] - 1.0).abs() < 1e-5,
            "hard left keeps the left channel at full scale, got {}",
            left[0]
        );
        assert!(
            right[0].abs() < 1e-5,
            "hard left must silence the right channel, got {}",
            right[0]
        );
    }

    /// SF2 pans a zone on a constant-power taper, so a centred layer reads
    /// 0.707 a side rather than 1.0 — the total power, not the per-channel
    /// level, is what stays put as it sweeps.
    #[test]
    fn a_centred_layer_splits_at_constant_power() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);

        let (left, right) = render_stereo(&patch, &store, 60, 64);
        let expected = std::f32::consts::FRAC_1_SQRT_2;
        assert!((left[0] - expected).abs() < 1e-5, "left {}", left[0]);
        assert!((right[0] - expected).abs() < 1e-5, "right {}", right[0]);
        let power = left[0] * left[0] + right[0] * right[0];
        assert!((power - 1.0).abs() < 1e-5, "total power {power}");
    }

    /// Two layers of one patch pointed opposite ways. **Deliberately unequal
    /// levels:** with both at the same level a layer that ignored its own pan
    /// and took the other's would be indistinguishable from correct.
    #[test]
    fn layers_panned_apart_land_in_different_channels() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let quiet = store.insert(SampleBuffer {
            data: std::sync::Arc::from(vec![0.25; 1000]),
            sample_rate: SR as u32,
        });
        patch.layers[0].pan = -1.0;
        let mut second = patch.layers[0].clone();
        second.source = Source::Sample { file: quiet };
        second.pan = 1.0;
        patch.layers.push(second);

        let (left, right) = render_stereo(&patch, &store, 60, 64);
        assert!(
            (left[0] - 1.0).abs() < 1e-5,
            "the loud layer belongs on the left alone, got {}",
            left[0]
        );
        assert!(
            (right[0] - 0.25).abs() < 1e-5,
            "the quiet layer belongs on the right alone, got {}",
            right[0]
        );
    }

    /// A mono caller has nowhere to pan to. Applying the centre pan-law gain
    /// anyway would drop every mono render 3 dB for no reason it can see —
    /// the same call `MixerTrackNode` makes for a mono track.
    #[test]
    fn a_mono_render_ignores_pan_rather_than_attenuating() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].pan = -1.0;

        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut out = vec![0.0; 64];
        voice.render(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            &mut [&mut out[..]],
        );
        assert!(
            (out[0] - 1.0).abs() < 1e-5,
            "a mono render must carry the layer at full level whatever its pan, got {}",
            out[0]
        );
    }

    #[test]
    fn the_mod_matrix_can_move_a_layers_pan() {
        use crate::mod_matrix::{Curve, ModDest, ModMatrix, ModRoute, ModSource};

        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.mod_matrix = ModMatrix {
            routes: vec![ModRoute {
                source: ModSource::Velocity,
                destination: ModDest::LayerPan(0),
                depth: 1.0,
                curve: Curve::Linear,
                via: None,
                invert: false,
                bypass: false,
            }],
        };

        // Velocity 127 is 1.0 normalised, so a full-depth route sweeps a
        // centred layer the whole way to hard right.
        let (left, right) = render_stereo(&patch, &store, 60, 64);
        assert!(
            left[0].abs() < 1e-4,
            "the route should have emptied the left channel, got {}",
            left[0]
        );
        assert!(
            (right[0] - 1.0).abs() < 1e-4,
            "and filled the right, got {}",
            right[0]
        );
    }

    /// Both channels of a stereo voice must filter independently. Sharing one
    /// filter's state between them makes each channel's output depend on the
    /// other's — an image that collapses and smears the moment the filter is
    /// on and the layers are panned apart.
    #[test]
    fn each_channel_carries_its_own_filter_state() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 4000);
        patch.layers[0].pan = -1.0;
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 1_000.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
            ..Default::default()
        };

        let (_left, right) = render_stereo(&patch, &store, 60, 2048);
        let leak = rms(&right);
        assert!(
            leak < 1e-6,
            "a hard-left voice must stay silent on the right through the filter, got {leak}"
        );
    }

    #[test]
    fn a_channel_pan_places_a_centred_layer() {
        let mut store = SampleStore::new();
        let patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        voice.render_with_pan(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            -1.0,
            &mut [&mut left[..], &mut right[..]],
        );
        assert!((left[0] - 1.0).abs() < 1e-5, "left {}", left[0]);
        assert!(right[0].abs() < 1e-5, "right {}", right[0]);
    }

    /// MIDI CC10 and an SF2 zone's own `pan` generator are two different
    /// controls over one placement, and a soundfont player adds them: a hard-
    /// left zone on a channel panned right should end up somewhere in
    /// between, not at whichever of the two was consulted last.
    #[test]
    fn a_channel_pan_and_a_layer_pan_combine() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].pan = -1.0;
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        voice.render_with_pan(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            1.0,
            &mut [&mut left[..], &mut right[..]],
        );
        let centred = std::f32::consts::FRAC_1_SQRT_2;
        assert!(
            (left[0] - centred).abs() < 1e-5 && (right[0] - centred).abs() < 1e-5,
            "hard left plus hard right is centre, got {} / {}",
            left[0],
            right[0]
        );
    }

    #[test]
    fn a_channel_pan_past_the_ends_of_the_field_clamps() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 1000, 0.0);
        patch.layers[0].pan = -1.0;
        let mut voice = Voice::new();
        voice.trigger(&patch, 60, 127, 0);
        let mut left = vec![0.0; 64];
        let mut right = vec![0.0; 64];
        voice.render_with_pan(
            &patch,
            &store,
            SR,
            Interpolation::Draft,
            -1.0,
            &mut [&mut left[..], &mut right[..]],
        );
        assert!(
            (left[0] - 1.0).abs() < 1e-5 && right[0].abs() < 1e-5,
            "-2.0 of combined pan is still hard left, not past it, got {} / {}",
            left[0],
            right[0]
        );
    }

    // --- Envelopes and LFOs as modulation sources ----------------------------

    use crate::mod_matrix::{Curve, ModDest, ModMatrix, ModRoute, ModSource};

    fn mod_route(source: ModSource, destination: ModDest, depth: f32) -> ModMatrix {
        ModMatrix {
            routes: vec![ModRoute {
                source,
                destination,
                depth,
                curve: Curve::Linear,
                via: None,
                invert: false,
                bypass: false,
            }],
        }
    }

    /// Renders `frames` into one mono buffer, block by block at `block`, the
    /// way the engine drives it — modulation is sampled per block, so a test
    /// that renders one giant buffer would see exactly one modulation value
    /// and prove nothing about anything that moves.
    fn render_blocks(patch: &Patch, store: &SampleStore, frames: usize, block: usize) -> Vec<f32> {
        let mut voice = Voice::new();
        voice.trigger(patch, 60, 127, 0);
        let mut out = vec![0.0; frames];
        let mut at = 0;
        while at < frames {
            let n = block.min(frames - at);
            voice.render(
                patch,
                store,
                SR,
                Interpolation::Draft,
                &mut [&mut out[at..at + n]],
            );
            at += n;
        }
        out
    }

    /// Envelope-to-cutoff is what makes a filter sing, and until the matrix had
    /// an envelope to read it was unreachable: a patch could only be as bright
    /// as its velocity made it, fixed for the length of the note.
    #[test]
    fn a_modulation_envelope_opens_the_filter_over_the_length_of_a_note() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 6_000.0, 48_000);
        patch.filters[0] = FilterSlot {
            mode: SvfMode::Lowpass,
            cutoff_hz: 400.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: true,
            ..Default::default()
        };
        // Envelope 1 is the modulation envelope: a slow attack to full, held
        // there.
        patch.envelopes = vec![
            instant_envelope(1.0),
            EnvelopeConfig {
                delay_s: 0.0,
                attack_s: 0.2,
                hold_s: 0.0,
                decay_s: 0.0,
                sustain_level: 1.0,
                release_s: 0.01,
                curve: EnvelopeCurve::Linear,
                ..Default::default()
            },
        ];
        // Four octaves up at full envelope.
        patch.mod_matrix = mod_route(ModSource::Envelope(1), ModDest::FilterCutoff(0), 0.5);

        let out = render_blocks(&patch, &store, 19_200, 128);
        let early = rms(&out[..2_400]);
        let late = rms(&out[14_400..]);
        assert!(
            late > early * 4.0,
            "the envelope must open the filter as the note develops: {early} \
             at the start against {late} at the end"
        );
    }

    /// The amp envelope is readable as `Envelope(0)` as well as driving the
    /// amp stage, so "louder means brighter" needs no second envelope.
    ///
    /// Measured against the *same patch without the route*, because the amp
    /// envelope raises the level either way: on a single-frequency tone a
    /// lowpass changes amplitude and not shape, so any proxy for brightness is
    /// really a proxy for level, and only the difference between the two
    /// renders isolates what the route did.
    #[test]
    fn the_amp_envelope_is_available_as_a_modulation_source() {
        let swell = EnvelopeConfig {
            delay_s: 0.0,
            attack_s: 0.2,
            hold_s: 0.0,
            decay_s: 0.0,
            sustain_level: 1.0,
            release_s: 0.01,
            curve: EnvelopeCurve::Linear,
            ..Default::default()
        };
        let growth = |routed: bool| {
            let mut store = SampleStore::new();
            let mut patch = tone_patch(&mut store, 6_000.0, 48_000);
            patch.filters[0] = FilterSlot {
                mode: SvfMode::Lowpass,
                cutoff_hz: 400.0,
                resonance: std::f32::consts::FRAC_1_SQRT_2,
                enabled: true,
                ..Default::default()
            };
            patch.envelopes = vec![swell];
            if routed {
                patch.mod_matrix = mod_route(ModSource::Envelope(0), ModDest::FilterCutoff(0), 0.5);
            }
            let out = render_blocks(&patch, &store, 19_200, 128);
            rms(&out[14_400..16_800]) / rms(&out[2_400..4_800]).max(1e-9)
        };

        let (with_route, without) = (growth(true), growth(false));
        assert!(
            with_route > without * 4.0,
            "routing the amp envelope to the cutoff must open the tone well \
             past what the envelope's own level explains: {with_route} against \
             {without}"
        );
    }

    /// Tremolo. An LFO that reaches nothing is a data shape, which is what
    /// `Lfo` was until now.
    #[test]
    fn an_lfo_makes_a_layers_gain_rise_and_fall_at_its_own_rate() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 96_000, 0.0);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 4.0,
            depth: 1.0,
            wave: fontelle_types::LfoWave::Sine,
            delay_s: 0.0,
            ..Default::default()
        }];
        // ±12 dB: unmistakable, and well short of the 96 dB full scale.
        patch.mod_matrix = mod_route(ModSource::Lfo(0), ModDest::LayerGain(0), 12.0 / 96.0);

        // One LFO cycle at 4 Hz is 12 000 samples; the peak is a quarter of the
        // way in and the trough three quarters.
        let out = render_blocks(&patch, &store, 24_000, 128);
        let peak = rms(&out[2_600..3_400]);
        let trough = rms(&out[8_600..9_400]);
        let ratio = peak / trough;
        // 24 dB between them, less whatever the window averages away.
        assert!(
            ratio > 8.0,
            "a ±12 dB tremolo should swing about 16x peak to trough, got {ratio}"
        );
        // And it must come back: a one-way ramp would pass the check above.
        let second_peak = rms(&out[14_600..15_400]);
        assert!(
            (second_peak / peak - 1.0).abs() < 0.1,
            "the LFO must be periodic: {peak} then {second_peak}"
        );
    }

    /// Vibrato. Measured as the pitch itself rather than the level, since a
    /// pitch route that quietly did nothing would still pass a loudness check.
    #[test]
    fn an_lfo_bends_a_layers_pitch_both_ways() {
        let mut store = SampleStore::new();
        let mut patch = tone_patch(&mut store, 1_000.0, 96_000);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 2.0,
            depth: 1.0,
            wave: fontelle_types::LfoWave::Sine,
            delay_s: 0.0,
            ..Default::default()
        }];
        // ±1200 cents: an octave each way, so the zero-crossing count moves
        // far enough to read off a short window.
        patch.mod_matrix = mod_route(ModSource::Lfo(0), ModDest::LayerPitch(0), 1200.0 / 9600.0);

        let out = render_blocks(&patch, &store, 24_000, 128);
        let crossings = |window: &[f32]| {
            window
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        // A 2 Hz LFO cycles in 24 000 samples: sharp a quarter in, flat three
        // quarters in.
        let sharp = crossings(&out[5_600..6_400]);
        let flat = crossings(&out[17_600..18_400]);
        assert!(
            sharp > flat * 3,
            "an octave of vibrato should roughly quadruple the crossing rate \
             between the two extremes: {sharp} against {flat}"
        );
    }

    /// Nothing routed anywhere means nothing moves — and, just as importantly,
    /// nothing is advanced: an envelope no route reads costs a `Vec` emptiness
    /// check rather than a stage advance per sample per voice.
    #[test]
    fn a_patch_with_no_routes_is_unmodulated() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 96_000, 0.0);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 4.0,
            depth: 1.0,
            wave: fontelle_types::LfoWave::Sine,
            delay_s: 0.0,
            ..Default::default()
        }];

        let out = render_blocks(&patch, &store, 24_000, 128);
        let first = rms(&out[2_600..3_400]);
        let later = rms(&out[8_600..9_400]);
        assert!(
            (first - later).abs() < 1e-5,
            "an LFO nothing routes must not reach the output: {first} against {later}"
        );
    }

    /// Vibrato that begins on the note's first sample is the single most
    /// recognisable way a sampled string section sounds synthetic.
    #[test]
    fn an_lfos_delay_holds_it_at_rest_before_it_starts() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 96_000, 0.0);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 4.0,
            depth: 1.0,
            wave: fontelle_types::LfoWave::Sine,
            // Half a second: past the LFO's first two peaks.
            delay_s: 0.5,
            ..Default::default()
        }];
        patch.mod_matrix = mod_route(ModSource::Lfo(0), ModDest::LayerGain(0), 12.0 / 96.0);

        let out = render_blocks(&patch, &store, 48_000, 128);
        // The LFO's first peak is 3 000 samples in and its first trough 9 000.
        let early_peak = rms(&out[2_600..3_400]);
        let early_trough = rms(&out[8_600..9_400]);
        assert!(
            (early_peak / early_trough - 1.0).abs() < 0.05,
            "nothing may move before the delay elapses: {early_peak} against \
             {early_trough}"
        );

        // 0.5 s is 24 000 samples, and the LFO's phase has kept turning: two
        // full cycles at 4 Hz, so it emerges where it would have been anyway.
        let late_peak = rms(&out[26_600..27_400]);
        let late_trough = rms(&out[32_600..33_400]);
        assert!(
            late_peak / late_trough > 8.0,
            "and it must be at full depth after: {late_peak} against {late_trough}"
        );
    }

    /// An LFO used only as a route's `via` still has to turn. It is not the
    /// thing being shaped, so a scan that looked at `source` alone would leave
    /// it parked at rest — and a route scaled by a source that never moves is
    /// a route that never fires.
    #[test]
    fn an_lfo_used_only_to_scale_another_route_still_runs() {
        let mut store = SampleStore::new();
        let mut patch = flat_patch(&mut store, 1.0, 96_000, 0.0);
        patch.lfos = vec![crate::patch::Lfo {
            rate_hz: 4.0,
            depth: 1.0,
            wave: fontelle_types::LfoWave::Sine,
            delay_s: 0.0,
            ..Default::default()
        }];
        patch.mod_matrix = ModMatrix {
            routes: vec![ModRoute {
                source: ModSource::Velocity,
                destination: ModDest::LayerGain(0),
                depth: 12.0 / 96.0,
                curve: Curve::Linear,
                via: Some(ModSource::Lfo(0)),
                invert: false,
                bypass: false,
            }],
        };

        let out = render_blocks(&patch, &store, 24_000, 128);
        let peak = rms(&out[2_600..3_400]);
        let trough = rms(&out[8_600..9_400]);
        assert!(
            peak / trough > 8.0,
            "the via LFO must be running: {peak} against {trough}"
        );
    }
}
