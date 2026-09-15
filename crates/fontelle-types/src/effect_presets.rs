//! Factory presets for the effects that shipped without any.
//!
//! > *"please also ensure that every built in effect plugin has a bunch of
//! > presets that will be generally useful in a wide variety of situations
//! > especially the compressor which im noticing has no presets right now."*
//!
//! Eight of the twelve built-in effects had no bank at all: the compressor,
//! the gate, the chorus, the delay, the reverb, the filter, the EQ and the
//! utility. These are their recipes, in the position the distortion's
//! [`DistortionPreset`](crate::DistortionPreset) holds (`effect.rs`): a
//! preset is a **constructor** that writes every knob and then has nothing
//! further to say — never a parameter (`docs/flopsynth-plan.md` §P.9, rule
//! 10). `cargo xtask export-factory-presets` runs each recipe once and writes
//! it under `assets/presets/fx-*/Factory/`, which is what the bank reads.
//!
//! **What "generally useful" was taken to mean.** Each bank covers the jobs
//! the effect is reached for on an ordinary mix, named by the job rather
//! than by the setting — *vocal leveler* rather than *3:1 slow* — because a
//! person opening a preset list is looking for a situation, not a number.
//! The extreme end is there too (*parallel crush*, *jet flange*, *infinite
//! pad*), because the test of a built-in is whether its knobs go somewhere
//! new, and a preset is how somebody finds out that they do. Every recipe is
//! held inside its parameters' own ranges by `tests/effect_presets.rs`, so
//! nothing here can ask a knob for a value the panel cannot draw.

use crate::effect::{
    BandType, ChorusConfig, ChorusMode, CompressorConfig, DelayConfig, DetectionMode, EqBand,
    EqConfig, FilterConfig, FilterShape, GateConfig, LfoWave, LimiterConfig, NoteDivision,
    ReverbConfig, UtilityConfig,
};

// ------------------------------------------------------------- compressor

/// The compressor's bank (`docs/effects-catalogue.md` §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CompressorPreset {
    /// Evens a vocal out without being heard doing it: RMS, soft knee, slow.
    VocalLeveler,
    /// The vocal pushed to the front: fast, hard, a lot of gain reduction.
    VocalSmash,
    /// The drum bus held together: low ratio, slow attack so the hits pass.
    DrumBusGlue,
    /// Drums with the transient let through and the body squeezed after it.
    PunchyDrums,
    /// A snare's crack, made of a medium attack and a quick release.
    SnareSnap,
    /// A kick kept tight: fast enough to hold the sustain, not the click.
    KickTight,
    /// A bass held at one level, the way the low end has to be.
    BassTightener,
    /// New York compression: hard squash, mostly dry.
    ParallelCrush,
    /// A master that moves by a decibel or two and no more.
    GentleMaster,
    /// A stop at the top: 20:1 and as fast as it goes.
    PeakStop,
    /// The sidechain pump — fast attack, a musical release. Feed the key.
    SidechainPump,
    /// An acoustic guitar smoothed rather than squashed.
    AcousticGuitar,
    /// A pad or a string part with its swells kept, just narrower.
    EvenPad,
    /// A piano's dynamics reined in a little, a lot of knee.
    Piano,
}

impl CompressorPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::VocalLeveler => "vocal leveler",
            Self::VocalSmash => "vocal smash",
            Self::DrumBusGlue => "drum bus glue",
            Self::PunchyDrums => "punchy drums",
            Self::SnareSnap => "snare snap",
            Self::KickTight => "kick tight",
            Self::BassTightener => "bass tightener",
            Self::ParallelCrush => "parallel crush",
            Self::GentleMaster => "gentle master",
            Self::PeakStop => "peak stop",
            Self::SidechainPump => "sidechain pump",
            Self::AcousticGuitar => "acoustic guitar",
            Self::EvenPad => "even pad",
            Self::Piano => "piano",
        }
    }

    pub const ALL: [Self; 14] = [
        Self::VocalLeveler,
        Self::VocalSmash,
        Self::DrumBusGlue,
        Self::PunchyDrums,
        Self::SnareSnap,
        Self::KickTight,
        Self::BassTightener,
        Self::ParallelCrush,
        Self::GentleMaster,
        Self::PeakStop,
        Self::SidechainPump,
        Self::AcousticGuitar,
        Self::EvenPad,
        Self::Piano,
    ];
}

