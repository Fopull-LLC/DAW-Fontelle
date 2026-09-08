use fontelle_types::AssetId;

use crate::mod_matrix::ModMatrix;
use crate::playback::PlaybackConfig;
use crate::voice::VoiceConfig;
use fontelle_dsp::{EnvelopeConfig, FilterModel, FilterSlope, OscKind, SynthOsc};

/// A zone/preset identifier inside an SF2 file, assigned by `fontelle-assets` at
/// import time. Opaque here — `fontelle-core` never parses SF2 metadata itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ZoneId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Sf2Zone {
        file: AssetId,
        zone: ZoneId,
    },
    Sample {
        file: AssetId,
    },
    Oscillator(OscKind),
    /// One synthesised drum hit — the built-in drum machine (see
    /// [`crate::drum_kit`]).
    ///
    /// A whole [`fontelle_dsp::DrumVoice`] rather than an index into a table,
    /// for the reason every other source here names its own content: a kit is
    /// **owned** once it is on a channel. A preset writes thirty-six of these
    /// and then has nothing further to say, so every hit stays editable
    /// afterwards and a saved project does not depend on this build still
    /// shipping the preset it came from.
    ///
    /// It names no file, which makes it the one source that never needs
    /// relinking (TDD §17.4) and the one instrument that works on a fresh
    /// install with no bank configured.
    Drum(fontelle_dsp::DrumVoice),
    /// One Flopsynth oscillator — the built-in wavetable synthesiser
    /// (`docs/flopsynth-plan.md` §2.1, and [`crate::flopsynth`]).
    ///
    /// Beside [`Self::Drum`] and for the same reason: a `Copy`, fixed-size
    /// description of one oscillator that a voice slot can hold, **naming no
    /// file**, so it never needs relinking and works on a fresh install. Every
    /// wavetable it can read is generated from a recipe at first use, so there
    /// is nothing for a preset to point at and nothing that can go missing.
    ///
    /// A Flopsynth patch is five of these in a fixed order — A, B, C, Sub,
    /// Noise — but that order is a **convention the window relies on**
    /// ([`crate::flopsynth::layer_role`]) and not a rule the voice enforces: a
    /// patch with a sampled layer appended is still a valid patch and still
    /// plays, which is the seam the hybrid instruments of §12 arrive through.
    Synth(SynthOsc),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub source: Source,
    pub key_range: (u8, u8),
    pub vel_range: (u8, u8),
    pub root_key: u8,
    pub fine_tune_cents: f32,
    pub playback: PlaybackConfig,
    pub gain_db: f32,
    pub pan: f32,
}

/// How an LFO relates to the notes it plays under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LfoMode {
    /// Starts over on every note-on — what the sampler has always done. A
    /// character an instrument can want, and the only one whose result does
    /// not depend on when the note was played.
    #[default]
    Retrigger,
    /// Reads its phase off the transport, so **every voice reads the same
    /// value** and the same bar sounds the same every time it plays. Costs no
    /// shared state, because the clock is the state.
    Free,
    /// Runs one cycle and holds its last value. An envelope with a shape
    /// chooser on it.
    OneShot,
}

impl LfoMode {
    pub const ALL: [Self; 3] = [Self::Retrigger, Self::Free, Self::OneShot];

    pub fn label(self) -> &'static str {
        match self {
            Self::Retrigger => "Retrigger",
            Self::Free => "Free",
            Self::OneShot => "One-shot",
        }
    }
}

