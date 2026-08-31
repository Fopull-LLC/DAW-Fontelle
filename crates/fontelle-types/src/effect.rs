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
    Eq,
    Compressor,
}

impl EffectKind {
    /// What the insert says on the strip.
    pub fn label(self) -> &'static str {
        match self {
            Self::Eq => "EQ",
            Self::Compressor => "Comp",
        }
    }

    /// Every effect that can be put in an insert slot, in the order the "add"
    /// menu lists them.
    ///
    /// One list rather than two, for the reason the roll's lane properties are
    /// one list: two lists of the same things is one list to forget.
    pub const ALL: [Self; 2] = [Self::Eq, Self::Compressor];
}

/// The parameters of whichever effect a slot holds.
///
/// A sum type rather than a bag of named floats. The bag is what a plugin host
/// needs and what §8's stable-id `ParamSet` is for; an effect that ships in
/// this binary has a shape known at compile time, and giving it one means the
/// mixer panel can draw an EQ curve instead of eight rows of "param 3".
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum EffectConfig {
    Eq(EqConfig),
    Compressor(CompressorConfig),
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
            Self::Eq(_) => EQ_PARAMS.as_slice(),
            Self::Compressor(_) => COMPRESSOR_PARAMS.as_slice(),
        }
    }

    fn spec(&self, id: &str) -> Option<&'static crate::ParamSpec> {
        self.specs().iter().find(|spec| spec.id == id)
    }

    /// One parameter's value, in its own units. `None` for an id this effect
    /// does not have.
    pub fn get(&self, id: &str) -> Option<f32> {
        match self {
            Self::Eq(eq) => eq.get(id),
            Self::Compressor(comp) => comp.get(id),
        }
    }

    /// Writes one, clamped into what it can hold — and onto a step, if it has
    /// steps. An automation curve reaching its top must not be able to produce
    /// a filter frequency of a million.
    pub fn set(&mut self, id: &str, value: f32) {
        let Some(spec) = self.spec(id) else { return };
        let value = spec.clamp(value);
        match self {
            Self::Eq(eq) => eq.set(id, value),
            Self::Compressor(comp) => comp.set(id, value),
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
            Self::Eq(_) => EffectKind::Eq,
            Self::Compressor(_) => EffectKind::Compressor,
        }
    }

    /// A fresh instance of `kind`, at whatever settings make it audible-but-
    /// harmless: an effect somebody just added should change nothing until
    /// they touch it.
    pub fn new(kind: EffectKind) -> Self {
        match kind {
            EffectKind::Eq => Self::Eq(EqConfig::new()),
            EffectKind::Compressor => Self::Compressor(CompressorConfig::new()),
        }
    }
}

/// The EQ's parameters, one row per band per control.
///
/// A `const` table rather than a built `Vec`: the ids are `&'static str` and
/// must be, because they are what a saved automation clip names and INVARIANT
/// 7 says those never change. Written out by a macro so eight bands cannot
/// drift apart from each other.
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
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".gain"),
                name: concat!("Band ", $n, " gain"),
                min: -24.0,
                max: 24.0,
                default: 0.0,
                unit: crate::Unit::Decibels,
                taper: crate::Taper::Linear,
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".q"),
                name: concat!("Band ", $n, " Q"),
                min: 0.1,
                max: 24.0,
                default: BUTTERWORTH_Q,
                unit: crate::Unit::None,
                taper: crate::Taper::Logarithmic,
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".on"),
                name: concat!("Band ", $n, " on"),
                min: 0.0,
                max: 1.0,
                default: 0.0,
                unit: crate::Unit::Switch,
                taper: crate::Taper::Stepped(2),
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".type"),
                name: concat!("Band ", $n, " type"),
                min: 0.0,
                max: 10.0,
                default: 0.0,
                unit: crate::Unit::None,
                taper: crate::Taper::Stepped(11),
            },
            crate::ParamSpec {
                id: concat!("band", $n, ".channel"),
                name: concat!("Band ", $n, " channel"),
                min: 0.0,
                max: 2.0,
                default: 0.0,
                unit: crate::Unit::None,
                taper: crate::Taper::Stepped(3),
            },
        )*]
    };
}

/// Forty-eight of them, and every one addressable — which is what "any value
/// in these mixer effects can become an automation lane" means in practice.
static EQ_PARAMS: [crate::ParamSpec; 48] = eq_band_params!(1, 2, 3, 4, 5, 6, 7, 8);

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
}

impl EqConfig {
    pub fn new() -> Self {
        Self {
            bands: [EqBand::new(); BANDS],
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
        }
    }

    fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
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
static COMPRESSOR_PARAMS: [crate::ParamSpec; 8] = [
    crate::ParamSpec {
        id: "threshold",
        name: "Threshold",
        min: -60.0,
        max: 0.0,
        default: -18.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
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
    },
    crate::ParamSpec {
        id: "attack",
        name: "Attack",
        min: 0.05,
        max: 500.0,
        default: 10.0,
        unit: crate::Unit::Seconds,
        taper: crate::Taper::Logarithmic,
    },
    crate::ParamSpec {
        id: "release",
        name: "Release",
        min: 5.0,
        max: 5_000.0,
        default: 100.0,
        unit: crate::Unit::Seconds,
        taper: crate::Taper::Logarithmic,
    },
    crate::ParamSpec {
        id: "knee",
        name: "Knee",
        min: 0.0,
        max: 24.0,
        default: 6.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
    },
    crate::ParamSpec {
        id: "makeup",
        name: "Makeup",
        min: -12.0,
        max: 24.0,
        default: 0.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
    },
    crate::ParamSpec {
        id: "auto",
        name: "Auto makeup",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::Switch,
        taper: crate::Taper::Stepped(2),
    },
    crate::ParamSpec {
        id: "detection",
        name: "Detection",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(2),
    },
];