impl CompressorConfig {
    /// Every knob, from the recipe.
    pub fn from_preset(preset: CompressorPreset) -> Self {
        use CompressorPreset::*;
        use DetectionMode::{Peak, Rms};
        // (threshold, ratio, attack, release, knee, makeup, auto, detection, mix)
        let (
            threshold_db,
            ratio,
            attack_ms,
            release_ms,
            knee_db,
            makeup_db,
            auto_makeup,
            detection,
            mix,
        ) = match preset {
            VocalLeveler => (-20.0, 3.0, 10.0, 120.0, 8.0, 0.0, true, Rms, 1.0),
            VocalSmash => (-30.0, 8.0, 2.0, 60.0, 4.0, 0.0, true, Peak, 1.0),
            DrumBusGlue => (-14.0, 2.5, 30.0, 200.0, 6.0, 2.0, false, Rms, 1.0),
            PunchyDrums => (-18.0, 4.0, 20.0, 80.0, 2.0, 3.0, false, Peak, 1.0),
            SnareSnap => (-16.0, 6.0, 15.0, 50.0, 0.0, 4.0, false, Peak, 1.0),
            KickTight => (-20.0, 5.0, 5.0, 40.0, 3.0, 3.0, false, Peak, 1.0),
            BassTightener => (-22.0, 4.0, 8.0, 150.0, 6.0, 0.0, true, Rms, 1.0),
            ParallelCrush => (-35.0, 12.0, 1.0, 80.0, 2.0, 8.0, false, Peak, 0.4),
            GentleMaster => (-10.0, 1.8, 40.0, 400.0, 12.0, 1.0, false, Rms, 1.0),
            PeakStop => (-6.0, 20.0, 0.1, 60.0, 0.0, 0.0, false, Peak, 1.0),
            SidechainPump => (-24.0, 10.0, 0.5, 250.0, 1.0, 0.0, false, Peak, 1.0),
            AcousticGuitar => (-18.0, 2.5, 25.0, 180.0, 10.0, 0.0, true, Rms, 1.0),
            EvenPad => (-20.0, 3.0, 60.0, 500.0, 12.0, 0.0, true, Rms, 1.0),
            Piano => (-16.0, 2.0, 35.0, 250.0, 10.0, 0.0, true, Rms, 1.0),
        };
        Self {
            threshold_db,
            ratio,
            attack_ms,
            release_ms,
            knee_db,
            makeup_db,
            auto_makeup,
            detection,
            mix,
        }
    }
}

// ---------------------------------------------------------------- limiter

/// The limiter's bank. A brickwall at the top of the range, and — the reason
/// the limiter takes a key — a **ducker** at the bottom, where the ceiling is
/// low enough that a kick keyed into it pushes the track down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum LimiterPreset {
    /// The sidechain duck: a low ceiling for the key to clear, a musical
    /// release, and no look-ahead to speak of so the duck sits right on the
    /// beat. Key it from the kick.
    Ducking,
    /// A tighter, faster duck — a shorter release, so the track comes back up
    /// between hits rather than staying down.
    TightDuck,
    /// A slow swell back up, for pads under a four-to-the-floor kick.
    PumpingPad,
    /// A transparent mastering ceiling, just under full scale with enough
    /// look-ahead to catch inter-sample peaks.
    Master,
    /// Loud: a lower ceiling and a fast release, pushed for a competitive
    /// level rather than transparency.
    Loud,
    /// A gentle duck for when a full sidechain pump is too much — the ceiling
    /// only a little below the material and a slow release, so the track dips
    /// under the kick rather than getting out of its way entirely.
    GentleDuck,
}

impl LimiterPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ducking => "ducking",
            Self::TightDuck => "tight duck",
            Self::PumpingPad => "pumping pad",
            Self::Master => "master",
            Self::Loud => "loud",
            Self::GentleDuck => "gentle duck",
        }
    }

    pub const ALL: [Self; 6] = [
        Self::Ducking,
        Self::TightDuck,
        Self::PumpingPad,
        Self::Master,
        Self::Loud,
        Self::GentleDuck,
    ];
}

impl LimiterConfig {
    pub fn from_preset(preset: LimiterPreset) -> Self {
        use LimiterPreset::*;
        // (ceiling_db, release_ms)
        let (ceiling_db, release_ms) = match preset {
            Ducking => (-12.0, 200.0),
            TightDuck => (-14.0, 90.0),
            PumpingPad => (-10.0, 400.0),
            Master => (-0.3, 60.0),
            Loud => (-1.0, 30.0),
            GentleDuck => (-7.0, 350.0),
        };
        Self {
            ceiling_db,
            release_ms,
            mix: 1.0,
        }
    }
}

// ------------------------------------------------------------------- gate

