//! A preset, for every device (`docs/flopsynth-plan.md` §P).
//!
//! > *"a preset system kind of like FL Studio's baked into the DAW itself that
//! > works for every instrument and effect so we don't have to hardcode presets
//! > in every plugin ... when you have a `*` for unsaved edits you're able to
//! > save it either to the same preset or save as to a new preset in your
//! > bank."* — Ty, 2026-09-06
//!
//! # The whole idea, in one paragraph
//!
//! A **preset is a file**: a name, a category, the device it is for, and that
//! device's own saved state. *Factory* presets are such files embedded in the
//! binary at build time; *user* presets are such files in a bank folder the
//! user owns. Every device — a built-in instrument, a built-in effect, a
//! hosted plugin — gets the same preset bar in its window, the same rows in
//! the browser's Presets tab, the same favourites and the same undo. What a
//! device contributes is nothing but *what its state is*; the system does the
//! rest.
//!
//! # Why it costs nothing per device
//!
//! [`PresetPayload`] invents no new shape. `PatchData` is what a channel
//! already stores, `EffectConfig` what an insert stores, `PluginState` what
//! either stores for a plugin. A device's state is already a serialisable
//! value the document holds, so saving one is writing that value to a file
//! with a name on it — which is why a preset system for *every* device is a
//! type and a folder walk rather than a feature per plugin.
//!
//! # Why it is in `fontelle-types`
//!
//! For the reason [`crate::Favorite`] is: the app writes the files, the window
//! draws the bar, the model stores which preset a channel came from, and none
//! of those three may depend on the others (§4.1).

use crate::{EffectConfig, EffectKind, InstrumentKind, PatchData, PluginKey, PluginState};

/// The revision of the preset file format this build writes.
///
/// Bumped when a change cannot be read by serde's own defaulting, on exactly
/// the reasoning [`fontelle_core::patch_format::PATCH_FORMAT_VERSION`] gives.
/// A file claiming a higher version is refused as *from the future* rather
/// than parsed into nonsense — see [`Preset::is_from_the_future`].
pub const PRESET_FORMAT_VERSION: u32 = 1;

/// Which device a preset is for.
///
/// Its [`slug`](DeviceKind::slug) is a **folder name**, and therefore
/// INVARIANT 7's: renaming one moves everybody's presets, so
/// `fontelle-types/tests/preset.rs` freezes the table with the literal strings
/// in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Instrument(InstrumentKind),
    Effect(EffectKind),
    /// A **mixer track's whole chain** — its level, its placement and every
    /// insert on it, saved and recalled as one thing.
    ///
    /// Not a device at all, strictly: it is a *rack* of them. It is here
    /// because everything a preset needs already works this way — a folder
    /// under the bank, a category, a name, a star, the same Save and Save-as —
    /// and a second mechanism beside it that did the same job for tracks would
    /// be a second set of bugs. There is one `track` folder rather than one
    /// per track, because a chain saved off a vocal is exactly the thing you
    /// want on a different vocal.
    Track,
    /// A hosted plugin, by the id it declares rather than the path it was
    /// found at — INVARIANT 8's reasoning, so a preset for a plugin survives
    /// that plugin being reinstalled somewhere else.
    Plugin(PluginKey),
}

