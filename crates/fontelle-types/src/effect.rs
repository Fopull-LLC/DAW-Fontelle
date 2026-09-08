//! What an effect *is*, as the document stores it (TDD §13.4).
//!
//! These are parameters, not DSP. They live here rather than in `fontelle-fx`
//! for the reason `PanLaw` does: the document has to name them and may not
//! depend on the effects crate (§4.1), and two parallel copies of the same
//! eight numbers with a translation layer between them is a place for them to
//! drift. `fontelle-fx` holds the filter memory and reads these, the mixer
//! panel draws them, and `project.json` stores them as legible JSON.
//!
//! The rule for every effect that follows: **its config lives here, its state
//! lives in `fontelle-fx`.** That split is what lets a knob move without the
//! audio thread rebuilding anything underneath the sound, and it is the split
//! the limiter already had before there was a chain to put it in.

/// Which effect an insert slot holds.
///
/// A closed enum rather than a plugin registry: v1's effects are the ones in
/// §13.4 and they ship in the binary, so a slot naming one that does not exist
/// is a state the type system can rule out rather than a load error somebody
/// has to handle. M2's plugin hosting adds a variant carrying an identifier;
/// it does not change what these mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EffectKind {
    Utility,
    Eq,
    Filter,
    Compressor,
    Gate,
    Distortion,
    Bitcrush,
    Soften,
    Chorus,
    Delay,
    Reverb,
}

impl EffectKind {
    /// What the insert says on the strip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Utility => "Utility",
            Self::Eq => "EQ",
            Self::Filter => "Filter",
            Self::Compressor => "Comp",
            Self::Gate => "Gate",
            Self::Distortion => "Dist",
            Self::Bitcrush => "Crush",
            Self::Soften => "Soften",
            Self::Chorus => "Chorus",
            Self::Delay => "Delay",
            Self::Reverb => "Reverb",
        }
    }

    /// Whether this effect produces something that belongs *under* the track
    /// rather than in place of it.
    ///
    /// A delay's output is its repeats and a reverb's is its tail — neither
    /// contains the sound that caused it, because `EffectNode` owns the
    /// dry/wet blend for every effect and one that mixed its own dry back in
    /// would be blended twice. So these two open part dry, and every other
    /// effect opens fully wet; see `tests/effect_mix.rs`, which is where that
    /// rule is argued.
    ///
    /// One predicate here rather than a `matches!` in each place that needs
    /// it: the constructor's default, the spec table's, and anything later
    /// that wants to group the menu.
    pub fn is_time_based(self) -> bool {
        matches!(self, Self::Chorus | Self::Delay | Self::Reverb)
    }

    /// Whether this effect has a **detector** — something that listens to a
    /// signal and decides a gain from it — and can therefore be keyed from
    /// another track (`docs/effects-catalogue.md` §2.1).
    ///
    /// One predicate here rather than a `matches!` wherever an external key is
    /// offered, for the reason [`is_time_based`](Self::is_time_based) is one:
    /// the document validates the routing edge, the compiler schedules it and
    /// the window offers it, and three copies of the same list is two to
    /// forget. An effect without a detector has nothing to do with a key, and
    /// one set on it is a routing edge that feeds nothing.
    pub fn takes_key(self) -> bool {
        matches!(self, Self::Compressor | Self::Gate)
    }

    /// Every effect that can be put in an insert slot, in the order the "add"
    /// menu lists them.
    ///
    /// One list rather than two, for the reason the roll's lane properties are
    /// one list: two lists of the same things is one list to forget. It is
    /// also what the menu, the generic effect window and
    /// `fontelle-engine/tests/effects.rs` all iterate, so an effect appears in
    /// all three the moment it appears here.
    ///
    /// The plumbing tool first, then the processors, then the two that sit
    /// under the track.
    pub const ALL: [Self; 11] = [
        Self::Utility,
        Self::Eq,
        Self::Filter,
        Self::Compressor,
        Self::Gate,
        Self::Distortion,
        Self::Bitcrush,
        Self::Soften,
        Self::Chorus,
        Self::Delay,
        Self::Reverb,
    ];
}

/// The parameters of whichever effect a slot holds.
///
/// A sum type rather than a bag of named floats. The bag is what a plugin host
/// needs and what §8's stable-id `ParamSet` is for; an effect that ships in
/// this binary has a shape known at compile time, and giving it one means the
/// mixer panel can draw an EQ curve instead of eight rows of "param 3".
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EffectConfig {
    Utility(UtilityConfig),
    Eq(EqConfig),
    Filter(FilterConfig),
    Compressor(CompressorConfig),
    Gate(GateConfig),
    Distortion(DistortionConfig),
    Bitcrush(BitcrushConfig),
    Soften(SoftenConfig),
    Chorus(ChorusConfig),
    Delay(DelayConfig),
    Reverb(ReverbConfig),
}

impl EffectConfig {
    /// Every parameter this effect has, as §8.2 describes one.
    ///
    /// **This is the list automation works from**, and the reason there is one
    /// list rather than a knob-drawing routine and an automation-target
    /// routine that have to agree: a parameter a person can move and a lane
    /// cannot is exactly the defect PROGRESS.md has recorded eight times, and
    /// here it would be silent — the lane would draw and the sound would not
    /// change.
    pub fn specs(&self) -> &'static [crate::ParamSpec] {
        match self {
            Self::Utility(_) => UTILITY_PARAMS.as_slice(),
            Self::Eq(_) => EQ_PARAMS.as_slice(),
            Self::Filter(_) => FILTER_PARAMS.as_slice(),
            Self::Compressor(_) => COMPRESSOR_PARAMS.as_slice(),
            Self::Gate(_) => GATE_PARAMS.as_slice(),
            Self::Distortion(_) => DISTORTION_PARAMS.as_slice(),
            Self::Bitcrush(_) => BITCRUSH_PARAMS.as_slice(),
            Self::Soften(_) => SOFTEN_PARAMS.as_slice(),
            Self::Chorus(_) => CHORUS_PARAMS.as_slice(),
            Self::Delay(_) => DELAY_PARAMS.as_slice(),
            Self::Reverb(_) => REVERB_PARAMS.as_slice(),
        }
    }

    /// How a panel groups [`specs`](Self::specs): consecutive runs of the
    /// table, each under a heading, in order.
    ///
    /// Fourteen knobs in one grid is a panel nobody can read, and the split
    /// belongs here beside the table rather than in the window, for the
    /// reason the table itself does — one list. An effect with fewer than
    /// about eight controls declares one section, which draws as it always
    /// did. `every_effects_sections_cover_its_parameters_exactly` in
    /// `tests/effect_families.rs` holds the counts to the table.
    pub fn sections(&self) -> &'static [crate::ParamSection] {
        match self {
            Self::Utility(_) => UTILITY_SECTIONS.as_slice(),
            Self::Eq(_) => EQ_SECTIONS.as_slice(),
            Self::Filter(_) => FILTER_SECTIONS.as_slice(),
            Self::Compressor(_) => COMPRESSOR_SECTIONS.as_slice(),
            Self::Gate(_) => GATE_SECTIONS.as_slice(),
            Self::Distortion(_) => DISTORTION_SECTIONS.as_slice(),
            Self::Bitcrush(_) => BITCRUSH_SECTIONS.as_slice(),
            Self::Soften(_) => SOFTEN_SECTIONS.as_slice(),
            Self::Chorus(_) => CHORUS_SECTIONS.as_slice(),
            Self::Delay(_) => DELAY_SECTIONS.as_slice(),
            Self::Reverb(_) => REVERB_SECTIONS.as_slice(),
        }
    }

    // `presets`, `apply_preset` and `matching_preset` were here.
    //
    // They wrote a named starting point into the knobs, and a chip row above
    // the panel drew them. `docs/flopsynth-plan.md` §P.9 took that job away
    // from every device at once: a preset is a **file** now, the bank reads
    // one for whatever device asks, and the preset bar in the window is the
    // same bar over a distortion, a drum kit and a synthesiser. The seven
    // distortions, six crushes and four softens that used to live in this
    // file were exported once by `cargo xtask export-factory-presets` and are
    // committed under `assets/presets/fx-*/`.
    //
    // What is kept from rule 10 is its first half: a preset is a constructor,
    // not a parameter. There is still no "preset" knob and no lane can sweep
    // one. What is replaced is its second half — see §P.6, which is Ty's
    // decision that a device *remembers the name* and *recognises* whether it
    // is still clean.

    fn spec(&self, id: &str) -> Option<&'static crate::ParamSpec> {
        self.specs().iter().find(|spec| spec.id == id)
    }

    /// One parameter's value, in its own units. `None` for an id this effect
    /// does not have.
    pub fn get(&self, id: &str) -> Option<f32> {
        match self {
            Self::Utility(utility) => utility.get(id),
            Self::Eq(eq) => eq.get(id),
            Self::Filter(filter) => filter.get(id),
            Self::Compressor(comp) => comp.get(id),
            Self::Gate(gate) => gate.get(id),
            Self::Distortion(dist) => dist.get(id),
            Self::Bitcrush(crush) => crush.get(id),
            Self::Soften(soften) => soften.get(id),
            Self::Chorus(chorus) => chorus.get(id),
            Self::Delay(delay) => delay.get(id),
            Self::Reverb(reverb) => reverb.get(id),
        }
    }

    /// Writes one, clamped into what it can hold — and onto a step, if it has
    /// steps. An automation curve reaching its top must not be able to produce
    /// a filter frequency of a million.
    pub fn set(&mut self, id: &str, value: f32) {
        let Some(spec) = self.spec(id) else { return };
        let value = spec.clamp(value);
        match self {
            Self::Utility(utility) => utility.set(id, value),
            Self::Eq(eq) => eq.set(id, value),
            Self::Filter(filter) => filter.set(id, value),
            Self::Compressor(comp) => comp.set(id, value),
            Self::Gate(gate) => gate.set(id, value),
            Self::Distortion(dist) => dist.set(id, value),
            Self::Bitcrush(crush) => crush.set(id, value),
            Self::Soften(soften) => soften.set(id, value),
            Self::Chorus(chorus) => chorus.set(id, value),
            Self::Delay(delay) => delay.set(id, value),
            Self::Reverb(reverb) => reverb.set(id, value),
        }
    }

    /// The same value on the 0..1 scale an automation point stores (§12.1).
    pub fn normalised(&self, id: &str) -> Option<f32> {
        let spec = self.spec(id)?;
        Some(spec.normalise(self.get(id)?))
    }

    pub fn set_normalised(&mut self, id: &str, t: f32) {
        let Some(spec) = self.spec(id) else { return };
        self.set(id, spec.denormalise(t));
    }

    pub fn kind(&self) -> EffectKind {
        match self {
            Self::Utility(_) => EffectKind::Utility,
            Self::Eq(_) => EffectKind::Eq,
            Self::Filter(_) => EffectKind::Filter,
            Self::Compressor(_) => EffectKind::Compressor,
            Self::Gate(_) => EffectKind::Gate,
            Self::Distortion(_) => EffectKind::Distortion,
            Self::Bitcrush(_) => EffectKind::Bitcrush,
            Self::Soften(_) => EffectKind::Soften,
            Self::Chorus(_) => EffectKind::Chorus,
            Self::Delay(_) => EffectKind::Delay,
            Self::Reverb(_) => EffectKind::Reverb,
        }
    }

    /// How much of this insert's output is the effect and how much is the
    /// signal that went into it, as a gain from 0 (dry) to 1 (wet).
    ///
    /// Every effect has one — parallel compression is the reason, and a bell
    /// blended back under the dry track is the other. Read as a gain here and
    /// written as a percentage through [`get`](Self::get), because that is
    /// what a knob says and what a lane's read-out shows.
    pub fn mix(&self) -> f32 {
        match self {
            Self::Utility(utility) => utility.mix,
            Self::Eq(eq) => eq.mix,
            Self::Filter(filter) => filter.mix,
            Self::Compressor(comp) => comp.mix,
            Self::Gate(gate) => gate.mix,
            Self::Distortion(dist) => dist.mix,
            Self::Bitcrush(crush) => crush.mix,
            Self::Soften(soften) => soften.mix,
            Self::Chorus(chorus) => chorus.mix,
            Self::Delay(delay) => delay.mix,
            Self::Reverb(reverb) => reverb.mix,
        }
    }

    /// Moves it, clamped to the range a mix can be.
    pub fn set_mix(&mut self, mix: f32) {
        self.set(MIX, mix * 100.0);
    }
}

/// The one parameter every effect has. A `const` because it is written in
/// three places — the two spec tables and the blend on the audio thread — and
/// a typo in any of them would be a knob that silently moved nothing.
pub const MIX: &str = "mix";

/// The spec every effect's table ends with. Percent rather than a bare 0..1
/// because "40" is what a person reads on a wet/dry knob everywhere else.
///
/// The **default** is the effect's, not the control's: an EQ replaces the
/// signal and a reverb sits under it, and one number for both makes one of
/// them wrong. See [`EffectKind::is_time_based`].
const fn mix_param(default: f32) -> crate::ParamSpec {
    crate::ParamSpec {
        id: MIX,
        name: "Mix",
        min: 0.0,
        max: 100.0,
        default,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    }
}

/// A processor is the effect, not half of it.
const ALL_WET: f32 = 100.0;

/// A fresh effect is the effect, not half of it.
fn all_wet() -> f32 {
    1.0
}

impl EffectConfig {
    /// A fresh instance of `kind`, at whatever settings make it audible-but-
    /// harmless: an effect somebody just added should change nothing until
    /// they touch it.
    pub fn new(kind: EffectKind) -> Self {
        match kind {
            EffectKind::Utility => Self::Utility(UtilityConfig::new()),
            EffectKind::Eq => Self::Eq(EqConfig::new()),
            EffectKind::Filter => Self::Filter(FilterConfig::new()),
            EffectKind::Compressor => Self::Compressor(CompressorConfig::new()),
            EffectKind::Gate => Self::Gate(GateConfig::new()),
            EffectKind::Distortion => Self::Distortion(DistortionConfig::new()),
            EffectKind::Bitcrush => Self::Bitcrush(BitcrushConfig::new()),
            EffectKind::Soften => Self::Soften(SoftenConfig::new()),
            EffectKind::Chorus => Self::Chorus(ChorusConfig::new()),
            EffectKind::Delay => Self::Delay(DelayConfig::new()),
            EffectKind::Reverb => Self::Reverb(ReverbConfig::new()),
        }
    }
}

// ----------------------------------------------------------------- utility