/// The gate's bank (`docs/effects-catalogue.md` §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GatePreset {
    /// Drums with the bleed gone: fast, a little lookahead, hard.
    DrumGateTight,
    /// A kick on its own — the key listens to everything, the hold is long
    /// enough for the whole hit.
    KickIsolate,
    /// A snare with the hats kept out of its key.
    SnareTighten,
    /// Toms, which ring: a longer hold and release so the ring is kept.
    TomCleanup,
    /// A vocal's room noise turned down between phrases, not cut off.
    VocalNoiseCut,
    /// Amp hum and hiss under a guitar part, gone when it stops.
    GuitarHum,
    /// A 2:1 downward expander: quieter quiet parts, nothing cut.
    GentleExpander,
    /// Instant open and shut on a hard threshold: rhythmic chopping.
    StutterChop,
    /// A reverb tail closed early, the gated-drums sound.
    ReverbTailCut,
    /// A bass with the fret noise and hum between notes gone.
    BassClean,
}

impl GatePreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::DrumGateTight => "drum gate tight",
            Self::KickIsolate => "kick isolate",
            Self::SnareTighten => "snare tighten",
            Self::TomCleanup => "tom cleanup",
            Self::VocalNoiseCut => "vocal noise cut",
            Self::GuitarHum => "guitar hum",
            Self::GentleExpander => "gentle expander",
            Self::StutterChop => "stutter chop",
            Self::ReverbTailCut => "reverb tail cut",
            Self::BassClean => "bass clean",
        }
    }

    pub const ALL: [Self; 10] = [
        Self::DrumGateTight,
        Self::KickIsolate,
        Self::SnareTighten,
        Self::TomCleanup,
        Self::VocalNoiseCut,
        Self::GuitarHum,
        Self::GentleExpander,
        Self::StutterChop,
        Self::ReverbTailCut,
        Self::BassClean,
    ];
}

impl GateConfig {
    pub fn from_preset(preset: GatePreset) -> Self {
        use GatePreset::*;
        // (threshold, hysteresis, key hp, lookahead, attack, hold, release, ratio, range)
        let (
            threshold_db,
            hysteresis_db,
            key_hp_hz,
            lookahead_ms,
            attack_ms,
            hold_ms,
            release_ms,
            ratio,
            range_db,
        ) = match preset {
            DrumGateTight => (-30.0, 4.0, 80.0, 2.0, 0.2, 30.0, 60.0, 100.0, -80.0),
            KickIsolate => (-28.0, 3.0, 20.0, 3.0, 0.1, 40.0, 80.0, 100.0, -80.0),
            SnareTighten => (-26.0, 4.0, 150.0, 1.0, 0.1, 50.0, 100.0, 100.0, -60.0),
            TomCleanup => (-32.0, 5.0, 60.0, 2.0, 0.3, 80.0, 150.0, 100.0, -80.0),
            VocalNoiseCut => (-45.0, 6.0, 100.0, 5.0, 2.0, 100.0, 300.0, 4.0, -20.0),
            GuitarHum => (-40.0, 5.0, 120.0, 1.0, 0.5, 60.0, 120.0, 20.0, -40.0),
            GentleExpander => (-38.0, 3.0, 20.0, 0.0, 5.0, 20.0, 250.0, 2.0, -12.0),
            StutterChop => (-20.0, 2.0, 20.0, 0.0, 0.1, 0.0, 20.0, 100.0, -80.0),
            ReverbTailCut => (-34.0, 6.0, 200.0, 4.0, 1.0, 150.0, 400.0, 100.0, -30.0),
            BassClean => (-40.0, 4.0, 20.0, 2.0, 1.0, 60.0, 200.0, 100.0, -50.0),
        };
        Self {
            threshold_db,
            hysteresis_db,
            key_hp_hz,
            lookahead_ms,
            attack_ms,
            hold_ms,
            release_ms,
            ratio,
            range_db,
            mix: 1.0,
        }
    }
}

// ----------------------------------------------------------------- chorus

/// The chorus's bank (`docs/effects-catalogue.md` §2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ChorusPreset {
    /// The pedal: two voices, a moderate sweep.
    ClassicPedal,
    /// Barely there — a little width on something too narrow.
    SubtleWiden,
    /// The string machine: three voices on their own clocks.
    StringEnsemble,
    /// Four voices spread all the way round, slow: the wide one.
    Dimension,
    /// One voice, all wet, fast and deep: a vibrato.
    Vibrato,
    /// The classic flange: short delay, high positive feedback.
    FlangerSweep,
    /// Negative feedback, as deep and slow as it goes: the jet.
    JetFlange,
    /// The 1980s: three voices, a touch of feedback, the top rolled off.
    EightiesSynth,
    /// A warped tape: one slow deep wobble with the top gone.
    LoFiWobble,
    /// Locked to the clock, a quarter note round.
    SyncedPulse,
}