/// A named LFO instance. Depth/rate are mod-matrix destinations (TDD §7.5).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Lfo {
    pub rate_hz: f32,
    pub depth: f32,
    /// The shape, from [`fontelle_types::LfoWave`] — **the same six the Filter
    /// insert offers**, so the chooser names its positions once (catalogue
    /// rule 3) and the shape the window draws is the shape the voice plays.
    ///
    /// This replaced an `OscKind`, which is why `PATCH_FORMAT_VERSION` is 1:
    /// the field changed name *and* meaning, and `OscKind::Noise` has no
    /// counterpart here but `SampleHold`, which is a different sound.
    pub wave: fontelle_types::LfoWave,
    /// How long after note-on before the LFO starts, in seconds.
    ///
    /// Not cosmetic: vibrato that begins on the note's first sample is the
    /// single most recognisable way a sampled string section sounds synthetic.
    /// Every real player leans into it, and SF2 has a generator
    /// (`delayVibLFO`) for exactly this.
    pub delay_s: f32,
    /// Whether [`rate_hz`](Self::rate_hz) is in hertz or in bars.
    #[serde(default)]
    pub sync: bool,
    /// The division a synced LFO runs at.
    #[serde(default = "default_division")]
    pub division: fontelle_types::NoteDivision,
    /// How long the depth takes to arrive, **after** the delay.
    ///
    /// The delay is when the vibrato starts; this is how long it takes to get
    /// there. A vibrato that switches on at full depth is as much of a tell as
    /// one that starts on the first sample — see [`Self::delay_s`].
    #[serde(default)]
    pub fade_s: f32,
    /// Where in its cycle a retriggered LFO starts, 0..1.
    #[serde(default)]
    pub phase: f32,
    #[serde(default)]
    pub mode: LfoMode,
    /// A one-pole on the output, 0..1. What turns sample & hold's steps into a
    /// glide and takes the corner off a square.
    #[serde(default)]
    pub smooth: f32,
}

fn default_division() -> fontelle_types::NoteDivision {
    fontelle_types::NoteDivision::Quarter
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FilterSlot {
    pub mode: fontelle_dsp::SvfMode,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub enabled: bool,
    /// Every field below is `#[serde(default)]`, so **every patch written
    /// before Flopsynth reads exactly as it did**: a clean 12 dB filter with
    /// no drive, no key tracking and no character is what the sampler has
    /// always had.
    #[serde(default)]
    pub slope: FilterSlope,
    #[serde(default)]
    pub model: FilterModel,
    /// 0..1 → 0..+24 dB into a `tanh` before the filter.
    #[serde(default)]
    pub drive: f32,
    /// 0..1: how far the corner follows the key, from middle C.
    #[serde(default)]
    pub key_track: f32,
    /// Per model — see [`FilterModel::character_label`].
    #[serde(default)]
    pub character: f32,
}

impl Default for FilterSlot {
    /// **A wire.** Wide open, undamped, clean and off — so a caller filling in
    /// a cutoff gets a filter that does what it asked and nothing else, and so
    /// that a `..Default::default()` in a patch written before Flopsynth adds
    /// no character it did not have.
    fn default() -> Self {
        Self {
            mode: fontelle_dsp::SvfMode::Lowpass,
            cutoff_hz: 20_000.0,
            resonance: std::f32::consts::FRAC_1_SQRT_2,
            enabled: false,
            slope: FilterSlope::Db12,
            model: FilterModel::Clean,
            drive: 0.0,
            key_track: 0.0,
            character: 0.0,
        }
    }
}

impl Default for Lfo {
    /// A slow sine at rest: retriggered, at full depth, with no delay and no
    /// fade. The depth is 1 because an LFO's *level* is the route's business
    /// (`ModRoute::depth`), and an LFO that arrived at zero would be one every
    /// patch had to turn up twice.
    fn default() -> Self {
        Self {
            rate_hz: 5.0,
            depth: 1.0,
            wave: fontelle_types::LfoWave::Sine,
            delay_s: 0.0,
            sync: false,
            division: fontelle_types::NoteDivision::Quarter,
            fade_s: 0.0,
            phase: 0.0,
            mode: LfoMode::Retrigger,
            smooth: 0.0,
        }
    }
}

/// One effect slot inside a patch (`docs/flopsynth-plan.md` §2.2).
///
/// **A Serum preset without its chorus and reverb is not that preset**, so the
/// chain belongs to the instrument rather than to the channel it happens to be
/// on. INVARIANT 4 forbids `fontelle-core` from seeing `fontelle-fx`, but
/// [`EffectConfig`](fontelle_types::EffectConfig) lives in `fontelle-types`,
/// which core already depends on — so the **document** carries the chain here
/// and the **engine**, which depends on both, is what runs it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PatchFx {
    pub config: fontelle_types::EffectConfig,
    pub enabled: bool,
}