/// The plumbing: gain, pan, width, the phase and mute switches, the
/// mono-maker and the rumble filter, in one insert
/// (`docs/effects-catalogue.md` §2.5).
///
/// One effect rather than seven, and that is the design rather than a
/// convenience. Every one of these is a *fix* — the take that came in six
/// decibels hot, the side that was wired backwards, the bass that will not
/// stay in the middle — and a fix is something a person reaches for once,
/// applies, and stops thinking about. Seven entries in the menu would make
/// the commonest thing in mixing the fiddliest.
///
/// The order the controls run in is the order the panel lists them, and it is
/// the order a repair happens in: what arrives is muted and flipped, then the
/// sides are put where they belong, then the image is set, then the bottom is
/// cleaned, then it is placed and levelled. In particular the **mute and
/// invert switches name the channel that arrives**, and the swap happens
/// after them, so "mute L, swap" drops the left signal and moves the right
/// one across.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UtilityConfig {
    /// ±24 dB. The knob this whole effect exists for: a gain staging move
    /// that does not cost the fader, so the fader stays where the mix put it.
    pub gain_db: f32,
    /// −1 (hard left) to +1 (hard right), read out as a percentage.
    ///
    /// A **balance** rather than a pan law: what arrives here is already
    /// stereo, and a constant-power law would attenuate a centred signal by
    /// 3 dB the moment the effect was added. See [`PanLaw::Linear`].
    ///
    /// [`PanLaw::Linear`]: crate::PanLaw::Linear
    pub pan: f32,
    /// Stereo width as a gain on the side signal: 0 is mono, 1 is untouched,
    /// 2 is twice as wide. Read out as 0–200 %.
    pub width: f32,
    /// Below this, the signal is summed to mono — the side signal is
    /// high-passed here and the middle is left alone. 20 Hz is off.
    ///
    /// The reason a mix can be wide and still have a low end that sits: a
    /// bass spread across the image is a bass whose level changes with the
    /// listener's room, and every vinyl cutter mono-ed it anyway.
    pub mono_below_hz: f32,
    /// Left out of the right and right out of the left.
    pub swap: bool,
    pub mute_left: bool,
    pub mute_right: bool,
    /// Polarity, not phase — the sign of the sample, which is what fixes a
    /// pair of microphones or a cable somebody wired backwards.
    pub invert_left: bool,
    pub invert_right: bool,
    /// The DC / rumble filter's corner, in Hz. 5 Hz is off; a notch above it
    /// is a DC blocker, and up at 80 Hz it is what takes a stage's footsteps
    /// off an acoustic guitar.
    pub dc_hz: f32,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`]. It is here because every
    /// effect has one, and it is not useless on this one: a polarity flip at
    /// half mix is a null test, and a width or a gain blended back under the
    /// signal is half of what it says.
    pub mix: f32,
}

impl UtilityConfig {
    /// A wire, exactly: unity gain, centred, unwidened, every switch off and
    /// both filters out of the path.
    pub fn new() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            width: 1.0,
            mono_below_hz: UTILITY_MONO_OFF_HZ,
            swap: false,
            mute_left: false,
            mute_right: false,
            invert_left: false,
            invert_right: false,
            dc_hz: UTILITY_DC_OFF_HZ,
            mix: 1.0,
        }
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "gain" => self.gain_db,
            "pan" => self.pan * 100.0,
            "width" => self.width * 100.0,
            "mono" => self.mono_below_hz,
            "swap" => switch_value(self.swap),
            "mute_l" => switch_value(self.mute_left),
            "mute_r" => switch_value(self.mute_right),
            "invert_l" => switch_value(self.invert_left),
            "invert_r" => switch_value(self.invert_right),
            "dc" => self.dc_hz,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "gain" => self.gain_db = value,
            "pan" => self.pan = value / 100.0,
            "width" => self.width = value / 100.0,
            "mono" => self.mono_below_hz = value,
            "swap" => self.swap = value >= 0.5,
            "mute_l" => self.mute_left = value >= 0.5,
            "mute_r" => self.mute_right = value >= 0.5,
            "invert_l" => self.invert_left = value >= 0.5,
            "invert_r" => self.invert_right = value >= 0.5,
            "dc" => self.dc_hz = value,
            _ => {}
        }
    }
}

impl Default for UtilityConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The bottom of the mono-maker's knob, where it is out of the path. The
/// same 20 Hz the distortion's pre-filter calls off, for the same reason:
/// a crossover at the bottom of hearing has nothing below it to sum.
pub const UTILITY_MONO_OFF_HZ: f32 = 20.0;

/// The bottom of the rumble filter's knob, where it is out of the path.
/// Below the DC blocker's own corner, so the first step off the stop is
/// still a DC blocker rather than a filter somebody has to aim.
pub const UTILITY_DC_OFF_HZ: f32 = 5.0;

/// What a switch reads as. A `const fn` would be nicer; this is a `fn`
/// because it is only ever called from `get`.
fn switch_value(on: bool) -> f32 {
    f32::from(u8::from(on))
}

static UTILITY_PARAMS: [crate::ParamSpec; 11] = with_mix(&UTILITY_OWN_PARAMS, ALL_WET);

/// What the panel reads: where it sits and how loud, how wide it is, what is
/// wrong with the two wires, and what leaves.
static UTILITY_SECTIONS: [crate::ParamSection; 4] = [
    crate::ParamSection {
        name: "Level",
        count: 2,
    },
    crate::ParamSection {
        name: "Stereo",
        count: 3,
    },
    crate::ParamSection {
        name: "Channels",
        count: 4,
    },
    crate::ParamSection {
        name: "Output",
        count: 2,
    },
];

/// An off/on switch, drawn as one and automated as one — see
/// [`crate::Unit::Switch`].
const fn switch_param(id: &'static str, name: &'static str) -> crate::ParamSpec {
    crate::ParamSpec {
        id,
        name,
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
        positions: &OFF_ON,
    }
}

static OFF_ON: [&str; 2] = ["off", "on"];

static UTILITY_OWN_PARAMS: [crate::ParamSpec; 10] = [
    crate::ParamSpec {
        id: "gain",
        name: "Gain",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "pan",
        name: "Pan",
        min: -100.0,
        max: 100.0,
        default: 0.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "width",
        name: "Width",
        min: 0.0,
        max: 200.0,
        default: 100.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "mono",
        name: "Mono below",
        min: UTILITY_MONO_OFF_HZ,
        max: 500.0,
        default: UTILITY_MONO_OFF_HZ,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    switch_param("swap", "Swap L/R"),
    switch_param("mute_l", "Mute L"),
    switch_param("mute_r", "Mute R"),
    switch_param("invert_l", "Invert L"),
    switch_param("invert_r", "Invert R"),
    crate::ParamSpec {
        id: "dc",
        name: "DC / rumble",
        min: UTILITY_DC_OFF_HZ,
        max: 500.0,
        default: UTILITY_DC_OFF_HZ,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
];

/// The EQ's parameters, one row per band per control.
///
/// A `const` table rather than a built `Vec`: the ids are `&'static str` and
/// must be, because they are what a saved automation clip names and INVARIANT
/// 7 says those never change. Written out by a macro so eight bands cannot
/// drift apart from each other.
/// What a band's two choosers are called, position by position — the same
/// words `BandType::label` and `BandChannel::label` give, in the order
/// `BandType::ALL` and `BandChannel::ALL` list them.
///
/// Written out rather than built from those, because a `ParamSpec` is a
/// `static` read on the audio thread and a `const fn` cannot call a method on
/// an enum to build one. `every_stepped_parameter_names_its_positions` in
/// `tests/parameters.rs` is what keeps the two in step.
static BAND_TYPES: [&str; 11] = [
    "bell",
    "low shelf",
    "high shelf",
    "low pass 12",
    "low pass 24",
    "low pass 48",
    "high pass 12",
    "high pass 24",
    "high pass 48",
    "notch",
    "band pass",
];

static BAND_CHANNELS: [&str; 3] = ["stereo", "mid", "side"];

macro_rules! eq_band_params {
    ($($n:literal),*) => {
        [$(
            crate::ParamSpec {
                id: concat!("band", $n, ".freq"),
                name: concat!("Band ", $n, " frequency"),
                min: 20.0,
                max: 20_000.0,
                default: 1_000.0,
                unit: crate::Unit::Hertz,
                // Ratio, not difference: an octave is an octave wherever it
                // sits, and a linear frequency lane spends nine tenths of its
                // travel above 2 kHz.
                taper: crate::Taper::Logarithmic,
                positions: &[],
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".gain"),
                name: concat!("Band ", $n, " gain"),
                min: -24.0,
                max: 24.0,
                default: 0.0,
                unit: crate::Unit::Decibels,
                taper: crate::Taper::Linear,
                positions: &[],
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".q"),
                name: concat!("Band ", $n, " Q"),
                min: 0.1,
                max: 24.0,
                default: BUTTERWORTH_Q,
                unit: crate::Unit::None,
                taper: crate::Taper::Logarithmic,
                positions: &[],
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".on"),
                name: concat!("Band ", $n, " on"),
                min: 0.0,
                max: 1.0,
                default: 0.0,
                unit: crate::Unit::Switch,
                taper: crate::Taper::Stepped(2),
                positions: &[],
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".type"),
                name: concat!("Band ", $n, " type"),
                min: 0.0,
                max: 10.0,
                default: 0.0,
                unit: crate::Unit::None,
                taper: crate::Taper::Stepped(11),
                positions: &BAND_TYPES,
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".channel"),
                name: concat!("Band ", $n, " channel"),
                min: 0.0,
                max: 2.0,
                default: 0.0,
                unit: crate::Unit::None,
                taper: crate::Taper::Stepped(3),
                positions: &BAND_CHANNELS,
            },
        )*]
    };
}

/// Forty-eight of them, and every one addressable — which is what "any value
/// in these mixer effects can become an automation lane" means in practice.
static EQ_BAND_PARAMS: [crate::ParamSpec; 48] = eq_band_params!(1, 2, 3, 4, 5, 6, 7, 8);

/// The bands, and the one control the effect has that is not a band's.
static EQ_PARAMS: [crate::ParamSpec; 49] = with_mix(&EQ_BAND_PARAMS, ALL_WET);

/// Appends the wet/dry to a table at compile time, at the default this effect
/// opens on.
///
/// A `const fn` rather than a `Vec` built at startup, because the ids in a
/// spec are `&'static str` by INVARIANT 7 and the table is a `static` every
/// effect reads on the audio thread.
const fn with_mix<const N: usize, const M: usize>(
    params: &[crate::ParamSpec; N],
    default_mix: f32,
) -> [crate::ParamSpec; M] {
    let mix = mix_param(default_mix);
    let mut out = [mix; M];
    let mut i = 0;
    while i < N {
        out[i] = params[i];
        i += 1;
    }
    out[N] = mix;
    out
}

/// The one-section tables, for the effects whose panel is one grid. The
/// count is the whole table, read off it rather than written twice.
static EQ_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "EQ",
    count: EQ_PARAMS.len(),
}];
static COMPRESSOR_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Comp",
    count: COMPRESSOR_PARAMS.len(),
}];
static SOFTEN_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Soften",
    count: SOFTEN_PARAMS.len(),
}];
static DELAY_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Delay",
    count: DELAY_PARAMS.len(),
}];
static REVERB_SECTIONS: [crate::ParamSection; 1] = [crate::ParamSection {
    name: "Reverb",
    count: REVERB_PARAMS.len(),
}];

// ------------------------------------------------------------ parametric EQ

/// How many bands the EQ has (TDD §13.4).
pub const BANDS: usize = 8;

/// The Q a pass filter's sections are scaled *from*. A band left at this value
/// is exactly Butterworth; above it the corner resonates, below it flattens.
pub const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BandType {
    Bell,
    LowShelf,
    HighShelf,
    LowPass12,
    LowPass24,
    LowPass48,
    HighPass12,
    HighPass24,
    HighPass48,
    Notch,
    BandPass,
}

impl BandType {
    /// How many 2-pole sections in series this band is: one per 12 dB of slope
    /// for the pass filters, one for everything else.
    ///
    /// A document fact rather than a DSP one — it is what "24 dB/oct" *means*
    /// — which is why it is here and the filter mode it maps to is not.
    pub fn sections(self) -> usize {
        match self {
            Self::LowPass24 | Self::HighPass24 => 2,
            Self::LowPass48 | Self::HighPass48 => 4,
            _ => 1,
        }
    }

    /// Whether `gain_db` means anything here: only the three bands that lift
    /// or cut a region rather than removing one.
    pub fn uses_gain(self) -> bool {
        matches!(self, Self::Bell | Self::LowShelf | Self::HighShelf)
    }

    /// Whether this is one of the pass filters, whose Q comes from a
    /// Butterworth cascade rather than straight off the band.
    pub fn is_pass(self) -> bool {
        matches!(
            self,
            Self::LowPass12
                | Self::LowPass24
                | Self::LowPass48
                | Self::HighPass12
                | Self::HighPass24
                | Self::HighPass48
        )
    }

    /// What the band's control says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Bell => "bell",
            Self::LowShelf => "low shelf",
            Self::HighShelf => "high shelf",
            Self::LowPass12 => "low pass 12",
            Self::LowPass24 => "low pass 24",
            Self::LowPass48 => "low pass 48",
            Self::HighPass12 => "high pass 12",
            Self::HighPass24 => "high pass 24",
            Self::HighPass48 => "high pass 48",
            Self::Notch => "notch",
            Self::BandPass => "band pass",
        }
    }

    /// Every band type, in the order a chooser lists them.
    pub const ALL: [Self; 11] = [
        Self::Bell,
        Self::LowShelf,
        Self::HighShelf,
        Self::LowPass12,
        Self::LowPass24,
        Self::LowPass48,
        Self::HighPass12,
        Self::HighPass24,
        Self::HighPass48,
        Self::Notch,
        Self::BandPass,
    ];
}

/// Which part of the stereo image a band works on.
///
/// **Per band, not per EQ**, and that is not a preference — it is the only way
/// the feature can do anything. A filter is linear, so filtering mid and side
/// identically and rotating back gives exactly the signal that filtering left
/// and right would have: `F(M) + F(S) = F(L)`. An EQ-wide "mid/side mode" with
/// the same eight bands either side of the rotation is provably a no-op, which
/// is what this field replaced.
///
/// Left and right as separate targets are a deliberate cut: an EQ with some
/// bands in L/R and some in M/S has no one rotation to run in, and the useful
/// half of the feature is the M/S half.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BandChannel {
    /// Both sides, which is an ordinary EQ band.
    Stereo,
    /// What the two channels have in common — everything panned centre.
    Mid,
    /// What they do not — the width.
    Side,
}

impl BandChannel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stereo => "stereo",
            Self::Mid => "mid",
            Self::Side => "side",
        }
    }

    pub const ALL: [Self; 3] = [Self::Stereo, Self::Mid, Self::Side];
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EqBand {
    pub band_type: BandType,
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
    pub enabled: bool,
    /// Listen to this band's region on its own — see [`EqConfig`].
    pub solo: bool,
    /// Which part of the stereo image this band works on.
    #[serde(default = "stereo")]
    pub channel: BandChannel,
}

fn stereo() -> BandChannel {
    BandChannel::Stereo
}

impl EqBand {
    /// A band that does nothing: a flat bell at 1 kHz, switched off.
    ///
    /// At 1 kHz rather than at zero because a band somebody switches on should
    /// land somewhere they can hear, and because a frequency of zero is not a
    /// filter anybody can build.
    pub fn new() -> Self {
        Self {
            band_type: BandType::Bell,
            freq_hz: 1_000.0,
            gain_db: 0.0,
            q: BUTTERWORTH_Q,
            enabled: false,
            solo: false,
            channel: BandChannel::Stereo,
        }
    }

    /// Whether this band would change the signal at all. A flat bell or shelf
    /// is an identity, and running it costs four multiplies a sample to
    /// produce the input back — with rounding error on top.
    pub fn is_audible(self) -> bool {
        self.enabled && !(self.band_type.uses_gain() && self.gain_db == 0.0)
    }
}