impl ChorusPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::ClassicPedal => "classic pedal",
            Self::SubtleWiden => "subtle widen",
            Self::StringEnsemble => "string ensemble",
            Self::Dimension => "dimension",
            Self::Vibrato => "vibrato",
            Self::FlangerSweep => "flanger sweep",
            Self::JetFlange => "jet flange",
            Self::EightiesSynth => "80s synth",
            Self::LoFiWobble => "lo-fi wobble",
            Self::SyncedPulse => "synced pulse",
        }
    }

    pub const ALL: [Self; 10] = [
        Self::ClassicPedal,
        Self::SubtleWiden,
        Self::StringEnsemble,
        Self::Dimension,
        Self::Vibrato,
        Self::FlangerSweep,
        Self::JetFlange,
        Self::EightiesSynth,
        Self::LoFiWobble,
        Self::SyncedPulse,
    ];
}

impl ChorusConfig {
    pub fn from_preset(preset: ChorusPreset) -> Self {
        use ChorusMode::{Chorus, Ensemble};
        use ChorusPreset::*;
        // (voices, mode, spread, rate, sync, depth, delay, feedback, tone, mix)
        let (voices, mode, spread, rate_hz, sync, depth, delay_ms, feedback, tone_hz, mix) =
            match preset {
                ClassicPedal => (2, Chorus, 0.5, 0.6, false, 0.45, 12.0, 0.0, 20_000.0, 0.5),
                SubtleWiden => (2, Chorus, 0.8, 0.25, false, 0.2, 10.0, 0.0, 20_000.0, 0.35),
                StringEnsemble => (3, Ensemble, 0.7, 0.8, false, 0.6, 18.0, 0.0, 9_000.0, 0.6),
                Dimension => (4, Chorus, 1.0, 0.3, false, 0.35, 20.0, 0.0, 20_000.0, 0.5),
                Vibrato => (1, Chorus, 0.0, 5.5, false, 0.5, 8.0, 0.0, 20_000.0, 1.0),
                FlangerSweep => (2, Chorus, 0.5, 0.2, false, 0.9, 5.0, 0.75, 20_000.0, 0.5),
                JetFlange => (2, Chorus, 0.5, 0.12, false, 1.0, 5.0, -0.85, 20_000.0, 0.5),
                EightiesSynth => (3, Chorus, 0.6, 0.9, false, 0.4, 14.0, 0.1, 12_000.0, 0.5),
                LoFiWobble => (1, Chorus, 0.0, 2.5, false, 0.7, 25.0, 0.0, 3_500.0, 0.5),
                SyncedPulse => (2, Chorus, 0.5, 1.0, true, 0.5, 15.0, 0.0, 20_000.0, 0.5),
            };
        Self {
            voices,
            mode,
            spread,
            rate_hz,
            sync,
            division: NoteDivision::Quarter,
            depth,
            delay_ms,
            feedback,
            tone_hz,
            mix,
        }
    }
}

// ------------------------------------------------------------------ delay

/// The delay's bank (`docs/effects-catalogue.md` §2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DelayPreset {
    /// One short repeat: the rockabilly vocal, the country guitar.
    Slapback,
    /// An eighth on the clock, a few repeats.
    EighthNote,
    /// The dotted eighth — the one that makes a guitar line into a rhythm.
    DottedEighth,
    /// A quarter, few repeats, a little dark.
    QuarterEcho,
    /// Eighths bouncing left and right.
    PingPongEighth,
    /// Dotted eighths bouncing, longer.
    PingPongDotted,
    /// Tape: free-running, dark, driven, many repeats.
    TapeEcho,
    /// Dub: dotted quarters, feedback near runaway, the top gone, driven.
    DubEcho,
    /// A doubler: one repeat too short to hear as one.
    Doubler,
    /// Over a second, many repeats: a wash behind a pad.
    LongWash,
    /// Eighth triplets, bouncing.
    TripletBounce,
    /// Sixteenths, quick and quiet: a trail behind a lead.
    SixteenthTrail,
}

impl DelayPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Slapback => "slapback",
            Self::EighthNote => "eighth note",
            Self::DottedEighth => "dotted eighth",
            Self::QuarterEcho => "quarter echo",
            Self::PingPongEighth => "ping pong eighth",
            Self::PingPongDotted => "ping pong dotted",
            Self::TapeEcho => "tape echo",
            Self::DubEcho => "dub echo",
            Self::Doubler => "doubler",
            Self::LongWash => "long wash",
            Self::TripletBounce => "triplet bounce",
            Self::SixteenthTrail => "sixteenth trail",
        }
    }

    pub const ALL: [Self; 12] = [
        Self::Slapback,
        Self::EighthNote,
        Self::DottedEighth,
        Self::QuarterEcho,
        Self::PingPongEighth,
        Self::PingPongDotted,
        Self::TapeEcho,
        Self::DubEcho,
        Self::Doubler,
        Self::LongWash,
        Self::TripletBounce,
        Self::SixteenthTrail,
    ];
}

