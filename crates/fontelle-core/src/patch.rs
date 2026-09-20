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
// `Clone` and not `Copy` since the shape: a `Vec` of points. The voice
// borrows its LFOs (`LfoState::advance_block`) rather than copying them.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// A drawn shape (`docs/flopsynth-next.md` §3.4), played in place of
    /// [`wave`](Self::wave) when there is one. The wave keeps its meaning:
    /// a patch written before the field reads with no shape and plays its
    /// wave, and a patch with no shape writes what it always wrote. The
    /// voice reads it in Phase 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<fontelle_types::LfoShape>,
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
            shape: None,
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
/// Eight (`docs/flopsynth-next.md` §3.6; four until 2026-09-19), and a limit
/// rather than an open list, because the chain runs inside the instrument's
/// node on every block whether or not a note is sounding — see §2.2's
/// argument about the tail keeping the graph awake.
pub const MAX_PATCH_FX: usize = 8;

/// One of a patch's eight macro knobs.
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

/// How many macros a patch has. Eight (`docs/flopsynth-next.md` §4.2, Ty's
/// §9.6; four until 2026-09-20). The file keeps writing four until a fifth
/// is touched — see `patch_format::StoredPatch::macros`.
pub const MACRO_COUNT: usize = 8;

/// One table a patch carries itself, built from a sound rather than from a
/// recipe (`docs/flopsynth-plan.md` §3.2's "no files" is about the *bank*).
///
/// > *"i want to like with omnisphere or serum ... drag audio files into it to
/// > use those waveforms in the synthesis."*
///
/// **The samples live in the patch**, which is the whole design: a preset made
/// from a file opens on a machine that has never seen that file and can never
/// need relinking (TDD §17.4), exactly as `Source::Drum` does for the drum
/// machine. The cost is size — a preset with a table in it is tens of
/// kilobytes rather than two hundred bytes — and that is the right trade for
/// something a person made by hand out of their own sound.
#[derive(Debug, Clone, PartialEq)]
pub struct UserWavetable {
    /// What to call it in the window. The file's stem, usually.
    pub name: String,
    /// How many cycles the samples are cut into — the travel of the position
    /// knob. Clamped to [`fontelle_dsp::MAX_USER_FRAMES`] when it is built.
    pub frames: usize,
    /// Mono, −1..=1. Whatever length the sound was; the table divides it.
    pub samples: Vec<f32>,
}

/// One recording a patch carries, kept whole and played across the keyboard
/// — what [`fontelle_dsp::SynthSource::Sample`] names.
///
/// > *"we could actually sample a real piano sound and then do effects and
/// > modulating and layering with other oscilators and stuff."*
///
/// [`UserWavetable`]'s decision, taken again: **the samples live in the
/// patch**, so a preset made from a recording opens anywhere and never needs
/// relinking. A recording is bigger than a table — seconds rather than
/// cycles — so the zones hold their samples behind an `Arc`: the patch is
/// cloned on every edit and every window refresh, and a clone that copied
/// thirty seconds of audio each time would be felt.
///
/// Several zones, because a sampled instrument is several recordings, one
/// per stretch of the keyboard: one note pitched four octaves away is a
/// chipmunk, and a piano dropped in as a folder of notes should play as one.
#[derive(Debug, Clone, PartialEq)]
pub struct UserSample {
    /// What to call it in the window. The file's stem, or the folder's.
    pub name: String,
    /// Which of the bank's own sets this is, if it is one — in which case
    /// a patch file stores the *name* and not the zones, because the audio
    /// is in the binary (`crate::factory_samples`) and a project with a
    /// piano in it should not be six megabytes of it.
    pub factory: Option<crate::factory_samples::FactorySampleSet>,
    pub zones: Vec<SampleZone>,
}

impl UserSample {
    /// The zone that plays `key`: the one whose range holds it, or failing
    /// that the one whose root is nearest — so two recordings still cover the
    /// whole keyboard rather than leaving holes between them.
    pub fn zone_for(&self, key: u8) -> Option<&SampleZone> {
        self.zones
            .iter()
            .find(|zone| zone.key_range.0 <= key && key <= zone.key_range.1)
            .or_else(|| {
                self.zones
                    .iter()
                    .min_by_key(|zone| (i16::from(zone.root_key) - i16::from(key)).unsigned_abs())
            })
    }