/// The most effects one patch may carry.
///
/// Four, and a limit rather than an open list, because the chain runs inside
/// the instrument's node on every block whether or not a note is sounding —
/// see §2.2's argument about the tail keeping the graph awake.
pub const MAX_PATCH_FX: usize = 4;

/// One of a patch's four macro knobs.
///
/// A macro is a **source and nothing else**: its whole meaning is the routes
/// that read it, which is what makes a preset's "Brightness" one knob rather
/// than four. `patch/macro[n]` is automatable, so a preset's own macro becomes
/// a lane without anything being told about it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Macro {
    /// Touched off the RT thread only; the audio thread reads `value`.
    pub name: String,
    pub value: f32,
}

impl Default for Macro {
    fn default() -> Self {
        Self {
            name: String::new(),
            value: 0.0,
        }
    }
}

/// How many macros a patch has. Four, matching the window's row.
pub const MACRO_COUNT: usize = 4;

/// The user's fully-owned instrument definition (TDD §7.2). An SF2 file seeds this
/// once at import; after that it has no live link back to the file's metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    /// The zones, stacked or split by key/velocity.
    ///
    /// **There is no limit here, and there deliberately is not.** A drum kit
    /// is one zone per hit and real ones run to forty-odd — `Nokia_30.sf2`'s
    /// kit has 47. `voice::MAX_LAYERS` bounds how many may sound *at once*
    /// (TDD §7.4), which is a different quantity: a key split is not a stack,
    /// and only the zones covering the note being played take a slot.
    ///
    /// This once said "up to 16", and the voice enforced it against the wrong
    /// thing — it zipped its sixteen slots against this list, so every zone
    /// past index 15 was silently unreachable. That is not a limit anybody
    /// chose; it is `zip` stopping at the shorter side, and it cost three of
    /// the kits in the development bank 30, 37 and 68 keys apiece.
    pub layers: Vec<Layer>,
    pub filters: [FilterSlot; 2],
    /// At least amp + mod envelopes; the user may add more.
    pub envelopes: Vec<EnvelopeConfig>,
    pub lfos: Vec<Lfo>,
    pub mod_matrix: ModMatrix,
    pub voice_config: VoiceConfig,
    /// The instrument's own effects, run after the voice sum and before the
    /// channel's gain — see [`PatchFx`]. `#[serde(default)]` on the stored
    /// side, so every patch written before this reads as "no effects".
    pub fx: Vec<PatchFx>,
    pub macros: [Macro; MACRO_COUNT],
    /// A trim after the voice sum and before the channel's own gain.
    ///
    /// It exists so a bank of presets can be **loudness-matched** without
    /// anybody having to rebalance their layers to do it: the balance inside a
    /// preset is a sound-design decision and how loud the preset is is not.
    pub output_db: f32,
}