impl DelayConfig {
    pub fn from_preset(preset: DelayPreset) -> Self {
        use DelayPreset::*;
        use NoteDivision::*;
        // (time, sync, division, feedback, damping, drive, ping pong, mix)
        let (time_ms, sync, division, feedback, damping_hz, drive, ping_pong, mix) = match preset {
            Slapback => (110.0, false, Eighth, 0.1, 6_000.0, 0.0, false, 0.3),
            EighthNote => (300.0, true, Eighth, 0.4, 8_000.0, 0.0, false, 0.35),
            DottedEighth => (300.0, true, EighthDotted, 0.45, 7_000.0, 0.0, false, 0.35),
            QuarterEcho => (500.0, true, Quarter, 0.5, 6_000.0, 0.0, false, 0.3),
            PingPongEighth => (300.0, true, Eighth, 0.5, 7_000.0, 0.0, true, 0.35),
            PingPongDotted => (300.0, true, EighthDotted, 0.55, 6_000.0, 0.0, true, 0.4),
            TapeEcho => (380.0, false, Eighth, 0.6, 3_000.0, 0.4, false, 0.35),
            DubEcho => (750.0, true, QuarterDotted, 0.78, 2_200.0, 0.55, true, 0.45),
            Doubler => (22.0, false, Eighth, 0.0, 12_000.0, 0.0, false, 0.5),
            LongWash => (1_200.0, false, Eighth, 0.7, 4_000.0, 0.0, false, 0.3),
            TripletBounce => (300.0, true, EighthTriplet, 0.45, 8_000.0, 0.0, true, 0.3),
            SixteenthTrail => (150.0, true, Sixteenth, 0.35, 9_000.0, 0.0, false, 0.25),
        };
        Self {
            time_ms,
            sync,
            division,
            feedback,
            damping_hz,
            drive,
            ping_pong,
            mix,
        }
    }
}

// ----------------------------------------------------------------- reverb

/// The reverb's bank (`docs/effects-catalogue.md` §2.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ReverbPreset {
    /// A small room: short, a little bright.
    SmallRoom,
    /// The vocal plate: medium, wide, pre-delayed off the voice.
    VocalPlate,
    /// A large hall.
    LargeHall,
    /// A cathedral: as big as it goes, long, dark.
    Cathedral,
    /// A drum room: short and lively.
    DrumRoom,
    /// Ambience: barely a tail, just a sense of a place.
    TightAmbience,
    /// A chamber: between the room and the hall.
    Chamber,
    /// A long dark wash with the top rolled off.
    DarkWash,
    /// Long and bright: a shimmer behind a pad.
    BrightShimmer,
    /// As long as it goes, as wet as a send: the pad *is* the reverb.
    InfinitePad,
    /// A room kept narrow, for something that must stay in the middle.
    NarrowRoom,
}

impl ReverbPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::SmallRoom => "small room",
            Self::VocalPlate => "vocal plate",
            Self::LargeHall => "large hall",
            Self::Cathedral => "cathedral",
            Self::DrumRoom => "drum room",
            Self::TightAmbience => "tight ambience",
            Self::Chamber => "chamber",
            Self::DarkWash => "dark wash",
            Self::BrightShimmer => "bright shimmer",
            Self::InfinitePad => "infinite pad",
            Self::NarrowRoom => "narrow room",
        }
    }

    pub const ALL: [Self; 11] = [
        Self::SmallRoom,
        Self::VocalPlate,
        Self::LargeHall,
        Self::Cathedral,
        Self::DrumRoom,
        Self::TightAmbience,
        Self::Chamber,
        Self::DarkWash,
        Self::BrightShimmer,
        Self::InfinitePad,
        Self::NarrowRoom,
    ];
}

impl ReverbConfig {
    pub fn from_preset(preset: ReverbPreset) -> Self {
        use ReverbPreset::*;
        // (size, decay, damping, pre-delay, width, mix)
        let (size, decay_s, damping_hz, pre_delay_ms, width, mix) = match preset {
            SmallRoom => (0.25, 0.6, 5_000.0, 5.0, 0.8, 0.2),
            VocalPlate => (0.5, 1.8, 7_000.0, 20.0, 1.0, 0.25),
            LargeHall => (0.9, 3.5, 4_500.0, 30.0, 1.0, 0.3),
            Cathedral => (1.0, 8.0, 3_000.0, 60.0, 1.0, 0.35),
            DrumRoom => (0.35, 0.9, 6_500.0, 10.0, 0.9, 0.2),
            TightAmbience => (0.15, 0.35, 8_000.0, 0.0, 0.6, 0.15),
            Chamber => (0.6, 2.5, 5_500.0, 15.0, 0.9, 0.25),
            DarkWash => (0.7, 5.0, 1_800.0, 40.0, 1.0, 0.3),
            BrightShimmer => (0.8, 6.0, 14_000.0, 50.0, 1.0, 0.3),
            InfinitePad => (1.0, 18.0, 2_500.0, 80.0, 1.0, 0.5),
            NarrowRoom => (0.3, 0.8, 5_000.0, 8.0, 0.3, 0.2),
        };
        Self {
            size,
            decay_s,
            damping_hz,
            pre_delay_ms,
            width,
            mix,
        }
    }
}