impl Default for EqBand {
    fn default() -> Self {
        Self::new()
    }
}

/// Everything the EQ's sound depends on, and none of its state.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EqConfig {
    pub bands: [EqBand; BANDS],
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`]. Defaulted on load so a
    /// project written before the control existed opens fully wet, which is
    /// the EQ it was saved as.
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl EqConfig {
    pub fn new() -> Self {
        Self {
            bands: [EqBand::new(); BANDS],
            mix: 1.0,
        }
    }

    /// Whether any band is asking to be listened to on its own.
    pub fn soloing(&self) -> bool {
        self.bands.iter().any(|band| band.enabled && band.solo)
    }

    /// Whether the EQ has to work in mid/side at all.
    ///
    /// Only if some band asks: the rotation is exact but not free, and an EQ
    /// whose bands are all plain stereo must be the same wire it would be
    /// without the mode existing.
    pub fn needs_mid_side(&self) -> bool {
        self.bands
            .iter()
            .any(|band| band.is_audible() && band.channel != BandChannel::Stereo)
    }
}

impl Default for EqConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl EqConfig {
    /// Splits `band3.gain` into `(2, "gain")`.
    fn addressed(id: &str) -> Option<(usize, &str)> {
        let (band, field) = id.split_once('.')?;
        let index: usize = band.strip_prefix("band")?.parse().ok()?;
        (1..=BANDS).contains(&index).then_some((index - 1, field))
    }

    fn get(&self, id: &str) -> Option<f32> {
        if id == MIX {
            return Some(self.mix * 100.0);
        }
        let (index, field) = Self::addressed(id)?;
        let band = self.bands[index];
        Some(match field {
            "freq" => band.freq_hz,
            "gain" => band.gain_db,
            "q" => band.q,
            "on" => f32::from(u8::from(band.enabled)),
            "type" => BandType::ALL.iter().position(|t| *t == band.band_type)? as f32,
            "channel" => BandChannel::ALL.iter().position(|c| *c == band.channel)? as f32,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        if id == MIX {
            self.mix = value / 100.0;
            return;
        }
        let Some((index, field)) = Self::addressed(id) else {
            return;
        };
        let band = &mut self.bands[index];
        match field {
            "freq" => band.freq_hz = value,
            "gain" => band.gain_db = value,
            "q" => band.q = value,
            "on" => band.enabled = value >= 0.5,
            "type" => {
                if let Some(kind) = BandType::ALL.get(value.round().max(0.0) as usize) {
                    band.band_type = *kind;
                }
            }
            "channel" => {
                if let Some(channel) = BandChannel::ALL.get(value.round().max(0.0) as usize) {
                    band.channel = *channel;
                }
            }
            _ => {}
        }
    }
}

impl EqBand {
    /// What this band does to a sine at `freq_hz`, in dB.
    ///
    /// The analogue prototype's magnitude, which is what the display wants: a
    /// curve drawn from the same numbers the filter is built from, rather than
    /// from a second idea of what the filter is doing. `fontelle-fx`'s tests
    /// check this against the response the real filter measures, which is the
    /// only way the two can be kept honest — a curve that lies about the sound
    /// is worse than no curve, because it is believed.
    ///
    /// Above roughly a fifth of the sample rate a digital filter's response
    /// departs from its prototype (the bilinear transform warps the frequency
    /// axis), and the display does not model that. It is a display.
    pub fn response_db(self, freq_hz: f32) -> f32 {
        if !self.is_audible() || freq_hz <= 0.0 {
            return 0.0;
        }
        let w = (freq_hz / self.freq_hz.max(1.0)) as f64;
        let q = self.q.max(0.05) as f64;
        let a = 10f64.powf(self.gain_db as f64 / 40.0);

        // Every one of these is the magnitude of the standard 2-pole analogue
        // section, written in terms of `w = f/f0` so the shape is the same at
        // every corner frequency.
        let magnitude = match self.band_type {
            BandType::Bell => {
                // A peaking section: gain A at the centre, unity far away.
                let num = (1.0 - w * w).powi(2) + (w * a / q).powi(2);
                let den = (1.0 - w * w).powi(2) + (w / (a * q)).powi(2);
                (num / den).sqrt()
            }
            BandType::LowShelf | BandType::HighShelf => {
                // The shelves are each other with `w` inverted, so one
                // expression serves both.
                let w = if self.band_type == BandType::LowShelf {
                    w
                } else {
                    1.0 / w.max(1e-9)
                };
                // The RBJ shelf prototype,
                // `H(s) = A(s² + (√A/Q)s + A) / (As² + (√A/Q)s + 1)`,
                // at `s = jw`. Half the gain is in the leading `A` and half in
                // the ratio, which is why `A` here is `10^(dB/40)` and not
                // `10^(dB/20)` — the first draft squared it in the wrong two
                // places and drew a +8 dB shelf as +11.4.
                let root_a = a.sqrt();
                let imaginary = (w * root_a / q).powi(2);
                let num = (a - w * w).powi(2) + imaginary;
                let den = (1.0 - a * w * w).powi(2) + imaginary;
                a * (num / den).sqrt()
            }
            BandType::LowPass12 | BandType::LowPass24 | BandType::LowPass48 => {
                // Butterworth of order 2n: flat to the corner, then n × 12 dB
                // an octave. The cascade's sections are what produce this and
                // the closed form is what draws it.
                let order = 2 * self.band_type.sections();
                (1.0 / (1.0 + w.powi(order as i32 * 2))).sqrt()
            }
            BandType::HighPass12 | BandType::HighPass24 | BandType::HighPass48 => {
                let order = 2 * self.band_type.sections();
                let inverse = 1.0 / w.max(1e-9);
                (1.0 / (1.0 + inverse.powi(order as i32 * 2))).sqrt()
            }
            BandType::Notch => {
                let num = (1.0 - w * w).powi(2);
                let den = num + (w / q).powi(2);
                (num / den).sqrt()
            }
            BandType::BandPass => {
                // Unity at the centre, as `fontelle-fx` normalises it.
                let num = (w / q).powi(2);
                let den = (1.0 - w * w).powi(2) + num;
                (num / den).sqrt()
            }
        };
        20.0 * magnitude.max(1e-9).log10() as f32
    }
}

impl EqConfig {
    /// The whole EQ's response at `freq_hz`, in dB: every audible band added
    /// up, because a chain of filters multiplies and decibels add.
    ///
    /// `channel` picks which of the mid/side curves to draw. A band pointed at
    /// the side is not in the mid's curve, and drawing one line for both would
    /// claim a shape neither of them has.
    pub fn response_db(&self, freq_hz: f32, channel: BandChannel) -> f32 {
        self.bands
            .iter()
            .filter(|band| band.channel == channel || band.channel == BandChannel::Stereo)
            .map(|band| band.response_db(freq_hz))
            .sum()
    }

    /// The curve as the panel draws it: one response per band, summed, over
    /// the stereo path.
    pub fn curve_db(&self, freq_hz: f32) -> f32 {
        self.response_db(freq_hz, BandChannel::Stereo)
    }
}

// ------------------------------------------------------------------ filter

/// What shape the filter is, and how steep
/// (`docs/effects-catalogue.md` §2.2).
///
/// Eight kinds rather than a "type" knob and a separate "slope" knob,
/// because half the combinations do not exist: a notch has one slope and a
/// peak is a boost rather than a cut, and a chooser with a greyed-out
/// neighbour is a chooser that lies.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum FilterShape {
    /// Twelve decibels an octave: one two-pole section. The gentle one, and
    /// the one that leaves a bass note under a sweep.
    LowPass12,
    /// Twenty-four: two sections, the second carrying the resonance. The
    /// synthesiser filter, and the default for that reason.
    #[default]
    LowPass24,
    HighPass12,
    HighPass24,
    /// A band either side of the corner. Narrow at high resonance, and the
    /// shape a wah pedal is.
    BandPass12,
    BandPass24,
    /// The corner removed and everything else left. What takes a hum out
    /// without taking the note with it.
    Notch,
    /// The corner *lifted* rather than cut — the resonance knob is how far
    /// up, to +24 dB. Not an EQ bell: it moves with the envelope and the
    /// LFO, which is what a resonant sweep upward sounds like.
    Peak,
}

impl FilterShape {
    pub fn label(self) -> &'static str {
        match self {
            Self::LowPass12 => "low pass 12",
            Self::LowPass24 => "low pass 24",
            Self::HighPass12 => "high pass 12",
            Self::HighPass24 => "high pass 24",
            Self::BandPass12 => "band pass 12",
            Self::BandPass24 => "band pass 24",
            Self::Notch => "notch",
            Self::Peak => "peak",
        }
    }

    /// How many two-pole sections it is built from.
    pub fn sections(self) -> usize {
        match self {
            Self::LowPass24 | Self::HighPass24 | Self::BandPass24 => 2,
            _ => 1,
        }
    }

    pub const ALL: [Self; 8] = [
        Self::LowPass12,
        Self::LowPass24,
        Self::HighPass12,
        Self::HighPass24,
        Self::BandPass12,
        Self::BandPass24,
        Self::Notch,
        Self::Peak,
    ];
}

static FILTER_SHAPES: [&str; 8] = [
    "low pass 12",
    "low pass 24",
    "high pass 12",
    "high pass 24",
    "band pass 12",
    "band pass 24",
    "notch",
    "peak",
];

/// What an LFO's cycle looks like.
///
/// Its own type rather than the filter's, because the tremolo, the phaser and
/// the flanger all want the same six and rule 3 says a chooser names its
/// positions once (`docs/effects-catalogue.md` §2.4).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum LfoWave {
    #[default]
    Sine,
    Triangle,
    /// Up and then back to the bottom: a ramp, and a rhythmic one.
    SawUp,
    /// The same, falling — a different sound on a filter, because a filter
    /// opening slowly and shutting is not a filter shutting slowly.
    SawDown,
    Square,
    /// A new random value each cycle, held. The one that does not sweep at
    /// all: it jumps, which is a rhythm rather than a movement.
    SampleHold,
}

impl LfoWave {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sine => "sine",
            Self::Triangle => "triangle",
            Self::SawUp => "saw up",
            Self::SawDown => "saw down",
            Self::Square => "square",
            Self::SampleHold => "sample & hold",
        }
    }

    /// The wave's value at `phase` in 0..1, in −1..=1.
    ///
    /// Here rather than in `fontelle-fx` for the reason
    /// [`NoteDivision::beats`] is here: what a saw *is* is a fact about the
    /// chooser's position, and two implementations of it — one for the audio
    /// thread and one for whatever draws the shape — is one of them to get
    /// wrong. [`SampleHold`](Self::SampleHold) is the exception and returns
    /// zero: it has no closed form, because its value is a memory.
    pub fn value(self, phase: f32) -> f32 {
        let phase = phase.rem_euclid(1.0);
        match self {
            Self::Sine => (std::f32::consts::TAU * phase).sin(),
            Self::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
            Self::SawUp => 2.0 * phase - 1.0,
            Self::SawDown => 1.0 - 2.0 * phase,
            Self::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Self::SampleHold => 0.0,
        }
    }

    pub const ALL: [Self; 6] = [
        Self::Sine,
        Self::Triangle,
        Self::SawUp,
        Self::SawDown,
        Self::Square,
        Self::SampleHold,
    ];
}

static LFO_WAVES: [&str; 6] = [
    "sine",
    "triangle",
    "saw up",
    "saw down",
    "square",
    "sample & hold",
];

/// The ends of the cutoff knob.
pub const MIN_FILTER_HZ: f32 = 20.0;
pub const MAX_FILTER_HZ: f32 = 20_000.0;

/// How far the envelope and the LFO can move the cutoff at full amount, in
/// octaves.
///
/// Four each, because an auto-wah that sweeps less than three is a tone
/// control that wobbles, and because four octaves from 200 Hz reaches 3 kHz —
/// which is where a wah's sweep actually ends.
pub const FILTER_MOD_OCTAVES: f32 = 4.0;

/// Everything the filter's sound depends on, and none of its state
/// (`docs/effects-catalogue.md` §2.2).
///
/// The synthesiser filter as an insert, which is a thing a sampler has one of
/// per voice and a mix wants one of per bus. What makes it more than an EQ
/// band is that its corner **moves**: an envelope follower on what is coming
/// in (which is an auto-wah) and an LFO that follows the song (which is
/// everything from a slow sweep to a rhythmic gate).
///
/// The path: in → drive → the filter, at wherever the envelope and the LFO
/// have put it → output gain.
///
/// **No key tracking.** A filter that follows the note needs a note, and an
/// insert on a bus does not have one — the sampler's own filter does, and
/// that is where key tracking lives (§2.2).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FilterConfig {
    pub shape: FilterShape,
    /// Where the corner sits when nothing is moving it, in Hz. At the top of
    /// its range a low-pass is a wire, which is where a fresh one opens.
    pub cutoff_hz: f32,
    /// 0..=1. Q at the corner, and on [`FilterShape::Peak`] the height of the
    /// lift instead.
    pub resonance: f32,
    /// A `tanh` **into** the filter, 0..=1, so resonance can scream without
    /// the bus clipping. Before the filter rather than after, which is where
    /// a synthesiser puts it and the only place it sounds like one: the
    /// filter then takes the harmonics the drive made and sweeps through
    /// them.
    pub drive: f32,
    /// How far a follower on the input moves the corner, −1..=1, where 1 is
    /// [`FILTER_MOD_OCTAVES`] up. Negative sweeps **down** as the signal gets
    /// loud, which is the other half of an auto-wah and the one nobody ships.
    pub env_amount: f32,
    pub env_attack_ms: f32,
    pub env_release_ms: f32,
    /// How far the LFO moves the corner, 0..=1 — a depth either side of
    /// wherever the envelope has left it.
    pub lfo_amount: f32,
    pub lfo_rate_hz: f32,
    /// Take the LFO's rate from the song's tempo: one cycle per
    /// [`lfo_division`](Self::lfo_division).
    pub lfo_sync: bool,
    pub lfo_division: NoteDivision,
    pub lfo_wave: LfoWave,
    /// Trim after the drive, in dB. Rule 4: anything nonlinear has a gain
    /// either side of it, because otherwise the drive knob is a volume.
    pub output_db: f32,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`].
    pub mix: f32,
}

impl FilterConfig {
    /// A 24 dB low-pass wide open: a wire, until the one knob a person came
    /// here for is moved.
    pub fn new() -> Self {
        Self {
            shape: FilterShape::LowPass24,
            cutoff_hz: MAX_FILTER_HZ,
            resonance: 0.0,
            drive: 0.0,
            env_amount: 0.0,
            env_attack_ms: 5.0,
            env_release_ms: 200.0,
            lfo_amount: 0.0,
            lfo_rate_hz: 1.0,
            lfo_sync: false,
            lfo_division: NoteDivision::Quarter,
            lfo_wave: LfoWave::Sine,
            output_db: 0.0,
            mix: 1.0,
        }
    }