    /// The zone that plays `key` at `velocity`: the one whose key window
    /// **and** velocity window hold it, else the one whose key window does
    /// (a folder's zones cover every velocity), else the nearest root.
    pub fn zone_for_note(&self, key: u8, velocity: u8) -> Option<&SampleZone> {
        self.zones
            .iter()
            .find(|zone| zone.covers(key, velocity))
            .or_else(|| {
                self.zones
                    .iter()
                    .find(|zone| zone.key_range.0 <= key && key <= zone.key_range.1)
            })
            .or_else(|| {
                self.zones
                    .iter()
                    .min_by_key(|zone| (i16::from(zone.root_key) - i16::from(key)).unsigned_abs())
            })
    }
}

/// The keys each of `roots` serves: the whole keyboard, split halfway between
/// neighbouring roots, so every key plays the nearest recording. `roots`
/// must be sorted.
pub fn key_ranges(roots: &[u8]) -> Vec<(u8, u8)> {
    roots
        .iter()
        .enumerate()
        .map(|(i, &root)| {
            // A key exactly halfway goes to the lower neighbour, and the
            // upper one starts on the key after — so the two never claim
            // the same key and never leave one unclaimed.
            let low = if i == 0 {
                0
            } else {
                ((u16::from(roots[i - 1]) + u16::from(root)) / 2 + 1) as u8
            };
            let high = if i + 1 == roots.len() {
                127
            } else {
                ((u16::from(root) + u16::from(roots[i + 1])) / 2) as u8
            };
            (low, high)
        })
        .collect()
}

/// One recording of a [`UserSample`]: the audio, the pitch it was recorded
/// at, and the keys it serves.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleZone {
    /// What the window calls this zone when it lists them — a hit's name in
    /// a kit, the file's stem for a dropped folder. Empty for a zone with
    /// nothing to say, which the window names by its root instead.
    pub name: String,
    /// The key the recording is *of*. A note at this key plays it as it is.
    pub root_key: u8,
    /// How far the recording actually sits from that key, in cents — what a
    /// pitch detector found when the file was dropped. The voice plays the
    /// note this much the other way, so the recording lands in tune.
    pub fine_cents: f32,
    /// Inclusive, both ends.
    pub key_range: (u8, u8),
    pub sample_rate: u32,
    /// Mono, −1..=1.
    pub samples: std::sync::Arc<[f32]>,
    /// The velocities this zone plays, inclusive — an SFZ's `lovel`/`hivel`
    /// (`docs/flopsynth-next.md` §4.3), the whole range for a zone from a
    /// folder or the bank. Two zones over one key with windows that meet
    /// are a soft and a hard recording; see [`UserSample::zone_for_note`].
    pub vel_range: (u8, u8),
    /// A trim on this zone alone, in decibels — an SFZ's `volume`. Nought
    /// for a zone with none.
    pub gain_db: f32,
    /// The zone's own loop, in frames of `samples` — the first and the
    /// **last**, inclusive, as an SFZ writes `loop_start`/`loop_end` under
    /// `loop_continuous` — when the file gave it one. Read
    /// when the oscillator's loop is on and its points are at their whole
    /// travel: the loop the recording asks for, unless somebody has drawn
    /// one.
    pub loop_frames: Option<(u32, u32)>,
}

impl SampleZone {
    /// Whether `key` at `velocity` is inside this zone's windows.
    pub fn covers(&self, key: u8, velocity: u8) -> bool {
        self.key_range.0 <= key
            && key <= self.key_range.1
            && self.vel_range.0 <= velocity
            && velocity <= self.vel_range.1
    }
}

impl SampleZone {
    /// The pitch the recording is at, in hertz.
    pub fn root_hz(&self) -> f32 {
        440.0 * 2f32.powf((f32::from(self.root_key) - 69.0 + self.fine_cents / 100.0) / 12.0)
    }

    /// What a chooser lists this zone as: its name, or its root key's note
    /// when it has none.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            note_name(self.root_key)
        } else {
            self.name.clone()
        }
    }
}