// ----------------------------------------------------------------- filter

/// The filter's bank (`docs/effects-catalogue.md` §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FilterPreset {
    /// A static low-pass, a little resonance: the tone knob.
    LowPassWarm,
    /// A narrow band in the middle: the telephone.
    TelephoneBand,
    /// The envelope opens the filter as the playing gets louder.
    AutoWah,
    /// The envelope *closes* it: a duck with a tone.
    ReverseWah,
    /// A slow sine on the cutoff, free-running.
    SlowSweep,
    /// A synced eighth-note wobble, resonant: the bass-music one.
    SyncedWobble,
    /// Sample-and-hold on sixteenths: the random steps.
    SampleAndHold,
    /// A high-pass that thins: everything under 300 gone.
    HighPassThin,
    /// A gentle high-pass for the rumble under everything.
    RumbleCut,
    /// The acid squelch: high resonance, drive, a fast envelope.
    AcidSquelch,
    /// A slow notch: the phaser you make out of a filter.
    NotchPhase,
    /// A square LFO on sixteenths: a gate made of tone.
    SquareChop,
}

impl FilterPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::LowPassWarm => "low pass warm",
            Self::TelephoneBand => "telephone band",
            Self::AutoWah => "auto wah",
            Self::ReverseWah => "reverse wah",
            Self::SlowSweep => "slow sweep",
            Self::SyncedWobble => "synced wobble",
            Self::SampleAndHold => "sample & hold",
            Self::HighPassThin => "high pass thin",
            Self::RumbleCut => "rumble cut",
            Self::AcidSquelch => "acid squelch",
            Self::NotchPhase => "notch phase",
            Self::SquareChop => "square chop",
        }
    }

    pub const ALL: [Self; 12] = [
        Self::LowPassWarm,
        Self::TelephoneBand,
        Self::AutoWah,
        Self::ReverseWah,
        Self::SlowSweep,
        Self::SyncedWobble,
        Self::SampleAndHold,
        Self::HighPassThin,
        Self::RumbleCut,
        Self::AcidSquelch,
        Self::NotchPhase,
        Self::SquareChop,
    ];
}

impl FilterConfig {
    pub fn from_preset(preset: FilterPreset) -> Self {
        use FilterPreset::*;
        use FilterShape::*;
        use LfoWave::{SampleHold, Sine, Square, Triangle};
        use NoteDivision::{Eighth, Quarter, Sixteenth};
        // (shape, cutoff, resonance, drive, env, env attack, env release,
        //  lfo, lfo rate, lfo sync, lfo division, lfo wave)
        let (
            shape,
            cutoff_hz,
            resonance,
            drive,
            env_amount,
            env_attack_ms,
            env_release_ms,
            lfo_amount,
            lfo_rate_hz,
            lfo_sync,
            lfo_division,
            lfo_wave,
        ) = match preset {
            LowPassWarm => (
                LowPass24, 800.0, 0.3, 0.0, 0.0, 5.0, 200.0, 0.0, 1.0, false, Quarter, Sine,
            ),
            TelephoneBand => (
                BandPass12, 1_500.0, 0.5, 0.0, 0.0, 5.0, 200.0, 0.0, 1.0, false, Quarter, Sine,
            ),
            AutoWah => (
                LowPass12, 400.0, 0.55, 0.0, 0.7, 5.0, 250.0, 0.0, 1.0, false, Quarter, Sine,
            ),
            ReverseWah => (
                LowPass24, 3_000.0, 0.4, 0.0, -0.6, 10.0, 300.0, 0.0, 1.0, false, Quarter, Sine,
            ),
            SlowSweep => (
                LowPass24, 1_200.0, 0.35, 0.0, 0.0, 5.0, 200.0, 0.6, 0.2, false, Quarter, Sine,
            ),
            SyncedWobble => (
                LowPass24, 600.0, 0.6, 0.0, 0.0, 5.0, 200.0, 0.8, 1.0, true, Eighth, Sine,
            ),
            SampleAndHold => (
                LowPass24, 900.0, 0.5, 0.0, 0.0, 5.0, 200.0, 0.7, 1.0, true, Sixteenth, SampleHold,
            ),
            HighPassThin => (
                HighPass24, 300.0, 0.1, 0.0, 0.0, 5.0, 200.0, 0.0, 1.0, false, Quarter, Sine,
            ),
            RumbleCut => (
                HighPass12, 60.0, 0.0, 0.0, 0.0, 5.0, 200.0, 0.0, 1.0, false, Quarter, Sine,
            ),
            AcidSquelch => (
                LowPass24, 500.0, 0.85, 0.5, 0.5, 2.0, 120.0, 0.0, 1.0, false, Quarter, Sine,
            ),
            NotchPhase => (
                Notch, 1_000.0, 0.6, 0.0, 0.0, 5.0, 200.0, 0.5, 0.3, false, Quarter, Triangle,
            ),
            SquareChop => (
                LowPass24, 2_000.0, 0.2, 0.0, 0.0, 5.0, 200.0, 1.0, 1.0, true, Sixteenth, Square,
            ),
        };
        Self {
            shape,
            cutoff_hz,
            resonance,
            drive,
            env_amount,
            env_attack_ms,
            env_release_ms,
            lfo_amount,
            lfo_rate_hz,
            lfo_sync,
            lfo_division,
            lfo_wave,
            output_db: 0.0,
            mix: 1.0,
        }
    }
}