impl DeviceKind {
    /// The folder this device's presets live in, under both the factory tree
    /// and the user's bank.
    ///
    /// Effects are prefixed `fx-` so that an instrument and an effect that
    /// share a name — Filter is both a `SvfMode` and an insert — can never
    /// share a folder.
    pub fn slug(&self) -> String {
        match self {
            Self::Instrument(kind) => match kind {
                InstrumentKind::SoundFont => "soundfont".to_string(),
                InstrumentKind::Osc3 => "3osc".to_string(),
                InstrumentKind::Sampler => "sampler".to_string(),
                InstrumentKind::DrumMachine => "drum-machine".to_string(),
                InstrumentKind::Flopsynth => "flopsynth".to_string(),
                // The odd one out, and it has to be: a channel of this kind
                // says its instrument is named somewhere else. A preset for
                // an actual plugin is `Self::Plugin`, which names which.
                InstrumentKind::Plugin => "plugin".to_string(),
            },
            Self::Effect(kind) => format!(
                "fx-{}",
                match kind {
                    EffectKind::Utility => "utility",
                    EffectKind::Eq => "eq",
                    EffectKind::Filter => "filter",
                    EffectKind::Compressor => "compressor",
                    EffectKind::Gate => "gate",
                    EffectKind::Distortion => "distortion",
                    EffectKind::Bitcrush => "bitcrush",
                    EffectKind::Soften => "soften",
                    EffectKind::Chorus => "chorus",
                    EffectKind::Delay => "delay",
                    EffectKind::Reverb => "reverb",
                    EffectKind::Tune => "tune",
                }
            ),
            // A plugin id is a reverse-domain name or a URI and may carry
            // anything the vendor liked, including slashes and colons. Every
            // character that is not portable in a folder name becomes `-`, so
            // the slug is one folder whatever the id was.
            Self::Track => "track".to_string(),
            Self::Plugin(key) => {
                let mut slug = format!("plugin-{}-", key.format.extension());
                for ch in key.id.chars() {
                    if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_') {
                        slug.push(ch);
                    } else {
                        slug.push('-');
                    }
                }
                slug
            }
        }
    }

    /// What the browser's row and the bar's tooltip call this device.
    pub fn label(&self) -> String {
        match self {
            Self::Instrument(kind) => kind.label().to_string(),
            Self::Effect(kind) => kind.label().to_string(),
            Self::Track => "Mixer track".to_string(),
            Self::Plugin(key) => key.id.clone(),
        }
    }
}

/// The device's own saved state — the three shapes the document already
/// stores, and deliberately no fourth.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetPayload {
    Patch(PatchData),
    Effect(EffectConfig),
    Plugin(PluginState),
    /// A mixer track's chain — see [`TrackChain`].
    Track(TrackChain),
}

/// A mixer track's chain, as a preset stores it.
///
/// **What is in it**: the level, the placement, the polarity, and every
/// built-in insert with its whole configuration and its bypass.
///
/// **What is deliberately not**, and why each:
///
/// - **The name.** A preset called "Rap Lead" applied to a track called
///   "Verse 2" should leave it called Verse 2. The preset names the *sound*,
///   the track names the *part*.
/// - **The sends.** A send points at another track by id, and an id from the
///   project it was saved in means nothing — or worse, something wrong — in
///   the project it is loaded into. A chain that silently re-pointed somebody's
///   reverb send at their drum bus is a worse failure than not carrying it.
/// - **The output and the input.** Routing and which microphone feeds the
///   track are facts about *this* session's wiring, not about a vocal sound.
/// - **Hosted plugins.** A preset that names a plugin the machine has not got
///   can only fail at load, and failing quietly in the middle of a chain is
///   the worst of the options. Saving skips them and says how many it left
///   out; the built-in inserts around them are kept.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackChain {
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub phase_invert: bool,
    #[serde(default)]
    pub inserts: Vec<TrackInsert>,
}

/// One insert in a saved chain.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TrackInsert {
    pub config: EffectConfig,
    #[serde(default)]
    pub bypassed: bool,
    /// The preset this insert was loaded from, if it was — the document's own
    /// [`PresetRef`], so a recalled chain's insert still says what it came
    /// from in its own preset bar, and still knows whether it has drifted from
    /// it, exactly as it did before the chain was saved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<PresetRef>,
}

impl TrackChain {
    /// An empty chain at unity — what a track that has had nothing done to it
    /// would save as.
    pub fn new() -> Self {
        Self {
            gain_db: 0.0,
            pan: 0.0,
            phase_invert: false,
            inserts: Vec::new(),
        }
    }
}

impl Default for TrackChain {
    fn default() -> Self {
        Self::new()
    }
}