    /// The LFO's rate at `bpm` — the same seam as
    /// [`ChorusConfig::effective_rate_hz`], and in the same place.
    pub fn effective_lfo_hz(&self, bpm: f32) -> f32 {
        let asked = if self.lfo_sync {
            let bpm = if bpm.is_finite() { bpm } else { 0.0 };
            let seconds = self.lfo_division.beats() * 60.0 / bpm.clamp(MIN_BPM, MAX_BPM);
            1.0 / seconds.max(1e-4)
        } else {
            self.lfo_rate_hz
        };
        asked.clamp(MIN_LFO_RATE_HZ, MAX_LFO_RATE_HZ)
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "shape" => FilterShape::ALL.iter().position(|s| *s == self.shape)? as f32,
            "cutoff" => self.cutoff_hz,
            "resonance" => self.resonance * 100.0,
            "drive" => self.drive * 100.0,
            "env" => self.env_amount * 100.0,
            "env_attack" => self.env_attack_ms,
            "env_release" => self.env_release_ms,
            "lfo" => self.lfo_amount * 100.0,
            "lfo_rate" => self.lfo_rate_hz,
            "lfo_sync" => f32::from(u8::from(self.lfo_sync)),
            "lfo_division" => NoteDivision::ALL
                .iter()
                .position(|d| *d == self.lfo_division)? as f32,
            "lfo_wave" => LfoWave::ALL.iter().position(|w| *w == self.lfo_wave)? as f32,
            "output" => self.output_db,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "shape" => {
                if let Some(shape) = FilterShape::ALL.get(value.round().max(0.0) as usize) {
                    self.shape = *shape;
                }
            }
            "cutoff" => self.cutoff_hz = value,
            "resonance" => self.resonance = value / 100.0,
            "drive" => self.drive = value / 100.0,
            "env" => self.env_amount = value / 100.0,
            "env_attack" => self.env_attack_ms = value,
            "env_release" => self.env_release_ms = value,
            "lfo" => self.lfo_amount = value / 100.0,
            "lfo_rate" => self.lfo_rate_hz = value,
            "lfo_sync" => self.lfo_sync = value >= 0.5,
            "lfo_division" => {
                if let Some(division) = NoteDivision::ALL.get(value.round().max(0.0) as usize) {
                    self.lfo_division = *division;
                }
            }
            "lfo_wave" => {
                if let Some(wave) = LfoWave::ALL.get(value.round().max(0.0) as usize) {
                    self.lfo_wave = *wave;
                }
            }
            "output" => self.output_db = value,
            _ => {}
        }
    }
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self::new()
    }
}

static FILTER_PARAMS: [crate::ParamSpec; 14] = with_mix(&FILTER_OWN_PARAMS, ALL_WET);

/// What the panel reads: the filter, then the two things that move it, then
/// what leaves.
static FILTER_SECTIONS: [crate::ParamSection; 4] = [
    crate::ParamSection {
        name: "Filter",
        count: 4,
    },
    crate::ParamSection {
        name: "Envelope",
        count: 3,
    },
    crate::ParamSection {
        name: "LFO",
        count: 5,
    },
    crate::ParamSection {
        name: "Output",
        count: 2,
    },
];

static FILTER_OWN_PARAMS: [crate::ParamSpec; 13] = [
    crate::ParamSpec {
        id: "shape",
        name: "Shape",
        min: 0.0,
        max: 7.0,
        default: 1.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(8),
        positions: &FILTER_SHAPES,
    },
    crate::ParamSpec {
        id: "cutoff",
        name: "Cutoff",
        min: MIN_FILTER_HZ,
        max: MAX_FILTER_HZ,
        default: MAX_FILTER_HZ,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    percent_param("resonance", "Resonance", 0.0),
    percent_param("drive", "Drive", 0.0),
    crate::ParamSpec {
        id: "env",
        name: "Env amount",
        // Signed, and that is the half of an auto-wah nobody ships: a filter
        // that *closes* as the signal gets loud is a duck with a tone.
        min: -100.0,
        max: 100.0,
        default: 0.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "env_attack",
        name: "Env attack",
        min: 0.1,
        max: 500.0,
        default: 5.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "env_release",
        name: "Env release",
        min: 5.0,
        max: 2_000.0,
        default: 200.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    percent_param("lfo", "LFO amount", 0.0),
    crate::ParamSpec {
        id: "lfo_rate",
        name: "LFO rate",
        min: MIN_LFO_RATE_HZ,
        max: MAX_LFO_RATE_HZ,
        default: 1.0,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "lfo_sync",
        name: "LFO sync",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
        positions: &["off", "on"],
    },
    crate::ParamSpec {
        id: "lfo_division",
        name: "LFO division",
        min: 0.0,
        max: (DIVISIONS.len() - 1) as f32,
        // A quarter note, which is the sweep a person means by "in time".
        // The literal is `NoteDivision::ALL`'s index for it, the way the
        // delay's eighth is — a `const fn` cannot search an array of enums.
        default: 5.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(DIVISIONS.len() as u32),
        positions: &DIVISIONS,
    },
    crate::ParamSpec {
        id: "lfo_wave",
        name: "LFO wave",
        min: 0.0,
        max: 5.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(6),
        positions: &LFO_WAVES,
    },
    crate::ParamSpec {
        id: "output",
        name: "Output",
        min: -24.0,
        max: 12.0,
        default: 0.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
];

// -------------------------------------------------------------- compressor

/// How the detector measures the signal it is deciding about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DetectionMode {
    /// The instantaneous level. Catches transients, and reads a sine 3 dB
    /// louder than its RMS does.
    Peak,
    /// A short average. Closer to how loud something *sounds*, and what a
    /// compressor put on a whole mix usually wants.
    Rms,
}

impl DetectionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Peak => "peak",
            Self::Rms => "RMS",
        }
    }

    pub const ALL: [Self; 2] = [Self::Peak, Self::Rms];
}

/// Everything the compressor's sound depends on, and none of its state
/// (TDD §13.4).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CompressorConfig {
    pub threshold_db: f32,
    /// `1.0` is a wire. The top of the range is limiting in all but name.
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    /// The width, in dB, of the region around the threshold where the curve
    /// bends rather than corners. `0.0` is a hard knee.
    pub knee_db: f32,
    pub makeup_db: f32,
    /// Put back what the threshold takes away, so moving the threshold does
    /// not also move the level.
    pub auto_makeup: bool,
    pub detection: DetectionMode,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`]. Parallel ("New York")
    /// compression is this knob and nothing else: the squashed signal under
    /// the one that still has its transients.
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl CompressorConfig {
    /// A compressor that does nothing until it is set: 1:1, which is a wire.
    ///
    /// The same rule the EQ's every-band-off default follows — an effect
    /// somebody just dropped on a track must not move the mix.
    pub fn new() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 1.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            knee_db: 6.0,
            makeup_db: 0.0,
            auto_makeup: false,
            detection: DetectionMode::Peak,
            mix: 1.0,
        }
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "threshold" => self.threshold_db,
            "ratio" => self.ratio,
            "attack" => self.attack_ms,
            "release" => self.release_ms,
            "knee" => self.knee_db,
            "makeup" => self.makeup_db,
            "auto" => f32::from(u8::from(self.auto_makeup)),
            "detection" => DetectionMode::ALL
                .iter()
                .position(|m| *m == self.detection)? as f32,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "threshold" => self.threshold_db = value,
            "ratio" => self.ratio = value,
            "attack" => self.attack_ms = value,
            "release" => self.release_ms = value,
            "knee" => self.knee_db = value,
            "makeup" => self.makeup_db = value,
            "auto" => self.auto_makeup = value >= 0.5,
            "detection" => {
                if let Some(mode) = DetectionMode::ALL.get(value.round().max(0.0) as usize) {
                    self.detection = *mode;
                }
            }
            _ => {}
        }
    }
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Eight of them, every one addressable.
static COMPRESSOR_PARAMS: [crate::ParamSpec; 9] = with_mix(&COMPRESSOR_OWN_PARAMS, ALL_WET);

/// The compressor's own eight, before the mix every effect has is put on the
/// end.
static COMPRESSOR_OWN_PARAMS: [crate::ParamSpec; 8] = [
    crate::ParamSpec {
        id: "threshold",
        name: "Threshold",
        min: -60.0,
        max: 0.0,
        default: -18.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "ratio",
        name: "Ratio",
        min: 1.0,
        max: 20.0,
        default: 1.0,
        unit: crate::Unit::Ratio,
        // A ratio is a ratio: the step from 2:1 to 4:1 is the same musical
        // move as the one from 8:1 to 16:1, and a linear lane would put every
        // useful setting in its bottom fifth.
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "attack",
        name: "Attack",
        min: 0.05,
        max: 500.0,
        default: 10.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "release",
        name: "Release",
        min: 5.0,
        max: 5_000.0,
        default: 100.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "knee",
        name: "Knee",
        min: 0.0,
        max: 24.0,
        default: 6.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "makeup",
        name: "Makeup",
        min: -12.0,
        max: 24.0,
        default: 0.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "auto",
        name: "Auto makeup",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
        positions: &["off", "on"],
    },
    crate::ParamSpec {
        id: "detection",
        name: "Detection",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(2),
        // Peak follows the waveform and RMS follows its energy: the difference
        // between catching a snare's transient and riding a vocal's level, and
        // it read as "0.00" until a spec could say so.
        positions: &["Peak", "RMS"],
    },
];

// -------------------------------------------------------------------- gate

/// The top of the look-ahead knob, in milliseconds. Ten is the useful end:
/// past it a gate is no longer opening early, it is playing late.
pub const MAX_GATE_LOOKAHEAD_MS: f32 = 10.0;

/// The bottom of the range knob and of the threshold, in dB. Far enough down
/// that "closed" is silence and "never" is a threshold nothing reaches.
pub const GATE_FLOOR_DB: f32 = -80.0;

/// The bottom of the key filter's knob, where it is out of the path.
pub const GATE_KEY_OFF_HZ: f32 = 20.0;

/// Everything the gate's sound depends on, and none of its envelope
/// (`docs/effects-catalogue.md` §2.1).
///
/// One effect for the gate and the expander, because they are one machine at
/// two settings: a gate is an expander whose ratio is steep enough and whose
/// range is deep enough that "quieter" becomes "gone". The two knobs that say
/// which one you have are [`ratio`](Self::ratio) and [`range_db`](Self::range_db),
/// and both are continuous, so the useful settings between them — the
/// expander that only ducks the spill by 6 dB — are reachable rather than
/// being a third effect.
///
/// The path: key → (key high-pass) → detector → the open/closed decision →
/// attack, hold, release → a gain on the audio, which the look-ahead has
/// delayed so the decision arrives first.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GateConfig {
    /// Above this, in dB, the gate opens. At the bottom of its range nothing
    /// audible is ever below it, which is this effect's "off" — see
    /// [`new`](Self::new).
    pub threshold_db: f32,
    /// How much **quieter** than the opening threshold the signal has to get
    /// before the gate closes again, in dB.
    ///
    /// The control that stops chatter. A signal sitting at the threshold
    /// crosses it dozens of times a second — a snare's decay, a room's
    /// hum — and a gate with one threshold opens and closes on every one of
    /// those crossings, which is audible as a stutter rather than as gating.
    pub hysteresis_db: f32,
    /// A high-pass on what the **detector** listens to, not on the audio.
    /// 20 Hz is off.
    ///
    /// The reason a kick in the overheads does not open the hi-hat's gate.
    /// Nothing here reaches the output: this filter shapes the decision.
    pub key_hp_hz: f32,
    /// How far ahead of the audio the detector looks, in milliseconds.
    ///
    /// The audio is delayed by this much and the decision is not, so the gate
    /// is already open when the transient that opened it arrives. It is the
    /// difference between a gated snare with its crack and one that starts a
    /// millisecond into its own decay.
    ///
    /// It is latency, and it is compensated: the graph holds the other
    /// tracks back to meet this one (TDD §5.5).
    pub lookahead_ms: f32,
    /// How fast it opens.
    pub attack_ms: f32,
    /// How long it stays open after the signal has fallen back under, in
    /// milliseconds. What keeps a decay from being cut in half.
    pub hold_ms: f32,
    /// How fast it closes.
    pub release_ms: f32,
    /// How steeply the gain falls away below the threshold. `1.0` is a wire
    /// at any threshold; the top of the range is a gate.
    pub ratio: f32,
    /// How far down "closed" is, in dB. The floor is a gate; −6 dB is an
    /// expander that only ducks what it does not like.
    pub range_db: f32,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`]. A gate at half mix is
    /// spill turned down rather than removed, which is what a drum kit
    /// usually wants; it is also the only thing on this effect that
    /// interacted badly with look-ahead — the dry it blends is delayed by
    /// the same look-ahead now, so an open gate at half mix is still a wire.
    pub mix: f32,
}

impl GateConfig {
    /// A gate with its threshold at the bottom: fully open, because nothing a
    /// person can hear is under −80 dBFS.
    ///
    /// A different reading of "a fresh effect is nearly a wire" from the
    /// compressor's, and deliberately: a compressor's knob is its ratio, so
    /// 1:1 is where it rests, but the knob somebody reaches for on a gate is
    /// the **threshold**, and a gate whose threshold does something only after
    /// a second knob has been found is a gate that looks broken. So the ratio
    /// and the range open at gate settings and the threshold opens at
    /// "never".
    pub fn new() -> Self {
        Self {
            threshold_db: GATE_FLOOR_DB,
            hysteresis_db: 3.0,
            key_hp_hz: GATE_KEY_OFF_HZ,
            lookahead_ms: 0.0,
            attack_ms: 1.0,
            hold_ms: 10.0,
            release_ms: 100.0,
            ratio: MAX_GATE_RATIO,
            range_db: GATE_FLOOR_DB,
            mix: 1.0,
        }
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "threshold" => self.threshold_db,
            "hysteresis" => self.hysteresis_db,
            "key_hp" => self.key_hp_hz,
            "lookahead" => self.lookahead_ms,
            "attack" => self.attack_ms,
            "hold" => self.hold_ms,
            "release" => self.release_ms,
            "ratio" => self.ratio,
            "range" => self.range_db,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "threshold" => self.threshold_db = value,
            "hysteresis" => self.hysteresis_db = value,
            "key_hp" => self.key_hp_hz = value,
            "lookahead" => self.lookahead_ms = value,
            "attack" => self.attack_ms = value,
            "hold" => self.hold_ms = value,
            "release" => self.release_ms = value,
            "ratio" => self.ratio = value,
            "range" => self.range_db = value,
            _ => {}
        }
    }
}

impl Default for GateConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The top of the ratio knob. Not infinity, because every number downstream
/// of it is arithmetic — and at 100:1 one decibel under the threshold is
/// already past the deepest range the knob can ask for, which is what
/// "1:∞" meant.
pub const MAX_GATE_RATIO: f32 = 100.0;

static GATE_PARAMS: [crate::ParamSpec; 10] = with_mix(&GATE_OWN_PARAMS, ALL_WET);

/// What the panel reads: what it listens to, how it moves, and how far down
/// it goes.
static GATE_SECTIONS: [crate::ParamSection; 3] = [
    crate::ParamSection {
        name: "Detection",
        count: 4,
    },
    crate::ParamSection {
        name: "Envelope",
        count: 3,
    },
    crate::ParamSection {
        name: "Amount",
        count: 3,
    },
];