/// A key's name the way the roll writes it: `C4` is middle C. The pitch
/// classes are `fontelle_types::TUNE_ROOTS`, for its reason.
pub fn note_name(key: u8) -> String {
    format!(
        "{}{}",
        fontelle_types::TUNE_ROOTS[usize::from(key % 12)],
        i32::from(key / 12) - 1
    )
}

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
    /// The tables this patch carries itself, named by
    /// [`fontelle_dsp::SynthSource::User`] — see [`UserWavetable`]. Empty for
    /// every patch that reads only the bank, which is every factory preset.
    pub wavetables: Vec<UserWavetable>,
    /// The recordings this patch carries itself, named by
    /// [`fontelle_dsp::SynthSource::Sample`] — see [`UserSample`]. Empty for
    /// every factory preset.
    pub samples: Vec<UserSample>,
    /// How many times over the session's rate the oscillators render and
    /// the ladder runs (`docs/flopsynth-next.md` §4.1). The patch's one
    /// answer: every oscillator at [`fontelle_dsp::Oversampling::Off`]
    /// takes it, and one with its own keeps its own. `Off` for every patch
    /// written before it existed, which is the whole bank, and left out of
    /// the file at `Off` so none of them is rewritten to say so.
    pub oversampling: fontelle_dsp::Oversampling,
    /// The chaos source's settings (`docs/flopsynth-next.md` §4.2). At
    /// rest — and out of the file — until its knob moves; every patch
    /// written before it reads with it at rest.
    pub chaos: crate::mod_sources::Chaos,
    /// The random walk's, the same way.
    pub walk: crate::mod_sources::RandomWalk,
    /// The two step sequencers', the same way.
    pub sequencers: [crate::mod_sources::StepSequencer; 2],
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
            wavetables: Vec::new(),
            samples: Vec::new(),
            oversampling: fontelle_dsp::Oversampling::Off,
            chaos: Default::default(),
            walk: Default::default(),
            sequencers: Default::default(),
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
            wavetables: Vec::new(),
            samples: Vec::new(),
            oversampling: fontelle_dsp::Oversampling::Off,
            chaos: Default::default(),
            walk: Default::default(),
            sequencers: Default::default(),
        }
    }
}

/// A modulation envelope nobody has touched: the shape the Init's filter
/// envelope has — a short attack, a decay to nothing, and the bends an ear
/// expects — so a slot opened for the first time is an envelope and not a
/// flat line at one.
///
/// A fixed value on purpose: the file writes a trailing envelope only when
/// it differs from this (`patch_format`), which is what keeps a row of the
/// bank on disk at the four it was written with.
pub fn envelope_at_rest() -> EnvelopeConfig {
    EnvelopeConfig {
        attack_s: 0.002,
        decay_s: 0.3,
        sustain_level: 0.0,
        release_s: 0.2,
        curve: fontelle_dsp::EnvelopeCurve::Linear,
        decay_shape: -0.6,
        release_shape: -0.6,
        ..Default::default()
    }
}

impl Patch {
    /// Gives a Flopsynth patch every modulator slot the strip shows
    /// (`docs/flopsynth-next.md` §4.2, Ty's §9.6): eight LFOs and six
    /// envelopes, the ones past what the patch carried at rest.
    ///
    /// **The slots are the instrument's, not the patch's** — a Serum patch
    /// does not "have" three LFOs — and a route to the seventh has to be
    /// something a person can make on any patch, not only on one whose
    /// author left room. Called when a patch is read from a file and when
    /// the Init is built; a patch that is not Flopsynth's (an import) keeps
    /// what it has, because its LFOs are a fact about the file it came from.
    pub fn fill_modulator_slots(&mut self) {
        if !crate::flopsynth::is_flopsynth(self) {
            return;
        }
        if self.lfos.len() < crate::voice::MAX_LFOS {
            self.lfos.resize(crate::voice::MAX_LFOS, Lfo::default());
        }
        let envelopes = crate::voice::MAX_MOD_ENVELOPES + 1;
        if self.envelopes.len() < envelopes {
            self.envelopes.resize(envelopes, envelope_at_rest());
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