// --------------------------------------------------------------------- EQ

/// The EQ's bank (`docs/effects-catalogue.md` §2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EqPreset {
    /// A high-pass at 80: the first move on almost anything that is not bass.
    LowCut80,
    /// A sub cut at 30, steep: what goes on the master.
    SubCut30,
    /// A vocal brought forward: the mud out, the presence and air in.
    VocalPresence,
    /// A kick: weight at 60, the box out, the beater in.
    KickPunch,
    /// A bass: the sub kept, the fundamental lifted, the fizz gone.
    BassFundamental,
    /// An acoustic guitar: the boom out, the string in.
    AcousticGuitar,
    /// The telephone: 400 to 3400 and nothing else.
    Telephone,
    /// Two cuts in the low mids.
    MudCut,
    /// Two cuts where a voice or a cymbal hurts.
    DeHarsh,
    /// A tilt towards the top.
    TiltBright,
    /// A tilt towards the bottom.
    TiltDark,
    /// A radio: band-limited with a bump in the middle.
    LoFiRadio,
    /// The smile: bottom and top up, the middle down.
    Smile,
    /// Air: one high shelf, up.
    Air,
}

impl EqPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::LowCut80 => "low cut 80",
            Self::SubCut30 => "sub cut 30",
            Self::VocalPresence => "vocal presence",
            Self::KickPunch => "kick punch",
            Self::BassFundamental => "bass fundamental",
            Self::AcousticGuitar => "acoustic guitar",
            Self::Telephone => "telephone",
            Self::MudCut => "mud cut",
            Self::DeHarsh => "de-harsh",
            Self::TiltBright => "tilt bright",
            Self::TiltDark => "tilt dark",
            Self::LoFiRadio => "lo-fi radio",
            Self::Smile => "smile",
            Self::Air => "air",
        }
    }

    pub const ALL: [Self; 14] = [
        Self::LowCut80,
        Self::SubCut30,
        Self::VocalPresence,
        Self::KickPunch,
        Self::BassFundamental,
        Self::AcousticGuitar,
        Self::Telephone,
        Self::MudCut,
        Self::DeHarsh,
        Self::TiltBright,
        Self::TiltDark,
        Self::LoFiRadio,
        Self::Smile,
        Self::Air,
    ];
}

/// One enabled band. `q` is the bell's width; shelves and passes take the
/// Butterworth value the default band carries, since it is the flat one.
fn band(band_type: BandType, freq_hz: f32, gain_db: f32, q: f32) -> EqBand {
    EqBand {
        band_type,
        freq_hz,
        gain_db,
        q,
        enabled: true,
        ..EqBand::new()
    }
}