/// The factory vocal chains.
///
/// **Named for the sound, not for a singer.** `docs/tune-plan.md` §13 refuses
/// a chooser of other products' voicings on the grounds that naming a control
/// after somebody else's plug-in is "a preset pretending to be a control", and
/// the same argument applies with more force to naming one after a person: it
/// promises something a chain of six inserts cannot deliver, and it is not
/// ours to promise. "Trap Lead" says what it is for; a name would say who to
/// blame when it does not sound like them.
///
/// A chain is a **starting point**, which is why every one of these is built
/// from the same handful of moves — clean up, control, colour, place — with
/// the settings varying rather than the idea.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackPreset {
    // ---- the corrected leads
    /// Hard tune, tight compression, a slap and a plate. The modern rap lead.
    RapLead,
    /// Harder, darker, and further back in the room.
    TrapLead,
    /// Correction you cannot hear, a wide chorus, a long tail.
    RnbSmooth,
    /// Bright, forward, and polished.
    PopLead,
    /// Drill's dark, gated, close sound.
    DrillLead,
    /// Crushed, bright and very wide.
    HyperpopLead,
    /// Correction and nothing else — the chain to start from.
    CleanCorrect,

    // ---- the uncorrected ones
    /// Compressed and open, with a long plate under it.
    BalladLead,
    /// Driven, mid-forward, with a slap.
    RockLead,
    /// A voice for talking: high-passed, levelled, de-essed.
    SpokenWord,
    /// Tucked under the lead — duller, narrower, quieter, wetter.
    BackingStack,
    /// Two voices where there was one.
    DoublerWide,

    // ---- the effects
    /// Band-limited and distorted.
    Telephone,
    /// Tape hiss and wow, roughly.
    LoFiTape,
    /// A robot reading the lyric.
    RobotVocal,
    /// Reversed-sounding wash for a chop.
    DreamWash,
}

impl TrackPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::RapLead => "Rap Lead",
            Self::TrapLead => "Trap Lead",
            Self::RnbSmooth => "R&B Smooth",
            Self::PopLead => "Pop Lead",
            Self::DrillLead => "Drill Lead",
            Self::HyperpopLead => "Hyperpop Lead",
            Self::CleanCorrect => "Clean Correct",
            Self::BalladLead => "Ballad Lead",
            Self::RockLead => "Rock Lead",
            Self::SpokenWord => "Spoken Word",
            Self::BackingStack => "Backing Stack",
            Self::DoublerWide => "Doubler Wide",
            Self::Telephone => "Telephone",
            Self::LoFiTape => "Lo-Fi Tape",
            Self::RobotVocal => "Robot Vocal",
            Self::DreamWash => "Dream Wash",
        }
    }

    /// Which shelf of the bank it sits on.
    pub fn category(self) -> &'static str {
        match self {
            Self::RapLead
            | Self::TrapLead
            | Self::RnbSmooth
            | Self::PopLead
            | Self::DrillLead
            | Self::HyperpopLead
            | Self::CleanCorrect => "Vocals \u{2014} tuned",
            Self::BalladLead
            | Self::RockLead
            | Self::SpokenWord
            | Self::BackingStack
            | Self::DoublerWide => "Vocals \u{2014} natural",
            Self::Telephone | Self::LoFiTape | Self::RobotVocal | Self::DreamWash => {
                "Vocals \u{2014} effects"
            }
        }
    }

    pub const ALL: [Self; 16] = [
        Self::RapLead,
        Self::TrapLead,
        Self::RnbSmooth,
        Self::PopLead,
        Self::DrillLead,
        Self::HyperpopLead,
        Self::CleanCorrect,
        Self::BalladLead,
        Self::RockLead,
        Self::SpokenWord,
        Self::BackingStack,
        Self::DoublerWide,
        Self::Telephone,
        Self::LoFiTape,
        Self::RobotVocal,
        Self::DreamWash,
    ];
}

/// One insert of a factory chain: a kind, and the handful of its parameters
/// that are not at their default.
fn fx(kind: EffectKind, set: &[(&str, f32)]) -> TrackInsert {
    let mut config = EffectConfig::new(kind);
    for (id, value) in set {
        config.set(id, *value);
    }
    TrackInsert {
        config,
        bypassed: false,
        preset: None,
    }
}

/// The corrector, from one of its own presets — so the two banks agree and a
/// chain does not re-type settings that already have a name.
fn tune(preset: crate::TunePreset) -> TrackInsert {
    TrackInsert {
        config: EffectConfig::Tune(crate::TuneConfig::from_preset(preset)),
        bypassed: false,
        preset: Some(PresetRef::new(
            preset.label(),
            "Factory",
            PresetOrigin::Factory,
        )),
    }
}