static GATE_OWN_PARAMS: [crate::ParamSpec; 9] = [
    crate::ParamSpec {
        id: "threshold",
        name: "Threshold",
        min: GATE_FLOOR_DB,
        max: 0.0,
        default: GATE_FLOOR_DB,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "hysteresis",
        name: "Hysteresis",
        min: 0.0,
        max: 24.0,
        default: 3.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "key_hp",
        name: "Key high-pass",
        min: GATE_KEY_OFF_HZ,
        max: 2_000.0,
        default: GATE_KEY_OFF_HZ,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "lookahead",
        name: "Look-ahead",
        min: 0.0,
        max: MAX_GATE_LOOKAHEAD_MS,
        default: 0.0,
        unit: crate::Unit::Milliseconds,
        // Linear, unlike every other time here: the useful settings are spread
        // evenly across ten milliseconds rather than bunched at one end, and
        // the bottom of the knob has to be reachable as exactly zero.
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "attack",
        name: "Attack",
        min: 0.05,
        max: 100.0,
        default: 1.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "hold",
        name: "Hold",
        min: 0.0,
        max: 500.0,
        default: 10.0,
        unit: crate::Unit::Milliseconds,
        // Linear, and for the same reason as the look-ahead: no hold at all
        // is a setting, and a logarithmic taper has no bottom.
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "release",
        name: "Release",
        min: 5.0,
        max: 5_000.0,
        default: 100.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "ratio",
        name: "Ratio",
        min: 1.0,
        max: MAX_GATE_RATIO,
        default: MAX_GATE_RATIO,
        unit: crate::Unit::Ratio,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "range",
        name: "Range",
        min: GATE_FLOOR_DB,
        max: 0.0,
        default: GATE_FLOOR_DB,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
];

// -------------------------------------------------------------- distortion

/// The shape the waveshaper bends the signal with.
///
/// Ten curves rather than one with a "character" knob, because these are not
/// points on a continuum: a folder reverses past its threshold and a clipper
/// saturates at it, and no amount of interpolation gets from one to the other.
/// What *is* a continuum is each curve's own family — how hard the soft clip
/// clips, how many times the fold folds — and that is
/// [`DistortionConfig::shape`], whose meaning each curve states in
/// [`shape_meaning`](Self::shape_meaning). A curve on which shape did nothing
/// would be a chooser position with a dead knob under it, and
/// `shape_moves_every_curve` in `fontelle-fx/tests/distortion.rs` is what
/// says none of them is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DistortionCurve {
    /// `x / (1 + |x|^p)^(1/p)`: rounds the peaks and adds odd harmonics that
    /// fall away smoothly. Shape is `p` — how hard it clips — from a wide
    /// round-over to something a step short of a hard clip. The one that
    /// sounds like an overdriven amplifier.
    SoftClip,
    /// A hard ceiling. Corners rather than curves, so the harmonic series
    /// stays strong a long way up — buzzy, and the one that aliases worst,
    /// which is why the oversampling is not optional. Shape rounds the
    /// corner into a knee.
    HardClip,
    /// Asymmetric soft clipping: the positive half rounds sooner than the
    /// negative one, which puts **even** harmonics in beside the odd ones.
    /// That asymmetry is what "valve warmth" means when it means anything,
    /// and shape is how much of it there is.
    Tube,
    /// Two clipping ceilings, one lower than the other, with a sharp knee at
    /// each: the germanium fuzz. Shape sharpens the knee.
    Diode,
    /// Past the threshold the curve turns back on itself, so a louder input
    /// gives a quieter output. Not monotonic, and the reason it sounds like a
    /// synthesiser rather than an amplifier. A sine fold, so the corners are
    /// round; shape is how many times it folds.
    Fold,
    /// The same reversal with straight edges and sharp corners — the West
    /// Coast fold rather than the sine one, and brighter for it. Shape is how
    /// many times it folds.
    TriangleFold,
    /// A polynomial with its turning points exactly at ±1, hard-limited past
    /// them. Shape moves from the cubic `1.5x - 0.5x³` to a quintic with
    /// more of the higher harmonics.
    WaveShape,
    /// The negative half is flipped up. A full-wave rectified note has no
    /// fundamental and a strong second harmonic — the octave-up fuzz. Shape
    /// runs from half-wave (the note kept, the octave under it) to full.
    Rectify,
    /// A dead zone around zero that nothing small gets out of: the spitting,
    /// gated fuzz, and the crossover distortion a badly biased amplifier
    /// makes. Shape is the zone's width.
    Crossover,
    /// Past the threshold the value comes back in from the other side, as an
    /// integer overflow does. The digital one. Shape lowers the threshold.
    Wrap,
}

impl DistortionCurve {
    pub fn label(self) -> &'static str {
        match self {
            Self::SoftClip => "soft clip",
            Self::HardClip => "hard clip",
            Self::Tube => "tube",
            Self::Diode => "diode",
            Self::Fold => "fold",
            Self::TriangleFold => "triangle fold",
            Self::WaveShape => "wave shape",
            Self::Rectify => "rectify",
            Self::Crossover => "crossover",
            Self::Wrap => "wrap",
        }
    }

    /// What the shape knob does on this curve, in a word or two — what a
    /// tooltip says under it.
    pub fn shape_meaning(self) -> &'static str {
        match self {
            Self::SoftClip => "hardness",
            Self::HardClip => "knee",
            Self::Tube => "asymmetry",
            Self::Diode => "knee",
            Self::Fold | Self::TriangleFold => "folds",
            Self::WaveShape => "order",
            Self::Rectify => "half to full wave",
            Self::Crossover => "dead zone",
            Self::Wrap => "threshold",
        }
    }

    /// The five that existed first, in the order they were, then the five
    /// added on 2026-09-02 — so a project saved before then still names a
    /// curve that is here under the same name.
    pub const ALL: [Self; 10] = [
        Self::SoftClip,
        Self::HardClip,
        Self::Tube,
        Self::Diode,
        Self::Fold,
        Self::TriangleFold,
        Self::WaveShape,
        Self::Rectify,
        Self::Crossover,
        Self::Wrap,
    ];
}

/// The chooser's positions, in `DistortionCurve::ALL`'s order — written out
/// for the reason [`BAND_TYPES`] is, and kept in step by
/// `the_distortion_offers_ten_curves_and_the_chooser_names_them_all`.
static DISTORTION_CURVES: [&str; 10] = [
    "soft clip",
    "hard clip",
    "tube",
    "diode",
    "fold",
    "triangle fold",
    "wave shape",
    "rectify",
    "crossover",
    "wrap",
];

/// How many times the shaper's rate is multiplied before the curve sees the
/// signal.
///
/// A chooser rather than the switch it replaced, because the cost is real
/// and so is the difference: two times catches a 7 kHz tone's seventh
/// harmonic; the ninth and eleventh fold back inside a 96 kHz rate and need
/// the next rung. `more_oversampling_is_less_alias` measures the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Oversampling {
    Off,
    Two,
    Four,
    Eight,
}

impl Oversampling {
    /// The multiplier itself. One for off, which is what makes "off" a
    /// factor rather than a special case in the DSP.
    pub fn factor(self) -> usize {
        match self {
            Self::Off => 1,
            Self::Two => 2,
            Self::Four => 4,
            Self::Eight => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Two => "2x",
            Self::Four => "4x",
            Self::Eight => "8x",
        }
    }

    pub const ALL: [Self; 4] = [Self::Off, Self::Two, Self::Four, Self::Eight];
}

static OVERSAMPLING: [&str; 4] = ["off", "2x", "4x", "8x"];

/// Reads the `oversample` field from a project saved when it was a switch.
///
/// `true` meant the one rate the switch had, which was twice. A file that
/// carries the chooser's own name reads as that.
fn oversample_compat<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Oversampling, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Either {
        Switch(bool),
        Factor(Oversampling),
    }
    Ok(match serde::Deserialize::deserialize(deserializer)? {
        Either::Switch(true) => Oversampling::Two,
        Either::Switch(false) => Oversampling::Off,
        Either::Factor(factor) => factor,
    })
}

fn two_times() -> Oversampling {
    Oversampling::Two
}

/// The bottom of a frequency knob, which is where "off" lives for the two
/// pre-filters: a high-pass at 20 Hz removes nothing anybody can hear.
fn twenty_hz() -> f32 {
    20.0
}

fn eight_hundred_hz() -> f32 {
    800.0
}

/// A named starting point for the distortion's fourteen knobs
/// (`docs/effects-catalogue.md` §3.1).
///
/// A preset is not a parameter, for the reason written on [`SoftenPreset`]:
/// it *sets* the knobs and then has nothing further to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DistortionPreset {
    /// A pedal: soft clip, a mid hump before it, the top rolled off after.
    Overdrive,
    /// Diode clipping with sag and a little bias, tight at the bottom.
    Fuzz,
    /// Tube asymmetry with a sagging supply and a cabinet's worth of tone.
    Amp,
    /// A wavefolder, oversampled, with the top left open.
    Fold,
    /// The bottom goes around the curve and the rest gets driven.
    BassGrit,
    /// Full-wave rectification: the octave-up fuzz.
    Octave,
    /// Integer overflow, with the aliasing left in on purpose.
    Digital,
}

impl DistortionPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Overdrive => "overdrive",
            Self::Fuzz => "fuzz",
            Self::Amp => "amp",
            Self::Fold => "fold",
            Self::BassGrit => "bass grit",
            Self::Octave => "octave",
            Self::Digital => "digital",
        }
    }

    pub const ALL: [Self; 7] = [
        Self::Overdrive,
        Self::Fuzz,
        Self::Amp,
        Self::Fold,
        Self::BassGrit,
        Self::Octave,
        Self::Digital,
    ];
}