impl EqConfig {
    pub fn from_preset(preset: EqPreset) -> Self {
        use BandType::*;
        use EqPreset::*;
        let q = crate::effect::BUTTERWORTH_Q;
        let on: Vec<EqBand> = match preset {
            LowCut80 => vec![band(HighPass24, 80.0, 0.0, q)],
            SubCut30 => vec![band(HighPass48, 30.0, 0.0, q)],
            VocalPresence => vec![
                band(HighPass24, 100.0, 0.0, q),
                band(Bell, 250.0, -2.0, 1.0),
                band(Bell, 3_000.0, 3.0, 1.0),
                band(HighShelf, 10_000.0, 2.0, q),
            ],
            KickPunch => vec![
                band(LowShelf, 60.0, 3.0, q),
                band(Bell, 300.0, -3.0, 1.2),
                band(Bell, 4_000.0, 3.0, 1.0),
            ],
            BassFundamental => vec![
                band(HighPass24, 30.0, 0.0, q),
                band(LowShelf, 80.0, 2.0, q),
                band(Bell, 700.0, 2.0, 1.0),
                band(LowPass12, 8_000.0, 0.0, q),
            ],
            AcousticGuitar => vec![
                band(HighPass24, 80.0, 0.0, q),
                band(Bell, 200.0, -2.0, 1.0),
                band(Bell, 2_500.0, 2.0, 1.0),
                band(HighShelf, 8_000.0, 1.5, q),
            ],
            Telephone => vec![
                band(HighPass48, 400.0, 0.0, q),
                band(LowPass48, 3_400.0, 0.0, q),
            ],
            MudCut => vec![band(Bell, 250.0, -3.0, 1.0), band(Bell, 500.0, -1.5, 1.0)],
            DeHarsh => vec![
                band(Bell, 3_500.0, -3.0, 2.0),
                band(Bell, 6_500.0, -2.0, 2.0),
            ],
            TiltBright => vec![
                band(LowShelf, 300.0, -2.0, q),
                band(HighShelf, 3_000.0, 2.0, q),
            ],
            TiltDark => vec![
                band(LowShelf, 300.0, 2.0, q),
                band(HighShelf, 3_000.0, -2.0, q),
            ],
            LoFiRadio => vec![
                band(HighPass24, 300.0, 0.0, q),
                band(Bell, 1_500.0, 3.0, 1.0),
                band(LowPass24, 4_000.0, 0.0, q),
            ],
            Smile => vec![
                band(LowShelf, 100.0, 3.0, q),
                band(Bell, 1_000.0, -2.0, 0.7),
                band(HighShelf, 8_000.0, 3.0, q),
            ],
            Air => vec![band(HighShelf, 12_000.0, 3.0, q)],
        };
        let mut config = Self::new();
        for (slot, b) in config.bands.iter_mut().zip(on) {
            *slot = b;
        }
        config
    }
}

// ---------------------------------------------------------------- utility

/// The utility's bank (`docs/effects-catalogue.md` §2.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum UtilityPreset {
    /// Everything to the middle.
    Mono,
    /// Half again as wide.
    Wide,
    /// The low end to the middle, the rest left alone: the vinyl rule.
    MonoBelow120,
    /// Left and right the other way round.
    SwapSides,
    /// The right channel muted: what the left has, alone.
    LeftOnly,
    /// Both channels' polarity flipped.
    PolarityFlip,
    /// Six decibels down.
    TrimDown6,
    /// Six decibels up.
    TrimUp6,
    /// DC blocked at 20.
    DcBlock,
    /// Hard left.
    HardLeft,
    /// Hard right.
    HardRight,
}

impl UtilityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mono => "mono",
            Self::Wide => "wide",
            Self::MonoBelow120 => "mono below 120",
            Self::SwapSides => "swap sides",
            Self::LeftOnly => "left only",
            Self::PolarityFlip => "polarity flip",
            Self::TrimDown6 => "-6 dB trim",
            Self::TrimUp6 => "+6 dB trim",
            Self::DcBlock => "dc block",
            Self::HardLeft => "hard left",
            Self::HardRight => "hard right",
        }
    }

    pub const ALL: [Self; 11] = [
        Self::Mono,
        Self::Wide,
        Self::MonoBelow120,
        Self::SwapSides,
        Self::LeftOnly,
        Self::PolarityFlip,
        Self::TrimDown6,
        Self::TrimUp6,
        Self::DcBlock,
        Self::HardLeft,
        Self::HardRight,
    ];
}

impl UtilityConfig {
    pub fn from_preset(preset: UtilityPreset) -> Self {
        use UtilityPreset::*;
        let mut config = Self::new();
        match preset {
            Mono => config.width = 0.0,
            Wide => config.width = 1.5,
            MonoBelow120 => config.mono_below_hz = 120.0,
            SwapSides => config.swap = true,
            LeftOnly => config.mute_right = true,
            PolarityFlip => {
                config.invert_left = true;
                config.invert_right = true;
            }
            TrimDown6 => config.gain_db = -6.0,
            TrimUp6 => config.gain_db = 6.0,
            DcBlock => config.dc_hz = 20.0,
            HardLeft => config.pan = -1.0,
            HardRight => config.pan = 1.0,
        }
        config
    }
}