/// The **blank instrument**: three oscillators, an amplitude envelope with a
/// little shape on it, and a filter you can open.
///
/// This is what a channel plays when nobody has chosen a soundfont for it, and
/// it exists because of what adding a channel used to feel like — *"when
/// clicking new instrument, right now it doesnt do anything until i select an
/// instrument in the soundfonts tab... then it actually happening later when
/// you werent intending"*. A channel that plays nothing is a channel whose
/// every control does nothing, so the button that makes one looked broken and
/// then looked haunted.
///
/// The choices in it are the ones a default patch has to get right:
///
/// - **A saw, a square and a sine**, in that order, because those are the three
///   a person reaches for and having all three already there is what makes the
///   panel worth opening. The square is an octave down and the sine two, so the
///   three of them are a shape rather than a chorus — and the saw is detuned a
///   few cents against nothing at all, which does nothing on its own and is the
///   knob somebody will find.
/// - **Only the saw is up.** Three oscillators at full level is a default that
///   clips the moment anybody plays a chord; the other two are down at the
///   bottom of their travel, ready to be brought in.
/// - **Headroom.** The saw sits well down, so that a four-note chord at full
///   velocity is still inside full scale with room for the other two
///   oscillators to be brought up. The master's brickwall limiter would catch
///   the overshoot, but a default patch that spends its life in the limiter is
///   a default patch that sounds squashed and nobody can tell why.
/// - **A short attack and release**, not zero: a hard-gated square is a click
///   at each end, and "why does it tick" is not a first impression worth
///   giving.
impl Patch {
    pub fn basic_synth() -> Self {
        let osc = |kind: fontelle_dsp::OscKind, root_key: u8, gain_db: f32, cents: f32| Layer {
            source: Source::Oscillator(kind),
            // Every key, always: an oscillator has no recorded range to run
            // out of, and a blank instrument with holes in the keyboard would
            // be a strange thing to hand somebody.
            key_range: (0, 127),
            vel_range: (0, 127),
            // Middle C, so a note plays its own pitch — see
            // `crate::voice::OSC_ROOT_HZ`. A root an octave *above* this plays
            // an octave down, which is how the sub oscillators are tuned.
            root_key,
            fine_tune_cents: cents,
            playback: PlaybackConfig::default(),
            gain_db,
            pan: 0.0,
        };
        Self {
            layers: vec![
                osc(fontelle_dsp::OscKind::Saw, 60, -14.0, 0.0),
                osc(fontelle_dsp::OscKind::Square, 72, SILENT_DB, 0.0),
                osc(fontelle_dsp::OscKind::Sine, 84, SILENT_DB, 0.0),
            ],
            filters: [
                FilterSlot {
                    mode: fontelle_dsp::SvfMode::Lowpass,
                    // Wide open. A default patch that is already filtered is
                    // one whose cutoff knob only ever brightens by undoing a
                    // choice nobody made.
                    cutoff_hz: 20_000.0,
                    resonance: 0.7,
                    enabled: true,
                    ..Default::default()
                },
                FilterSlot {
                    mode: fontelle_dsp::SvfMode::Highpass,
                    cutoff_hz: 20.0,
                    resonance: 0.7,
                    enabled: false,
                    ..Default::default()
                },
            ],
            // The amp envelope, then one spare for the matrix to route — the
            // same two every imported patch has.
            envelopes: vec![
                fontelle_dsp::EnvelopeConfig {
                    delay_s: 0.0,
                    attack_s: 0.005,
                    hold_s: 0.0,
                    decay_s: 0.0,
                    sustain_level: 1.0,
                    release_s: 0.12,
                    curve: fontelle_dsp::EnvelopeCurve::Decibel,
                    ..Default::default()
                },
                fontelle_dsp::EnvelopeConfig {
                    delay_s: 0.0,
                    attack_s: 0.0,
                    hold_s: 0.0,
                    decay_s: 0.3,
                    sustain_level: 0.0,
                    release_s: 0.1,
                    curve: fontelle_dsp::EnvelopeCurve::Decibel,
                    ..Default::default()
                },
            ],
            lfos: Vec::new(),
            mod_matrix: crate::mod_matrix::ModMatrix::default(),
            voice_config: crate::voice::VoiceConfig::default(),
            fx: Vec::new(),
            macros: Default::default(),
            output_db: 0.0,
        }
    }
}

impl Default for Patch {
    /// **The empty patch**: no layers, both filters off, one amp envelope that
    /// opens and shuts instantly.
    ///
    /// A real state and a reachable one — it is what a channel with its
    /// instrument cleared holds, and what a project saved before the built-in
    /// synth existed opens as (`Session::clear_channel_instrument`). Having it
    /// as `Default` means a caller building a patch says only what is
    /// different about it, which is what keeps a preset recipe readable.
    fn default() -> Self {
        Self {
            layers: Vec::new(),
            filters: [FilterSlot::default(); 2],
            envelopes: vec![EnvelopeConfig::default()],
            lfos: Vec::new(),
            mod_matrix: ModMatrix::default(),
            voice_config: VoiceConfig::default(),
            fx: Vec::new(),
            macros: Default::default(),
            output_db: 0.0,
        }
    }
}

/// The level a layer is **off** at.
///
/// A number rather than an `Option`, because a switched-off oscillator is one
/// whose level knob is at the bottom of its travel and not one that has been
/// removed from the patch — turn it up and it comes back. Sixty decibels down
/// is a thousandth of the amplitude: inaudible under anything, and a real
/// value that serialises, which `-inf` is not (JSON has no infinity, and a
/// patch that saved as `null` would not open).
pub const SILENT_DB: f32 = -60.0;