/// Everything the distortion's sound depends on, and none of its state
/// (TDD §13.4, `docs/effects-catalogue.md` §3.1).
///
/// The signal's path through it, which is the order the sections read in:
/// pre high-pass → pre mid bell → (the low band split off clean) → bias →
/// drive, less sag → the curve → DC removed → tone → auto-gain → output.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DistortionConfig {
    pub curve: DistortionCurve,
    /// Where on its own family this curve sits, 0..=1. What it means is the
    /// curve's to say — see [`DistortionCurve::shape_meaning`].
    #[serde(default)]
    pub shape: f32,
    /// How hard the signal is pushed into the curve, in dB. Zero is the
    /// bottom of the curve, which is very nearly straight.
    pub drive_db: f32,
    /// A DC offset into the curve, -1..=1, taken back out after it. Shifts
    /// the operating point, which breaks the curve's symmetry, which is even
    /// harmonics on any curve.
    #[serde(default)]
    pub bias: f32,
    /// How much the drive falls as the input gets loud, 0..=1. An amplifier's
    /// power supply cannot keep up with a loud passage, and its clipping
    /// softens under it; this is that, up to 12 dB of it.
    #[serde(default)]
    pub sag: f32,
    /// A high-pass **before** the curve. Tight versus flabby: a low end that
    /// never reaches the shaper never gets distorted, which is the difference
    /// between a bass fuzz and a mess. 20 Hz is off.
    #[serde(default = "twenty_hz")]
    pub pre_hp_hz: f32,
    /// A bell **before** the curve, at this frequency and by this much. The
    /// Tube Screamer's mid hump: whatever the bell lifts is driven harder
    /// than the rest, which is a voicing rather than an EQ.
    #[serde(default = "eight_hundred_hz")]
    pub pre_mid_hz: f32,
    #[serde(default)]
    pub pre_mid_db: f32,
    /// Below this frequency the signal goes *around* the curve and is added
    /// back clean. Bass distortion that keeps its bottom. 20 Hz is off. A
    /// fourth-order Linkwitz–Riley split, so the two paths sum flat.
    #[serde(default = "twenty_hz")]
    pub clean_low_hz: f32,
    /// A low-pass after the shaper. Distortion puts energy at every harmonic
    /// there is, and the top of that series is what makes it sound like a
    /// fault rather than a tone.
    pub tone_hz: f32,
    /// The level control, so drive can be a tone control rather than a volume.
    pub output_db: f32,
    /// Put back what the curve did to the level, so the drive knob changes
    /// the tone and not the volume. Measured on a reference sine at this
    /// drive rather than estimated, so it is right for every curve.
    ///
    /// **On for a fresh effect and off for a file that does not say**: a
    /// project saved before this knob existed sounded a certain level, and
    /// opening it must not move that.
    #[serde(default)]
    pub auto_gain: bool,
    /// Run the shaper at a multiple of the sample rate and filter before
    /// coming back down. Costs about that multiple; see
    /// `fontelle-fx/tests/distortion.rs` for what each rung buys. A file
    /// saved when this was a switch reads `true` as twice.
    #[serde(default = "two_times", deserialize_with = "oversample_compat")]
    pub oversample: Oversampling,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`].
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl DistortionConfig {
    /// A distortion at no drive, which is very nearly a wire: the bottom of a
    /// soft clip is straight to within a fraction of a percent, so an effect
    /// somebody has just added does not commit them to a tone. Every stage
    /// around the curve is at rest for the same reason.
    pub fn new() -> Self {
        Self {
            curve: DistortionCurve::SoftClip,
            shape: 0.0,
            drive_db: 0.0,
            bias: 0.0,
            sag: 0.0,
            pre_hp_hz: 20.0,
            pre_mid_hz: 800.0,
            pre_mid_db: 0.0,
            clean_low_hz: 20.0,
            tone_hz: 20_000.0,
            output_db: 0.0,
            auto_gain: true,
            oversample: Oversampling::Two,
            mix: 1.0,
        }
    }

    /// The knobs a named preset stands for.
    pub fn from_preset(preset: DistortionPreset) -> Self {
        let wire = Self::new();
        match preset {
            DistortionPreset::Overdrive => Self {
                curve: DistortionCurve::SoftClip,
                shape: 0.2,
                drive_db: 18.0,
                sag: 0.1,
                pre_hp_hz: 80.0,
                pre_mid_hz: 800.0,
                pre_mid_db: 6.0,
                tone_hz: 6_000.0,
                ..wire
            },
            DistortionPreset::Fuzz => Self {
                curve: DistortionCurve::Diode,
                shape: 0.7,
                drive_db: 36.0,
                bias: 0.2,
                sag: 0.4,
                pre_hp_hz: 120.0,
                pre_mid_hz: 1_000.0,
                pre_mid_db: 3.0,
                tone_hz: 5_000.0,
                oversample: Oversampling::Four,
                ..wire
            },
            DistortionPreset::Amp => Self {
                curve: DistortionCurve::Tube,
                shape: 0.4,
                drive_db: 24.0,
                bias: 0.1,
                sag: 0.5,
                pre_hp_hz: 60.0,
                pre_mid_hz: 1_200.0,
                pre_mid_db: 4.0,
                tone_hz: 4_500.0,
                ..wire
            },
            DistortionPreset::Fold => Self {
                curve: DistortionCurve::Fold,
                shape: 0.4,
                drive_db: 12.0,
                pre_hp_hz: 40.0,
                tone_hz: 12_000.0,
                oversample: Oversampling::Four,
                ..wire
            },
            DistortionPreset::BassGrit => Self {
                curve: DistortionCurve::SoftClip,
                shape: 0.5,
                drive_db: 20.0,
                pre_hp_hz: 30.0,
                pre_mid_hz: 900.0,
                pre_mid_db: 3.0,
                clean_low_hz: 120.0,
                tone_hz: 5_000.0,
                ..wire
            },
            DistortionPreset::Octave => Self {
                curve: DistortionCurve::Rectify,
                shape: 1.0,
                drive_db: 12.0,
                pre_hp_hz: 100.0,
                tone_hz: 3_000.0,
                ..wire
            },
            DistortionPreset::Digital => Self {
                curve: DistortionCurve::Wrap,
                shape: 0.5,
                drive_db: 9.0,
                oversample: Oversampling::Off,
                ..wire
            },
        }
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "curve" => DistortionCurve::ALL.iter().position(|c| *c == self.curve)? as f32,
            "shape" => self.shape * 100.0,
            "drive" => self.drive_db,
            "bias" => self.bias * 100.0,
            "sag" => self.sag * 100.0,
            "pre_hp" => self.pre_hp_hz,
            "pre_mid_hz" => self.pre_mid_hz,
            "pre_mid_db" => self.pre_mid_db,
            "clean_low" => self.clean_low_hz,
            "tone" => self.tone_hz,
            "output" => self.output_db,
            "auto_gain" => f32::from(u8::from(self.auto_gain)),
            "oversample" => Oversampling::ALL
                .iter()
                .position(|o| *o == self.oversample)? as f32,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "curve" => {
                if let Some(curve) = DistortionCurve::ALL.get(value.round().max(0.0) as usize) {
                    self.curve = *curve;
                }
            }
            "shape" => self.shape = value / 100.0,
            "drive" => self.drive_db = value,
            "bias" => self.bias = value / 100.0,
            "sag" => self.sag = value / 100.0,
            "pre_hp" => self.pre_hp_hz = value,
            "pre_mid_hz" => self.pre_mid_hz = value,
            "pre_mid_db" => self.pre_mid_db = value,
            "clean_low" => self.clean_low_hz = value,
            "tone" => self.tone_hz = value,
            "output" => self.output_db = value,
            "auto_gain" => self.auto_gain = value >= 0.5,
            "oversample" => {
                if let Some(factor) = Oversampling::ALL.get(value.round().max(0.0) as usize) {
                    self.oversample = *factor;
                }
            }
            _ => {}
        }
    }
}

impl Default for DistortionConfig {
    fn default() -> Self {
        Self::new()
    }
}

static DISTORTION_PARAMS: [crate::ParamSpec; 14] = with_mix(&DISTORTION_OWN_PARAMS, ALL_WET);

/// How the panel reads: what goes into the curve, what shapes the signal
/// either side of it, and what comes out. The counts are the table's, and
/// `every_effects_sections_cover_its_parameters_exactly` holds them to it.
static DISTORTION_SECTIONS: [crate::ParamSection; 3] = [
    crate::ParamSection {
        name: "Drive",
        count: 5,
    },
    crate::ParamSection {
        name: "Voicing",
        count: 5,
    },
    crate::ParamSection {
        name: "Output",
        count: 4,
    },
];

/// A 0–100 % knob at `default`, for the amounts.
const fn percent_param(id: &'static str, name: &'static str, default: f32) -> crate::ParamSpec {
    crate::ParamSpec {
        id,
        name,
        min: 0.0,
        max: 100.0,
        default,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    }
}

static DISTORTION_OWN_PARAMS: [crate::ParamSpec; 13] = [
    crate::ParamSpec {
        id: "curve",
        name: "Curve",
        min: 0.0,
        max: 9.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(10),
        positions: &DISTORTION_CURVES,
    },
    percent_param("shape", "Shape", 0.0),
    crate::ParamSpec {
        id: "drive",
        name: "Drive",
        min: 0.0,
        max: 48.0,
        default: 0.0,
        // Decibels are already logarithmic; a log taper on top of them would
        // be a lane that spends its travel twice.
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "bias",
        name: "Bias",
        min: -100.0,
        max: 100.0,
        default: 0.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    percent_param("sag", "Sag", 0.0),
    crate::ParamSpec {
        id: "pre_hp",
        name: "Pre high-pass",
        min: 20.0,
        max: 2_000.0,
        default: 20.0,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "pre_mid_hz",
        name: "Pre mid",
        min: 200.0,
        max: 5_000.0,
        default: 800.0,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "pre_mid_db",
        name: "Pre mid gain",
        min: -18.0,
        max: 18.0,
        default: 0.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "clean_low",
        name: "Clean low",
        min: 20.0,
        max: 500.0,
        default: 20.0,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "tone",
        name: "Tone",
        min: 200.0,
        max: 20_000.0,
        default: 20_000.0,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "output",
        name: "Output",
        min: -24.0,
        max: 12.0,
        default: 0.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "auto_gain",
        name: "Auto gain",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
        positions: &["off", "on"],
    },
    crate::ParamSpec {
        id: "oversample",
        name: "Oversample",
        min: 0.0,
        max: 3.0,
        default: 1.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(4),
        positions: &OVERSAMPLING,
    },
];

// --------------------------------------------------------------- bitcrush

/// The highest bit depth the knob offers, which is also the one that is
/// transparent: sixteen bits is a quantisation step of 1/32768.
pub const MAX_BITS: f32 = 16.0;

/// How a sample is put onto the grid of `2^bits` levels.
///
/// The grid is the same size whichever of these is chosen; what differs is
/// *which* level a value between two of them lands on, and — for the last —
/// where the levels are. That is most of the difference between one lo-fi
/// machine and another.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Quantiser {
    /// To the nearest level. The least error, and the one every other
    /// bitcrusher does.
    #[default]
    Round,
    /// Toward zero. Anything smaller than one whole step falls to silence,
    /// which is the gating an 8-bit sample player does to the tail of every
    /// note, and the reason those tails end the way they do.
    Truncate,
    /// A logarithmic grid: fine near zero, coarse near full scale. How
    /// telephony got speech through eight bits, and how the samplers that
    /// used companding kept quiet detail while crunching the loud. µ = 255.
    MuLaw,
}

impl Quantiser {
    pub fn label(self) -> &'static str {
        match self {
            Self::Round => "round",
            Self::Truncate => "truncate",
            Self::MuLaw => "mu-law",
        }
    }

    pub const ALL: [Self; 3] = [Self::Round, Self::Truncate, Self::MuLaw];
}

static QUANTISERS: [&str; 3] = ["round", "truncate", "mu-law"];

/// What is added before the quantiser to break its error's correlation with
/// the signal.
///
/// A chooser rather than the switch it replaced. Dither does not add levels
/// — the output still lands on the grid — it changes which level gets
/// chosen, and the three kinds differ in what the leftover noise sounds
/// like.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Dither {
    #[default]
    Off,
    /// One uniform random value, one step wide. The cheapest; its noise
    /// floor moves with the signal.
    Rectangular,
    /// The sum of two, which is the standard choice: the level of the
    /// remaining error stops depending on the signal. What the switch did.
    Triangular,
    /// Triangular, with the previous sample's error fed back and subtracted.
    /// Pushes the noise toward the top of the band, where it is least
    /// audible — the hi-fi one, and a different sound from the other two at
    /// four bits.
    Shaped,
}

impl Dither {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Rectangular => "rectangular",
            Self::Triangular => "triangular",
            Self::Shaped => "shaped",
        }
    }

    pub const ALL: [Self; 4] = [Self::Off, Self::Rectangular, Self::Triangular, Self::Shaped];
}

static DITHERS: [&str; 4] = ["off", "rectangular", "triangular", "shaped"];

/// Reads the `dither` field from a project saved when it was a switch.
/// `true` meant triangular, which was the only kind there was.
fn dither_compat<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Dither, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Either {
        Switch(bool),
        Kind(Dither),
    }
    Ok(match serde::Deserialize::deserialize(deserializer)? {
        Either::Switch(true) => Dither::Triangular,
        Either::Switch(false) => Dither::Off,
        Either::Kind(kind) => kind,
    })
}

/// What happens to the samples between one take and the next.
///
/// The rate says how often a new value is taken; this says what the output
/// does in the meantime, and the three answers are three different machines.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum Decimation {
    /// Sample-and-hold: the value stays until the next one. The stair, and
    /// the aliasing every other bitcrusher has.
    #[default]
    Hold,
    /// A straight line from the last value to the next: a sampler with
    /// interpolation. Smoother, darker, and no stair to be found.
    Linear,
    /// The value plays for one sample and the rest of the period is silence.
    /// Sparse, comb-like, and unlike either of the others — the output loses
    /// energy in proportion, which is what the output gain is for.
    Drop,
}

impl Decimation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::Linear => "linear",
            Self::Drop => "drop",
        }
    }

    pub const ALL: [Self; 3] = [Self::Hold, Self::Linear, Self::Drop];
}

static DECIMATIONS: [&str; 3] = ["hold", "linear", "drop"];

fn twenty_khz() -> f32 {
    20_000.0
}

/// A named starting point for the bitcrusher's eleven knobs
/// (`docs/effects-catalogue.md` §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BitcrushPreset {
    /// Twelve bits at 26 kHz with interpolation and an output filter: the
    /// drum machines of 1987.
    TwelveBitSampler,
    /// Eight bits, truncated, at 16 kHz, driven a little: the consoles of
    /// the same decade.
    EightBitConsole,
    /// Eight bits of µ-law at 8 kHz, band-limited both sides: a telephone.
    Telephone,
    /// A clock that cannot keep time.
    BrokenClock,
    /// Every sample but the held one dropped.
    Sparse,
    /// Four bits, truncated, driven hard.
    Crunch,
}

impl BitcrushPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::TwelveBitSampler => "12-bit sampler",
            Self::EightBitConsole => "8-bit console",
            Self::Telephone => "telephone",
            Self::BrokenClock => "broken clock",
            Self::Sparse => "sparse",
            Self::Crunch => "crunch",
        }
    }

    pub const ALL: [Self; 6] = [
        Self::TwelveBitSampler,
        Self::EightBitConsole,
        Self::Telephone,
        Self::BrokenClock,
        Self::Sparse,
        Self::Crunch,
    ];
}

/// Everything the bitcrusher's sound depends on, and none of its held sample
/// (TDD §13.4, `docs/effects-catalogue.md` §3.2).
///
/// The signal's path: input gain → (anti-alias) → dither → quantiser → the
/// decimator → post filter → output gain.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BitcrushConfig {
    /// Gain into the quantiser, in dB. A quantiser is a level-dependent
    /// effect: a quiet signal at four bits is a gated crackle and a loud one
    /// is a square wave, and this knob is the difference between them.
    #[serde(default)]
    pub input_db: f32,
    /// How many bits the amplitude is rounded to. Fractional on purpose — the
    /// knob and an automation lane both move continuously, and a step size of
    /// `2^-bits` is meaningful between whole numbers.
    pub bits: f32,
    /// How a value lands on the grid — see [`Quantiser`].
    #[serde(default)]
    pub quantiser: Quantiser,
    /// What breaks the quantiser's error up — see [`Dither`]. A file saved
    /// when this was a switch reads `true` as triangular.
    #[serde(default, deserialize_with = "dither_compat")]
    pub dither: Dither,
    /// The sample-and-hold rate. At or above the device's own rate nothing is
    /// held, which is what makes the top of the knob transparent.
    pub rate_hz: f32,
    /// What the output does between takes — see [`Decimation`].
    #[serde(default)]
    pub decimation: Decimation,
    /// Random variation of the hold period, 0..=1. An unstable clock: a
    /// tape-like smear at small amounts, a broken machine at large ones.
    #[serde(default)]
    pub jitter: f32,
    /// Band-limit before decimating.
    ///
    /// **Off by default**, and that is the interesting default: the aliasing a
    /// sample-and-hold produces *is* the effect, and a bitcrusher that filters
    /// it away is a low-pass with extra steps. This is here for the times the
    /// grit is wanted and the ring modulation is not.
    pub anti_alias: bool,
    /// A low-pass after everything. An old sampler's output stage: it tames
    /// the aliasing after the fact, which is a different sound from
    /// preventing it. 20 kHz is off.
    #[serde(default = "twenty_khz")]
    pub post_lp_hz: f32,
    /// Gain out, in dB.
    #[serde(default)]
    pub output_db: f32,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`].
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl BitcrushConfig {
    /// Full depth, full rate: an effect somebody has just added must not
    /// commit them to a sound.
    pub fn new() -> Self {
        Self {
            input_db: 0.0,
            bits: MAX_BITS,
            quantiser: Quantiser::Round,
            dither: Dither::Off,
            rate_hz: MAX_CRUSH_RATE_HZ,
            decimation: Decimation::Hold,
            jitter: 0.0,
            anti_alias: false,
            post_lp_hz: 20_000.0,
            output_db: 0.0,
            mix: 1.0,
        }
    }

    /// The knobs a named preset stands for.
    pub fn from_preset(preset: BitcrushPreset) -> Self {
        let wire = Self::new();
        match preset {
            BitcrushPreset::TwelveBitSampler => Self {
                bits: 12.0,
                rate_hz: 26_040.0,
                decimation: Decimation::Linear,
                post_lp_hz: 14_000.0,
                ..wire
            },
            BitcrushPreset::EightBitConsole => Self {
                input_db: 6.0,
                bits: 8.0,
                quantiser: Quantiser::Truncate,
                rate_hz: 16_000.0,
                ..wire
            },
            BitcrushPreset::Telephone => Self {
                bits: 8.0,
                quantiser: Quantiser::MuLaw,
                rate_hz: 8_000.0,
                anti_alias: true,
                post_lp_hz: 3_400.0,
                ..wire
            },
            BitcrushPreset::BrokenClock => Self {
                bits: 10.0,
                rate_hz: 12_000.0,
                jitter: 0.6,
                ..wire
            },
            BitcrushPreset::Sparse => Self {
                rate_hz: 6_000.0,
                decimation: Decimation::Drop,
                post_lp_hz: 8_000.0,
                output_db: 6.0,
                ..wire
            },
            BitcrushPreset::Crunch => Self {
                input_db: 12.0,
                bits: 4.0,
                quantiser: Quantiser::Truncate,
                output_db: -3.0,
                ..wire
            },
        }
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "input" => self.input_db,
            "bits" => self.bits,
            "quantiser" => Quantiser::ALL.iter().position(|q| *q == self.quantiser)? as f32,
            "dither" => Dither::ALL.iter().position(|d| *d == self.dither)? as f32,
            "rate" => self.rate_hz,
            "decimation" => Decimation::ALL.iter().position(|d| *d == self.decimation)? as f32,
            "jitter" => self.jitter * 100.0,
            "antialias" => f32::from(u8::from(self.anti_alias)),
            "post_lp" => self.post_lp_hz,
            "output" => self.output_db,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "input" => self.input_db = value,
            "bits" => self.bits = value,
            "quantiser" => {
                if let Some(quantiser) = Quantiser::ALL.get(value.round().max(0.0) as usize) {
                    self.quantiser = *quantiser;
                }
            }
            "dither" => {
                if let Some(dither) = Dither::ALL.get(value.round().max(0.0) as usize) {
                    self.dither = *dither;
                }
            }
            "rate" => self.rate_hz = value,
            "decimation" => {
                if let Some(decimation) = Decimation::ALL.get(value.round().max(0.0) as usize) {
                    self.decimation = *decimation;
                }
            }
            "jitter" => self.jitter = value / 100.0,
            "antialias" => self.anti_alias = value >= 0.5,
            "post_lp" => self.post_lp_hz = value,
            "output" => self.output_db = value,
            _ => {}
        }
    }
}