/// A vocal high-pass on EQ band 1 — the move nearly every chain starts with.
///
/// Its own function because it is four parameters that only mean anything
/// together: a band that is off, or a bell where a high pass was meant, is a
/// filter that quietly does nothing.
fn hp(hz: f32) -> [(&'static str, f32); 4] {
    [
        ("band1.on", 1.0),
        // 7 is `high pass 24` in the band-type list — twenty-four decibels an
        // octave, which is what takes a room out from under a voice without
        // taking the voice's own bottom with it.
        ("band1.type", 7.0),
        ("band1.freq", hz),
        ("band1.q", 0.7),
    ]
}

/// A bell on band `n`.
fn bell(band: usize, hz: f32, db: f32, q: f32) -> [(String, f32); 5] {
    [
        (format!("band{band}.on"), 1.0),
        (format!("band{band}.type"), 0.0),
        (format!("band{band}.freq"), hz),
        (format!("band{band}.gain"), db),
        (format!("band{band}.q"), q),
    ]
}

/// A high shelf on band `n` — the "air" move.
fn air(band: usize, hz: f32, db: f32) -> [(String, f32); 5] {
    [
        (format!("band{band}.on"), 1.0),
        (format!("band{band}.type"), 2.0),
        (format!("band{band}.freq"), hz),
        (format!("band{band}.gain"), db),
        (format!("band{band}.q"), 0.7),
    ]
}

/// An EQ built from the moves above.
fn eq(moves: Vec<(String, f32)>) -> TrackInsert {
    let mut config = EffectConfig::new(EffectKind::Eq);
    for (id, value) in &moves {
        config.set(id, *value);
    }
    TrackInsert {
        config,
        bypassed: false,
        preset: None,
    }
}

/// The four settings a vocal compressor is: how far down, how hard, how fast
/// in, how fast out — plus the make-up that keeps the chain at one level.
fn comp(threshold: f32, ratio: f32, attack: f32, release: f32, makeup: f32) -> TrackInsert {
    fx(
        EffectKind::Compressor,
        &[
            ("threshold", threshold),
            ("ratio", ratio),
            ("attack", attack),
            ("release", release),
            ("makeup", makeup),
            ("knee", 6.0),
        ],
    )
}

/// A reverb, by the four numbers that decide what room it is.
fn verb(size: f32, decay: f32, predelay: f32, damping: f32, mix: f32) -> TrackInsert {
    fx(
        EffectKind::Reverb,
        &[
            ("size", size),
            ("decay", decay),
            ("predelay", predelay),
            ("damping", damping),
            ("mix", mix),
        ],
    )
}

/// A tempo-synced delay: which division, how much comes back, how wet.
fn echo(division: f32, feedback: f32, mix: f32, damping: f32) -> TrackInsert {
    fx(
        EffectKind::Delay,
        &[
            ("sync", 1.0),
            ("division", division),
            ("feedback", feedback),
            ("mix", mix),
            ("damping", damping),
        ],
    )
}

/// A gate, for a close-mic'd voice in a room that is not silent.
fn gate(threshold: f32, release: f32) -> TrackInsert {
    fx(
        EffectKind::Gate,
        &[
            ("threshold", threshold),
            ("release", release),
            ("attack", 1.0),
            ("hold", 30.0),
            // Not all the way down: a gate that slams a breath to silence is
            // more audible than the breath was.
            ("range", -24.0),
        ],
    )
}

impl TrackChain {
    /// What a named factory chain is made of.
    ///
    /// The order is always the same and it is the order a person would build
    /// it in: **clean up** (gate, high pass), **control** (correction, then
    /// compression), **colour** (drive, crush, tone), **place** (delay, then
    /// reverb). Correction goes before compression because a corrector tracks
    /// pitch and a compressor changes level, and the tracker should hear the
    /// dynamics the singer actually sang.
    ///
    /// Every chain leaves `mix` on its reverb and delay well under half: these
    /// are inserts on the voice, not sends, and a starting point that arrives
    /// drowned is one whose first move is always the same.
    pub fn from_preset(preset: TrackPreset) -> Self {
        use crate::TunePreset as T;
        use EffectKind as K;
        let mut gain_db = 0.0;
        let inserts = match preset {
            // ---- the corrected leads ----------------------------------
            TrackPreset::RapLead => vec![
                gate(-46.0, 120.0),
                tune(T::HardTune),
                comp(-20.0, 4.0, 3.0, 90.0, 3.0),
                eq(hp(90.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain(bell(2, 400.0, -2.0, 1.1))
                    .chain(air(3, 8000.0, 2.5))
                    .collect()),
                echo(8.0, 22.0, 14.0, 4500.0),
                verb(62.0, 1.8, 18.0, 4800.0, 18.0),
            ],
            TrackPreset::TrapLead => vec![
                gate(-42.0, 90.0),
                tune(T::TrapRobot),
                comp(-22.0, 6.0, 1.0, 70.0, 4.0),
                eq(hp(110.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain(bell(2, 300.0, -3.0, 1.0))
                    .chain(air(3, 9000.0, 3.0))
                    .collect()),
                echo(6.0, 30.0, 18.0, 3800.0),
                verb(72.0, 2.6, 26.0, 4200.0, 22.0),
            ],
            TrackPreset::RnbSmooth => vec![
                eq(hp(80.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain(bell(2, 250.0, -1.5, 1.2))
                    .chain(air(3, 10000.0, 2.0))
                    .collect()),
                tune(T::PopPolish),
                comp(-24.0, 3.0, 8.0, 140.0, 3.0),
                fx(
                    K::Soften,
                    &[("shelf", 30.0), ("suppressor", 45.0), ("air", 35.0)],
                ),
                fx(
                    K::Chorus,
                    &[
                        ("voices", 2.0),
                        ("rate", 0.35),
                        ("depth", 28.0),
                        ("spread", 70.0),
                        ("mix", 20.0),
                    ],
                ),
                echo(8.0, 26.0, 12.0, 5200.0),
                verb(70.0, 2.4, 22.0, 5600.0, 20.0),
            ],
            TrackPreset::PopLead => vec![
                gate(-50.0, 150.0),
                tune(T::PopPolish),
                comp(-22.0, 4.0, 5.0, 110.0, 3.5),
                fx(K::Soften, &[("suppressor", 55.0), ("air", 45.0)]),
                eq(hp(95.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain(bell(2, 2500.0, 2.0, 0.9))
                    .chain(air(3, 11000.0, 3.5))
                    .collect()),
                echo(8.0, 18.0, 10.0, 6000.0),
                verb(58.0, 1.6, 14.0, 6000.0, 16.0),
            ],
            TrackPreset::DrillLead => vec![
                gate(-40.0, 70.0),
                tune(T::Drill),
                comp(-20.0, 5.0, 2.0, 80.0, 3.0),
                eq(hp(120.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain(bell(2, 500.0, -2.5, 1.0))
                    .chain(bell(3, 9000.0, -2.0, 0.7))
                    .collect()),
                verb(48.0, 1.1, 10.0, 3200.0, 14.0),
            ],
            TrackPreset::HyperpopLead => vec![
                tune(T::Hyperpop),
                comp(-24.0, 8.0, 1.0, 60.0, 4.0),
                fx(
                    K::Bitcrush,
                    &[
                        ("bits", 10.0),
                        ("rate", 22000.0),
                        ("mix", 35.0),
                        ("post_lp", 14000.0),
                    ],
                ),
                fx(
                    K::Chorus,
                    &[
                        ("rate", 1.2),
                        ("depth", 45.0),
                        ("spread", 100.0),
                        ("mix", 30.0),
                    ],
                ),
                echo(10.0, 34.0, 20.0, 7000.0),
                verb(80.0, 3.2, 8.0, 8000.0, 24.0),
            ],
            TrackPreset::CleanCorrect => vec![
                gate(-52.0, 160.0),
                tune(T::Transparent),
                comp(-20.0, 2.5, 12.0, 160.0, 2.0),
                eq(hp(85.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .collect()),
            ],

            // ---- the natural ones -------------------------------------
            TrackPreset::BalladLead => vec![
                comp(-26.0, 3.0, 15.0, 200.0, 3.5),
                eq(hp(75.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain(bell(2, 350.0, -1.5, 1.1))
                    .chain(air(3, 12000.0, 2.5))
                    .collect()),
                fx(K::Soften, &[("suppressor", 40.0), ("air", 30.0)]),
                echo(4.0, 24.0, 9.0, 5000.0),
                verb(85.0, 4.2, 35.0, 5200.0, 24.0),
            ],
            TrackPreset::RockLead => vec![
                gate(-44.0, 100.0),
                comp(-18.0, 4.5, 4.0, 90.0, 3.0),
                fx(
                    K::Distortion,
                    // 2 is `tube` — the curve that adds a second harmonic
                    // rather than a third, which on a voice is warmth and not
                    // grit.
                    &[
                        ("curve", 2.0),
                        ("drive", 8.0),
                        ("tone", 9000.0),
                        ("mix", 45.0),
                    ],
                ),
                eq(hp(100.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain(bell(2, 1800.0, 3.0, 1.0))
                    .collect()),
                echo(8.0, 20.0, 12.0, 4000.0),
                verb(66.0, 2.0, 20.0, 4500.0, 18.0),
            ],
            TrackPreset::SpokenWord => vec![
                gate(-44.0, 140.0),
                eq(hp(110.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain(bell(2, 320.0, -3.0, 1.2))
                    .chain(bell(3, 4200.0, 1.5, 0.8))
                    .collect()),
                comp(-22.0, 4.0, 8.0, 130.0, 4.0),
                fx(
                    K::Soften,
                    &[("suppressor", 60.0), ("shelf", 20.0), ("air", 15.0)],
                ),
            ],
            TrackPreset::BackingStack => {
                // Quieter as well as duller: tucking a stack under a lead is a
                // level decision first and an EQ decision second, and a chain
                // that only did the second would come back too loud.
                gain_db = -4.0;
                vec![
                    comp(-24.0, 4.0, 6.0, 120.0, 2.0),
                    eq(hp(140.0)
                        .map(|(id, v)| (id.to_string(), v))
                        .into_iter()
                        .chain(bell(2, 2800.0, -2.5, 0.9))
                        .chain(air(3, 10000.0, -2.0))
                        .collect()),
                    fx(
                        K::Chorus,
                        &[
                            ("voices", 3.0),
                            ("rate", 0.4),
                            ("depth", 35.0),
                            ("spread", 100.0),
                            ("mix", 25.0),
                        ],
                    ),
                    verb(74.0, 2.8, 24.0, 5000.0, 28.0),
                ]
            }
            TrackPreset::DoublerWide => vec![
                fx(
                    K::Chorus,
                    &[
                        ("voices", 2.0),
                        ("rate", 0.18),
                        ("depth", 22.0),
                        ("spread", 100.0),
                        ("delay", 24.0),
                        ("mix", 45.0),
                    ],
                ),
                // A very short slap either side of the middle is the other
                // half of a doubler: the chorus makes it two voices, this
                // makes them two *places*.
                fx(
                    K::Delay,
                    &[
                        ("time", 28.0),
                        ("feedback", 0.0),
                        ("pingpong", 1.0),
                        ("mix", 22.0),
                    ],
                ),
                eq(air(2, 9000.0, 1.5).into_iter().collect()),
            ],

            // ---- the effects ------------------------------------------
            TrackPreset::Telephone => vec![
                eq(hp(600.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain([
                        ("band2.on".to_string(), 1.0),
                        // 4 is `low pass 24`.
                        ("band2.type".to_string(), 4.0),
                        ("band2.freq".to_string(), 3000.0),
                        ("band2.q".to_string(), 0.7),
                    ])
                    .chain(bell(3, 1600.0, 5.0, 1.4))
                    .collect()),
                fx(
                    K::Distortion,
                    &[
                        ("curve", 1.0),
                        ("drive", 14.0),
                        ("tone", 3500.0),
                        ("mix", 60.0),
                    ],
                ),
                comp(-16.0, 8.0, 2.0, 60.0, 5.0),
            ],
            TrackPreset::LoFiTape => vec![
                fx(
                    K::Distortion,
                    &[
                        ("curve", 2.0),
                        ("drive", 10.0),
                        ("sag", 30.0),
                        ("tone", 7000.0),
                        ("mix", 55.0),
                    ],
                ),
                fx(
                    K::Bitcrush,
                    &[
                        ("bits", 12.0),
                        ("rate", 26000.0),
                        ("jitter", 18.0),
                        ("post_lp", 9000.0),
                        ("mix", 45.0),
                    ],
                ),
                fx(
                    K::Chorus,
                    &[
                        ("voices", 1.0),
                        ("rate", 0.22),
                        ("depth", 14.0),
                        ("mix", 30.0),
                    ],
                ),
                eq(hp(90.0)
                    .map(|(id, v)| (id.to_string(), v))
                    .into_iter()
                    .chain(air(2, 8000.0, -4.0))
                    .collect()),
                verb(55.0, 1.5, 12.0, 3600.0, 16.0),
            ],
            TrackPreset::RobotVocal => vec![
                tune(T::VocoderLite),
                fx(
                    K::Bitcrush,
                    &[
                        ("bits", 8.0),
                        ("rate", 12000.0),
                        ("mix", 60.0),
                        ("post_lp", 8000.0),
                    ],
                ),
                comp(-20.0, 6.0, 1.0, 60.0, 3.0),
                echo(8.0, 30.0, 18.0, 4000.0),
                verb(60.0, 2.0, 14.0, 4000.0, 18.0),
            ],
            TrackPreset::DreamWash => vec![
                tune(T::WhisperTwin),
                fx(
                    K::Chorus,
                    &[
                        ("voices", 4.0),
                        ("rate", 0.25),
                        ("depth", 55.0),
                        ("spread", 100.0),
                        ("mix", 40.0),
                    ],
                ),
                echo(10.0, 45.0, 28.0, 6500.0),
                // The long one. A wash is the only chain here whose reverb is
                // past a third: it *is* the sound rather than the room round
                // it.
                verb(95.0, 6.5, 45.0, 7000.0, 42.0),
            ],
        };
        Self {
            gain_db,
            pan: 0.0,
            phase_invert: false,
            inserts,
        }
    }
}

/// One preset file.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub format_version: u32,
    pub device: DeviceKind,
    pub name: String,
    pub category: String,
    pub payload: PresetPayload,
}

impl Preset {
    /// A preset of `device`, at the current format version.
    pub fn new(
        device: DeviceKind,
        name: impl Into<String>,
        category: impl Into<String>,
        payload: PresetPayload,
    ) -> Self {
        Self {
            format_version: PRESET_FORMAT_VERSION,
            device,
            name: name.into(),
            category: category.into(),
            payload,
        }
    }

    /// Whether the payload is the shape this device stores.
    ///
    /// The bank lists a file that fails this as *unreadable* rather than
    /// loading it: an effect's settings applied to an instrument is not a
    /// degraded preset, it is a preset for something else.
    pub fn is_consistent(&self) -> bool {
        match (&self.device, &self.payload) {
            // A plugin *instrument* stores a `PluginState`, not a patch —
            // which is the one place the two enums do not line up by name.
            (DeviceKind::Instrument(InstrumentKind::Plugin), PresetPayload::Plugin(_)) => true,
            (DeviceKind::Instrument(_), PresetPayload::Patch(_)) => true,
            (DeviceKind::Effect(kind), PresetPayload::Effect(config)) => config.kind() == *kind,
            (DeviceKind::Plugin(key), PresetPayload::Plugin(state)) => state.key == *key,
            (DeviceKind::Track, PresetPayload::Track(_)) => true,
            _ => false,
        }
    }

    /// Whether this file was written by a build newer than this one.
    pub fn is_from_the_future(&self) -> bool {
        self.format_version > PRESET_FORMAT_VERSION
    }
}

/// Whether a preset came with the program or from the user's own bank.
///
/// Two origins rather than a flag, because they behave differently in exactly
/// one way that matters everywhere: a factory preset is read-only, so "Save"
/// is disabled on one and "Save as…" is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetOrigin {
    Factory,
    User,
}

/// What a device remembers about the preset it was loaded from.
///
/// **The name is remembered; the cleanliness is recognised.** A device carries
/// this through every edit and never stores whether it is dirty — that is
/// computed by comparing the device's current payload against the bank's for
/// this ref, so an undo makes the `*` go out with nothing to remember. See
/// §P.6, which is Ty's replacement for the second half of the effects
/// catalogue's rule 10.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PresetRef {
    pub name: String,
    pub category: String,
    pub origin: PresetOrigin,
}

impl PresetRef {
    pub fn new(name: impl Into<String>, category: impl Into<String>, origin: PresetOrigin) -> Self {
        Self {
            name: name.into(),
            category: category.into(),
            origin,
        }
    }
}