impl Default for BitcrushConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The top of the rate knob. Above any device rate this build runs at, so the
/// knob at its maximum holds nothing whatever the sound card is doing.
pub const MAX_CRUSH_RATE_HZ: f32 = 48_000.0;

static BITCRUSH_PARAMS: [crate::ParamSpec; 11] = with_mix(&BITCRUSH_OWN_PARAMS, ALL_WET);

/// What the panel reads: what happens to the amplitude, what happens to the
/// time, and what comes out.
static BITCRUSH_SECTIONS: [crate::ParamSection; 3] = [
    crate::ParamSection {
        name: "Depth",
        count: 4,
    },
    crate::ParamSection {
        name: "Rate",
        count: 4,
    },
    crate::ParamSection {
        name: "Output",
        count: 3,
    },
];

/// A ±24 dB gain knob at unity.
const fn gain_param(id: &'static str, name: &'static str) -> crate::ParamSpec {
    crate::ParamSpec {
        id,
        name,
        min: -24.0,
        max: 24.0,
        default: 0.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    }
}

static BITCRUSH_OWN_PARAMS: [crate::ParamSpec; 10] = [
    gain_param("input", "Input"),
    crate::ParamSpec {
        id: "bits",
        name: "Bits",
        min: 1.0,
        max: MAX_BITS,
        default: MAX_BITS,
        unit: crate::Unit::None,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "quantiser",
        name: "Quantiser",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(3),
        positions: &QUANTISERS,
    },
    crate::ParamSpec {
        id: "dither",
        name: "Dither",
        min: 0.0,
        max: 3.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(4),
        positions: &DITHERS,
    },
    crate::ParamSpec {
        id: "rate",
        name: "Rate",
        min: 200.0,
        max: MAX_CRUSH_RATE_HZ,
        default: MAX_CRUSH_RATE_HZ,
        unit: crate::Unit::Hertz,
        // A rate is a ratio, like every other frequency here: halving it is
        // one octave of grit wherever it starts.
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "decimation",
        name: "Decimation",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(3),
        positions: &DECIMATIONS,
    },
    percent_param("jitter", "Jitter", 0.0),
    crate::ParamSpec {
        id: "antialias",
        name: "Anti-alias",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
        positions: &["off", "on"],
    },
    crate::ParamSpec {
        id: "post_lp",
        name: "Post filter",
        min: 200.0,
        max: 20_000.0,
        default: 20_000.0,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    gain_param("output", "Output"),
];

// ----------------------------------------------------------------- soften

/// A named starting point for the four amounts (TDD §13.5).
///
/// A preset is not a parameter: it *sets* the four knobs and then has nothing
/// further to say, so it is a constructor rather than something an automation
/// lane can sweep. A "preset" knob would fight the four it had just written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SoftenPreset {
    Gentle,
    Standard,
    Aggressive,
    /// Not a point on the gentle-to-aggressive line. A rompler's harshness is
    /// a **resonance** problem rather than a treble one — the same few
    /// megabytes of samples stretched across the keyboard put a peak in the
    /// same place on every note — so this leans on the suppressor and keeps
    /// the top.
    VintageRompler,
}

impl SoftenPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gentle => "gentle",
            Self::Standard => "standard",
            Self::Aggressive => "aggressive",
            Self::VintageRompler => "vintage rompler",
        }
    }

    pub const ALL: [Self; 4] = [
        Self::Gentle,
        Self::Standard,
        Self::Aggressive,
        Self::VintageRompler,
    ];
}

/// Everything the softener's sound depends on, and none of its filters
/// (TDD §13.5).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SoftenConfig {
    /// How deep the dynamic high shelf can cut, 0..=1.
    pub shelf_amount: f32,
    /// How hard the 1-6 kHz suppressor ducks a hot band, 0..=1.
    pub suppressor_amount: f32,
    /// How much of a sudden attack is taken off, 0..=1.
    pub transient_amount: f32,
    /// How much of the very top is given back, 0..=1.
    pub air_restore_amount: f32,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`].
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl SoftenConfig {
    /// **Gentle, not silent**, which is a deliberate departure from the rule
    /// the EQ and the compressor follow.
    ///
    /// Those two open as an identity because you add them in order to dial in
    /// your own settings. This is one macro with one job, and somebody adding
    /// "Soften" has already said what they want. Opening at zero would be the
    /// dead panel this project keeps finding — an effect you add and cannot
    /// hear — and the honest answer is a real but conservative setting.
    pub fn new() -> Self {
        Self::from_preset(SoftenPreset::Gentle)
    }

    /// The four amounts a named preset stands for.
    pub fn from_preset(preset: SoftenPreset) -> Self {
        let (shelf, suppressor, transient, air) = match preset {
            SoftenPreset::Gentle => (0.25, 0.25, 0.15, 0.25),
            SoftenPreset::Standard => (0.5, 0.5, 0.35, 0.4),
            SoftenPreset::Aggressive => (0.85, 0.8, 0.6, 0.55),
            SoftenPreset::VintageRompler => (0.3, 0.85, 0.4, 0.5),
        };
        Self {
            shelf_amount: shelf,
            suppressor_amount: suppressor,
            transient_amount: transient,
            air_restore_amount: air,
            mix: 1.0,
        }
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "shelf" => self.shelf_amount * 100.0,
            "suppressor" => self.suppressor_amount * 100.0,
            "transient" => self.transient_amount * 100.0,
            "air" => self.air_restore_amount * 100.0,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "shelf" => self.shelf_amount = value / 100.0,
            "suppressor" => self.suppressor_amount = value / 100.0,
            "transient" => self.transient_amount = value / 100.0,
            "air" => self.air_restore_amount = value / 100.0,
            _ => {}
        }
    }
}

impl Default for SoftenConfig {
    fn default() -> Self {
        Self::new()
    }
}

static SOFTEN_PARAMS: [crate::ParamSpec; 5] = with_mix(&SOFTEN_OWN_PARAMS, ALL_WET);

macro_rules! soften_param {
    ($id:literal, $name:literal, $default:literal) => {
        crate::ParamSpec {
            id: $id,
            name: $name,
            min: 0.0,
            max: 100.0,
            default: $default,
            unit: crate::Unit::Percent,
            taper: crate::Taper::Linear,
            positions: &[],
        }
    };
}

/// The four, at the `Gentle` preset's values — which is what
/// [`SoftenConfig::new`] builds, and the two must agree or the knob jumps the
/// first time anybody touches it.
static SOFTEN_OWN_PARAMS: [crate::ParamSpec; 4] = [
    soften_param!("shelf", "Shelf", 25.0),
    soften_param!("suppressor", "Suppressor", 25.0),
    soften_param!("transient", "Transient", 15.0),
    soften_param!("air", "Air", 25.0),
];

// ------------------------------------------------------------------ chorus

/// How the voices of a chorus relate to each other
/// (`docs/effects-catalogue.md` §2.4).
///
/// Two kinds rather than two presets, because they are two machines: one LFO
/// shared between voices at different points of its cycle, or one LFO each at
/// rates that never line up. The first is a chorus and the second is a string
/// machine, and no setting of the first is the second.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ChorusMode {
    /// Every voice on the same LFO, spread evenly around its cycle. Coherent:
    /// the voices sweep together and the comb they make with the dry signal
    /// moves as one. The pedal.
    #[default]
    Chorus,
    /// Every voice on its own LFO at a rate near but not equal to the knob's,
    /// so the voices never come back into step. Incoherent, and much thicker
    /// for it: the sound of three cheap oscillators in a 1970s string
    /// machine, which is what an "ensemble" was.
    Ensemble,
}

impl ChorusMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Chorus => "chorus",
            Self::Ensemble => "ensemble",
        }
    }

    pub const ALL: [Self; 2] = [Self::Chorus, Self::Ensemble];
}

static CHORUS_MODES: [&str; 2] = ["chorus", "ensemble"];

/// The most voices the chorus runs. Four, because the fifth is not audible
/// under the four and every one of them costs an interpolated read per
/// sample.
pub const MAX_CHORUS_VOICES: u32 = 4;

/// The ends of the centre-delay knob, in milliseconds
/// (`docs/effects-catalogue.md` §2.4).
///
/// Five is where a chorus stops being a flanger: under it the comb's teeth
/// are far enough apart to be heard as a swept resonance rather than as
/// thickness. Thirty is where the voices stop being the same note and start
/// being a slapback.
pub const MIN_CHORUS_DELAY_MS: f32 = 5.0;
pub const MAX_CHORUS_DELAY_MS: f32 = 30.0;

/// The top of the tone knob, where the filter is out of the path.
pub const CHORUS_TONE_OPEN_HZ: f32 = 20_000.0;

/// Where a fresh chorus opens its mix, as a percentage.
///
/// Half and half, and that is not a preference: a chorus **is** the
/// interference between the modulated copies and the signal that made them.
/// `EffectNode` owns the dry side (rule 7), so a fully wet chorus is a
/// detuned copy with nothing to beat against — which is a vibrato, and a
/// person who added a chorus did not ask for one.
const CHORUS_MIX: f32 = 50.0;

/// Everything the chorus's sound depends on, and none of its delay line
/// (`docs/effects-catalogue.md` §2.4).
///
/// The path: in → line → *n* interpolated reads, each at the centre delay
/// plus its own LFO → summed → tone → out, with a share of the sum written
/// back into the line.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChorusConfig {
    /// How many modulated copies, 1 to [`MAX_CHORUS_VOICES`].
    pub voices: u32,
    /// Whether they share an LFO or each have their own — see [`ChorusMode`].
    pub mode: ChorusMode,
    /// How far apart the two sides are in the LFO's cycle, 0..=1, where 1 is
    /// half a cycle.
    ///
    /// At zero a mono signal comes out mono; at one the two sides are sweeping
    /// in opposite directions, which is as wide as a chorus goes without
    /// inventing information. The voices themselves are always spread evenly
    /// around the cycle — that is what having more than one is *for* — so this
    /// knob is about the image rather than about the voices.
    pub spread: f32,
    /// The LFO's rate in Hz, in force when [`sync`](Self::sync) is off. Kept
    /// even while synced, for the reason the delay keeps its milliseconds.
    pub rate_hz: f32,
    /// Take the rate from the song's tempo instead of the Hz knob: one LFO
    /// cycle per [`division`](Self::division).
    pub sync: bool,
    pub division: NoteDivision,
    /// How far the LFO moves the read heads, 0..=1.
    ///
    /// A **proportion of the room there is**, not a number of milliseconds:
    /// the swing is measured against how far the centre delay can move before
    /// it reaches the write head, so full depth at a 6 ms centre is a smaller
    /// sweep than full depth at 25 ms. A depth in milliseconds would let the
    /// two knobs produce a read pointer ahead of the write pointer, which is
    /// not a sound, it is a wrap.
    pub depth: f32,
    /// The centre of the sweep, in milliseconds — see
    /// [`MIN_CHORUS_DELAY_MS`].
    pub delay_ms: f32,
    /// How much of the voices' sum goes back into the line, −1..=1.
    ///
    /// Zero is a chorus. Positive is the resonant comb a flanger has, and
    /// **negative** is the hollow one — the same comb with its teeth in the
    /// gaps, which is a different sound and the reason this is signed rather
    /// than a percentage.
    pub feedback: f32,
    /// A low-pass on the voices, in Hz. What keeps four detuned copies of a
    /// bright sound from turning into a wash of top end.
    /// [`CHORUS_TONE_OPEN_HZ`] is off.
    pub tone_hz: f32,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`] and [`CHORUS_MIX`].
    pub mix: f32,
}

impl ChorusConfig {
    /// A gentle two-voice chorus, half and half.
    ///
    /// The delay's reading of "a fresh effect" rather than the compressor's,
    /// and for the delay's reason: what this writes is the voices, with none
    /// of the signal that made them, so an instance at rest would be silence
    /// under a dry track rather than a wire. See [`EffectKind::is_time_based`].
    pub fn new() -> Self {
        Self {
            voices: 2,
            mode: ChorusMode::Chorus,
            spread: 0.5,
            rate_hz: 0.5,
            sync: false,
            division: NoteDivision::Whole,
            depth: 0.5,
            delay_ms: 15.0,
            feedback: 0.0,
            tone_hz: CHORUS_TONE_OPEN_HZ,
            mix: CHORUS_MIX / 100.0,
        }
    }

    /// The rate this chorus is actually asking for, at `bpm`.
    ///
    /// The same seam as [`DelayConfig::effective_time_ms`] and in the same
    /// place, for the same reason: what a whole note *is* is a document fact.
    /// One LFO cycle per division, so "sync to a bar" means the sweep takes a
    /// bar.
    pub fn effective_rate_hz(&self, bpm: f32) -> f32 {
        let asked = if self.sync {
            let bpm = if bpm.is_finite() { bpm } else { 0.0 };
            let seconds = self.division.beats() * 60.0 / bpm.clamp(MIN_BPM, MAX_BPM);
            1.0 / seconds.max(1e-4)
        } else {
            self.rate_hz
        };
        asked.clamp(MIN_LFO_RATE_HZ, MAX_LFO_RATE_HZ)
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "voices" => self.voices as f32,
            "mode" => ChorusMode::ALL.iter().position(|m| *m == self.mode)? as f32,
            "spread" => self.spread * 100.0,
            "rate" => self.rate_hz,
            "sync" => f32::from(u8::from(self.sync)),
            "division" => NoteDivision::ALL.iter().position(|d| *d == self.division)? as f32,
            "depth" => self.depth * 100.0,
            "delay" => self.delay_ms,
            "feedback" => self.feedback * 100.0,
            "tone" => self.tone_hz,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "voices" => self.voices = (value.round().max(1.0) as u32).min(MAX_CHORUS_VOICES),
            "mode" => {
                if let Some(mode) = ChorusMode::ALL.get(value.round().max(0.0) as usize) {
                    self.mode = *mode;
                }
            }
            "spread" => self.spread = value / 100.0,
            "rate" => self.rate_hz = value,
            "sync" => self.sync = value >= 0.5,
            "division" => {
                if let Some(division) = NoteDivision::ALL.get(value.round().max(0.0) as usize) {
                    self.division = *division;
                }
            }
            "depth" => self.depth = value / 100.0,
            "delay" => self.delay_ms = value,
            "feedback" => self.feedback = value / 100.0,
            "tone" => self.tone_hz = value,
            _ => {}
        }
    }
}

impl Default for ChorusConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The ends of an LFO's rate knob, in Hz. A hundredth of a hertz is a sweep
/// that takes a minute and a half; twenty is where an LFO stops being
/// modulation and starts being a ring modulator.
///
/// Shared by every effect with an LFO rather than one pair each, for rule 3's
/// reason: two ranges for the same control is one of them to get wrong, and a
/// person who has learned where "slow" is on the chorus has learned where it
/// is on the filter.
pub const MIN_LFO_RATE_HZ: f32 = 0.01;
pub const MAX_LFO_RATE_HZ: f32 = 20.0;

static CHORUS_PARAMS: [crate::ParamSpec; 11] = with_mix(&CHORUS_OWN_PARAMS, CHORUS_MIX);

/// What the panel reads: how many copies there are, how they move, and what
/// leaves.
static CHORUS_SECTIONS: [crate::ParamSection; 3] = [
    crate::ParamSection {
        name: "Voices",
        count: 3,
    },
    crate::ParamSection {
        name: "Modulation",
        count: 5,
    },
    crate::ParamSection {
        name: "Output",
        count: 3,
    },
];

static CHORUS_OWN_PARAMS: [crate::ParamSpec; 10] = [
    crate::ParamSpec {
        id: "voices",
        name: "Voices",
        min: 1.0,
        max: MAX_CHORUS_VOICES as f32,
        default: 2.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(MAX_CHORUS_VOICES),
        // A stepped parameter whose positions really are numbers, which is
        // the case `ParamSpec::positions` leaves empty.
        positions: &[],
    },
    crate::ParamSpec {
        id: "mode",
        name: "Mode",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(2),
        positions: &CHORUS_MODES,
    },
    percent_param("spread", "Spread", 50.0),
    crate::ParamSpec {
        id: "rate",
        name: "Rate",
        min: MIN_LFO_RATE_HZ,
        max: MAX_LFO_RATE_HZ,
        default: 0.5,
        unit: crate::Unit::Hertz,
        // Three decades of rate: linear, the whole slow half would be in the
        // bottom two percent of the lane.
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "sync",
        name: "Sync",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
        positions: &["off", "on"],
    },
    crate::ParamSpec {
        id: "division",
        name: "Division",
        min: 0.0,
        max: (DIVISIONS.len() - 1) as f32,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(DIVISIONS.len() as u32),
        positions: &DIVISIONS,
    },
    percent_param("depth", "Depth", 50.0),
    crate::ParamSpec {
        id: "delay",
        name: "Delay",
        min: MIN_CHORUS_DELAY_MS,
        max: MAX_CHORUS_DELAY_MS,
        default: 15.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "feedback",
        name: "Feedback",
        // Signed, and that is the whole point of the control — see
        // `ChorusConfig::feedback`.
        min: -90.0,
        max: 90.0,
        default: 0.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "tone",
        name: "Tone",
        min: 200.0,
        max: CHORUS_TONE_OPEN_HZ,
        default: CHORUS_TONE_OPEN_HZ,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
];

// ------------------------------------------------------------------- delay

/// What a fresh delay's repeats sit at, as a percentage.
///
/// Part dry, unlike a processor's — see [`EffectKind::is_time_based`]. Enough
/// to hear on the track it was just added to and not enough to be the track.
const DELAY_MIX: f32 = 35.0;

/// A note value a delay can be set to, longest first.
///
/// Longest first because that is the order a chooser steps through them and
/// the order an automation lane sweeps: turning the knob one way should
/// shorten the delay monotonically rather than jumping about, which is what an
/// enum grouped by "plain, then dotted, then triplet" would do.
///
/// The set is the one a delay actually gets used at. A dotted eighth is the
/// reason the dotted values are here at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NoteDivision {
    Whole,
    HalfDotted,
    Half,
    QuarterDotted,
    HalfTriplet,
    Quarter,
    EighthDotted,
    QuarterTriplet,
    Eighth,
    SixteenthDotted,
    EighthTriplet,
    Sixteenth,
    SixteenthTriplet,
    ThirtySecond,
}

impl NoteDivision {
    /// How many **quarter notes** long this is, which is the unit a tempo is
    /// in: at 120 bpm there are 120 of these a minute, by definition.
    ///
    /// A dot adds half again and a triplet takes a third off — the two
    /// modifiers, written out per variant rather than derived, because a
    /// `const fn` reading this from a base value and a modifier would be three
    /// enums where one will do.
    pub fn beats(self) -> f32 {
        match self {
            Self::Whole => 4.0,
            Self::HalfDotted => 3.0,
            Self::Half => 2.0,
            Self::HalfTriplet => 4.0 / 3.0,
            Self::QuarterDotted => 1.5,
            Self::Quarter => 1.0,
            Self::QuarterTriplet => 2.0 / 3.0,
            Self::EighthDotted => 0.75,
            Self::Eighth => 0.5,
            Self::EighthTriplet => 1.0 / 3.0,
            Self::SixteenthDotted => 0.375,
            Self::Sixteenth => 0.25,
            Self::SixteenthTriplet => 1.0 / 6.0,
            Self::ThirtySecond => 0.125,
        }
    }

    /// What the chooser says. The notation a musician reads, not the fraction
    /// of a bar: "1/8." is a dotted eighth and everybody knows it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Whole => "1/1",
            Self::HalfDotted => "1/2.",
            Self::Half => "1/2",
            Self::HalfTriplet => "1/2T",
            Self::QuarterDotted => "1/4.",
            Self::Quarter => "1/4",
            Self::QuarterTriplet => "1/4T",
            Self::EighthDotted => "1/8.",
            Self::Eighth => "1/8",
            Self::EighthTriplet => "1/8T",
            Self::SixteenthDotted => "1/16.",
            Self::Sixteenth => "1/16",
            Self::SixteenthTriplet => "1/16T",
            Self::ThirtySecond => "1/32",
        }
    }

    /// Strictly **descending by length**, which is not the order the three
    /// families would fall in if they were listed one after another: a dotted
    /// quarter (1.5 beats) is longer than a half-note triplet (1⅓), and an
    /// eighth-note triplet is shorter than a dotted sixteenth. Interleaved is
    /// what makes turning the knob shorten the delay monotonically, and
    /// `the_divisions_run_from_longest_to_shortest` is what caught the
    /// grouped-by-family order this replaced.
    pub const ALL: [Self; 14] = [
        Self::Whole,
        Self::HalfDotted,
        Self::Half,
        Self::QuarterDotted,
        Self::HalfTriplet,
        Self::Quarter,
        Self::EighthDotted,
        Self::QuarterTriplet,
        Self::Eighth,
        Self::SixteenthDotted,
        Self::EighthTriplet,
        Self::Sixteenth,
        Self::SixteenthTriplet,
        Self::ThirtySecond,
    ];
}

/// The chooser's positions, in `NoteDivision::ALL`'s order — written out for
/// the reason [`BAND_TYPES`] is.
static DIVISIONS: [&str; 14] = [
    "1/1", "1/2.", "1/2", "1/4.", "1/2T", "1/4", "1/8.", "1/4T", "1/8", "1/16.", "1/8T", "1/16",
    "1/16T", "1/32",
];

/// Everything the delay's sound depends on, and none of its four seconds of
/// memory (TDD §13.4).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DelayConfig {
    /// The time in milliseconds, in force when [`sync`](Self::sync) is off.
    ///
    /// Kept even while synced, so switching sync off gives back the time the
    /// knob was on rather than whatever the last note value happened to be.
    pub time_ms: f32,
    /// Take the time from the song's tempo instead of the millisecond knob.
    #[serde(default)]
    pub sync: bool,
    /// Which note value, when it does.
    #[serde(default = "an_eighth")]
    pub division: NoteDivision,
    /// How much of each repeat goes round again, 0..=1. Never quite 1: the
    /// spec's top is 95 %, because a loop at unity gain is an oscillator.
    pub feedback: f32,
    /// A low-pass **in the feedback path**, so each repeat is duller than the
    /// one before it. On the output instead it would be a tone control.
    pub damping_hz: f32,
    /// Saturation in the feedback path, 0..=1. What keeps a long feedback
    /// from piling up, and what makes repeats sit down the way tape's do.
    pub drive: f32,
    /// Each repeat on the opposite side from the last.
    pub ping_pong: bool,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`].
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl DelayConfig {
    pub fn new() -> Self {
        Self {
            time_ms: 300.0,
            sync: false,
            division: NoteDivision::Eighth,
            feedback: 0.35,
            damping_hz: 8_000.0,
            drive: 0.0,
            ping_pong: false,
            mix: DELAY_MIX / 100.0,
        }
    }

    /// The time this delay is actually asking for, at `bpm`.
    ///
    /// **The one place a note value becomes a duration.** It lives here rather
    /// than in `fontelle-fx` because what a dotted eighth *is* is a document
    /// fact, and rather than in the engine because the engine's job is to
    /// supply the tempo, not to interpret it.
    ///
    /// Clamped into what the line can hold, and defended against a tempo that
    /// is not a tempo: a malformed project can carry a zero, and every number
    /// downstream of this indexes a buffer.
    pub fn effective_time_ms(&self, bpm: f32) -> f32 {
        let asked = if self.sync {
            // A tempo of zero, a negative one, or a NaN all land on the
            // clamp's floor rather than on an infinite delay.
            let bpm = if bpm.is_finite() { bpm } else { 0.0 };
            self.division.beats() * 60_000.0 / bpm.clamp(MIN_BPM, MAX_BPM)
        } else {
            self.time_ms
        };
        asked.clamp(1.0, MAX_DELAY_MS)
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "time" => self.time_ms,
            "sync" => f32::from(u8::from(self.sync)),
            "division" => NoteDivision::ALL.iter().position(|d| *d == self.division)? as f32,
            "feedback" => self.feedback * 100.0,
            "damping" => self.damping_hz,
            "drive" => self.drive * 100.0,
            "pingpong" => f32::from(u8::from(self.ping_pong)),
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "time" => self.time_ms = value,
            "sync" => self.sync = value >= 0.5,
            "division" => {
                if let Some(division) = NoteDivision::ALL.get(value.round().max(0.0) as usize) {
                    self.division = *division;
                }
            }
            "feedback" => self.feedback = value / 100.0,
            "damping" => self.damping_hz = value,
            "drive" => self.drive = value / 100.0,
            "pingpong" => self.ping_pong = value >= 0.5,
            _ => {}
        }
    }
}

impl Default for DelayConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The longest delay the line holds, which is what `Delay::prepare` sizes it
/// for. Here rather than in `fontelle-fx` because it is the top of a
/// parameter's range, and INVARIANT 7 says a range is the document's.
///
/// Four seconds rather than two so that the **longest useful synced division
/// just fits**: a whole note at 60 bpm is exactly this, and anything past it
/// is a tempo slower than most music. A shorter line would silently clamp a
/// setting the chooser offers, which is worse than not offering it.
pub const MAX_DELAY_MS: f32 = 4_000.0;

/// The tempo range a synced delay will believe.
///
/// Wider than the transport's own limits on purpose: this is the last defence
/// before a number becomes a buffer index, and it has to hold for a tempo that
/// arrived from a file rather than from a person.
const MIN_BPM: f32 = 1.0;
const MAX_BPM: f32 = 1_000.0;

fn an_eighth() -> NoteDivision {
    NoteDivision::Eighth
}

static DELAY_PARAMS: [crate::ParamSpec; 8] = with_mix(&DELAY_OWN_PARAMS, DELAY_MIX);

static DELAY_OWN_PARAMS: [crate::ParamSpec; 7] = [
    crate::ParamSpec {
        id: "time",
        name: "Time",
        min: 1.0,
        max: MAX_DELAY_MS,
        default: 300.0,
        unit: crate::Unit::Milliseconds,
        // A time is a ratio: doubling it is one musical step wherever it
        // starts, and a linear lane would put every slapback in its first
        // twentieth.
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "sync",
        name: "Sync",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
        positions: &["off", "on"],
    },
    crate::ParamSpec {
        id: "division",
        name: "Division",
        min: 0.0,
        max: 13.0,
        // 1/8, which is where a delay gets set more often than anywhere else.
        default: 8.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(14),
        positions: &DIVISIONS,
    },
    crate::ParamSpec {
        id: "feedback",
        name: "Feedback",
        min: 0.0,
        // Not 100: a feedback loop at unity gain never decays, and one above
        // it is an oscillator that gets louder until something clips.
        max: 95.0,
        default: 35.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "damping",
        name: "Damping",
        min: 200.0,
        max: 20_000.0,
        default: 8_000.0,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "drive",
        name: "Drive",
        min: 0.0,
        max: 100.0,
        default: 0.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "pingpong",
        name: "Ping-pong",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
        positions: &["off", "on"],
    },
];

// ------------------------------------------------------------------ reverb

/// What a fresh reverb's tail sits at, as a percentage. See [`DELAY_MIX`].
const REVERB_MIX: f32 = 30.0;

/// Everything the reverb's sound depends on, and none of its network
/// (TDD §13.4).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReverbConfig {
    /// How far apart the walls are, 0..=1 — it scales every delay line in the
    /// network together, so it moves the first reflection and the density of
    /// the tail without touching how long the tail lasts.
    pub size: f32,
    /// RT60: how long the tail takes to fall 60 dB. Independent of `size`,
    /// which is what makes the two knobs two knobs.
    pub decay_s: f32,
    /// A low-pass **inside** the network, so each trip round takes a little
    /// more off the top — which is what air does, and what stops a long tail
    /// sounding like a metal tank.
    pub damping_hz: f32,
    /// The gap between the sound and the room answering it. What lets a big
    /// reverb sit behind a vocal rather than on top of it.
    pub pre_delay_ms: f32,
    /// How far apart the two sides of the tail are, 0..=1. At zero the tail is
    /// mono, which is what a mix destined for a mono system wants.
    pub width: f32,
    /// Dry/wet, 0..=1 — see [`EffectConfig::mix`].
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl ReverbConfig {
    pub fn new() -> Self {
        Self {
            size: 0.5,
            decay_s: 2.0,
            damping_hz: 6_000.0,
            pre_delay_ms: 0.0,
            width: 1.0,
            mix: REVERB_MIX / 100.0,
        }
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "size" => self.size * 100.0,
            "decay" => self.decay_s,
            "damping" => self.damping_hz,
            "predelay" => self.pre_delay_ms,
            "width" => self.width * 100.0,
            _ => return None,
        })
    }

    fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "size" => self.size = value / 100.0,
            "decay" => self.decay_s = value,
            "damping" => self.damping_hz = value,
            "predelay" => self.pre_delay_ms = value,
            "width" => self.width = value / 100.0,
            _ => {}
        }
    }
}

impl Default for ReverbConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The longest pre-delay the buffer holds, which is what `FdnReverb::prepare`
/// sizes it for. Here for the reason [`MAX_DELAY_MS`] is.
pub const MAX_PRE_DELAY_MS: f32 = 200.0;

static REVERB_PARAMS: [crate::ParamSpec; 6] = with_mix(&REVERB_OWN_PARAMS, REVERB_MIX);

static REVERB_OWN_PARAMS: [crate::ParamSpec; 5] = [
    crate::ParamSpec {
        id: "size",
        name: "Size",
        min: 0.0,
        max: 100.0,
        default: 50.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "decay",
        name: "Decay",
        min: 0.1,
        max: 20.0,
        default: 2.0,
        unit: crate::Unit::Seconds,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "damping",
        name: "Damping",
        min: 200.0,
        max: 20_000.0,
        default: 6_000.0,
        unit: crate::Unit::Hertz,
        taper: crate::Taper::Logarithmic,
        positions: &[],
    },
    crate::ParamSpec {
        id: "predelay",
        name: "Pre-delay",
        min: 0.0,
        max: MAX_PRE_DELAY_MS,
        default: 0.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    crate::ParamSpec {
        id: "width",
        name: "Width",
        min: 0.0,
        max: 100.0,
        default: 100.0,
        unit: crate::Unit::Percent,
        taper: crate::Taper::Linear,
        positions: &[],
    },
];
